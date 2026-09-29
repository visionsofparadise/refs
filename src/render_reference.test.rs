use super::*;
use std::path::PathBuf;

const ABSENT_ROOT: &str = "refs-absent-root";

fn absolute_path_of(names: &[&str]) -> PathBuf {
    let mut path = if cfg!(windows) {
        PathBuf::from("C:\\")
    } else {
        PathBuf::from("/")
    };

    path.push(ABSENT_ROOT);

    for name in names {
        path.push(name);
    }

    path
}

fn create_plain_style() -> PathStyle {
    PathStyle {
        separator: '/',
        doubled_backslashes: false,
        escaped_slashes: false,
        dot_prefix: false,
        trailing_separator: false,
        percent_encoded: false,
        file_scheme: false,
        scheme: String::new(),
        lowercase_drive: false,
        encoded_drive_colon: false,
        lowercase_hex: false,
    }
}

fn render_with_suffix(
    target: &[&str],
    form: PathForm,
    style: PathStyle,
    suffix: &str,
) -> Option<String> {
    render_reference(
        &absolute_path_of(target),
        &form,
        &style,
        suffix,
        &absolute_path_of(&["work", "notes"]),
        &absolute_path_of(&["work"]),
        &|_| false,
    )
}

fn render(target: &[&str], form: PathForm, style: PathStyle) -> Option<String> {
    render_with_suffix(target, form, style, "")
}

fn render_file_relative(target: &[&str], style: PathStyle) -> Option<String> {
    render(target, PathForm::FileRelative, style)
}

#[test]
fn renders_a_file_relative_path_across_a_depth_change() {
    assert_eq!(
        render_file_relative(&["work", "docs", "deep", "a.md"], create_plain_style()).as_deref(),
        Some("../docs/deep/a.md")
    );
}

#[test]
fn renders_a_working_directory_relative_path_across_a_depth_change() {
    assert_eq!(
        render(
            &["work", "docs", "deep", "a.md"],
            PathForm::WorkingDirectoryRelative,
            create_plain_style()
        )
        .as_deref(),
        Some("docs/deep/a.md")
    );
}

#[test]
fn renders_with_a_suffix_that_round_trips() {
    assert_eq!(
        render_with_suffix(
            &["work", "docs", "a.md"],
            PathForm::FileRelative,
            create_plain_style(),
            "#h"
        )
        .as_deref(),
        Some("../docs/a.md")
    );
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn compares_relative_components_through_the_key() {
    assert_eq!(
        render_file_relative(&["WORK", "NOTES", "a.md"], create_plain_style()).as_deref(),
        Some("a.md")
    );
}

#[test]
fn renders_with_the_recorded_separator() {
    let style = PathStyle {
        separator: '\\',
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "docs", "a.md"], style).as_deref(),
        Some("..\\docs\\a.md")
    );
}

#[test]
fn renders_doubled_backslashes() {
    let style = PathStyle {
        separator: '\\',
        doubled_backslashes: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "docs", "a.md"], style).as_deref(),
        Some("..\\\\docs\\\\a.md")
    );
}

#[test]
fn renders_a_dot_prefix_except_before_a_parent_segment() {
    let style = PathStyle {
        dot_prefix: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "notes", "sub", "a.md"], style.clone()).as_deref(),
        Some("./sub/a.md")
    );
    assert_eq!(
        render_file_relative(&["work", "docs", "a.md"], style).as_deref(),
        Some("../docs/a.md")
    );
}

#[test]
fn renders_a_trailing_separator() {
    let style = PathStyle {
        trailing_separator: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "docs"], style).as_deref(),
        Some("../docs/")
    );
}

#[test]
fn renders_percent_encoding() {
    let style = PathStyle {
        percent_encoded: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "docs", "my file (1).md"], style).as_deref(),
        Some("../docs/my%20file%20%281%29.md")
    );
}

#[test]
fn renders_non_ascii_percent_encoding() {
    let style = PathStyle {
        percent_encoded: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "notes", "été.md"], style).as_deref(),
        Some("%C3%A9t%C3%A9.md")
    );
}

#[test]
fn reports_a_rendering_that_splits_into_other_tokens_as_unrewritable() {
    assert_eq!(
        render_file_relative(&["work", "my docs", "a.md"], create_plain_style()),
        None
    );
    assert_eq!(
        render_file_relative(&["work", "docs", "a(1).md"], create_plain_style()),
        None
    );
}

#[test]
fn reports_a_rendering_that_tokenizes_differently_as_unrewritable() {
    let cases: [&[&str]; 6] = [
        &["work", "notes", "@types", "x.d.ts"],
        &["work", "notes", "x%20y.md"],
        &["work", "notes", "sub", "notes:12"],
        &["work", "notes", "~old", "a.md"],
        &["work", "notes", "cafe:x", "a.md"],
        &["work", "notes", "a.md."],
    ];

    for target in cases {
        assert_eq!(
            render_file_relative(target, create_plain_style()),
            None,
            "{target:?}"
        );
    }
}

#[test]
fn reports_a_bare_dot_or_parent_as_unrewritable() {
    assert_eq!(
        render_file_relative(&["work", "notes"], create_plain_style()),
        None
    );
    assert_eq!(render_file_relative(&["work"], create_plain_style()), None);
}

#[cfg(windows)]
#[test]
fn renders_drive_and_msys_absolute_paths_in_their_written_case() {
    let target = &["x", "deep", "a.md"];

    let backslashed = PathStyle {
        separator: '\\',
        ..create_plain_style()
    };

    let lowercase = PathStyle {
        lowercase_drive: true,
        ..create_plain_style()
    };

    assert_eq!(
        render(
            target,
            PathForm::Absolute(AbsoluteStyle::Drive),
            backslashed
        )
        .as_deref(),
        Some("C:\\refs-absent-root\\x\\deep\\a.md")
    );
    assert_eq!(
        render(
            target,
            PathForm::Absolute(AbsoluteStyle::Drive),
            lowercase.clone()
        )
        .as_deref(),
        Some("c:/refs-absent-root/x/deep/a.md")
    );
    assert_eq!(
        render(target, PathForm::Absolute(AbsoluteStyle::Msys), lowercase).as_deref(),
        Some("/c/refs-absent-root/x/deep/a.md")
    );
    assert_eq!(
        render(
            target,
            PathForm::Absolute(AbsoluteStyle::Msys),
            create_plain_style()
        )
        .as_deref(),
        Some("/C/refs-absent-root/x/deep/a.md")
    );
}

#[cfg(windows)]
#[test]
fn renders_a_drive_file_url() {
    let style = PathStyle {
        file_scheme: true,
        scheme: "FILE".to_string(),
        ..create_plain_style()
    };

    assert_eq!(
        render(
            &["x", "a.md"],
            PathForm::Absolute(AbsoluteStyle::Drive),
            style
        )
        .as_deref(),
        Some("FILE:///C:/refs-absent-root/x/a.md")
    );
}

#[cfg(windows)]
#[test]
fn renders_a_percent_encoded_drive_file_url_keeping_the_drive_colon() {
    let style = PathStyle {
        percent_encoded: true,
        file_scheme: true,
        scheme: "file".to_string(),
        ..create_plain_style()
    };

    assert_eq!(
        render(
            &["my dir", "a.md"],
            PathForm::Absolute(AbsoluteStyle::Drive),
            style
        )
        .as_deref(),
        Some("file:///C:/refs-absent-root/my%20dir/a.md")
    );
}

#[cfg(windows)]
#[test]
fn renders_an_encoded_drive_colon_as_written() {
    let style = PathStyle {
        percent_encoded: true,
        file_scheme: true,
        scheme: "file".to_string(),
        lowercase_drive: true,
        encoded_drive_colon: true,
        ..create_plain_style()
    };

    assert_eq!(
        render(
            &["Users", "a.md"],
            PathForm::Absolute(AbsoluteStyle::Drive),
            style
        )
        .as_deref(),
        Some("file:///c%3A/refs-absent-root/Users/a.md")
    );
}

#[cfg(windows)]
#[test]
fn reports_a_relative_rendering_across_drives_as_unrewritable() {
    assert_eq!(
        render_reference(
            Path::new("D:\\a.md"),
            &PathForm::FileRelative,
            &create_plain_style(),
            "",
            Path::new("C:\\refs-absent-root"),
            Path::new("C:\\refs-absent-root"),
            &|_| false,
        ),
        None
    );
}

#[cfg(not(windows))]
#[test]
fn renders_a_posix_absolute_path() {
    assert_eq!(
        render(
            &["x", "deep", "a.md"],
            PathForm::Absolute(AbsoluteStyle::Posix),
            create_plain_style()
        )
        .as_deref(),
        Some("/refs-absent-root/x/deep/a.md")
    );
}

#[cfg(not(windows))]
#[test]
fn renders_a_posix_file_url() {
    let style = PathStyle {
        file_scheme: true,
        scheme: "file".to_string(),
        ..create_plain_style()
    };

    assert_eq!(
        render(
            &["x", "a.md"],
            PathForm::Absolute(AbsoluteStyle::Posix),
            style
        )
        .as_deref(),
        Some("file:///refs-absent-root/x/a.md")
    );
}

#[test]
fn renders_a_working_directory_form_when_the_referrer_is_the_working_directory() {
    let work = absolute_path_of(&["work"]);

    assert_eq!(
        render_reference(
            &absolute_path_of(&["work", "docs", "a.md"]),
            &PathForm::WorkingDirectoryRelative,
            &create_plain_style(),
            "",
            &work,
            &work,
            &|_| false,
        )
        .as_deref(),
        Some("docs/a.md")
    );
}

#[test]
fn renders_the_base_itself_with_a_trailing_separator() {
    let style = PathStyle {
        trailing_separator: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "notes"], style).as_deref(),
        Some("./")
    );
}

#[test]
fn renders_percent_encoding_in_the_recorded_hex_case() {
    let style = PathStyle {
        percent_encoded: true,
        lowercase_hex: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "notes", "é.md"], style).as_deref(),
        Some("%c3%a9.md")
    );
}

#[test]
fn encodes_a_leading_tilde() {
    let style = PathStyle {
        percent_encoded: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "notes", "~old", "a.md"], style).as_deref(),
        Some("%7Eold/a.md")
    );
}

#[cfg(not(windows))]
#[test]
fn renders_a_posix_file_url_in_its_written_scheme_case() {
    let style = PathStyle {
        file_scheme: true,
        scheme: "FILE".to_string(),
        ..create_plain_style()
    };

    assert_eq!(
        render(
            &["x", "a.md"],
            PathForm::Absolute(AbsoluteStyle::Posix),
            style
        )
        .as_deref(),
        Some("FILE:///refs-absent-root/x/a.md")
    );
}

#[test]
fn renders_escaped_slashes() {
    let style = PathStyle {
        escaped_slashes: true,
        ..create_plain_style()
    };

    assert_eq!(
        render_file_relative(&["work", "notes", "lib", "b.md"], style).as_deref(),
        Some("lib\\/b.md")
    );
}

#[test]
fn prefixes_a_lone_name_that_would_not_be_a_token() {
    assert_eq!(
        render_file_relative(&["work", "notes", "components"], create_plain_style()).as_deref(),
        Some("./components")
    );
    assert_eq!(
        render_file_relative(&["work", "notes", "LICENSE"], create_plain_style()).as_deref(),
        Some("./LICENSE")
    );
}

#[test]
fn checks_earlier_candidates_through_the_given_exists() {
    let shadow = absolute_path_of(&["work", "notes", "x", "b.md"]);

    let render_with = |exists: &dyn Fn(&Path) -> bool| {
        render_reference(
            &absolute_path_of(&["work", "x", "b.md"]),
            &PathForm::WorkingDirectoryRelative,
            &create_plain_style(),
            "",
            &absolute_path_of(&["work", "notes"]),
            &absolute_path_of(&["work"]),
            exists,
        )
    };

    assert_eq!(render_with(&|_| false).as_deref(), Some("x/b.md"));
    assert_eq!(render_with(&|path| path == shadow), None);
}
