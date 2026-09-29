use super::*;
use std::fs;
use tempfile::TempDir;

struct Tree {
    _directory: TempDir,
    root: PathBuf,
}

fn tree_of(files: &[(&str, &str)]) -> Tree {
    let directory = TempDir::new().unwrap();
    let root = dunce::canonicalize(directory.path()).unwrap();

    for (name, content) in files {
        let path = root.join(name);

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    Tree {
        _directory: directory,
        root,
    }
}

fn outcome_of(tree: &Tree, arguments: &[&str]) -> (String, String, i32) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let arguments = std::iter::once("refs")
        .chain(arguments.iter().copied())
        .map(OsString::from)
        .collect();

    let code = run(
        arguments,
        Ok(tree.root.clone()),
        &mut Streams {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );

    (
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
        code,
    )
}

#[test]
fn prints_each_reference_and_exits_zero() {
    let tree = tree_of(&[("docs/a.md", "[b](b.md#h)\n"), ("docs/b.md", "")]);

    assert_eq!(
        outcome_of(&tree, &[]),
        (
            "docs/a.md:1:5: b.md#h -> docs/b.md\n".to_string(),
            String::new(),
            SUCCESS
        )
    );
}

#[test]
fn exits_one_when_dangling_references_print() {
    let tree = tree_of(&[("a.md", "gone/b.md\n")]);

    assert_eq!(
        outcome_of(&tree, &["--dangling"]),
        (
            "a.md:1:1: gone/b.md -> gone/b.md (dangling)\n".to_string(),
            String::new(),
            FINDINGS
        )
    );
}

#[test]
fn exits_zero_when_no_dangling_reference_prints() {
    let tree = tree_of(&[("a.md", "")]);

    assert_eq!(outcome_of(&tree, &["--dangling"]).2, SUCCESS);
}

#[test]
fn prints_help_and_version_to_stdout_with_exit_zero() {
    let tree = tree_of(&[]);
    let (help, _, code) = outcome_of(&tree, &["--help"]);

    assert_eq!(code, SUCCESS);
    assert!(help.starts_with("List file references and repair them from declared moves\n"));

    assert_eq!(
        outcome_of(&tree, &["--version"]),
        (
            format!("refs {}\n", env!("CARGO_PKG_VERSION")),
            String::new(),
            SUCCESS
        )
    );
}

#[test]
fn prefixes_clap_and_usage_errors_with_refs() {
    let tree = tree_of(&[]);
    let (stdout, stderr, code) = outcome_of(&tree, &["--bogus"]);

    assert_eq!((stdout.as_str(), code), ("", FAILURE));
    assert!(stderr.starts_with("refs: unexpected argument '--bogus' found"));

    assert_eq!(
        outcome_of(&tree, &["--dry-run"]),
        (
            String::new(),
            "refs: --dry-run applies only to fixing with -\n".to_string(),
            FAILURE
        )
    );
}

#[test]
fn rejects_a_missing_to_path_with_exit_two() {
    let tree = tree_of(&[("a.md", "b.md\n"), ("b.md", "")]);
    let (stdout, stderr, code) = outcome_of(&tree, &["--to", "gone"]);

    assert_eq!(
        (stdout.as_str(), stderr.as_str(), code),
        ("", "refs: gone: no such file or directory\n", FAILURE)
    );
}

#[cfg(unix)]
#[test]
fn reports_an_unreadable_file_after_listing_the_rest_with_exit_two() {
    use std::os::unix::fs::PermissionsExt;

    let tree = tree_of(&[("a.md", "b.md\n"), ("b.md", ""), ("c.md", "b.md\n")]);
    let locked = tree.root.join("a.md");

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let (stdout, stderr, code) = outcome_of(&tree, &[]);

    assert_eq!(stdout, "c.md:1:1: b.md -> b.md\n");
    assert!(stderr.starts_with(&format!("refs: {}: ", locked.display())));
    assert_eq!(code, FAILURE);
}
