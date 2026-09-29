use ignore::WalkBuilder;
use std::collections::{BTreeMap, HashSet};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use crate::format_path::format_path;
use crate::resolve_reference::{is_beneath, key_of, locate_path, normalize_path};

const VCS_NAMES: [&str; 5] = [".git", ".hg", ".svn", ".jj", ".bzr"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopeOptions {
    pub no_ignore: bool,
    pub no_ignore_vcs: bool,
    pub hidden: bool,
}

#[derive(Debug, Default)]
pub struct Scope {
    pub files: Vec<PathBuf>,
    pub entries: HashSet<Vec<String>>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

fn is_vcs_name(name: &std::ffi::OsStr) -> bool {
    let key = key_of(Path::new(name));

    VCS_NAMES.iter().any(|vcs| key_of(Path::new(vcs)) == key)
}

fn is_inside_vcs(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => is_vcs_name(name),
        _ => false,
    })
}

fn resolve_argument(path: &Path, working_directory: &Path) -> Option<PathBuf> {
    let path = dunce::simplified(path);

    match path.to_str() {
        Some(text) => locate_path(text, working_directory),
        None => Some(normalize_path(&working_directory.join(path))),
    }
}

fn missing_message_of(path: &Path, error: &std::io::Error, working_directory: &Path) -> String {
    let path = format_path(path, working_directory);

    if error.kind() == ErrorKind::NotFound {
        return format!("{path}: no such file or directory");
    }

    format!("{path}: {error}")
}

pub fn locate_argument(path: &Path, working_directory: &Path) -> Result<PathBuf, String> {
    let Some(resolved) = resolve_argument(path, working_directory) else {
        return Err(format!("{}: unsupported path", path.display()));
    };

    if let Err(error) = resolved.symlink_metadata() {
        return Err(missing_message_of(&resolved, &error, working_directory));
    }

    Ok(dunce::canonicalize(&resolved).unwrap_or(resolved))
}

fn root_of(path: &Path, working_directory: &Path) -> Result<PathBuf, String> {
    let root = locate_argument(path, working_directory)?;
    let lexical = resolve_argument(path, working_directory).unwrap_or_else(|| root.clone());

    if is_inside_vcs(&lexical) || is_inside_vcs(&root) {
        return Err(format!(
            "{}: inside a VCS directory",
            format_path(&lexical, working_directory)
        ));
    }

    Ok(root)
}

fn is_covered(root: &Path, index: usize, roots: &[PathBuf]) -> bool {
    root.is_dir()
        && roots.iter().enumerate().any(|(other_index, other)| {
            other_index != index
                && is_beneath(root, other)
                && (key_of(root) != key_of(other) || other_index < index)
        })
}

fn walk_error_of(error: &std::io::Error) -> String {
    let source = error
        .get_ref()
        .and_then(|inner| inner.source())
        .and_then(|source| source.downcast_ref::<std::io::Error>());

    source.unwrap_or(error).to_string()
}

fn is_ignore_file_error(error: &ignore::Error) -> bool {
    match error {
        ignore::Error::Partial(_) | ignore::Error::Glob { .. } => true,
        ignore::Error::WithLineNumber { err, .. }
        | ignore::Error::WithPath { err, .. }
        | ignore::Error::WithDepth { err, .. } => is_ignore_file_error(err),
        _ => false,
    }
}

fn messages_of(error: &ignore::Error, working_directory: &Path) -> Vec<String> {
    let format = |path: &Path| format_path(dunce::simplified(path), working_directory);

    match error {
        ignore::Error::Partial(errors) => errors
            .iter()
            .flat_map(|error| messages_of(error, working_directory))
            .collect(),
        ignore::Error::WithLineNumber { line, err } => messages_of(err, working_directory)
            .into_iter()
            .map(|message| format!("line {line}: {message}"))
            .collect(),
        ignore::Error::WithPath { path, err } => messages_of(err, working_directory)
            .into_iter()
            .map(|message| format!("{}: {message}", format(path)))
            .collect(),
        ignore::Error::WithDepth { err, .. } => messages_of(err, working_directory),
        ignore::Error::Loop { ancestor, child } => vec![format!(
            "{}: file system loop to its ancestor {}",
            format(child),
            format(ancestor)
        )],
        ignore::Error::Io(error) => vec![walk_error_of(error)],
        other => vec![other.to_string().replace(['\r', '\n'], " ")],
    }
}

pub fn walk_scope(paths: &[PathBuf], options: &ScopeOptions, working_directory: &Path) -> Scope {
    let mut scope = Scope::default();
    let mut roots = Vec::new();

    for path in paths {
        match root_of(path, working_directory) {
            Ok(root) => roots.push(root),
            Err(message) => scope.errors.push(message),
        }
    }

    let roots: Vec<PathBuf> = roots
        .iter()
        .enumerate()
        .filter(|(index, root)| !is_covered(root, *index, &roots))
        .map(|(_, root)| root.clone())
        .collect();

    let Some((first, rest)) = roots.split_first() else {
        return scope;
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
        .filter_entry(|entry| !is_vcs_name(entry.file_name()));

    let mut files = BTreeMap::new();

    for result in builder.build() {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                let messages = messages_of(&error, working_directory);

                if is_ignore_file_error(&error) {
                    scope.warnings.extend(messages);
                } else {
                    scope.errors.extend(messages);
                }

                continue;
            }
        };

        if let Some(error) = entry.error() {
            scope.warnings.extend(messages_of(error, working_directory));
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

    let mut seen = HashSet::new();

    scope
        .warnings
        .retain(|warning| seen.insert(warning.clone()));

    scope
        .files
        .sort_by_cached_key(|file| format_path(file, working_directory));

    scope
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
