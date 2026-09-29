use super::*;
use crate::tree_of::{tree_of, Tree};
use crate::walk_scope::{walk_scope, ScopeOptions};
use std::fs;

fn work_tree_of(files: &[(&str, &str)]) -> (Tree, PathBuf) {
    let names: Vec<(String, &str)> = files
        .iter()
        .map(|(name, content)| (format!("work/{name}"), *content))
        .collect();

    let files: Vec<(&str, &str)> = names
        .iter()
        .map(|(name, content)| (name.as_str(), *content))
        .collect();

    let tree = tree_of(&files);
    let work = tree.root.join("work");

    fs::create_dir_all(&work).unwrap();

    (tree, work)
}

fn lines_of(work: &Path, to: &[&str], dangling_only: bool) -> Vec<String> {
    let scope = walk_scope(&[PathBuf::from(".")], &ScopeOptions::default(), work);
    let to: Vec<PathBuf> = to.iter().map(PathBuf::from).collect();
    let to = resolve_targets(&to, work).unwrap();

    list_references(&scope, work, &to, dangling_only)
        .listed
        .into_iter()
        .map(|listed| {
            format!(
                "{}:{}:{} {}{} -> {}{}",
                format_path(&listed.file, work),
                listed.token.line,
                listed.token.column,
                listed.token.path,
                listed.token.suffix,
                format_path(&listed.target, work),
                if listed.dangling { " (dangling)" } else { "" }
            )
        })
        .collect()
}

#[test]
fn lists_file_relative_and_working_directory_relative_references() {
    let (_tree, work) = work_tree_of(&[
        ("notes/a.md", "[b](b.md#top) and `src/main.rs:3`\n"),
        ("notes/b.md", ""),
        ("src/main.rs", ""),
    ]);

    assert_eq!(
        lines_of(&work, &[], false),
        [
            "notes/a.md:1:5 b.md#top -> notes/b.md",
            "notes/a.md:1:20 src/main.rs:3 -> src/main.rs",
        ]
    );
}

#[test]
fn flags_a_dangling_reference_with_its_first_candidate() {
    let (_tree, work) =
        work_tree_of(&[("notes/a.md", "see missing/c.md, @missing/d.md and and/or\n")]);

    assert_eq!(
        lines_of(&work, &[], false),
        ["notes/a.md:1:5 missing/c.md -> notes/missing/c.md (dangling)"]
    );
}

#[test]
fn skips_a_missing_absolute_path_outside_the_working_directory() {
    let (tree, work) = work_tree_of(&[]);
    let outside = tree.root.join("outside").join("x.md");
    let inside = work.join("gone").join("y.md");

    fs::write(
        work.join("a.md"),
        format!("{}\n{}\n", outside.display(), inside.display()),
    )
    .unwrap();

    assert_eq!(
        lines_of(&work, &[], false),
        [format!(
            "a.md:2:1 {} -> gone/y.md (dangling)",
            inside.display()
        )]
    );
}

#[test]
fn keeps_references_beneath_a_to_directory_and_dangling_ones_on_request() {
    let (_tree, work) = work_tree_of(&[
        ("a.md", "docs/deep/b.md docs/gone.md other/c.md\n"),
        ("docs/deep/b.md", ""),
        ("other/c.md", ""),
    ]);

    assert_eq!(
        lines_of(&work, &["docs"], false),
        [
            "a.md:1:1 docs/deep/b.md -> docs/deep/b.md",
            "a.md:1:16 docs/gone.md -> docs/gone.md (dangling)",
        ]
    );
    assert_eq!(
        lines_of(&work, &[], true),
        ["a.md:1:16 docs/gone.md -> docs/gone.md (dangling)"]
    );
}

#[test]
fn orders_by_file_then_line_then_column() {
    let (_tree, work) = work_tree_of(&[
        ("z/y.md", "../a.md\n"),
        ("a.md", "z/y.md\nb.md z/y.md\n"),
        ("b.md", ""),
    ]);

    assert_eq!(
        lines_of(&work, &[], false),
        [
            "a.md:1:1 z/y.md -> z/y.md",
            "a.md:2:1 b.md -> b.md",
            "a.md:2:6 z/y.md -> z/y.md",
            "z/y.md:1:1 ../a.md -> a.md",
        ]
    );
}

#[test]
fn reassembles_parallel_results_in_scope_order() {
    let names: Vec<String> = (0..300).map(|index| format!("f{index:03}.md")).collect();

    let mut files: Vec<(&str, &str)> = names
        .iter()
        .map(|name| (name.as_str(), "t.md\nt.md\n"))
        .collect();

    files.push(("t.md", ""));

    let (_tree, work) = work_tree_of(&files);

    let expected: Vec<String> = names
        .iter()
        .flat_map(|name| {
            [
                format!("{name}:1:1 t.md -> t.md"),
                format!("{name}:2:1 t.md -> t.md"),
            ]
        })
        .collect();

    assert_eq!(lines_of(&work, &[], false), expected);
}

#[test]
fn never_flags_a_token_without_candidates_as_dangling() {
    let (_tree, work) = work_tree_of(&[("index.md", "see a%2Fb/c.md and x/a:b.md")]);

    assert!(lines_of(&work, &[], false).is_empty());
}

#[test]
fn reports_a_vanished_file_and_lists_the_rest() {
    let (_tree, work) = work_tree_of(&[("a.md", "b.md\n"), ("b.md", "")]);

    let scope = Scope {
        files: vec![work.join("a.md"), work.join("gone.md")],
        ..Scope::default()
    };

    let listing = list_references(&scope, &work, &[], false);

    assert_eq!(listing.listed.len(), 1);
    assert_eq!(listing.errors.len(), 1);
    assert!(
        listing.errors[0].starts_with("gone.md: "),
        "{:?}",
        listing.errors
    );
    assert!(!listing.errors[0].contains('\n'));
}

#[cfg(windows)]
#[test]
fn reports_a_file_locked_by_another_handle() {
    use std::os::windows::fs::OpenOptionsExt;

    let (_tree, work) = work_tree_of(&[("a.md", "b.md\n"), ("b.md", "")]);

    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(work.join("a.md"))
        .unwrap();

    let scope = Scope {
        files: vec![work.join("a.md"), work.join("b.md")],
        ..Scope::default()
    };

    let listing = list_references(&scope, &work, &[], false);

    assert_eq!(listing.errors.len(), 1, "{:?}", listing.errors);
    assert!(
        listing.errors[0].starts_with("a.md: "),
        "{:?}",
        listing.errors
    );
}

#[cfg(unix)]
#[test]
fn reports_an_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;

    let (_tree, work) = work_tree_of(&[("a.md", "b.md\n"), ("b.md", "")]);
    let locked = work.join("a.md");

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    if fs::read(&locked).is_ok() {
        eprintln!("skipped: permissions are not enforced for this user");

        return;
    }

    let scope = Scope {
        files: vec![locked, work.join("b.md")],
        ..Scope::default()
    };

    let listing = list_references(&scope, &work, &[], false);

    assert_eq!(listing.errors.len(), 1, "{:?}", listing.errors);
    assert!(
        listing.errors[0].starts_with("a.md: "),
        "{:?}",
        listing.errors
    );
}

#[test]
fn rejects_to_paths_that_do_not_exist_or_are_unsupported() {
    let (_tree, work) = work_tree_of(&[("a.md", "")]);

    assert_eq!(
        resolve_targets(
            &[
                PathBuf::from("a.md"),
                PathBuf::from("gone"),
                PathBuf::from("x/a:b.md")
            ],
            &work
        ),
        Err(vec![
            "gone: no such file or directory".to_string(),
            "x/a:b.md: unsupported path".to_string()
        ])
    );
}

#[test]
fn finds_a_differently_cased_target_where_the_filesystem_folds_case() {
    let (tree, work) = work_tree_of(&[("a.md", "Docs/B.md docs/\n"), ("docs/b.md", "")]);

    let expected: &[&str] = if tree.is_case_insensitive() {
        &["a.md:1:1 Docs/B.md -> Docs/B.md", "a.md:1:11 docs/ -> docs"]
    } else {
        &[
            "a.md:1:1 Docs/B.md -> Docs/B.md (dangling)",
            "a.md:1:11 docs/ -> docs",
        ]
    };

    assert_eq!(lines_of(&work, &[], false), expected);
}

fn never_stat(path: &Path) -> bool {
    panic!("stat called for {}", path.display())
}

fn always_stat(_: &Path) -> bool {
    true
}

#[test]
fn answers_a_listed_name_without_a_stat() {
    let tree = tree_of(&[("a.md", "")]);
    let cache = ExistenceCache::new(never_stat);

    assert!(cache.exists(&tree.root.join("a.md")));
}

#[test]
fn confirms_an_unlisted_name_with_a_stat() {
    let tree = tree_of(&[("a.md", "")]);

    assert!(ExistenceCache::new(always_stat).exists(&tree.root.join("A.MD")));
    assert!(!ExistenceCache::new(Path::exists).exists(&tree.root.join("b.md")));
}

#[test]
fn falls_back_to_a_stat_beneath_an_unlistable_parent() {
    let tree = tree_of(&[("a.md", "")]);

    assert!(ExistenceCache::new(always_stat).exists(&tree.root.join("a.md").join("x")));
    assert!(ExistenceCache::new(always_stat).exists(&tree.root.join("gone").join("x")));
}

#[cfg(windows)]
#[test]
fn agrees_with_path_exists_on_short_names_and_device_names() {
    let tree = tree_of(&[("verylongname.md", "")]);
    let cache = ExistenceCache::new(Path::exists);

    for name in ["VERYLO~1.MD", "NUL", "nul.md"] {
        let path = tree.root.join(name);

        assert_eq!(cache.exists(&path), path.exists(), "{name}");
    }
}
