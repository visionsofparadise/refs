use super::*;
use crate::walk_scope::{walk_scope, ScopeOptions};
use std::fs;
use tempfile::TempDir;

struct Tree {
    _directory: TempDir,
    root: PathBuf,
    work: PathBuf,
}

fn tree_of(files: &[(&str, &str)]) -> Tree {
    let directory = TempDir::new().unwrap();
    let root = dunce::canonicalize(directory.path()).unwrap();
    let work = root.join("work");

    fs::create_dir_all(&work).unwrap();

    for (name, content) in files {
        let path = work.join(name);

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    Tree {
        _directory: directory,
        root,
        work,
    }
}

fn lines_of(tree: &Tree, to: &[&str], dangling_only: bool) -> Vec<String> {
    let scope = walk_scope(&[PathBuf::from(".")], &ScopeOptions::default(), &tree.work).unwrap();
    let to: Vec<PathBuf> = to.iter().map(PathBuf::from).collect();

    list_references(&scope, &tree.work, &to, dangling_only)
        .unwrap()
        .listed
        .into_iter()
        .map(|listed| {
            let file = listed.file.strip_prefix(&tree.work).unwrap();
            let target = listed.target.strip_prefix(&tree.work).unwrap();

            format!(
                "{}:{}:{} {}{} -> {}{}",
                file.to_string_lossy().replace('\\', "/"),
                listed.token.line,
                listed.token.column,
                listed.token.path,
                listed.token.suffix,
                target.to_string_lossy().replace('\\', "/"),
                if listed.dangling { " (dangling)" } else { "" }
            )
        })
        .collect()
}

#[test]
fn lists_file_relative_and_working_directory_relative_references() {
    let tree = tree_of(&[
        ("notes/a.md", "[b](b.md#top) and `src/main.rs:3`\n"),
        ("notes/b.md", ""),
        ("src/main.rs", ""),
    ]);

    assert_eq!(
        lines_of(&tree, &[], false),
        [
            "notes/a.md:1:5 b.md#top -> notes/b.md",
            "notes/a.md:1:20 src/main.rs:3 -> src/main.rs",
        ]
    );
}

#[test]
fn flags_a_dangling_reference_with_its_first_candidate() {
    let tree = tree_of(&[("notes/a.md", "see missing/c.md, @missing/d.md and and/or\n")]);

    assert_eq!(
        lines_of(&tree, &[], false),
        ["notes/a.md:1:5 missing/c.md -> notes/missing/c.md (dangling)"]
    );
}

#[test]
fn skips_a_missing_absolute_path_outside_the_working_directory() {
    let tree = tree_of(&[]);
    let outside = tree.root.join("outside").join("x.md");
    let inside = tree.work.join("gone").join("y.md");

    fs::write(
        tree.work.join("a.md"),
        format!("{}\n{}\n", outside.display(), inside.display()),
    )
    .unwrap();

    assert_eq!(
        lines_of(&tree, &[], false),
        [format!(
            "a.md:2:1 {} -> gone/y.md (dangling)",
            inside.display()
        )]
    );
}

#[test]
fn keeps_references_beneath_a_to_directory_and_dangling_ones_on_request() {
    let tree = tree_of(&[
        ("a.md", "docs/deep/b.md docs/gone.md other/c.md\n"),
        ("docs/deep/b.md", ""),
        ("other/c.md", ""),
    ]);

    assert_eq!(
        lines_of(&tree, &["docs"], false),
        [
            "a.md:1:1 docs/deep/b.md -> docs/deep/b.md",
            "a.md:1:16 docs/gone.md -> docs/gone.md (dangling)",
        ]
    );
    assert_eq!(
        lines_of(&tree, &[], true),
        ["a.md:1:16 docs/gone.md -> docs/gone.md (dangling)"]
    );
}

#[test]
fn orders_by_file_then_line_then_column() {
    let tree = tree_of(&[
        ("z/y.md", "../a.md\n"),
        ("a.md", "z/y.md\nb.md z/y.md\n"),
        ("b.md", ""),
    ]);

    assert_eq!(
        lines_of(&tree, &[], false),
        [
            "a.md:1:1 z/y.md -> z/y.md",
            "a.md:2:1 b.md -> b.md",
            "a.md:2:6 z/y.md -> z/y.md",
            "z/y.md:1:1 ../a.md -> a.md",
        ]
    );
}

#[test]
fn never_flags_a_token_without_candidates_as_dangling() {
    let tree = tree_of(&[("index.md", "see a%2Fb/c.md and x/a:b.md")]);

    assert!(lines_of(&tree, &[], false).is_empty());
}

#[test]
fn reports_an_unreadable_file_and_lists_the_rest() {
    let tree = tree_of(&[("a.md", "b.md\n"), ("b.md", "")]);
    let missing = tree.work.join("gone.md");

    let scope = Scope {
        files: vec![tree.work.join("a.md"), missing.clone()],
        ..Scope::default()
    };

    let listing = list_references(&scope, &tree.work, &[], false).unwrap();

    assert_eq!(listing.listed.len(), 1);
    assert_eq!(listing.errors.len(), 1);
    assert!(listing.errors[0].starts_with(&format!("{}: ", missing.display())));
}

#[test]
fn rejects_a_to_path_that_does_not_exist() {
    let tree = tree_of(&[]);

    let error = list_references(
        &Scope::default(),
        &tree.work,
        &[PathBuf::from("gone")],
        false,
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "gone: no such file or directory");
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn finds_a_differently_cased_target_on_a_case_insensitive_filesystem() {
    let tree = tree_of(&[("a.md", "Docs/B.md docs/\n"), ("docs/b.md", "")]);

    assert_eq!(
        lines_of(&tree, &[], false),
        ["a.md:1:1 Docs/B.md -> Docs/B.md", "a.md:1:11 docs/ -> docs"]
    );
}
