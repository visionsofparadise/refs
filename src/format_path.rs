use std::path::Path;

use crate::resolve_reference::{is_beneath, key_of};

pub fn format_path(path: &Path, working_directory: &Path) -> String {
    if !is_beneath(path, working_directory) {
        return path.display().to_string();
    }

    let names: Vec<String> = path
        .components()
        .skip(key_of(working_directory).len())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    if names.is_empty() {
        return ".".to_string();
    }

    names.join("/")
}

#[cfg(test)]
#[path = "format_path.test.rs"]
mod tests;
