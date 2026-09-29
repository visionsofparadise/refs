use crate::path_text::{has_interior_dot, is_delimiter, is_separator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub path: String,
    pub suffix: String,
    pub at_prefixed: bool,
}

const BYTE_ORDER_MARK: char = '\u{feff}';

const EMPHASIS_MARKERS: [&str; 2] = ["**", "*"];

fn emphasis_length_of(text: &str) -> Option<usize> {
    let marker = EMPHASIS_MARKERS.iter().find(|marker| {
        text.len() >= 2 * marker.len() && text.starts_with(*marker) && text.ends_with(*marker)
    });

    if let Some(marker) = marker {
        return Some(marker.len());
    }

    let single_underscore = text.len() >= 2
        && text.starts_with('_')
        && text.ends_with('_')
        && !text.starts_with("__")
        && !text.ends_with("__");

    single_underscore.then_some(1)
}

fn trailing_dots_of(text: &str) -> usize {
    let dots = text.len() - text.trim_end_matches('.').len();
    let last_segment = text.rsplit(is_separator).next().unwrap_or(text);
    let ellipsis = last_segment.len() >= 3 && last_segment.len() == dots;

    if last_segment == ".." || ellipsis {
        return 0;
    }

    dots
}

fn ends_with_lone_backslash(text: &str) -> bool {
    text.ends_with('\\') && !text.ends_with("\\\\")
}

struct Trimmed {
    start: usize,
    end: usize,
    at_prefixed: bool,
}

fn trim(content: &str, mut start: usize, mut end: usize) -> Trimmed {
    let mut at_prefixed = false;

    loop {
        let text = &content[start..end];
        let dots = trailing_dots_of(text);

        if let Some(length) = emphasis_length_of(text) {
            start += length;
            end -= length;
        } else if text.starts_with('@') {
            start += 1;
            at_prefixed = true;
        } else if text.ends_with([':', ',']) || ends_with_lone_backslash(text) {
            end -= 1;
        } else if dots > 0 {
            end -= dots;
        } else {
            return Trimmed {
                start,
                end,
                at_prefixed,
            };
        }
    }
}

fn is_scheme(text: &str) -> bool {
    let mut characters = text.chars();

    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && text.len() >= 2
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || character == '+' || character == '-'
        })
}

fn is_local_file_url(text: &str) -> bool {
    text.get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"))
        && text[7..].starts_with('/')
}

fn has_foreign_scheme(text: &str) -> bool {
    match text.split_once(':') {
        Some((scheme, _)) if is_scheme(scheme) => {
            !scheme.eq_ignore_ascii_case("file") || !is_local_file_url(text)
        }
        _ => false,
    }
}

fn is_git_coordinate(text: &str) -> bool {
    match text.split_once(':') {
        Some((object, _)) => {
            (4..=40).contains(&object.len())
                && object
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
        }
        None => false,
    }
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_line_suffix(text: &str) -> bool {
    match text.split_once(':') {
        Some((line, column)) => is_number(line) && is_number(column),
        None => is_number(text),
    }
}

fn is_remote(text: &str) -> bool {
    text.split_once(':').is_some_and(|(head, tail)| {
        head.contains('@') && !head.contains(is_separator) && !is_line_suffix(tail)
    })
}

fn is_excluded(text: &str) -> bool {
    text.contains('*')
        || text.starts_with('~')
        || is_git_coordinate(text)
        || has_foreign_scheme(text)
        || is_remote(text)
}

fn find_line_suffix(text: &str) -> Option<usize> {
    let digits = text.len()
        - text
            .trim_end_matches(|character: char| character.is_ascii_digit())
            .len();

    if digits == 0 || !text[..text.len() - digits].ends_with(':') {
        return None;
    }

    Some(text.len() - digits - 1)
}

fn find_suffix(text: &str) -> usize {
    if let Some(index) = text.find(['#', '?']) {
        return index;
    }

    match find_line_suffix(text) {
        Some(line) => find_line_suffix(&text[..line]).unwrap_or(line),
        None => text.len(),
    }
}

fn has_dotted_segment(path: &str) -> bool {
    path.split(is_separator)
        .any(|segment| segment.ends_with('.') && segment != "." && segment != "..")
}

fn is_path_shaped(path: &str) -> bool {
    let has_separator = path.contains(is_separator);
    let has_name = path.contains(|character: char| !is_separator(character));

    ((has_separator && has_name) || has_interior_dot(path)) && !has_dotted_segment(path)
}

fn read_token(
    content: &str,
    start: usize,
    end: usize,
    line: usize,
    line_start: usize,
) -> Option<Token> {
    let Trimmed {
        start,
        end,
        at_prefixed,
    } = trim(content, start, end);

    let text = &content[start..end];

    if text.is_empty() || is_excluded(text) {
        return None;
    }

    let (path, suffix) = text.split_at(find_suffix(text));

    if path.is_empty() || !is_path_shaped(path) {
        return None;
    }

    Some(Token {
        start,
        end,
        line,
        column: start - line_start + 1,
        path: path.to_string(),
        suffix: suffix.to_string(),
        at_prefixed,
    })
}

pub fn tokenize_references(content: &str) -> Vec<Token> {
    let offset = if content.starts_with(BYTE_ORDER_MARK) {
        BYTE_ORDER_MARK.len_utf8()
    } else {
        0
    };

    let mut tokens = Vec::new();
    let mut line = 1;
    let mut line_start = offset;
    let mut run_start: Option<usize> = None;

    for (index, character) in content[offset..].char_indices() {
        let index = index + offset;

        if !is_delimiter(character) {
            run_start.get_or_insert(index);

            continue;
        }

        if let Some(start) = run_start.take() {
            tokens.extend(read_token(content, start, index, line, line_start));
        }

        if character == '\n' {
            line += 1;
            line_start = index + 1;
        }
    }

    if let Some(start) = run_start {
        tokens.extend(read_token(content, start, content.len(), line, line_start));
    }

    tokens
}

#[cfg(test)]
#[path = "tokenize_references.test.rs"]
mod tests;
