use percent_encoding::percent_decode_str;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR_STR};

use crate::path_text::{has_interior_dot, is_separator};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsoluteStyle {
    #[cfg(windows)]
    Drive,
    #[cfg(windows)]
    Msys,
    #[cfg(not(windows))]
    Posix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathForm {
    FileRelative,
    WorkingDirectoryRelative,
    Absolute(AbsoluteStyle),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathStyle {
    pub separator: char,
    pub doubled_backslashes: bool,
    pub escaped_slashes: bool,
    pub dot_prefix: bool,
    pub parent_prefix: bool,
    pub trailing_separator: bool,
    pub percent_encoded: bool,
    pub file_scheme: bool,
    pub scheme: String,
    pub lowercase_drive: bool,
    pub encoded_drive_colon: bool,
    pub lowercase_hex: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub target: PathBuf,
    pub form: PathForm,
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut root = OsString::new();
    let mut rooted = false;
    let mut names: Vec<&OsStr> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => root.push(prefix.as_os_str()),
            Component::RootDir => {
                root.push(MAIN_SEPARATOR_STR);

                rooted = true;
            }
            Component::CurDir => {}
            Component::ParentDir => match names.last() {
                Some(name) if *name != ".." => {
                    names.pop();
                }
                None if rooted => {}
                _ => names.push(component.as_os_str()),
            },
            Component::Normal(name) => names.push(name),
        }
    }

    if root.is_empty() && names.is_empty() {
        return PathBuf::from(".");
    }

    let mut normalized = root;

    let reads_as_prefix = cfg!(windows)
        && normalized.is_empty()
        && names
            .first()
            .is_some_and(|name| name.to_string_lossy().contains(':'));

    if reads_as_prefix {
        normalized.push(".");
        normalized.push(MAIN_SEPARATOR_STR);
    }

    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            normalized.push(MAIN_SEPARATOR_STR);
        }

        normalized.push(name);
    }

    PathBuf::from(normalized)
}

#[cfg(any(windows, target_os = "macos"))]
fn fold_case(name: &str) -> String {
    name.to_lowercase()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn fold_case(name: &str) -> String {
    name.to_string()
}

pub fn key_of(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| fold_case(&component.as_os_str().to_string_lossy()))
        .collect()
}

pub fn is_beneath(path: &Path, base: &Path) -> bool {
    key_of(path).starts_with(&key_of(base))
}

fn join_relative(base: &Path, relative: &str) -> PathBuf {
    let mut joined = base.as_os_str().to_owned();

    for segment in relative
        .split(is_separator)
        .filter(|segment| !segment.is_empty())
    {
        joined.push(MAIN_SEPARATOR_STR);
        joined.push(segment);
    }

    normalize_path(Path::new(&joined))
}

#[cfg(windows)]
pub fn parse_absolute(path: &str) -> Option<(PathBuf, AbsoluteStyle)> {
    let bytes = path.as_bytes();

    if bytes.len() < 3 || !is_separator(char::from(bytes[2])) {
        return None;
    }

    let drive = |letter: u8| {
        let root = format!("{}:\\", char::from(letter.to_ascii_uppercase()));

        join_relative(Path::new(&root), &path[2..])
    };

    match (bytes[0], bytes[1]) {
        (letter, b':') if letter.is_ascii_alphabetic() => {
            Some((drive(letter), AbsoluteStyle::Drive))
        }
        (b'/', letter) if letter.is_ascii_alphabetic() => {
            Some((drive(letter), AbsoluteStyle::Msys))
        }
        _ => None,
    }
}

#[cfg(not(windows))]
pub fn parse_absolute(path: &str) -> Option<(PathBuf, AbsoluteStyle)> {
    if !path.starts_with(is_separator) {
        return None;
    }

    Some((join_relative(Path::new("/"), path), AbsoluteStyle::Posix))
}

pub fn locate_path(path: &str, base_directory: &Path) -> Option<PathBuf> {
    if has_foreign_colon(path) {
        return None;
    }

    if let Some((target, _)) = parse_absolute(path) {
        return Some(target);
    }

    if path.starts_with(is_separator) || path.contains(':') {
        return None;
    }

    Some(join_relative(base_directory, path))
}

pub fn is_dangling_shaped(path: &str) -> bool {
    match path.rfind(is_separator) {
        Some(index) => has_interior_dot(&path[index + 1..]),
        None => false,
    }
}

fn split_file_scheme(path: &str) -> (Option<&str>, &str) {
    match path.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("file://") => (Some(&path[..4]), &path[7..]),
        _ => (None, path),
    }
}

fn strip_drive_slash(path: &str) -> &str {
    let bytes = path.as_bytes();

    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return &path[1..];
    }

    path
}

fn escapes_of(path: &str) -> impl Iterator<Item = &[u8]> {
    path.as_bytes().windows(3).filter(|window| {
        window[0] == b'%' && window[1].is_ascii_hexdigit() && window[2].is_ascii_hexdigit()
    })
}

fn has_encoded_separator(path: &str) -> bool {
    escapes_of(path).any(|escape| {
        let code = escape[1..].to_ascii_uppercase();

        code == b"2F" || code == b"5C"
    })
}

fn has_lowercase_hex(path: &str) -> bool {
    escapes_of(path)
        .flat_map(|escape| [escape[1], escape[2]])
        .find(u8::is_ascii_alphabetic)
        .is_some_and(|digit| digit.is_ascii_lowercase())
}

fn drive_letter_of(path: &str) -> Option<u8> {
    let bytes = path.as_bytes();

    match bytes {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => Some(*letter),
        [b'/', letter, separator, ..]
            if letter.is_ascii_alphabetic() && is_separator(char::from(*separator)) =>
        {
            Some(*letter)
        }
        _ => None,
    }
}

fn has_encoded_drive_colon(path: &str, file_scheme: bool) -> bool {
    let path = if file_scheme {
        path.strip_prefix('/').unwrap_or(path)
    } else {
        path
    };

    path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && path
            .get(1..4)
            .is_some_and(|colon| colon.eq_ignore_ascii_case("%3A"))
}

fn root_length_of(path: &str) -> usize {
    let bytes = path.as_bytes();

    let separator_at = |index: usize| {
        bytes
            .get(index)
            .is_some_and(|byte| is_separator(char::from(*byte)))
    };

    if cfg!(windows) && drive_letter_of(path).is_some() && separator_at(2) {
        return 3;
    }

    usize::from(separator_at(0))
}

fn separator_of(path: &str) -> char {
    let (root, rest) = path.split_at(root_length_of(path));

    rest.chars()
        .find(|character| is_separator(*character))
        .or_else(|| {
            root.chars()
                .rev()
                .find(|character| is_separator(*character))
        })
        .unwrap_or('/')
}

fn has_foreign_colon(path: &str) -> bool {
    path.match_indices(':')
        .any(|(index, _)| !cfg!(windows) || index != 1)
}

fn build_candidates(
    path: &str,
    file_scheme: bool,
    referrer_directory: &Path,
    working_directory: &Path,
) -> Vec<Candidate> {
    if has_foreign_colon(path) {
        return Vec::new();
    }

    if let Some((target, style)) = parse_absolute(path) {
        #[cfg(windows)]
        if file_scheme && style == AbsoluteStyle::Msys {
            return Vec::new();
        }

        return vec![Candidate {
            target,
            form: PathForm::Absolute(style),
        }];
    }

    if file_scheme || path.starts_with(is_separator) || path.contains(':') {
        return Vec::new();
    }

    let file_relative = Candidate {
        target: join_relative(referrer_directory, path),
        form: PathForm::FileRelative,
    };

    if key_of(referrer_directory) == key_of(working_directory) {
        return vec![file_relative];
    }

    vec![
        file_relative,
        Candidate {
            target: join_relative(working_directory, path),
            form: PathForm::WorkingDirectoryRelative,
        },
    ]
}

pub fn resolve_reference(
    path: &str,
    referrer_directory: &Path,
    working_directory: &Path,
) -> (Vec<Candidate>, PathStyle) {
    let (scheme, path) = split_file_scheme(path);
    let file_scheme = scheme.is_some();
    let escaped_slashes = path.contains("\\/");
    let path = path.replace("\\/", "/");
    let doubled_backslashes = path.contains("\\\\");

    let undoubled = if doubled_backslashes {
        path.replace("\\\\", "\\")
    } else {
        path
    };

    let decoded = if escapes_of(&undoubled).next().is_some() {
        percent_decode_str(&undoubled)
            .decode_utf8()
            .ok()
            .map(|decoded| decoded.into_owned())
    } else {
        None
    };

    let percent_encoded = decoded.is_some();
    let decoded = decoded.unwrap_or_else(|| undoubled.clone());

    let path = if file_scheme {
        strip_drive_slash(&decoded)
    } else {
        &decoded
    };

    let style = PathStyle {
        separator: separator_of(path),
        doubled_backslashes,
        escaped_slashes,
        dot_prefix: path.starts_with("./") || path.starts_with(".\\"),
        parent_prefix: path.starts_with("../") || path.starts_with("..\\"),
        trailing_separator: path.ends_with(is_separator),
        percent_encoded,
        file_scheme,
        scheme: scheme.unwrap_or_default().to_string(),
        lowercase_drive: drive_letter_of(path).is_some_and(|letter| letter.is_ascii_lowercase()),
        encoded_drive_colon: percent_encoded && has_encoded_drive_colon(&undoubled, file_scheme),
        lowercase_hex: percent_encoded && has_lowercase_hex(&undoubled),
    };

    let candidates = if percent_encoded && has_encoded_separator(&undoubled) {
        Vec::new()
    } else {
        build_candidates(path, file_scheme, referrer_directory, working_directory)
    };

    (candidates, style)
}

#[cfg(test)]
#[path = "resolve_reference.test.rs"]
mod tests;
