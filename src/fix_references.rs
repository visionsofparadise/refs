use std::path::{Path, PathBuf};

use crate::format_path::format_path;
use crate::list_references::ExistenceCache;
use crate::map_in_parallel::map_in_parallel;
use crate::parse_declarations::{Declaration, Origin, Rejected};
use crate::render_reference::render_reference;
use crate::resolve_reference::{is_beneath, key_of, normalize_path, resolve_reference, PathForm};
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

const SOURCE_STILL_EXISTS: &str = "source still exists";
const DESTINATION_MISSING: &str = "destination missing";
const PATH_STILL_EXISTS: &str = "path still exists";

struct Move {
    from: PathBuf,
    to: PathBuf,
}

struct Moves {
    moves: Vec<Move>,
    deletes: Vec<PathBuf>,
}

struct Mapped {
    path: PathBuf,
    case_only: bool,
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

fn rebase(path: &Path, base: &Path, onto: &Path) -> PathBuf {
    path.components()
        .skip(key_of(base).len())
        .fold(onto.to_path_buf(), |joined, part| joined.join(part))
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

fn canonical_of(path: &Path, cache: &ExistenceCache) -> PathBuf {
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

fn canonicalize_declaration(declaration: Declaration, cache: &ExistenceCache) -> Declaration {
    match declaration {
        Declaration::Move { from, to, origin } => Declaration::Move {
            from: canonical_of(&from, cache),
            to: canonical_of(&to, cache),
            origin,
        },
        Declaration::Delete { path, origin } => Declaration::Delete {
            path: canonical_of(&path, cache),
            origin,
        },
    }
}

fn origin_of(declaration: &Declaration) -> &Origin {
    match declaration {
        Declaration::Move { origin, .. } | Declaration::Delete { origin, .. } => origin,
    }
}

fn is_composable(declaration: &Declaration, snapshot: Option<usize>) -> bool {
    snapshot.is_none() || origin_of(declaration).snapshot != snapshot
}

fn earlier_source_of(
    composed: &[Declaration],
    path: &Path,
    snapshot: Option<usize>,
) -> Option<(usize, PathBuf, bool)> {
    composed
        .iter()
        .enumerate()
        .filter(|(_, declaration)| is_composable(declaration, snapshot))
        .filter_map(|(index, declaration)| match declaration {
            Declaration::Move { from, to, .. } if is_beneath(path, to) => Some((index, from, to)),
            _ => None,
        })
        .max_by_key(|(_, _, to)| key_of(to).len())
        .map(|(index, from, to)| (index, rebase(path, to, from), key_of(path) == key_of(to)))
}

fn compose_move(composed: &mut Vec<Declaration>, from: PathBuf, to: PathBuf, origin: Origin) {
    let snapshot = origin.snapshot;
    let chained = earlier_source_of(composed, &from, snapshot);

    for (index, declaration) in composed.iter_mut().enumerate() {
        if !is_composable(declaration, snapshot)
            || chained.as_ref().is_some_and(|(chain, ..)| *chain == index)
        {
            continue;
        }

        let Declaration::Move {
            from: earlier_from,
            to: earlier_to,
            origin: earlier_origin,
        } = declaration
        else {
            continue;
        };

        if key_of(earlier_to) == key_of(&to) {
            *declaration = Declaration::Delete {
                path: earlier_from.clone(),
                origin: origin.clone(),
            };
        } else if is_beneath(earlier_to, &from) && key_of(earlier_to) != key_of(&from) {
            *earlier_to = rebase(earlier_to, &from, &to);
            earlier_origin.snapshot = snapshot;
        }
    }

    let (from, index) = match chained {
        Some((index, source, true)) => (source, Some(index)),
        Some((_, source, false)) => (source, None),
        None => (from, None),
    };

    let round_trip = from == to;
    let declaration = Declaration::Move { from, to, origin };

    match (index, round_trip) {
        (Some(index), true) => {
            composed.remove(index);
        }
        (Some(index), false) => composed[index] = declaration,
        (None, true) => {}
        (None, false) => composed.push(declaration),
    }
}

fn compose_delete(composed: &mut Vec<Declaration>, path: PathBuf, origin: Origin) {
    let snapshot = origin.snapshot;
    let chained = earlier_source_of(composed, &path, snapshot);
    let mut covered = false;

    for declaration in composed.iter_mut() {
        if !is_composable(declaration, snapshot) {
            continue;
        }

        if let Declaration::Move { from, to, .. } = declaration {
            if is_beneath(to, &path) {
                covered |= key_of(to) == key_of(&path);

                *declaration = Declaration::Delete {
                    path: from.clone(),
                    origin: origin.clone(),
                };
            }
        }
    }

    if covered {
        return;
    }

    let path = chained.map_or(path, |(_, source, _)| source);

    composed.push(Declaration::Delete { path, origin });
}

fn compose(declarations: Vec<Declaration>) -> Vec<Declaration> {
    let mut composed: Vec<Declaration> = Vec::new();

    for declaration in declarations {
        match declaration {
            Declaration::Move { from, to, origin } => {
                compose_move(&mut composed, from, to, origin);
            }
            Declaration::Delete { path, origin } => {
                compose_delete(&mut composed, path, origin);
            }
        }
    }

    composed
}

fn validate(declarations: Vec<Declaration>, cache: &ExistenceCache) -> (Moves, Vec<Rejected>) {
    let mut moves = Moves {
        moves: Vec::new(),
        deletes: Vec::new(),
    };
    let mut rejected = Vec::new();

    let refilled = |from: &Path, snapshot: Option<usize>| {
        snapshot.is_some()
            && declarations.iter().any(|declaration| match declaration {
                Declaration::Move { to, origin, .. } => {
                    origin.snapshot == snapshot && is_beneath(from, to)
                }
                Declaration::Delete { .. } => false,
            })
    };
    let refilled: Vec<bool> = declarations
        .iter()
        .map(|declaration| match declaration {
            Declaration::Move { from, origin, .. } => refilled(from, origin.snapshot),
            Declaration::Delete { .. } => false,
        })
        .collect();

    for (declaration, refilled) in declarations.into_iter().zip(refilled) {
        match declaration {
            Declaration::Move { from, to, origin } => {
                let reason = if key_of(&from) == key_of(&to) {
                    None
                } else if cache.exists(&from) && !refilled {
                    Some(SOURCE_STILL_EXISTS)
                } else if !cache.exists(&to) {
                    Some(DESTINATION_MISSING)
                } else {
                    None
                };

                match reason {
                    Some(reason) => rejected.push(Rejected {
                        origin,
                        reason: reason.to_string(),
                    }),
                    None => moves.moves.push(Move { from, to }),
                }
            }
            Declaration::Delete { path, origin } => {
                if cache.exists(&path) {
                    rejected.push(Rejected {
                        origin,
                        reason: PATH_STILL_EXISTS.to_string(),
                    });
                } else {
                    moves.deletes.push(path);
                }
            }
        }
    }

    rejected.sort_by_key(|rejection| rejection.origin.line);

    (moves, rejected)
}

impl Moves {
    fn forward_of(&self, path: &Path) -> Mapped {
        let found = self
            .moves
            .iter()
            .filter(|found| is_beneath(path, &found.from))
            .max_by_key(|found| key_of(&found.from).len());

        match found {
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
        }
    }

    fn origin_of(&self, file: &Path) -> PathBuf {
        self.moves
            .iter()
            .filter(|found| is_beneath(file, &found.to))
            .max_by_key(|found| key_of(&found.to).len())
            .map_or_else(
                || file.to_path_buf(),
                |found| rebase(file, &found.to, &found.from),
            )
    }

    fn is_deleted(&self, path: &Path) -> bool {
        self.deletes.iter().any(|deleted| is_beneath(path, deleted))
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

    candidates
        .iter()
        .find(|candidate| {
            candidate.form == *form
                || (shared_base && is_relative(form) && is_relative(&candidate.form))
        })
        .is_some_and(|found| {
            if mapped.case_only {
                found.target == mapped.path
            } else {
                key_of(&found.target) == key_of(&mapped.path)
            }
        })
}

enum Decision {
    Rewrite(Rewrite),
    Report(Outcome),
}

fn decide(
    token: &Token,
    origin_directory: &Path,
    directory: &Path,
    context: &Context,
) -> Option<Decision> {
    let (candidates, style) =
        resolve_reference(&token.path, origin_directory, context.working_directory);

    for candidate in &candidates {
        let mapped = context.moves.forward_of(&candidate.target);

        if !context.cache.exists(&mapped.path) {
            if context.moves.is_deleted(&candidate.target) {
                return Some(Decision::Report(Outcome::Deleted {
                    target: mapped.path,
                }));
            }

            continue;
        }

        if resolves_to(token, &candidate.form, directory, &mapped, context) {
            return None;
        }

        if !context.scope.entries.contains(&key_of(&mapped.path)) {
            return Some(Decision::Report(Outcome::OutOfScope {
                target: mapped.path,
            }));
        }

        let rendered = render_reference(
            &mapped.path,
            &candidate.form,
            &style,
            &token.suffix,
            directory,
            context.working_directory,
            &|path| context.cache.exists(path),
        );

        return match rendered {
            Some(rendered) if rendered == token.path => None,
            Some(rendered) => Some(Decision::Rewrite(Rewrite {
                token: token.clone(),
                rendered,
                target: mapped.path,
            })),
            None => Some(Decision::Report(Outcome::Unrewritable {
                target: mapped.path,
            })),
        };
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
        .map(|declaration| canonicalize_declaration(declaration, &cache))
        .collect();

    let (moves, rejected) = validate(compose(declarations), &cache);

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
            .filter(|found| !scope.entries.contains(&key_of(&found.to)))
            .map(|found| found.to.clone())
            .collect(),
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
