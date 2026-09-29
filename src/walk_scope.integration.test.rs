use super::*;
use crate::tree_of::{tree_of, tree_of_bytes, Tree};
use std::fs;
use std::process::Command;

fn initialize_git(root: &Path) {
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
        .status()
        .unwrap();

    assert!(status.success());
}

fn scope_of(tree: &Tree, paths: &[&str], options: ScopeOptions) -> Scope {
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();

    walk_scope(&paths, &options, &tree.root)
}

fn names_of(tree: &Tree, paths: &[&str], options: ScopeOptions) -> Vec<String> {
    let scope = scope_of(tree, paths, options);

    assert!(scope.errors.is_empty(), "{:?}", scope.errors);

    scope
        .files
        .iter()
        .map(|file| format_path(file, &tree.root))
        .collect()
}

fn errors_of(tree: &Tree, paths: &[&str]) -> Vec<String> {
    scope_of(tree, paths, ScopeOptions::default()).errors
}

fn options_of(no_ignore: bool, no_ignore_vcs: bool, hidden: bool) -> ScopeOptions {
    ScopeOptions {
        no_ignore,
        no_ignore_vcs,
        hidden,
    }
}

#[test]
fn excludes_a_gitignored_file_unless_vcs_ignores_are_off() {
    let tree = tree_of(&[
        (".gitignore", "ignored.md\n"),
        ("ignored.md", ""),
        ("kept.md", ""),
    ]);

    initialize_git(&tree.root);

    assert_eq!(
        names_of(&tree, &["."], ScopeOptions::default()),
        ["kept.md"]
    );
    assert_eq!(
        names_of(&tree, &["."], options_of(false, true, false)),
        ["ignored.md", "kept.md"]
    );
}

#[test]
fn excludes_an_rgignored_file_unless_ignores_are_off() {
    let tree = tree_of(&[
        (".rgignore", "ignored.md\n"),
        ("ignored.md", ""),
        ("kept.md", ""),
    ]);

    assert_eq!(
        names_of(&tree, &["."], ScopeOptions::default()),
        ["kept.md"]
    );
    assert_eq!(
        names_of(&tree, &["."], options_of(true, false, false)),
        ["ignored.md", "kept.md"]
    );
}

#[test]
fn excludes_an_ignored_file() {
    let tree = tree_of(&[
        (".ignore", "ignored.md\n"),
        ("ignored.md", ""),
        ("kept.md", ""),
    ]);

    assert_eq!(
        names_of(&tree, &["."], ScopeOptions::default()),
        ["kept.md"]
    );
}

#[test]
fn excludes_a_file_named_in_git_info_exclude() {
    let tree = tree_of(&[("excluded.md", ""), ("kept.md", "")]);

    initialize_git(&tree.root);
    fs::write(tree.root.join(".git/info/exclude"), "excluded.md\n").unwrap();

    assert_eq!(
        names_of(&tree, &["."], ScopeOptions::default()),
        ["kept.md"]
    );
    assert_eq!(
        names_of(&tree, &["."], options_of(false, true, false)),
        ["excluded.md", "kept.md"]
    );
}

#[test]
fn honours_ignore_and_rgignore_when_vcs_ignores_are_off() {
    let tree = tree_of(&[
        (".ignore", "a.md\n"),
        (".rgignore", "b.md\n"),
        ("a.md", ""),
        ("b.md", ""),
        ("kept.md", ""),
    ]);

    initialize_git(&tree.root);

    assert_eq!(
        names_of(&tree, &["."], options_of(false, true, false)),
        ["kept.md"]
    );
}

#[test]
fn excludes_a_hidden_file_unless_hidden_is_set() {
    let tree = tree_of(&[(".hidden.md", ""), ("kept.md", "")]);

    assert_eq!(
        names_of(&tree, &["."], ScopeOptions::default()),
        ["kept.md"]
    );
    assert_eq!(
        names_of(&tree, &["."], options_of(false, false, true)),
        [".hidden.md", "kept.md"]
    );
}

#[test]
fn excludes_every_vcs_entry_under_default_and_widest_options() {
    let tree = tree_of(&[
        (".git/a.md", ""),
        (".hg/a.md", ""),
        (".svn/a.md", ""),
        (".jj/a.md", ""),
        (".bzr/a.md", ""),
        ("worktree/.git", "gitdir: ../.git/worktrees/worktree\n"),
        ("worktree/kept.md", ""),
        ("kept.md", ""),
    ]);

    let kept = ["kept.md", "worktree/kept.md"];

    assert_eq!(names_of(&tree, &["."], ScopeOptions::default()), kept);
    assert_eq!(names_of(&tree, &["."], options_of(true, true, true)), kept);
}

#[test]
fn refuses_a_path_that_is_or_lies_inside_a_vcs_entry_and_walks_the_rest() {
    let tree = tree_of(&[
        (".git/config", ""),
        (".hg/a.md", ""),
        (".svn/a.md", ""),
        (".jj/a.md", ""),
        (".bzr/a.md", ""),
        ("kept.md", ""),
    ]);

    for (path, printed) in [
        (".git", ".git"),
        (".hg", ".hg"),
        (".svn", ".svn"),
        (".jj", ".jj"),
        (".bzr", ".bzr"),
        (".git/config", ".git/config"),
        ("./.hg/a.md", ".hg/a.md"),
    ] {
        let scope = scope_of(&tree, &[path, "kept.md"], ScopeOptions::default());

        assert_eq!(scope.errors, [format!("{printed}: inside a VCS directory")]);
        assert_eq!(scope.files, [tree.root.join("kept.md")]);
    }

    let scope = walk_scope(
        &[PathBuf::from(".")],
        &ScopeOptions::default(),
        &tree.root.join(".git"),
    );

    assert_eq!(scope.errors, [".: inside a VCS directory"]);
}

#[cfg(unix)]
fn link_directory(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn link_directory(target: &Path, link: &Path) -> bool {
    Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn refuses_a_link_that_resolves_inside_a_vcs_directory() {
    let tree = tree_of(&[(".git/objects/a.md", "")]);

    assert!(link_directory(
        &tree.root.join(".git").join("objects"),
        &tree.root.join("objects")
    ));
    assert_eq!(
        errors_of(&tree, &["objects"]),
        ["objects: inside a VCS directory"]
    );
}

#[test]
fn reports_a_missing_path_and_walks_the_rest() {
    let tree = tree_of(&[("a.md", "")]);
    let scope = scope_of(&tree, &["missing.md", "a.md"], ScopeOptions::default());

    assert_eq!(scope.errors, ["missing.md: no such file or directory"]);
    assert_eq!(scope.files, [tree.root.join("a.md")]);
}

#[test]
fn refuses_a_path_the_grammar_gives_no_candidate() {
    let tree = tree_of(&[("a.md", "")]);

    assert_eq!(
        errors_of(&tree, &["x/a:b.md"]),
        ["x/a:b.md: unsupported path"]
    );
}

#[cfg(windows)]
#[test]
fn refuses_drive_relative_and_driveless_rooted_paths() {
    let tree = tree_of(&[("a.md", "")]);

    assert_eq!(
        errors_of(&tree, &["C:a.md", "/tmp", "\\tmp"]),
        [
            "C:a.md: unsupported path",
            "/tmp: unsupported path",
            "\\tmp: unsupported path"
        ]
    );
}

#[cfg(windows)]
#[test]
fn resolves_msys_and_verbatim_path_arguments_to_one_file() {
    let tree = tree_of(&[("docs/a.md", "")]);
    let text = tree.root.to_string_lossy().replace('\\', "/");
    let msys = format!("/{}{}/docs", text[..1].to_lowercase(), &text[2..]);
    let verbatim = format!("\\\\?\\{}", tree.root.join("docs").display());

    assert_eq!(
        names_of(&tree, &[&msys, &verbatim, "docs"], ScopeOptions::default()),
        ["docs/a.md"]
    );
}

#[test]
fn yields_a_file_reached_through_overlapping_paths_once() {
    let tree = tree_of(&[("docs/a.md", ""), ("docs/deep/b.md", "")]);

    assert_eq!(
        names_of(
            &tree,
            &["docs", ".", "docs/deep/b.md"],
            ScopeOptions::default()
        ),
        ["docs/a.md", "docs/deep/b.md"]
    );
}

#[test]
fn sorts_files_by_printed_path_ordinally() {
    let tree = tree_of(&[("b.md", ""), ("C.md", ""), ("_x.md", ""), ("a/z.md", "")]);

    assert_eq!(
        names_of(&tree, &["."], ScopeOptions::default()),
        ["C.md", "_x.md", "a/z.md", "b.md"]
    );
}

#[test]
fn warns_of_an_ignore_file_that_does_not_parse() {
    let tree = tree_of(&[(".ignore", "[z-a\n"), ("kept.md", "")]);
    let scope = scope_of(&tree, &["."], ScopeOptions::default());

    assert_eq!(scope.warnings.len(), 1, "{:?}", scope.warnings);
    assert!(scope.warnings[0].starts_with(".ignore: line 1: "));
    assert!(scope.errors.is_empty());
}

#[test]
fn warns_of_a_parent_ignore_file_reached_through_a_subdirectory() {
    let tree = tree_of(&[(".ignore", "[z-a\n"), ("sub/kept.md", "")]);
    let scope = scope_of(&tree, &["sub"], ScopeOptions::default());

    assert_eq!(scope.warnings.len(), 1, "{:?}", scope.warnings);
    assert!(
        scope.warnings[0].starts_with(".ignore: line 1: "),
        "{:?}",
        scope.warnings
    );
    assert!(scope.errors.is_empty());
    assert_eq!(scope.files, [tree.root.join("sub").join("kept.md")]);
}

#[cfg(unix)]
#[test]
fn reports_an_unreadable_directory_and_walks_on() {
    use std::os::unix::fs::PermissionsExt;

    let tree = tree_of(&[("locked/a.md", ""), ("kept.md", "")]);
    let locked = tree.root.join("locked");

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let enforced = fs::read_dir(&locked).is_err();
    let scope = scope_of(&tree, &["."], ScopeOptions::default());

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    if !enforced {
        eprintln!("skipped: permissions are not enforced for this user");

        return;
    }

    assert_eq!(scope.errors.len(), 1, "{:?}", scope.errors);
    assert!(
        scope.errors[0].starts_with("locked: "),
        "{:?}",
        scope.errors
    );
    assert_eq!(scope.files, [tree.root.join("kept.md")]);
}

#[test]
fn reads_text_and_skips_binary_content() {
    let tree = tree_of_bytes(&[
        ("text.md", "a é".as_bytes()),
        ("nul.bin", b"a\0b"),
        ("latin.txt", b"caf\xe9"),
    ]);

    assert_eq!(
        read_text(&tree.root.join("text.md")).unwrap().as_deref(),
        Some("a é")
    );
    assert_eq!(read_text(&tree.root.join("nul.bin")).unwrap(), None);
    assert_eq!(read_text(&tree.root.join("latin.txt")).unwrap(), None);
}
