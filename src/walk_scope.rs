use ignore::WalkBuilder;
use std::collections::{BTreeMap, HashSet};
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use crate::resolve_reference::{key_of, normalize_path};

const VCS_DIRECTORIES: [&str; 5] = [".git", ".hg", ".svn", ".jj", ".bzr"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopeOptions {
    pub no_ignore: bool,
    pub no_ignore_vcs: bool,
    pub hidden: bool,
}

#[derive(Debug, Default)]
pub struct Scope {
    pub files: Vec<PathBuf>,
    #[allow(dead_code)]
    pub entries: HashSet<Vec<String>>,
}

fn is_vcs_directory(entry: &ignore::DirEntry) -> bool {
    entry.file_type().is_some_and(|kind| kind.is_dir())
        && VCS_DIRECTORIES
            .iter()
            .any(|name| entry.file_name() == *name)
}

pub fn walk_scope(
    paths: &[PathBuf],
    options: &ScopeOptions,
    working_directory: &Path,
) -> std::io::Result<Scope> {
    let roots = paths
        .iter()
        .map(|path| {
            let root = normalize_path(&working_directory.join(path));

            if root.symlink_metadata().is_err() {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("{}: no such file or directory", path.display()),
                ));
            }

            Ok(root)
        })
        .collect::<std::io::Result<Vec<PathBuf>>>()?;

    let Some((first, rest)) = roots.split_first() else {
        return Ok(Scope::default());
    };

    let mut builder = WalkBuilder::new(first);

    for root in rest {
        builder.add(root);
    }

    if options.no_ignore {
        builder.standard_filters(false);
    } else {
        builder.add_custom_ignore_filename(".rgignore");
    }

    if options.no_ignore_vcs {
        builder
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false);
    }

    builder
        .hidden(!options.hidden)
        .follow_links(false)
        .filter_entry(|entry| !is_vcs_directory(entry));

    let mut files = BTreeMap::new();
    let mut entries = HashSet::new();

    for entry in builder.build().flatten() {
        let key = key_of(entry.path());

        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            files
                .entry(key.clone())
                .or_insert_with(|| entry.path().to_path_buf());
        }

        entries.insert(key);
    }

    Ok(Scope {
        files: files.into_values().collect(),
        entries,
    })
}

pub fn read_text(path: &Path) -> std::io::Result<Option<String>> {
    let bytes = std::fs::read(path)?;

    if bytes.contains(&0) {
        return Ok(None);
    }

    Ok(String::from_utf8(bytes).ok())
}

#[cfg(test)]
#[path = "walk_scope.integration.test.rs"]
mod integration;
