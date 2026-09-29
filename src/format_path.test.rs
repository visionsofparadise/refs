use super::*;
use std::path::PathBuf;

#[cfg(windows)]
fn absolute(path: &str) -> PathBuf {
    PathBuf::from(format!("C:\\{}", path.replace('/', "\\")))
}

#[cfg(not(windows))]
fn absolute(path: &str) -> PathBuf {
    PathBuf::from(format!("/{path}"))
}

#[test]
fn joins_a_path_beneath_the_working_directory_with_slashes() {
    assert_eq!(
        format_path(&absolute("work/docs/a.md"), &absolute("work")),
        "docs/a.md"
    );
}

#[test]
fn prints_the_working_directory_itself_as_a_dot() {
    assert_eq!(format_path(&absolute("work"), &absolute("work")), ".");
}

#[test]
fn prints_a_path_outside_the_working_directory_in_native_form() {
    let outside = absolute("elsewhere/docs/a.md");

    assert_eq!(
        format_path(&outside, &absolute("work")),
        outside.display().to_string()
    );
    assert_eq!(
        format_path(&absolute("workshop/a.md"), &absolute("work")),
        absolute("workshop/a.md").display().to_string()
    );
}

#[cfg(windows)]
#[test]
fn keeps_the_written_case_of_a_path_beneath_a_differently_cased_working_directory() {
    assert_eq!(
        format_path(&absolute("Work/Docs/A.md"), &absolute("work")),
        "Docs/A.md"
    );
}
