use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn reports_a_rendering_an_earlier_existing_candidate_would_shadow() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let notes = root.join("notes");
    let target = root.join("x").join("b.md");
    let shadow = notes.join("x").join("b.md");

    let render_working_directory_relative = || {
        render_reference(
            &target,
            &PathForm::WorkingDirectoryRelative,
            &PathStyle {
                separator: '/',
                doubled_backslashes: false,
                escaped_slashes: false,
                dot_prefix: false,
                parent_prefix: false,
                trailing_separator: false,
                percent_encoded: false,
                file_scheme: false,
                scheme: String::new(),
                lowercase_drive: false,
                encoded_drive_colon: false,
                lowercase_hex: false,
            },
            "",
            &notes,
            root,
            &|path: &Path| path.exists(),
        )
    };

    fs::create_dir_all(shadow.parent().unwrap()).unwrap();

    assert_eq!(
        render_working_directory_relative().as_deref(),
        Some("x/b.md")
    );

    fs::write(&shadow, "").unwrap();

    assert_eq!(render_working_directory_relative(), None);
}
