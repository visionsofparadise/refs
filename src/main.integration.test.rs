use super::*;
use crate::tree_of::{tree_of, Tree};
use same_file::Handle;

fn outcome_of(tree: &Tree, arguments: &[&str]) -> (String, String, i32) {
    outcome_with_input_of(tree, arguments, "")
}

fn outcome_with_input_of(tree: &Tree, arguments: &[&str], input: &str) -> (String, String, i32) {
    let mut stdin = input.as_bytes();
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
            stdin: &mut stdin,
            stdin_handle: None,
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

#[test]
fn reports_a_missing_path_argument_after_listing_the_rest_with_exit_two() {
    let tree = tree_of(&[("a.md", "b.md\n"), ("b.md", "")]);

    assert_eq!(
        outcome_of(&tree, &["missing.md", "a.md"]),
        (
            "a.md:1:1: b.md -> b.md\n".to_string(),
            "refs: missing.md: no such file or directory\n".to_string(),
            FAILURE
        )
    );
}

#[test]
fn keeps_the_exit_code_on_an_ignore_file_warning() {
    let tree = tree_of(&[(".ignore", "[z-a\n"), ("a.md", "")]);
    let (stdout, stderr, code) = outcome_of(&tree, &[]);

    assert_eq!((stdout.as_str(), code), ("", SUCCESS));
    assert!(stderr.starts_with("refs: .ignore: line 1: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1);
}

#[test]
fn prints_only_the_first_line_of_a_clap_error() {
    let tree = tree_of(&[]);
    let (_, stderr, _) = outcome_of(&tree, &["--bogus"]);

    assert_eq!(stderr, "refs: unexpected argument '--bogus' found\n");
}

#[test]
fn validates_to_paths_before_walking() {
    let tree = tree_of(&[("a.md", "")]);

    assert_eq!(
        outcome_of(&tree, &["missing.md", "--to", "gone"]),
        (
            String::new(),
            "refs: gone: no such file or directory\n".to_string(),
            FAILURE
        )
    );
}

#[test]
fn previews_a_fix_without_writing_then_writes_it_with_exit_zero() {
    let tree = tree_of(&[
        (
            "index.md",
            "see docs/a.md
",
        ),
        ("docs/a.md", ""),
    ]);

    std::fs::rename(tree.root.join("docs/a.md"), tree.root.join("docs/b.md")).unwrap();

    let expected = (
        "index.md:1:5: docs/a.md -> docs/b.md
"
        .to_string(),
        String::new(),
        SUCCESS,
    );

    assert_eq!(
        outcome_with_input_of(
            &tree,
            &["-", "--dry-run"],
            "R	docs/a.md	docs/b.md
"
        ),
        expected
    );
    assert_eq!(
        std::fs::read_to_string(tree.root.join("index.md")).unwrap(),
        "see docs/a.md
"
    );

    assert_eq!(
        outcome_with_input_of(
            &tree,
            &["-"],
            "R	docs/a.md	docs/b.md
"
        ),
        expected
    );
    assert_eq!(
        std::fs::read_to_string(tree.root.join("index.md")).unwrap(),
        "see docs/b.md
"
    );
}

#[test]
fn reports_deletes_and_skipped_declarations_with_exit_one() {
    let tree = tree_of(&[
        (
            "index.md",
            "see docs/a.md
",
        ),
        ("docs/b.md", ""),
    ]);

    assert_eq!(
        outcome_with_input_of(
            &tree,
            &["-"],
            "D docs/a.md
bogus
D docs/b.md
"
        ),
        (
            "index.md:1:5: docs/a.md -> docs/a.md (deleted)
"
            .to_string(),
            "refs: skipped declaration line 2: unrecognized: bogus
refs: skipped declaration line 3: path still exists: D docs/b.md
"
            .to_string(),
            FINDINGS
        )
    );
}

#[test]
fn reports_an_unreadable_file_and_still_writes_the_rest_with_exit_two() {
    let tree = tree_of(&[
        ("index.md", "see docs/a.md\n"),
        ("locked.md", "see docs/a.md\n"),
        ("docs/a.md", ""),
    ]);

    std::fs::rename(tree.root.join("docs/a.md"), tree.root.join("docs/b.md")).unwrap();

    let Some(_lock) = crate::tree_of::lock_file(&tree.root.join("locked.md")) else {
        eprintln!("skipped: the file stays readable for this user");

        return;
    };

    let (stdout, stderr, code) = outcome_with_input_of(&tree, &["-"], "R\tdocs/a.md\tdocs/b.md\n");

    assert_eq!(
        (stdout.as_str(), code),
        ("index.md:1:5: docs/a.md -> docs/b.md\n", FAILURE)
    );
    assert!(stderr.starts_with("refs: locked.md: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert_eq!(
        std::fs::read_to_string(tree.root.join("index.md")).unwrap(),
        "see docs/b.md\n"
    );
}

#[test]
fn never_rewrites_the_file_read_as_declarations() {
    let tree = tree_of(&[
        ("index.md", "see docs/a.md\n"),
        ("decl.txt", "R\tdocs/a.md\tdocs/b.md\n"),
        ("docs/a.md", ""),
    ]);

    std::fs::rename(tree.root.join("docs/a.md"), tree.root.join("docs/b.md")).unwrap();

    let declarations = tree.root.join("decl.txt");
    let mut stdin = std::fs::File::open(&declarations).unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run(
        ["refs", "-"].into_iter().map(OsString::from).collect(),
        Ok(tree.root.clone()),
        &mut Streams {
            stdin: &mut stdin,
            stdin_handle: Handle::from_path(&declarations).ok(),
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );

    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        "decl.txt:1:3: docs/a.md -> docs/b.md (not rewritten)\nindex.md:1:5: docs/a.md -> docs/b.md\n"
    );
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "refs: decl.txt: declaration input not rewritten\n"
    );
    assert_eq!(code, FINDINGS);
    assert_eq!(
        std::fs::read_to_string(&declarations).unwrap(),
        "R\tdocs/a.md\tdocs/b.md\n"
    );
    assert_eq!(
        std::fs::read_to_string(tree.root.join("index.md")).unwrap(),
        "see docs/b.md\n"
    );
}
