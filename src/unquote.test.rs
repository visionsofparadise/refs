use super::*;

#[test]
fn reads_a_single_quoted_word_and_the_rest() {
    assert_eq!(
        unquote_shell("'a b' -> 'c'"),
        Some(("a b".to_string(), " -> 'c'"))
    );
}

#[test]
fn reads_a_bare_word_up_to_whitespace() {
    assert_eq!(
        unquote_shell("a/b.md rest"),
        Some(("a/b.md".to_string(), " rest"))
    );
}

#[test]
fn joins_an_escaped_quote_between_quoted_segments() {
    assert_eq!(unquote_shell("'it'\\''s'"), Some(("it's".to_string(), "")));
}

#[test]
fn joins_adjacent_quoted_and_bare_segments() {
    assert_eq!(unquote_shell("'a'b'c d'"), Some(("abc d".to_string(), "")));
}

#[test]
fn decodes_ansi_c_escapes() {
    assert_eq!(
        unquote_shell("$'x\\ny\\t\\\\\\'z'"),
        Some(("x\ny\t\\'z".to_string(), ""))
    );
}

#[test]
fn decodes_ansi_c_octal_bytes_as_utf8() {
    assert_eq!(
        unquote_shell("$'\\303\\251'"),
        Some(("\u{e9}".to_string(), ""))
    );
    assert_eq!(unquote_shell("$'\\101B'"), Some(("AB".to_string(), "")));
}

#[test]
fn keeps_a_dollar_that_opens_no_quote() {
    assert_eq!(unquote_shell("a$b"), Some(("a$b".to_string(), "")));
}

#[test]
fn reads_an_empty_quoted_word() {
    assert_eq!(unquote_shell("'' x"), Some((String::new(), " x")));
}

#[test]
fn rejects_unterminated_and_empty_input() {
    assert_eq!(unquote_shell("'abc"), None);
    assert_eq!(unquote_shell("$'abc"), None);
    assert_eq!(unquote_shell(""), None);
    assert_eq!(unquote_shell(" a"), None);
    assert_eq!(unquote_shell("$'\\q'"), None);
    assert_eq!(unquote_shell("$'\\400'"), None);
    assert_eq!(unquote_shell("$'\\377'"), None);
}

#[test]
fn decodes_a_git_quoted_path() {
    assert_eq!(
        unquote_git("\"a\\\"b\\\\c\\nd\\te\""),
        Some("a\"b\\c\nd\te".to_string())
    );
}

#[test]
fn decodes_git_octal_bytes_as_utf8() {
    assert_eq!(
        unquote_git("\"caf\\303\\251.md\""),
        Some("caf\u{e9}.md".to_string())
    );
}

#[test]
fn returns_a_bare_git_path_unchanged() {
    assert_eq!(unquote_git("src/a b.md"), Some("src/a b.md".to_string()));
}

#[test]
fn rejects_a_malformed_git_quote() {
    assert_eq!(unquote_git("\"abc"), None);
    assert_eq!(unquote_git("\"abc\"d"), None);
    assert_eq!(unquote_git("\"\\303\""), None);
    assert_eq!(unquote_git("\"\\q\""), None);
}
