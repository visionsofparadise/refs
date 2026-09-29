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

fn options(no_ignore: bool, no_ignore_vcs: bool, hidden: bool) -> ScopeOptions {
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
        names_of(&tree, &["."], options(false, true, false)),
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
        names_of(&tree, &["."], options(true, false, false)),
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
        names_of(&tree, &["."], options(false, false, true)),
        [".hidden.md", "kept.md"]
    );
}

#[test]
fn excludes_a_vcs_directory_under_every_flag() {
    let tree = tree_of(&[(".git/config.md", b""), (".jj/x.md", b""), ("kept.md", b"")]);

    assert_eq!(
        names_of(&tree, &["."], options(true, false, true)),
        ["kept.md"]
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
