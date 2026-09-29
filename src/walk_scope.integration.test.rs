use super::*;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

struct Tree {
    _directory: TempDir,
    root: PathBuf,
}

fn tree_of(files: &[(&str, &[u8])]) -> Tree {
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

fn initialize_git(root: &Path) {
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
        .status()
        .unwrap();

    assert!(status.success());
}

fn names_of(tree: &Tree, paths: &[&str], options: ScopeOptions) -> Vec<String> {
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();

    walk_scope(&paths, &options, &tree.root)
        .unwrap()
        .files
        .iter()
        .map(|file| {
            file.strip_prefix(&tree.root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
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
        (".gitignore", b"ignored.md\n"),
        ("ignored.md", b""),
        ("kept.md", b""),
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
        (".rgignore", b"ignored.md\n"),
        ("ignored.md", b""),
        ("kept.md", b""),
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
fn excludes_a_hidden_file_unless_hidden_is_set() {
    let tree = tree_of(&[(".hidden.md", b""), ("kept.md", b"")]);

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
fn excludes_every_vcs_directory_under_default_and_widest_options() {
    let tree = tree_of(&[
        (".git/a.md", b""),
        (".hg/a.md", b""),
        (".svn/a.md", b""),
        (".jj/a.md", b""),
        (".bzr/a.md", b""),
        ("kept.md", b""),
    ]);

    assert_eq!(
        names_of(&tree, &["."], ScopeOptions::default()),
        ["kept.md"]
    );
    assert_eq!(
        names_of(&tree, &["."], options_of(true, true, true)),
        ["kept.md"]
    );
}

#[test]
fn refuses_a_path_that_is_or_lies_inside_a_vcs_directory() {
    let tree = tree_of(&[
        (".git/config", b""),
        (".hg/a.md", b""),
        (".svn/a.md", b""),
        (".jj/a.md", b""),
        (".bzr/a.md", b""),
    ]);

    for path in [
        ".git",
        ".hg",
        ".svn",
        ".jj",
        ".bzr",
        ".git/config",
        "./.hg/a.md",
    ] {
        let error =
            walk_scope(&[PathBuf::from(path)], &ScopeOptions::default(), &tree.root).unwrap_err();

        assert_eq!(error.to_string(), format!("{path}: inside a VCS directory"));
    }

    let error = walk_scope(
        &[PathBuf::from(".")],
        &ScopeOptions::default(),
        &tree.root.join(".git"),
    )
    .unwrap_err();

    assert_eq!(error.to_string(), ".: inside a VCS directory");
}

#[test]
fn excludes_an_ignored_file() {
    let tree = tree_of(&[
        (".ignore", b"ignored.md\n"),
        ("ignored.md", b""),
        ("kept.md", b""),
    ]);

    assert_eq!(
        names_of(&tree, &["."], ScopeOptions::default()),
        ["kept.md"]
    );
}

#[test]
fn excludes_a_file_named_in_git_info_exclude() {
    let tree = tree_of(&[("excluded.md", b""), ("kept.md", b"")]);

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
        (".ignore", b"a.md\n"),
        (".rgignore", b"b.md\n"),
        ("a.md", b""),
        ("b.md", b""),
        ("kept.md", b""),
    ]);

    initialize_git(&tree.root);

    assert_eq!(
        names_of(&tree, &["."], options_of(false, true, false)),
        ["kept.md"]
    );
}

#[test]
fn warns_of_an_ignore_file_that_does_not_parse() {
    let tree = tree_of(&[(".ignore", b"[z-a\n"), ("kept.md", b"")]);
    let scope = walk_scope(&[PathBuf::from(".")], &ScopeOptions::default(), &tree.root).unwrap();

    assert_eq!(scope.warnings.len(), 1, "{:?}", scope.warnings);
    assert!(scope.errors.is_empty());
}

#[cfg(unix)]
#[test]
fn reports_an_unreadable_directory_and_walks_on() {
    use std::os::unix::fs::PermissionsExt;

    let tree = tree_of(&[("locked/a.md", b""), ("kept.md", b"")]);
    let locked = tree.root.join("locked");

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let scope = walk_scope(&[PathBuf::from(".")], &ScopeOptions::default(), &tree.root);

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    let scope = scope.unwrap();

    assert_eq!(scope.errors.len(), 1, "{:?}", scope.errors);
    assert_eq!(scope.files, [tree.root.join("kept.md")]);
}

#[cfg(windows)]
#[test]
fn resolves_an_msys_path_argument() {
    let tree = tree_of(&[("docs/a.md", b"")]);
    let text = tree.root.to_string_lossy().replace('\\', "/");
    let msys = format!("/{}{}/docs", text[..1].to_lowercase(), &text[2..]);

    assert_eq!(
        names_of(&tree, &[&msys], ScopeOptions::default()),
        ["docs/a.md"]
    );
}

#[test]
fn yields_a_file_reached_through_overlapping_paths_once() {
    let tree = tree_of(&[("docs/a.md", b""), ("docs/deep/b.md", b"")]);

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
fn rejects_a_path_that_does_not_exist() {
    let tree = tree_of(&[]);
    let error = walk_scope(
        &[PathBuf::from("missing")],
        &ScopeOptions::default(),
        &tree.root,
    )
    .unwrap_err();

    assert!(error.to_string().contains("missing"));
}

#[test]
fn reads_text_and_skips_binary_content() {
    let tree = tree_of(&[
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
