pub fn is_separator(character: char) -> bool {
    character == '/' || character == '\\'
}

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

pub fn has_interior_dot(text: &str) -> bool {
    text.char_indices()
        .any(|(index, character)| character == '.' && index > 0 && index + 1 < text.len())
}

#[cfg(test)]
#[path = "path_text.test.rs"]
mod tests;
