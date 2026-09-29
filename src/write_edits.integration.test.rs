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
