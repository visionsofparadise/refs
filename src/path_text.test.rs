use super::*;

#[test]
fn treats_both_slashes_as_separators() {
    assert!(is_separator('/'));
    assert!(is_separator('\\'));
    assert!(!is_separator(':'));
}

#[test]
fn delimits_on_whitespace_quotes_brackets_and_punctuation() {
    for character in [
        ' ', '\t', '\r', '"', '\'', '`', '(', ')', '[', ']', '<', '>', '{', '}', ',', ';', '|', '=',
    ] {
        assert!(is_delimiter(character), "{character:?}");
    }

    for character in ['.', ':', '#', '?', '@', '*', '~', '%', '/', '\\'] {
        assert!(!is_delimiter(character), "{character:?}");
    }
}

#[test]
fn finds_a_dot_only_between_other_characters() {
    assert!(has_interior_dot("a.md"));
    assert!(has_interior_dot("..."));
    assert!(!has_interior_dot(".gitignore"));
    assert!(!has_interior_dot("a."));
    assert!(!has_interior_dot("LICENSE"));
}
