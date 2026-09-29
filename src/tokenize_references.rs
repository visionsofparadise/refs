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

const EMPHASIS_MARKERS: [&str; 3] = ["**", "*", "~~"];

const DELIMITING_ENTITIES: [&str; 5] = ["&quot;", "&apos;", "&#39;", "&lt;", "&gt;"];

fn delimiter_length_of(rest: &str) -> usize {
    match rest.chars().next() {
        Some(character) if is_delimiter(character) => character.len_utf8(),
        _ => DELIMITING_ENTITIES
            .iter()
            .find(|entity| rest.starts_with(*entity))
            .map_or(0, |entity| entity.len()),
    }
}

fn bracket_segment_length_of(content: &str, index: usize) -> Option<usize> {
    let close = match content[index..].chars().next()? {
        '(' => ')',
        '[' => ']',
        _ => return None,
    };

    if !content[..index].ends_with(is_separator) {
        return None;
    }

    for (offset, character) in content[index + 1..].char_indices() {
        let end = index + 1 + offset + character.len_utf8();

        if character == close {
            return content[end..]
                .starts_with(is_separator)
                .then_some(end - index);
        }

        if is_separator(character) || delimiter_length_of(&content[index + 1 + offset..]) > 0 {
            return None;
        }
    }

    None
}

fn is_text(character: Option<char>) -> bool {
    character.is_some_and(|character| !is_separator(character) && !matches!(character, '*' | '~'))
}

fn leading_emphasis_of(text: &str) -> usize {
    let marker = EMPHASIS_MARKERS
        .iter()
        .find(|marker| text.starts_with(*marker) && is_text(text[marker.len()..].chars().next()));

    if let Some(marker) = marker {
        return marker.len();
    }

    let paired_underscore = text.len() >= 2
        && text.starts_with('_')
        && !text.starts_with("__")
        && text.ends_with('_')
        && trailing_emphasis_of(text) == 1
        && is_text(text[1..].chars().next());

    usize::from(paired_underscore)
}

fn trailing_emphasis_of(text: &str) -> usize {
    let touches_text = |length: usize| is_text(text[..text.len() - length].chars().next_back());

    let marker = EMPHASIS_MARKERS
        .iter()
        .find(|marker| text.ends_with(*marker) && touches_text(marker.len()));

    if let Some(marker) = marker {
        return marker.len();
    }

    usize::from(text.ends_with('_') && !text.ends_with("__") && touches_text(1))
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

fn has_odd_trailing_backslashes(text: &str) -> bool {
    let backslashes = text.len() - text.trim_end_matches('\\').len();

    backslashes % 2 == 1
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
        let leading = leading_emphasis_of(text);
        let trailing = trailing_emphasis_of(text);
        let dots = trailing_dots_of(text);

        if leading > 0 {
            start += leading;
        } else if trailing > 0 {
            end -= trailing;
        } else if text.starts_with('@') {
            start += 1;
            at_prefixed = true;
        } else if text.starts_with('!') {
            start += 1;
        } else if let Some(mark) = ['!', '…', ':']
            .into_iter()
            .find(|mark| text.ends_with(*mark))
        {
            end -= mark.len_utf8();
        } else if has_odd_trailing_backslashes(text) {
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

fn is_batch_variable(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('%') else {
        return false;
    };

    if rest.starts_with('~') {
        return true;
    }

    let bytes = rest.as_bytes();
    let is_escape =
        bytes.len() >= 2 && bytes[0].is_ascii_hexdigit() && bytes[1].is_ascii_hexdigit();

    let name_length = rest
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .unwrap_or(rest.len());

    let is_named = bytes
        .first()
        .is_some_and(|first| first.is_ascii_alphabetic() || *first == b'_')
        && rest[name_length..].starts_with('%');

    is_named && !(is_escape && name_length == 2)
}

fn is_excluded(text: &str) -> bool {
    text.contains('*')
        || text.starts_with(['~', '$'])
        || is_batch_variable(text)
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

fn follows_an_expansion(content: &str, start: usize) -> bool {
    content[..start].ends_with(['}', ')'])
}

fn read_token(
    content: &str,
    start: usize,
    end: usize,
    line: usize,
    line_start: usize,
) -> Option<Token> {
    if follows_an_expansion(content, start) {
        return None;
    }

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
    let mut index = offset;

    while index < content.len() {
        if let Some(length) = bracket_segment_length_of(content, index) {
            run_start.get_or_insert(index);

            index += length;

            continue;
        }

        let rest = &content[index..];
        let delimiter = delimiter_length_of(rest);

        if delimiter == 0 {
            run_start.get_or_insert(index);

            index += rest.chars().next().map_or(1, char::len_utf8);

            continue;
        }

        if let Some(start) = run_start.take() {
            tokens.extend(read_token(content, start, index, line, line_start));
        }

        if rest.starts_with('\n') {
            line += 1;
            line_start = index + 1;
        }

        index += delimiter;
    }

    if let Some(start) = run_start {
        tokens.extend(read_token(content, start, content.len(), line, line_start));
    }

    tokens
}

#[cfg(test)]
#[path = "tokenize_references.test.rs"]
mod tests;
