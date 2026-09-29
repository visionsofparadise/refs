use std::path::{Path, PathBuf};

use crate::compose_declarations::{compose_declarations, rebase, Move};
use crate::format_path::format_path;
use crate::list_references::ExistenceCache;
use crate::map_in_parallel::map_in_parallel;
use crate::parse_declarations::{Declaration, Rejected};
use crate::render_reference::render_reference;
use crate::resolve_reference::{
    is_beneath, key_of, normalize_path, resolve_reference, Candidate, PathForm, PathStyle,
};
use crate::tokenize_references::{tokenize_references, Token};
use crate::walk_scope::{read_text, Scope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Rewritten { replacement: String },
    Deleted { target: PathBuf },
    OutOfScope { target: PathBuf },
    Unrewritable { target: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: PathBuf,
    pub token: Token,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub file: PathBuf,
    pub content: String,
}

#[derive(Debug, Default)]
pub struct FixPlan {
    pub findings: Vec<Finding>,
    pub unscanned: Vec<PathBuf>,
    pub rejected: Vec<Rejected>,
    pub edits: Vec<FileEdit>,
    pub errors: Vec<String>,
}

struct Moves {
    moves: Vec<Move>,
    deletes: Vec<PathBuf>,
}

struct Mapped {
    path: PathBuf,
    case_only: bool,
}

enum Forward {
    Mapped(Mapped),
    Deleted,
}

enum Inferred {
    Mapped(PathBuf),
    Deleted,
}

struct Rewrite {
    token: Token,
    rendered: String,
    target: PathBuf,
}

struct Context<'a> {
    moves: &'a Moves,
    scope: &'a Scope,
    working_directory: &'a Path,
    cache: &'a ExistenceCache,
}

fn recase(path: &Path, from: &Path, to: &Path) -> PathBuf {
    let renamed = from.components().zip(to.components());

    path.components()
        .zip(renamed.map(Some).chain(std::iter::repeat(None)))
        .fold(PathBuf::new(), |mut joined, (part, names)| {
            match names {
                Some((before, after)) if before != after => joined.push(after),
                _ => joined.push(part),
            }

            joined
        })
}

fn canonical_of(path: &Path, working_directory: &Path, cache: &ExistenceCache) -> PathBuf {
    if is_beneath(path, working_directory) {
        return path.to_path_buf();
    }

    for ancestor in path.ancestors().skip(1) {
        if !cache.exists(ancestor) {
            continue;
        }

        if let Ok(canonical) = dunce::canonicalize(ancestor) {
            return normalize_path(&rebase(path, ancestor, &canonical));
        }
    }

    path.to_path_buf()
}

fn canonicalize_declaration(
    declaration: Declaration,
    working_directory: &Path,
    cache: &ExistenceCache,
) -> Declaration {
    let canonical = |path: &Path| canonical_of(path, working_directory, cache);

    match declaration {
        Declaration::Move { from, to, origin } => Declaration::Move {
            from: canonical(&from),
            to: canonical(&to),
            origin,
        },
        Declaration::Delete { path, origin } => Declaration::Delete {
            path: canonical(&path),
            origin,
        },
    }
}

fn prefix_of(from: &Path, to: &Path, ancestor: &Path) -> Option<PathBuf> {
    let from_key = key_of(from);
    let to_key = key_of(to);
    let depth = from_key.len() - key_of(ancestor).len();

    if to_key.len() < depth || from_key[from_key.len() - depth..] != to_key[to_key.len() - depth..]
    {
        return None;
    }

    to.ancestors().nth(depth).map(Path::to_path_buf)
}

impl Moves {
    fn forward_of(&self, path: &Path) -> Forward {
        let found = self
            .moves
            .iter()
            .filter(|found| is_beneath(path, &found.from))
            .max_by_key(|found| key_of(&found.from).len());

        let deleted = self
            .deletes
            .iter()
            .filter(|deleted| is_beneath(path, deleted))
            .map(|deleted| key_of(deleted).len())
            .max();

        let moved = found.map(|found| key_of(&found.from).len());

        if deleted.is_some() && deleted > moved {
            return Forward::Deleted;
        }

        Forward::Mapped(match found {
            Some(found) if key_of(&found.from) == key_of(&found.to) => Mapped {
                path: recase(path, &found.from, &found.to),
                case_only: true,
            },
            Some(found) => Mapped {
                path: rebase(path, &found.from, &found.to),
                case_only: false,
            },
            None => Mapped {
                path: path.to_path_buf(),
                case_only: false,
            },
        })
    }

    fn origin_of(&self, file: &Path) -> PathBuf {
        let mut best: Option<&Move> = None;

        for found in self
            .moves
            .iter()
            .filter(|found| is_beneath(file, &found.to))
        {
            if best.is_none_or(|best| key_of(&found.to).len() > key_of(&best.to).len()) {
                best = Some(found);
            }
        }

        best.map_or_else(
            || file.to_path_buf(),
            |found| rebase(file, &found.to, &found.from),
        )
    }

    fn is_source(&self, path: &Path) -> bool {
        self.moves.iter().any(|found| is_beneath(path, &found.from))
    }

    fn is_exact_source(&self, path: &Path) -> bool {
        self.moves
            .iter()
            .any(|found| key_of(path) == key_of(&found.from))
    }

    fn is_arrival(&self, path: &Path) -> bool {
        !self.is_source(path) && self.moves.iter().any(|found| is_beneath(path, &found.to))
    }

    fn inferred_of(&self, ancestor: &Path) -> Option<Inferred> {
        let strictly_beneath =
            |path: &Path| is_beneath(path, ancestor) && key_of(path) != key_of(ancestor);

        let moves: Vec<&Move> = self
            .moves
            .iter()
            .filter(|found| strictly_beneath(&found.from))
            .collect();

        let deleted = self.deletes.iter().any(|deleted| strictly_beneath(deleted));

        match (moves.as_slice(), deleted) {
            ([], true) => Some(Inferred::Deleted),
            ([], false) | ([_, ..], true) => None,
            ([first, rest @ ..], false) => {
                let prefix = prefix_of(&first.from, &first.to, ancestor)?;

                rest.iter()
                    .all(|found| {
                        prefix_of(&found.from, &found.to, ancestor)
                            .is_some_and(|other| key_of(&other) == key_of(&prefix))
                    })
                    .then_some(Inferred::Mapped(prefix))
            }
        }
    }
}

fn is_relative(form: &PathForm) -> bool {
    matches!(
        form,
        PathForm::FileRelative | PathForm::WorkingDirectoryRelative
    )
}

fn resolves_to(
    token: &Token,
    form: &PathForm,
    directory: &Path,
    mapped: &Mapped,
    context: &Context,
) -> bool {
    let (candidates, _) = resolve_reference(&token.path, directory, context.working_directory);
    let shared_base = key_of(directory) == key_of(context.working_directory);

    for candidate in &candidates {
        let same_form = candidate.form == *form
            || (shared_base && is_relative(form) && is_relative(&candidate.form));

        if same_form {
            return if mapped.case_only {
                candidate.target == mapped.path
            } else {
                key_of(&candidate.target) == key_of(&mapped.path)
            };
        }

        if context.cache.exists(&candidate.target) {
            return false;
        }
    }

    false
}

fn is_in_scope(path: &Path, context: &Context) -> bool {
    context.scope.entries.contains(&key_of(path))
        || dunce::canonicalize(path)
            .is_ok_and(|canonical| context.scope.entries.contains(&key_of(&canonical)))
}

enum Decision {
    Rewrite(Rewrite),
    Report(Outcome),
}

fn settle(
    token: &Token,
    candidate: &Candidate,
    style: &PathStyle,
    mapped: Mapped,
    directory: &Path,
    context: &Context,
) -> Option<Decision> {
    if resolves_to(token, &candidate.form, directory, &mapped, context) {
        return None;
    }

    if !is_in_scope(&mapped.path, context) {
        return Some(Decision::Report(Outcome::OutOfScope {
            target: mapped.path,
        }));
    }

    let rendered = render_reference(
        &mapped.path,
        &candidate.form,
        style,
        &token.suffix,
        directory,
        context.working_directory,
        &|path| context.cache.exists(path),
    );

    match rendered {
        Some(rendered) if rendered == token.path => None,
        Some(rendered) => Some(Decision::Rewrite(Rewrite {
            token: token.clone(),
            rendered,
            target: mapped.path,
        })),
        None => Some(Decision::Report(Outcome::Unrewritable {
            target: mapped.path,
        })),
    }
}

fn decide(
    token: &Token,
    origin_directory: &Path,
    directory: &Path,
    context: &Context,
) -> Option<Decision> {
    let (candidates, style) =
        resolve_reference(&token.path, origin_directory, context.working_directory);
    let deleted = |candidate: &Candidate| {
        Some(Decision::Report(Outcome::Deleted {
            target: candidate.target.clone(),
        }))
    };

    for candidate in &candidates {
        let mapped = match context.moves.forward_of(&candidate.target) {
            Forward::Deleted => return deleted(candidate),
            Forward::Mapped(mapped) => mapped,
        };

        if context.cache.exists(&mapped.path) {
            if context.moves.is_arrival(&candidate.target) {
                continue;
            }

            return settle(token, candidate, &style, mapped, directory, context);
        }

        if context.cache.exists(&candidate.target)
            || context.moves.is_exact_source(&candidate.target)
        {
            continue;
        }

        match context.moves.inferred_of(&candidate.target) {
            Some(Inferred::Deleted) => return deleted(candidate),
            Some(Inferred::Mapped(path)) if context.cache.exists(&path) => {
                let mapped = Mapped {
                    path,
                    case_only: false,
                };

                return settle(token, candidate, &style, mapped, directory, context);
            }
            _ => {}
        }
    }

    None
}

fn splice(content: &str, rewrites: &[Rewrite]) -> String {
    let mut spliced = content.to_string();

    for rewrite in rewrites.iter().rev() {
        spliced.replace_range(
            rewrite.token.start..rewrite.token.end,
            &format!("{}{}", rewrite.rendered, rewrite.token.suffix),
        );
    }

    spliced
}

fn misread_of(spliced: &str, rewrites: &[Rewrite]) -> Vec<usize> {
    let tokens = tokenize_references(spliced);
    let mut shift = 0isize;
    let mut misread = Vec::new();

    for (index, rewrite) in rewrites.iter().enumerate() {
        let start = rewrite.token.start.saturating_add_signed(shift);
        let length = rewrite.rendered.len() + rewrite.token.suffix.len();

        let intact = tokens.iter().any(|token| {
            token.start == start
                && token.end == start + length
                && token.path == rewrite.rendered
                && token.suffix == rewrite.token.suffix
        });

        if !intact {
            misread.push(index);
        }

        shift += length as isize - (rewrite.token.end - rewrite.token.start) as isize;
    }

    misread
}

struct FileFix {
    findings: Vec<Finding>,
    edit: Option<FileEdit>,
}

fn fix_file(file: &Path, context: &Context) -> Result<FileFix, String> {
    let content = read_text(file)
        .map_err(|error| format!("{}: {error}", format_path(file, context.working_directory)))?;

    let Some(content) = content else {
        return Ok(FileFix {
            findings: Vec::new(),
            edit: None,
        });
    };

    let origin = context.moves.origin_of(file);
    let (Some(origin_directory), Some(directory)) = (origin.parent(), file.parent()) else {
        return Ok(FileFix {
            findings: Vec::new(),
            edit: None,
        });
    };

    let mut rewrites = Vec::new();
    let mut findings = Vec::new();

    for token in tokenize_references(&content) {
        match decide(&token, origin_directory, directory, context) {
            Some(Decision::Rewrite(rewrite)) => rewrites.push(rewrite),
            Some(Decision::Report(outcome)) => findings.push(Finding {
                file: file.to_path_buf(),
                token,
                outcome,
            }),
            None => {}
        }
    }

    let mut spliced = splice(&content, &rewrites);

    loop {
        let misread = misread_of(&spliced, &rewrites);

        if misread.is_empty() {
            break;
        }

        for index in misread.into_iter().rev() {
            let rewrite = rewrites.remove(index);

            findings.push(Finding {
                file: file.to_path_buf(),
                token: rewrite.token,
                outcome: Outcome::Unrewritable {
                    target: rewrite.target,
                },
            });
        }

        spliced = splice(&content, &rewrites);
    }

    let edit = (!rewrites.is_empty()).then(|| FileEdit {
        file: file.to_path_buf(),
        content: spliced,
    });

    findings.extend(rewrites.into_iter().map(|rewrite| Finding {
        file: file.to_path_buf(),
        outcome: Outcome::Rewritten {
            replacement: format!("{}{}", rewrite.rendered, rewrite.token.suffix),
        },
        token: rewrite.token,
    }));

    findings.sort_by_key(|finding| finding.token.start);

    Ok(FileFix { findings, edit })
}

pub fn plan_fix(
    declarations: Vec<Declaration>,
    scope: &Scope,
    working_directory: &Path,
) -> FixPlan {
    let cache = ExistenceCache::new(Path::exists);

    let declarations = declarations
        .into_iter()
        .map(|declaration| canonicalize_declaration(declaration, working_directory, &cache))
        .collect();

    let composition = compose_declarations(declarations, &|path| cache.exists(path));
    let moves = Moves {
        moves: composition.moves,
        deletes: composition.deletes,
    };
    let rejected = composition.rejected;

    let context = Context {
        moves: &moves,
        scope,
        working_directory,
        cache: &cache,
    };

    let mut plan = FixPlan {
        unscanned: moves
            .moves
            .iter()
            .filter(|found| !is_in_scope(&found.to, &context))
            .fold(Vec::new(), |mut unscanned: Vec<PathBuf>, found| {
                if !unscanned
                    .iter()
                    .any(|seen| key_of(seen) == key_of(&found.to))
                {
                    unscanned.push(found.to.clone());
                }

                unscanned
            }),
        rejected,
        ..FixPlan::default()
    };

    for fixed in map_in_parallel(&scope.files, |file| fix_file(file, &context)) {
        match fixed {
            Ok(fixed) => {
                plan.findings.extend(fixed.findings);
                plan.edits.extend(fixed.edit);
            }
            Err(error) => plan.errors.push(error),
        }
    }

    plan
}

#[cfg(test)]
#[path = "fix_references.integration.test.rs"]
mod integration;
