use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::format_path::format_path;
use crate::resolve_reference::{is_beneath, is_dangling_shaped, resolve_reference, PathForm};
use crate::tokenize_references::{tokenize_references, Token};
use crate::walk_scope::{missing_message_of, read_text, resolve_argument, Scope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub file: PathBuf,
    pub token: Token,
    pub target: PathBuf,
    pub dangling: bool,
}

#[derive(Debug, Default)]
pub struct Listing {
    pub listed: Vec<Listed>,
    pub errors: Vec<String>,
}

type Names = Option<HashSet<OsString>>;

struct ExistenceCache {
    directories: Mutex<HashMap<PathBuf, Arc<Names>>>,
    paths: Mutex<HashMap<PathBuf, bool>>,
    stat: fn(&Path) -> bool,
}

fn names_of(directory: &Path) -> Names {
    let entries = std::fs::read_dir(directory).ok()?;

    Some(
        entries
            .flatten()
            .filter(|entry| {
                !entry.file_type().is_ok_and(|kind| kind.is_symlink()) || entry.path().exists()
            })
            .map(|entry| entry.file_name())
            .collect(),
    )
}

impl ExistenceCache {
    fn new(stat: fn(&Path) -> bool) -> Self {
        ExistenceCache {
            directories: Mutex::default(),
            paths: Mutex::default(),
            stat,
        }
    }

    fn names(&self, directory: &Path) -> Arc<Names> {
        if let Some(names) = self.directories.lock().unwrap().get(directory) {
            return Arc::clone(names);
        }

        let names = Arc::new(names_of(directory));

        self.directories
            .lock()
            .unwrap()
            .insert(directory.to_path_buf(), Arc::clone(&names));

        names
    }

    fn stat(&self, path: &Path) -> bool {
        if let Some(known) = self.paths.lock().unwrap().get(path) {
            return *known;
        }

        let exists = (self.stat)(path);

        self.paths
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), exists);

        exists
    }

    fn exists(&self, path: &Path) -> bool {
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            let listed = self
                .names(parent)
                .as_ref()
                .as_ref()
                .is_some_and(|names| names.contains(name));

            if listed {
                return true;
            }
        }

        self.stat(path)
    }
}

struct Filter<'a> {
    working_directory: &'a Path,
    to: &'a [PathBuf],
    dangling_only: bool,
    cache: &'a ExistenceCache,
}

fn listed_of(token: Token, file: &Path, filter: &Filter) -> Option<Listed> {
    let directory = file.parent()?;
    let (candidates, _) = resolve_reference(&token.path, directory, filter.working_directory);

    if let Some(found) = candidates
        .iter()
        .find(|candidate| filter.cache.exists(&candidate.target))
    {
        return Some(Listed {
            file: file.to_path_buf(),
            target: found.target.clone(),
            token,
            dangling: false,
        });
    }

    let first = candidates.into_iter().next()?;

    let local = match first.form {
        PathForm::Absolute(_) => is_beneath(&first.target, filter.working_directory),
        PathForm::FileRelative | PathForm::WorkingDirectoryRelative => true,
    };

    if !local || token.at_prefixed || !is_dangling_shaped(&token.path) {
        return None;
    }

    Some(Listed {
        file: file.to_path_buf(),
        target: first.target,
        token,
        dangling: true,
    })
}

fn list_file(file: &Path, filter: &Filter) -> Result<Vec<Listed>, String> {
    let content = read_text(file)
        .map_err(|error| format!("{}: {error}", format_path(file, filter.working_directory)))?;

    let Some(content) = content else {
        return Ok(Vec::new());
    };

    Ok(tokenize_references(&content)
        .into_iter()
        .filter_map(|token| listed_of(token, file, filter))
        .filter(|entry| !filter.dangling_only || entry.dangling)
        .filter(|entry| {
            filter.to.is_empty() || filter.to.iter().any(|base| is_beneath(&entry.target, base))
        })
        .collect())
}

pub fn resolve_targets(
    to: &[PathBuf],
    working_directory: &Path,
) -> Result<Vec<PathBuf>, Vec<String>> {
    let mut resolved = Vec::new();
    let mut errors = Vec::new();

    for path in to {
        match resolve_argument(path, working_directory) {
            None => errors.push(format!("{}: unsupported path", path.display())),
            Some(target) => match target.symlink_metadata() {
                Ok(_) => resolved.push(target),
                Err(error) => errors.push(missing_message_of(&target, &error, working_directory)),
            },
        }
    }

    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(errors)
    }
}

fn list_with(
    scope: &Scope,
    working_directory: &Path,
    to: &[PathBuf],
    dangling_only: bool,
    cache: &ExistenceCache,
) -> Listing {
    let filter = Filter {
        working_directory,
        to,
        dangling_only,
        cache,
    };

    let next = AtomicUsize::new(0);

    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(scope.files.len().max(1));

    let mut results: Vec<(usize, Result<Vec<Listed>, String>)> = std::thread::scope(|threads| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                threads.spawn(|| {
                    let mut results = Vec::new();

                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);

                        let Some(file) = scope.files.get(index) else {
                            return results;
                        };

                        results.push((index, list_file(file, &filter)));
                    }
                })
            })
            .collect();

        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });

    results.sort_unstable_by_key(|(index, _)| *index);

    let mut listing = Listing::default();

    for (_, result) in results {
        match result {
            Ok(listed) => listing.listed.extend(listed),
            Err(error) => listing.errors.push(error),
        }
    }

    listing
}

pub fn list_references(
    scope: &Scope,
    working_directory: &Path,
    to: &[PathBuf],
    dangling_only: bool,
) -> Listing {
    let cache = ExistenceCache::new(Path::exists);

    list_with(scope, working_directory, to, dangling_only, &cache)
}

#[cfg(test)]
#[path = "list_references.integration.test.rs"]
mod integration;
