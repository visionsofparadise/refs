use std::io::Error;
use std::path::{Path, PathBuf};

use crate::resolve_reference::{
    is_beneath, is_dangling_shaped, key_of, normalize_path, resolve_reference, PathForm,
};
use crate::tokenize_references::{tokenize_references, Token};
use crate::walk_scope::{read_text, Scope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub file: PathBuf,
    pub token: Token,
    pub target: PathBuf,
    pub dangling: bool,
}

fn listed_of(token: Token, file: &Path, working_directory: &Path) -> Option<Listed> {
    let directory = file.parent()?;
    let (candidates, _) = resolve_reference(&token.path, directory, working_directory);

    if let Some(found) = candidates
        .iter()
        .find(|candidate| candidate.target.exists())
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
        PathForm::Absolute(_) => is_beneath(&first.target, working_directory),
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

pub fn list_references(
    scope: &Scope,
    working_directory: &Path,
    to: &[PathBuf],
    dangling_only: bool,
) -> std::io::Result<Vec<Listed>> {
    let to: Vec<PathBuf> = to
        .iter()
        .map(|path| normalize_path(&working_directory.join(path)))
        .collect();

    let mut listed = Vec::new();

    for file in &scope.files {
        let content = read_text(file)
            .map_err(|error| Error::new(error.kind(), format!("{}: {error}", file.display())))?;

        let Some(content) = content else {
            continue;
        };

        listed.extend(
            tokenize_references(&content)
                .into_iter()
                .filter_map(|token| listed_of(token, file, working_directory))
                .filter(|entry| !dangling_only || entry.dangling)
                .filter(|entry| {
                    to.is_empty() || to.iter().any(|base| is_beneath(&entry.target, base))
                }),
        );
    }

    listed.sort_by_cached_key(|entry| (key_of(&entry.file), entry.token.line, entry.token.column));

    Ok(listed)
}

#[cfg(test)]
#[path = "list_references.integration.test.rs"]
mod integration;
