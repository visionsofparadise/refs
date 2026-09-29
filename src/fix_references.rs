use std::io;
use std::path::{Path, PathBuf};

use crate::format_path::format_path;
use crate::list_references::ExistenceCache;
use crate::map_in_parallel::map_in_parallel;
use crate::parse_declarations::{Declaration, Rejected};
use crate::render_reference::render_reference;
use crate::resolve_reference::{is_beneath, key_of, normalize_path, resolve_reference};
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

fn earlier_source_of(composed: &[Declaration], path: &Path) -> Option<(usize, PathBuf, bool)> {
    composed
        .iter()
        .enumerate()
        .filter_map(|(index, declaration)| match declaration {
            Declaration::Move { from, to, .. } if is_beneath(path, to) => Some((index, from, to)),
            _ => None,
        })
        .max_by_key(|(_, _, to)| key_of(to).len())
        .map(|(index, from, to)| (index, rebase(path, to, from), key_of(path) == key_of(to)))
}

fn compose(declarations: Vec<Declaration>) -> Vec<Declaration> {
    let mut composed: Vec<Declaration> = Vec::new();

    for declaration in declarations {
        let path = match &declaration {
            Declaration::Move { from, .. } => from,
            Declaration::Delete { path, .. } => path,
        };

        let Some((index, source, replaces)) = earlier_source_of(&composed, path) else {
            composed.push(declaration);

            continue;
        };

        let declaration = match declaration {
            Declaration::Move { to, origin, .. } => Declaration::Move {
                from: source,
                to,
                origin,
            },
            Declaration::Delete { origin, .. } => Declaration::Delete {
                path: source,
                origin,
            },
        };

        if replaces {
            composed[index] = declaration;
        } else {
            composed.push(declaration);
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

    for declaration in declarations {
        match declaration {
            Declaration::Move { from, to, origin } => {
                let reason = if key_of(&from) == key_of(&to) {
                    None
                } else if cache.exists(&from) {
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
            Some(found) => Mapped {
                path: rebase(path, &found.from, &found.to),
                case_only: key_of(&found.from) == key_of(&found.to),
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

fn resolves_to(token: &Token, directory: &Path, mapped: &Mapped, context: &Context) -> bool {
    let (candidates, _) = resolve_reference(&token.path, directory, context.working_directory);

    candidates
        .iter()
        .find(|candidate| context.cache.exists(&candidate.target))
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

        if resolves_to(token, directory, &mapped, context) {
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

fn fix_file(file: &Path, context: &Context) -> io::Result<FileFix> {
    let content = read_text(file).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{}: {error}", format_path(file, context.working_directory)),
        )
    })?;

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
) -> io::Result<FixPlan> {
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
        let fixed = fixed?;

        plan.findings.extend(fixed.findings);
        plan.edits.extend(fixed.edit);
    }

    Ok(plan)
}

#[cfg(test)]
#[path = "fix_references.integration.test.rs"]
mod integration;
