use crate::resolve_reference::{has_interior_dot, is_separator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub path: String,
    pub suffix: String,
}

const BYTE_ORDER_MARK: char = '\u{feff}';

const EMPHASIS_MARKERS: [&str; 3] = ["**", "__", "*"];

pub fn is_delimiter(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\''
                | '`'
                | '('
                | ')'
                | '['
                | ']'
                | '<'
                | '>'
                | '{'
                | '}'
                | ','
                | ';'
                | '|'
                | '='
        )
}

fn trim(content: &str, mut start: usize, mut end: usize) -> (usize, usize) {
    loop {
        let text = &content[start..end];

        let emphasis = EMPHASIS_MARKERS.iter().find(|marker| {
            text.len() >= 2 * marker.len() && text.starts_with(*marker) && text.ends_with(*marker)
        });

        if let Some(marker) = emphasis {
            start += marker.len();
            end -= marker.len();
        } else if text.starts_with('@') {
            start += 1;
        } else if text.ends_with(['.', ':', ',']) {
            end -= 1;
        } else {
            return (start, end);
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

fn is_excluded(text: &str) -> bool {
    text.contains('*')
        || text.starts_with('~')
        || is_git_coordinate(text)
        || has_foreign_scheme(text)
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

fn is_path_shaped(path: &str) -> bool {
    path.contains(is_separator) || has_interior_dot(path)
}

fn read_token(
    content: &str,
    start: usize,
    end: usize,
    line: usize,
    line_start: usize,
) -> Option<Token> {
    let (start, end) = trim(content, start, end);
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
