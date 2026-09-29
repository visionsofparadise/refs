use std::fs;
use std::io::{self, Write};
use std::path::Path;
use tempfile::NamedTempFile;

use crate::fix_references::FileEdit;

#[cfg(windows)]
fn wide_of(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain([0]).collect()
}

#[cfg(windows)]
fn replace_file(replacement: &Path, file: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let replaced = wide_of(file);
    let replacement = wide_of(replacement);

    let succeeded = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };

    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(windows)]
fn replace(temporary: NamedTempFile, file: &Path, permissions: fs::Permissions) -> io::Result<()> {
    let temporary = temporary.into_temp_path();

    if !permissions.readonly() {
        return replace_file(&temporary, file);
    }

    let mut writable = permissions.clone();

    #[allow(clippy::permissions_set_readonly_false)]
    writable.set_readonly(false);

    fs::set_permissions(file, writable)?;

    if let Err(error) = replace_file(&temporary, file) {
        let _ = fs::set_permissions(file, permissions);

        return Err(error);
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
