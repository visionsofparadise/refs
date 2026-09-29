use super::*;
use crate::tree_of::tree_of;

#[test]
fn replaces_content_in_place_without_leaving_a_temporary_file() {
    let tree = tree_of(&[("docs/a.md", "old\n")]);
    let file = tree.root.join("docs").join("a.md");

    write_edits(&[FileEdit {
        file: file.clone(),
        content: "new\n".to_string(),
    }])
    .unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), "new\n");

    let names: Vec<_> = fs::read_dir(tree.root.join("docs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();

    assert_eq!(names, ["a.md"]);
}

#[test]
fn rewrites_a_read_only_original_and_keeps_it_read_only() {
    let tree = tree_of(&[("a.md", "old\n")]);
    let file = tree.root.join("a.md");
    let mut permissions = fs::metadata(&file).unwrap().permissions();

    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();

    write_edits(&[FileEdit {
        file: file.clone(),
        content: "new\n".to_string(),
    }])
    .unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), "new\n");
    assert!(fs::metadata(&file).unwrap().permissions().readonly());

    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(&file).unwrap().permissions();

        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&file, permissions).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn keeps_a_hidden_attribute_and_an_explicit_access_entry() {
    use std::os::windows::ffi::OsStrExt;

    use std::os::windows::fs::MetadataExt;

    use std::process::Command;

    use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};

    let tree = tree_of(&[("a.md", "old\n")]);
    let file = tree.root.join("a.md");
    let wide: Vec<u16> = file.as_os_str().encode_wide().chain([0]).collect();

    assert_ne!(
        unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) },
        0
    );

    let access_of = || {
        Command::new("icacls")
            .arg(&file)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| output.stdout)
    };

    let granted = Command::new("icacls")
        .arg(&file)
        .args(["/grant", "*S-1-1-0:(R)"])
        .output()
        .is_ok_and(|output| output.status.success());
    let before = access_of();

    write_edits(&[FileEdit {
        file: file.clone(),
        content: "new\n".to_string(),
    }])
    .unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), "new\n");
    assert_ne!(
        fs::metadata(&file).unwrap().file_attributes() & FILE_ATTRIBUTE_HIDDEN,
        0
    );

    if granted && before.is_some() {
        assert_eq!(access_of(), before);
    } else {
        eprintln!("skipped the access entry check: icacls is unavailable");
    }
}
