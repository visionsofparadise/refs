use super::*;

const MOVED: Template = Template {
    literals: &["", " -> ", " umbenannt"],
    arguments: &[0, 1],
};
const REORDERED: Template = Template {
    literals: &["nach ", " von ", " verschoben"],
    arguments: &[1, 0],
};
const JOINED: Template = Template {
    literals: &["", "을(를) 제거했습니다"],
    arguments: &[0],
};
const RAW: Template = Template {
    literals: &["Benenne ", " nach ", " um"],
    arguments: &[0, 1],
};

fn strings_of(readings: &[&[&str]]) -> Vec<Vec<String>> {
    readings
        .iter()
        .map(|reading| reading.iter().map(|text| (*text).to_string()).collect())
        .collect()
}

#[test]
fn reads_shell_quoted_arguments_between_literals() {
    assert_eq!(
        match_template(b"'a b' -> 'c'\\''d' umbenannt", &MOVED, Reading::Shell),
        strings_of(&[&["a b", "c'd"]])
    );
    assert_eq!(
        match_template(b"'a -> b' -> $'c\\nd' umbenannt", &MOVED, Reading::Shell),
        strings_of(&[&["a -> b", "c\nd"]])
    );
}

#[test]
fn ends_a_quoted_argument_where_a_literal_follows_without_a_space() {
    assert_eq!(
        match_template(
            "'x.md'을(를) 제거했습니다".as_bytes(),
            &JOINED,
            Reading::Shell
        ),
        strings_of(&[&["x.md"]])
    );
}

#[test]
fn rejects_bare_words_in_the_shell_reading() {
    assert!(match_template(b"a -> 'b' umbenannt", &MOVED, Reading::Shell).is_empty());
    assert!(match_template(b"'a' -> 'b' umbenannt!", &MOVED, Reading::Shell).is_empty());
}

#[test]
fn returns_arguments_in_printf_order_for_a_reordering_template() {
    assert_eq!(
        match_template(
            b"nach 'neu' von 'alt' verschoben",
            &REORDERED,
            Reading::Shell
        ),
        strings_of(&[&["alt", "neu"]])
    );
    assert_eq!(
        match_template(b"nach neu von alt verschoben", &REORDERED, Reading::Raw),
        strings_of(&[&["alt", "neu"]])
    );
}

#[test]
fn returns_every_raw_split() {
    assert_eq!(
        match_template(b"Benenne a b.md nach c.md um", &RAW, Reading::Raw),
        strings_of(&[&["a b.md", "c.md"]])
    );
    assert_eq!(
        match_template(b"Benenne a nach b nach c um", &RAW, Reading::Raw),
        strings_of(&[&["a", "b nach c"], &["a nach b", "c"]])
    );
    assert!(match_template(b"Benenne  nach c um", &RAW, Reading::Raw).is_empty());
}
