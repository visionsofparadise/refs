use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use std::path::{Component, Path};

use crate::resolve_reference::{key_of, AbsoluteStyle, PathForm, PathStyle};
use crate::tokenize_references::is_delimiter;

const ENCODED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn is_unrewritable(character: char) -> bool {
    is_delimiter(character) || matches!(character, '#' | '?' | '*')
}

fn name_of(component: Component) -> Option<String> {
    match component {
        Component::Normal(name) => name.to_str().map(str::to_string),
        Component::ParentDir => Some("..".to_string()),
        _ => None,
    }
}

fn relative_names(target: &Path, base: &Path) -> Option<Vec<String>> {
    if key_of(target).first() != key_of(base).first() {
        return None;
    }

    let names = pathdiff::diff_paths(target, base)?
        .components()
        .map(name_of)
        .collect::<Option<Vec<String>>>()?;

    if names.is_empty() {
        return Some(vec![".".to_string()]);
    }

    Some(names)
}

fn relative_parts(target: &Path, base: &Path, style: &PathStyle) -> Option<(String, Vec<String>)> {
    let names = relative_names(target, base)?;
    let bare = names[0] == ".." || names[0] == ".";

    let head = if style.dot_prefix && !bare {
        format!(".{}", style.separator)
    } else {
        String::new()
    };

    Some((head, names))
}

fn absolute_names<'a>(components: impl Iterator<Item = Component<'a>>) -> Option<Vec<String>> {
    components
        .filter(|component| !matches!(component, Component::RootDir))
        .map(name_of)
        .collect()
}

#[cfg(windows)]
fn absolute_parts(
    target: &Path,
    absolute: AbsoluteStyle,
    separator: char,
) -> Option<(String, Vec<String>)> {
    use std::path::Prefix;

    let mut components = target.components();

    let letter = match components.next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => char::from(letter),
            _ => return None,
        },
        _ => return None,
    };

    let head = match absolute {
        AbsoluteStyle::Drive => format!("{letter}:{separator}"),
        AbsoluteStyle::Msys => format!("/{}{separator}", letter.to_ascii_lowercase()),
    };

    Some((head, absolute_names(components)?))
}

#[cfg(not(windows))]
fn absolute_parts(
    target: &Path,
    absolute: AbsoluteStyle,
    separator: char,
) -> Option<(String, Vec<String>)> {
    match absolute {
        AbsoluteStyle::Posix => Some((separator.to_string(), absolute_names(target.components())?)),
    }
}

pub fn render_reference(
    target: &Path,
    form: &PathForm,
    style: &PathStyle,
    referrer_directory: &Path,
    working_directory: &Path,
) -> Option<String> {
    let (head, names) = match form {
        PathForm::FileRelative => relative_parts(target, referrer_directory, style)?,
        PathForm::WorkingDirectoryRelative => relative_parts(target, working_directory, style)?,
        PathForm::Absolute(absolute) => absolute_parts(target, *absolute, style.separator)?,
    };

    let names: Vec<String> = if style.percent_encoded {
        names
            .iter()
            .map(|name| utf8_percent_encode(name, ENCODED).to_string())
            .collect()
    } else {
        names
    };

    let mut rendered = head + &names.join(&style.separator.to_string());

    if style.trailing_separator && !rendered.ends_with(style.separator) {
        rendered.push(style.separator);
    }

    if !style.percent_encoded && rendered.contains(is_unrewritable) {
        return None;
    }

    if style.doubled_backslashes {
        rendered = rendered.replace('\\', "\\\\");
    }

    if style.file_scheme {
        let slashes = if rendered.starts_with('/') {
            "//"
        } else {
            "///"
        };

        rendered = format!("file:{slashes}{rendered}");
    }

    Some(rendered)
}

#[cfg(test)]
#[path = "render_reference.test.rs"]
mod tests;
