use std::fs;
use std::io::{self, Write};
use std::path::Path;
use tempfile::NamedTempFile;

use crate::fix_references::FileEdit;

#[cfg(windows)]
fn replace(temporary: NamedTempFile, file: &Path, permissions: fs::Permissions) -> io::Result<()> {
    if !permissions.readonly() {
        fs::set_permissions(temporary.path(), permissions)?;
        temporary.persist(file)?;

        return Ok(());
    }

    let mut writable = permissions.clone();

    #[allow(clippy::permissions_set_readonly_false)]
    writable.set_readonly(false);

    fs::set_permissions(temporary.path(), writable.clone())?;
    fs::set_permissions(file, writable)?;

    if let Err(error) = temporary.persist(file) {
        let _ = fs::set_permissions(file, permissions);

        return Err(error.error);
    }

    fs::set_permissions(file, permissions)
}

#[cfg(not(windows))]
fn replace(temporary: NamedTempFile, file: &Path, permissions: fs::Permissions) -> io::Result<()> {
    fs::set_permissions(temporary.path(), permissions)?;
    temporary.persist(file)?;

    Ok(())
}

fn write_edit(edit: &FileEdit) -> io::Result<()> {
    let parent = edit.file.parent().unwrap_or_else(|| Path::new("."));
    let permissions = fs::metadata(&edit.file)?.permissions();
    let mut temporary = NamedTempFile::new_in(parent)?;

    temporary.write_all(edit.content.as_bytes())?;
    temporary.as_file().sync_all()?;

    replace(temporary, &edit.file, permissions)
}

pub fn write_edits(edits: &[FileEdit]) -> io::Result<()> {
    for edit in edits {
        write_edit(edit)?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "write_edits.integration.test.rs"]
mod integration;
