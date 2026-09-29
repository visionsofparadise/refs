use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::format_path::format_path;
use crate::map_in_parallel::map_in_parallel;
use crate::resolve_reference::{is_beneath, is_dangling_shaped, resolve_reference, PathForm};
use crate::tokenize_references::{tokenize_references, Token};
use crate::walk_scope::{locate_argument, read_text, Scope};

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

const SHARDS: usize = 64;

struct ShardedMap<V> {
    shards: Vec<Mutex<HashMap<PathBuf, V>>>,
    hasher: RandomState,
}

impl<V: Clone> ShardedMap<V> {
    fn new() -> Self {
        ShardedMap {
            shards: (0..SHARDS).map(|_| Mutex::default()).collect(),
            hasher: RandomState::new(),
        }
    }

    fn get_or_insert_with(&self, path: &Path, create: impl FnOnce() -> V) -> V {
        let shard = &self.shards[self.hasher.hash_one(path) as usize % SHARDS];

        if let Some(value) = shard.lock().unwrap().get(path) {
            return value.clone();
        }

        let value = create();

        shard
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), value.clone());

        value
    }
}

pub struct ExistenceCache {
    directories: ShardedMap<Arc<Names>>,
    paths: ShardedMap<bool>,
    probe: fn(&Path) -> bool,
}

fn read_names(directory: &Path) -> Names {
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
    pub fn new(probe: fn(&Path) -> bool) -> Self {
        ExistenceCache {
            directories: ShardedMap::new(),
            paths: ShardedMap::new(),
            probe,
        }
    }

    fn names_of(&self, directory: &Path) -> Arc<Names> {
        self.directories
            .get_or_insert_with(directory, || Arc::new(read_names(directory)))
    }

    fn stat(&self, path: &Path) -> bool {
        self.paths.get_or_insert_with(path, || (self.probe)(path))
    }

    pub fn exists(&self, path: &Path) -> bool {
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            let listed = self
                .names_of(parent)
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
    let (resolved, errors): (Vec<_>, Vec<_>) = to
        .iter()
        .map(|path| locate_argument(path, working_directory))
        .partition(Result::is_ok);

    if errors.is_empty() {
        Ok(resolved.into_iter().flatten().collect())
    } else {
        Err(errors.into_iter().filter_map(Result::err).collect())
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

    let results = map_in_parallel(&scope.files, |file| list_file(file, &filter));

    let mut listing = Listing::default();

    for result in results {
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
