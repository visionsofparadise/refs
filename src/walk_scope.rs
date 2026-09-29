use ignore::WalkBuilder;
use std::collections::{BTreeMap, HashSet};
use std::io::{Error, ErrorKind};
use std::path::{Component, Path, PathBuf};

use crate::resolve_reference::{key_of, normalize_path, parse_absolute};

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
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

fn is_vcs_name(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();

    VCS_DIRECTORIES
        .iter()
        .any(|vcs| key_of(Path::new(vcs)) == key_of(Path::new(name.as_ref())))
}

fn is_vcs_directory(entry: &ignore::DirEntry) -> bool {
    entry.file_type().is_some_and(|kind| kind.is_dir()) && is_vcs_name(entry.file_name())
}

fn is_inside_vcs(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => is_vcs_name(name),
        _ => false,
    })
}

pub fn resolve_argument(path: &Path, working_directory: &Path) -> PathBuf {
    match path.to_str().and_then(parse_absolute) {
        Some((absolute, _)) => absolute,
        None => normalize_path(&working_directory.join(path)),
    }
}

fn root_of(path: &Path, working_directory: &Path) -> std::io::Result<PathBuf> {
    let root = resolve_argument(path, working_directory);

    if is_inside_vcs(&root) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("{}: inside a VCS directory", path.display()),
        ));
    }

    if root.symlink_metadata().is_err() {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!("{}: no such file or directory", path.display()),
        ));
    }

    Ok(root)
}

pub fn walk_scope(
    paths: &[PathBuf],
    options: &ScopeOptions,
    working_directory: &Path,
) -> std::io::Result<Scope> {
    let roots = paths
        .iter()
        .map(|path| root_of(path, working_directory))
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
    let mut scope = Scope::default();

    for result in builder.build() {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                scope.errors.push(error.to_string());

                continue;
            }
        };

        if let Some(error) = entry.error() {
            scope.warnings.push(error.to_string());
        }

        let key = key_of(entry.path());

        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            files
                .entry(key.clone())
                .or_insert_with(|| entry.path().to_path_buf());
        }

        scope.entries.insert(key);
    }

    scope.files = files.into_values().collect();

    Ok(scope)
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
