use std::collections::{HashMap, HashSet};
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::resolve_reference::{
    is_beneath, is_dangling_shaped, key_of, resolve_reference, PathForm,
};
use crate::tokenize_references::{tokenize_references, Token};
use crate::walk_scope::{read_text, resolve_argument, Scope};

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

enum Directory {
    Listed(HashSet<Vec<String>>),
    Missing,
    Unlistable,
}

#[derive(Default)]
struct ExistenceCache {
    directories: Mutex<HashMap<PathBuf, Arc<Directory>>>,
    paths: Mutex<HashMap<PathBuf, bool>>,
}

fn directory_of(path: &Path) -> Directory {
    match std::fs::read_dir(path) {
        Ok(entries) => Directory::Listed(
            entries
                .flatten()
                .map(|entry| key_of(Path::new(&entry.file_name())))
                .collect(),
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => Directory::Missing,
        Err(_) => Directory::Unlistable,
    }
}

impl ExistenceCache {
    fn directory(&self, path: &Path) -> Arc<Directory> {
        if let Some(directory) = self.directories.lock().unwrap().get(path) {
            return Arc::clone(directory);
        }

        let directory = Arc::new(directory_of(path));

        self.directories
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), Arc::clone(&directory));

        directory
    }

    fn stat(&self, path: &Path) -> bool {
        if let Some(known) = self.paths.lock().unwrap().get(path) {
            return *known;
        }

        let exists = path.exists();

        self.paths
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), exists);

        exists
    }

    fn exists(&self, path: &Path) -> bool {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return self.stat(path);
        };

        match &*self.directory(parent) {
            Directory::Listed(names) => names.contains(&key_of(Path::new(name))) && self.stat(path),
            Directory::Missing => false,
            Directory::Unlistable => self.stat(path),
        }
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
    let content = read_text(file).map_err(|error| format!("{}: {error}", file.display()))?;

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

fn resolve_to(to: &[PathBuf], working_directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    to.iter()
        .map(|path| {
            let resolved = resolve_argument(path, working_directory);

            if resolved.symlink_metadata().is_err() {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("{}: no such file or directory", path.display()),
                ));
            }

            Ok(resolved)
        })
        .collect()
}

pub fn list_references(
    scope: &Scope,
    working_directory: &Path,
    to: &[PathBuf],
    dangling_only: bool,
) -> std::io::Result<Listing> {
    let to = resolve_to(to, working_directory)?;
    let cache = ExistenceCache::default();

    let filter = Filter {
        working_directory,
        to: &to,
        dangling_only,
        cache: &cache,
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

    Ok(listing)
}

#[cfg(test)]
#[path = "list_references.integration.test.rs"]
mod integration;
