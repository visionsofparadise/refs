use super::*;
use std::path::PathBuf;

fn absolute(names: &[&str]) -> PathBuf {
    let mut path = if cfg!(windows) {
        PathBuf::from("C:\\")
    } else {
        PathBuf::from("/")
    };

    for name in names {
        path.push(name);
    }

    path
}

fn plain() -> PathStyle {
    PathStyle {
        separator: '/',
        doubled_backslashes: false,
        dot_prefix: false,
        trailing_separator: false,
        percent_encoded: false,
        file_scheme: false,
    }
}

fn render(target: &[&str], form: PathForm, style: PathStyle) -> Option<String> {
    render_reference(
        &absolute(target),
        &form,
        &style,
        &absolute(&["work", "notes"]),
        &absolute(&["work"]),
    )
}

fn render_file_relative(target: &[&str], style: PathStyle) -> Option<String> {
    render(target, PathForm::FileRelative, style)
}

#[test]
fn renders_a_file_relative_path_across_a_depth_change() {
    assert_eq!(
        render_file_relative(&["work", "docs", "deep", "a.md"], plain()).as_deref(),
        Some("../docs/deep/a.md")
    );
}

#[test]
fn renders_a_working_directory_relative_path_across_a_depth_change() {
    assert_eq!(
        render(
            &["work", "docs", "deep", "a.md"],
            PathForm::WorkingDirectoryRelative,
            plain()
        )
        .as_deref(),
        Some("docs/deep/a.md")
    );
}

#[test]
fn renders_the_base_itself_as_a_dot() {
    assert_eq!(
        render_file_relative(&["work", "notes"], plain()).as_deref(),
        Some(".")
    );
}

#[test]
fn renders_with_the_recorded_separator() {
    let style = PathStyle {
        separator: '\\',
        ..plain()
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
        ..plain()
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
        ..plain()
    };

    assert_eq!(
        render_file_relative(&["work", "notes", "sub", "a.md"], style).as_deref(),
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
        ..plain()
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
        ..plain()
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
        ..plain()
    };

    assert_eq!(
        render_file_relative(&["work", "notes", "été.md"], style).as_deref(),
        Some("%C3%A9t%C3%A9.md")
    );
}

#[test]
fn reports_an_unencoded_rendering_containing_a_delimiter_as_unrewritable() {
    assert_eq!(
        render_file_relative(&["work", "my docs", "a.md"], plain()),
        None
    );
    assert_eq!(
        render_file_relative(&["work", "docs", "a(1).md"], plain()),
        None
    );
}

#[cfg(windows)]
#[test]
fn renders_drive_and_msys_absolute_paths() {
    let target = &["x", "deep", "a.md"];

    assert_eq!(
        render(
            target,
            PathForm::Absolute(AbsoluteStyle::Drive),
            PathStyle {
                separator: '\\',
                ..plain()
            }
        )
        .as_deref(),
        Some("C:\\x\\deep\\a.md")
    );
    assert_eq!(
        render(target, PathForm::Absolute(AbsoluteStyle::Msys), plain()).as_deref(),
        Some("/c/x/deep/a.md")
    );
}

#[cfg(windows)]
#[test]
fn renders_a_percent_encoded_drive_file_url_keeping_the_drive_colon() {
    let style = PathStyle {
        percent_encoded: true,
        file_scheme: true,
        ..plain()
    };

    assert_eq!(
        render(
            &["my dir", "a.md"],
            PathForm::Absolute(AbsoluteStyle::Drive),
            style
        )
        .as_deref(),
        Some("file:///C:/my%20dir/a.md")
    );
}

#[cfg(windows)]
#[test]
fn reports_a_relative_rendering_across_drives_as_unrewritable() {
    assert_eq!(
        render_reference(
            Path::new("D:\\a.md"),
            &PathForm::FileRelative,
            &plain(),
            Path::new("C:\\work"),
            Path::new("C:\\work"),
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
            plain()
        )
        .as_deref(),
        Some("/x/deep/a.md")
    );
}

#[cfg(not(windows))]
#[test]
fn renders_a_posix_file_url() {
    let style = PathStyle {
        file_scheme: true,
        ..plain()
    };

    assert_eq!(
        render(
            &["x", "a.md"],
            PathForm::Absolute(AbsoluteStyle::Posix),
            style
        )
        .as_deref(),
        Some("file:///x/a.md")
    );
}
