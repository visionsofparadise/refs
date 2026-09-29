use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use std::path::{Component, Path};

use crate::resolve_reference::{key_of, resolve_reference, AbsoluteStyle, PathForm, PathStyle};
use crate::tokenize_references::tokenize_references;

const ENCODED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn name_of(component: Component) -> Option<String> {
    match component {
        Component::Normal(name) => name.to_str().map(str::to_string),
        _ => None,
    }
}

fn diff_names(target: &Path, base: &Path) -> Option<Vec<String>> {
    let target_key = key_of(target);
    let base_key = key_of(base);

    let common = target_key
        .iter()
        .zip(&base_key)
        .take_while(|(target_name, base_name)| target_name == base_name)
        .count();

    if common == 0 {
        return None;
    }

    let parents = std::iter::repeat_n("..".to_string(), base_key.len() - common);

    let names = target
        .components()
        .skip(common)
        .map(name_of)
        .collect::<Option<Vec<String>>>()?;

    let names: Vec<String> = parents.chain(names).collect();

    if names.is_empty() {
        return Some(vec![".".to_string()]);
    }

    Some(names)
}

fn split_relative(target: &Path, base: &Path, style: &PathStyle) -> Option<(String, Vec<String>)> {
    let names = diff_names(target, base)?;
    let bare = names[0] == ".." || names[0] == ".";

    let head = if style.dot_prefix && !bare {
        format!(".{}", style.separator)
    } else {
        String::new()
    };

    Some((head, names))
}

fn collect_names<'a>(components: impl Iterator<Item = Component<'a>>) -> Option<Vec<String>> {
    components
        .filter(|component| !matches!(component, Component::RootDir))
        .map(name_of)
        .collect()
}

#[cfg(windows)]
fn split_absolute(
    target: &Path,
    absolute: AbsoluteStyle,
    style: &PathStyle,
) -> Option<(String, Vec<String>)> {
    use std::path::Prefix;

    let mut components = target.components();

    let letter = match components.next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
            _ => return None,
        },
        _ => return None,
    };

    let letter = char::from(if style.lowercase_drive {
        letter.to_ascii_lowercase()
    } else {
        letter.to_ascii_uppercase()
    });

    let separator = style.separator;

    let head = match absolute {
        AbsoluteStyle::Drive if style.percent_encoded && style.encoded_drive_colon => {
            format!("{letter}%3A{separator}")
        }
        AbsoluteStyle::Drive => format!("{letter}:{separator}"),
        AbsoluteStyle::Msys => format!("/{letter}{separator}"),
    };

    Some((head, collect_names(components)?))
}

#[cfg(not(windows))]
fn split_absolute(
    target: &Path,
    absolute: AbsoluteStyle,
    style: &PathStyle,
) -> Option<(String, Vec<String>)> {
    match absolute {
        AbsoluteStyle::Posix => Some((
            style.separator.to_string(),
            collect_names(target.components())?,
        )),
    }
}

fn lowercase_escapes(text: &str) -> String {
    let mut lowered = String::with_capacity(text.len());
    let mut pending = 0;

    for character in text.chars() {
        if pending > 0 {
            lowered.push(character.to_ascii_lowercase());

            pending -= 1;
        } else {
            if character == '%' {
                pending = 2;
            }

            lowered.push(character);
        }
    }

    lowered
}

fn is_relative(form: &PathForm) -> bool {
    matches!(
        form,
        PathForm::FileRelative | PathForm::WorkingDirectoryRelative
    )
}

fn round_trips(
    rendered: &str,
    suffix: &str,
    target: &Path,
    form: &PathForm,
    referrer_directory: &Path,
    working_directory: &Path,
) -> bool {
    let text = format!("{rendered}{suffix}");
    let tokens = tokenize_references(&text);

    let [token] = tokens.as_slice() else {
        return false;
    };

    if token.start != 0
        || token.end != text.len()
        || token.path != rendered
        || token.suffix != suffix
    {
        return false;
    }

    let target_key = key_of(target);
    let shared_base = key_of(referrer_directory) == key_of(working_directory);

    for candidate in resolve_reference(rendered, referrer_directory, working_directory).0 {
        let same_form = candidate.form == *form
            || (shared_base && is_relative(form) && is_relative(&candidate.form));

        if same_form && key_of(&candidate.target) == target_key {
            return true;
        }

        if candidate.target.exists() {
            return false;
        }
    }

    false
}

pub fn render_reference(
    target: &Path,
    form: &PathForm,
    style: &PathStyle,
    suffix: &str,
    referrer_directory: &Path,
    working_directory: &Path,
) -> Option<String> {
    let (head, names) = match form {
        PathForm::FileRelative => split_relative(target, referrer_directory, style)?,
        PathForm::WorkingDirectoryRelative => split_relative(target, working_directory, style)?,
        PathForm::Absolute(absolute) => split_absolute(target, *absolute, style)?,
    };

    let mut names: Vec<String> = if style.percent_encoded {
        names
            .iter()
            .map(|name| utf8_percent_encode(name, ENCODED).to_string())
            .collect()
    } else {
        names
    };

    if style.percent_encoded && head.is_empty() {
        if let Some(rest) = names.first().and_then(|name| name.strip_prefix('~')) {
            names[0] = format!("%7E{rest}");
        }
    }

    let mut rendered = head + &names.join(&style.separator.to_string());

    if style.percent_encoded && style.lowercase_hex {
        rendered = lowercase_escapes(&rendered);
    }

    if style.trailing_separator && !rendered.ends_with(style.separator) {
        rendered.push(style.separator);
    }

    if style.doubled_backslashes {
        rendered = rendered.replace('\\', "\\\\");
    }

    if style.file_scheme {
        let scheme = if style.scheme.is_empty() {
            "file"
        } else {
            &style.scheme
        };
        let slashes = if rendered.starts_with('/') {
            "//"
        } else {
            "///"
        };

        rendered = format!("{scheme}:{slashes}{rendered}");
    }

    if !round_trips(
        &rendered,
        suffix,
        target,
        form,
        referrer_directory,
        working_directory,
    ) {
        return None;
    }

    Some(rendered)
}

#[cfg(test)]
#[path = "render_reference.test.rs"]
mod tests;
