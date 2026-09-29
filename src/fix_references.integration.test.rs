use super::*;
use crate::parse_declarations::Origin;
use crate::tree_of::{tree_of, Tree};
use crate::walk_scope::{walk_scope, ScopeOptions};
use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

fn path_of(tree: &Tree, name: &str) -> PathBuf {
    normalize_path(&tree.root.join(name))
}

fn origin_of(line: usize) -> Origin {
    Origin {
        line,
        text: format!("declaration {line}"),
    }
}

fn declare_move(tree: &Tree, from: &str, to: &str, line: usize) -> Declaration {
    Declaration::Move {
        from: path_of(tree, from),
        to: path_of(tree, to),
        origin: origin_of(line),
    }
}

fn relocate(tree: &Tree, from: &str, to: &str, line: usize) -> Declaration {
    let destination = path_of(tree, to);

    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::rename(path_of(tree, from), &destination).unwrap();

    declare_move(tree, from, to, line)
}

fn remove(tree: &Tree, path: &str, line: usize) -> Declaration {
    fs::remove_file(path_of(tree, path)).unwrap();

    Declaration::Delete {
        path: path_of(tree, path),
        origin: origin_of(line),
    }
}

fn initialize_git(tree: &Tree) {
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&tree.root)
        .status()
        .unwrap();

    assert!(status.success());
}

struct Report {
    lines: Vec<String>,
    contents: BTreeMap<String, String>,
}

fn report_of(tree: &Tree, declarations: Vec<Declaration>) -> Report {
    let root = &tree.root;
    let scope = walk_scope(&[PathBuf::from(".")], &ScopeOptions::default(), root);
    let plan = plan_fix(declarations, &scope, root).unwrap();

    let findings = plan.findings.iter().map(|finding| {
        let written = match &finding.outcome {
            Outcome::Rewritten { replacement } => replacement.clone(),
            Outcome::Deleted { target } => format!("{} (deleted)", format_path(target, root)),
            Outcome::OutOfScope { target } => {
                format!("{} (out of scope)", format_path(target, root))
            }
            Outcome::Unrewritable { target } => {
                format!("{} (unrewritable)", format_path(target, root))
            }
        };

        format!(
            "{}:{}:{}: {}{} -> {written}",
            format_path(&finding.file, root),
            finding.token.line,
            finding.token.column,
            finding.token.path,
            finding.token.suffix
        )
    });

    let unscanned = plan
        .unscanned
        .iter()
        .map(|destination| format!("{}: unscanned", format_path(destination, root)));

    let rejected = plan.rejected.iter().map(|rejection| {
        format!(
            "declaration {}: {}",
            rejection.origin.line, rejection.reason
        )
    });

    Report {
        lines: findings.chain(unscanned).chain(rejected).collect(),
        contents: plan
            .edits
            .iter()
            .map(|edit| (format_path(&edit.file, root), edit.content.clone()))
            .collect(),
    }
}

fn contents_of(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, content)| (name.to_string(), content.to_string()))
        .collect()
}

#[test]
fn rewrites_an_inbound_file_relative_reference_across_a_depth_change() {
    let tree = tree_of(&[("notes/a.md", "[b](../docs/b.md#h)\n"), ("docs/b.md", "")]);
    let moved = relocate(&tree, "docs/b.md", "docs/guide/b.md", 1);
    let report = report_of(&tree, vec![moved]);

    assert_eq!(
        report.lines,
        ["notes/a.md:1:5: ../docs/b.md#h -> ../docs/guide/b.md#h"]
    );
    assert_eq!(
        report.contents,
        contents_of(&[("notes/a.md", "[b](../docs/guide/b.md#h)\n")])
    );
}

#[test]
fn rewrites_a_working_directory_relative_reference() {
    let tree = tree_of(&[("notes/a.md", "`src/x.md`\n"), ("src/x.md", "")]);
    let moved = relocate(&tree, "src/x.md", "lib/x.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["notes/a.md:1:2: src/x.md -> lib/x.md"]
    );
}

#[test]
fn rewrites_an_absolute_reference_in_its_form() {
    let tree = tree_of(&[("docs/b.md", "")]);
    let written = |name: &str| path_of(&tree, name).display().to_string();

    fs::write(
        tree.root.join("index.md"),
        format!("see {}\n", written("docs/b.md")),
    )
    .unwrap();

    let moved = relocate(&tree, "docs/b.md", "lib/b.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        [format!(
            "index.md:1:5: {} -> {}",
            written("docs/b.md"),
            written("lib/b.md")
        )]
    );
}

#[test]
fn rewrites_beneath_a_moved_directory_and_leaves_a_sibling_prefix_alone() {
    let tree = tree_of(&[
        ("index.md", "src/a/x.md src/ab/y.md\n"),
        ("src/a/x.md", ""),
        ("src/ab/y.md", ""),
    ]);
    let moved = relocate(&tree, "src/a", "lib/a", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["index.md:1:1: src/a/x.md -> lib/a/x.md"]
    );
}

#[test]
fn re_relativizes_a_moved_files_outbound_references() {
    let tree = tree_of(&[("docs/a.md", "[b](b.md)\n"), ("docs/b.md", "")]);
    let moved = relocate(&tree, "docs/a.md", "notes/deep/a.md", 1);
    let report = report_of(&tree, vec![moved]);

    assert_eq!(
        report.lines,
        ["notes/deep/a.md:1:5: b.md -> ../../docs/b.md"]
    );
    assert_eq!(
        report.contents,
        contents_of(&[("notes/deep/a.md", "[b](../../docs/b.md)\n")])
    );
}

#[test]
fn leaves_a_moved_files_working_directory_relative_reference_alone() {
    let tree = tree_of(&[("docs/a.md", "`src/main.rs`\n"), ("src/main.rs", "")]);
    let moved = relocate(&tree, "docs/a.md", "notes/a.md", 1);

    assert!(report_of(&tree, vec![moved]).lines.is_empty());
}

#[test]
fn leaves_a_moved_files_still_resolving_parent_walk_alone() {
    let tree = tree_of(&[("a/f.md", "../x/../y.md\n"), ("y.md", "")]);
    let moved = relocate(&tree, "a/f.md", "b/f.md", 1);

    assert!(report_of(&tree, vec![moved]).lines.is_empty());
}

#[test]
fn keeps_the_mutual_reference_of_two_co_moved_files() {
    let tree = tree_of(&[("a.md", "[b](b.md)\n"), ("b.md", "[a](a.md)\n")]);
    let first = relocate(&tree, "a.md", "x/a.md", 1);
    let second = relocate(&tree, "b.md", "x/b.md", 2);

    assert!(report_of(&tree, vec![first, second]).lines.is_empty());
}

#[test]
fn composes_a_chained_pair_of_moves() {
    let tree = tree_of(&[("index.md", "see docs/a.md\n"), ("docs/a.md", "")]);
    let first = relocate(&tree, "docs/a.md", "docs/b.md", 1);
    let second = relocate(&tree, "docs/b.md", "docs/c.md", 2);

    assert_eq!(
        report_of(&tree, vec![first, second]).lines,
        ["index.md:1:5: docs/a.md -> docs/c.md"]
    );
}

#[test]
fn composes_a_directory_move_and_a_move_out_of_it() {
    let tree = tree_of(&[
        ("index.md", "d1/x.md d1/y.md\n"),
        ("d1/x.md", ""),
        ("d1/y.md", ""),
    ]);
    let first = relocate(&tree, "d1", "d2", 1);
    let second = relocate(&tree, "d2/x.md", "x.md", 2);

    assert_eq!(
        report_of(&tree, vec![first, second]).lines,
        [
            "index.md:1:1: d1/x.md -> x.md",
            "index.md:1:9: d1/y.md -> d2/y.md"
        ]
    );
}

#[test]
fn reports_a_move_followed_by_a_delete_of_its_destination_as_deleted() {
    let tree = tree_of(&[("index.md", "see docs/a.md\n"), ("docs/a.md", "")]);
    let moved = relocate(&tree, "docs/a.md", "docs/b.md", 1);
    let deleted = remove(&tree, "docs/b.md", 2);

    assert_eq!(
        report_of(&tree, vec![moved, deleted]).lines,
        ["index.md:1:5: docs/a.md -> docs/a.md (deleted)"]
    );
}

#[test]
fn rejects_a_swap_through_a_temporary_name_and_touches_nothing() {
    let tree = tree_of(&[
        ("index.md", "docs/a.md docs/b.md\n"),
        ("docs/a.md", "a"),
        ("docs/b.md", "b"),
    ]);
    let first = relocate(&tree, "docs/a.md", "docs/t.md", 1);
    let second = relocate(&tree, "docs/b.md", "docs/a.md", 2);
    let third = relocate(&tree, "docs/t.md", "docs/b.md", 3);
    let report = report_of(&tree, vec![first, second, third]);

    assert_eq!(
        report.lines,
        [
            "declaration 2: source still exists",
            "declaration 3: source still exists"
        ]
    );
    assert!(report.contents.is_empty());
}

#[test]
fn rewrites_a_bare_name_whose_sibling_moved_despite_a_working_directory_match() {
    let tree = tree_of(&[
        ("README.md", ""),
        ("docs/README.md", ""),
        ("docs/a.md", "see README.md\n"),
    ]);
    let moved = relocate(&tree, "docs/README.md", "docs/guide/README.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["docs/a.md:1:5: README.md -> guide/README.md"]
    );
}

#[test]
fn leaves_a_still_resolving_reference_alone() {
    let tree = tree_of(&[
        ("index.md", "docs/b.md\n"),
        ("docs/a.md", ""),
        ("docs/b.md", ""),
    ]);
    let moved = relocate(&tree, "docs/a.md", "docs/c.md", 1);

    assert!(report_of(&tree, vec![moved]).lines.is_empty());
}

#[test]
fn leaves_a_previously_dangling_reference_beneath_a_moved_directory_alone() {
    let tree = tree_of(&[("index.md", "d1/missing.md\n"), ("d1/x.md", "")]);
    let moved = relocate(&tree, "d1", "d2", 1);

    assert!(report_of(&tree, vec![moved]).lines.is_empty());
}

#[test]
fn reports_a_delete() {
    let tree = tree_of(&[("index.md", "see docs/a.md\n"), ("docs/a.md", "")]);
    let deleted = remove(&tree, "docs/a.md", 1);

    assert_eq!(
        report_of(&tree, vec![deleted]).lines,
        ["index.md:1:5: docs/a.md -> docs/a.md (deleted)"]
    );
}

#[test]
fn reports_a_move_into_an_ignored_directory_as_out_of_scope() {
    let tree = tree_of(&[
        (".gitignore", "ignored/\n"),
        ("index.md", "see docs/a.md\n"),
        ("docs/a.md", ""),
    ]);

    initialize_git(&tree);

    let moved = relocate(&tree, "docs/a.md", "ignored/a.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        [
            "index.md:1:5: docs/a.md -> ignored/a.md (out of scope)",
            "ignored/a.md: unscanned"
        ]
    );
}

#[test]
fn rewrites_a_move_into_scope_from_an_ignored_directory() {
    let tree = tree_of(&[
        (".gitignore", "ignored/\n"),
        ("index.md", "see ignored/a.md\n"),
        ("ignored/a.md", ""),
    ]);

    initialize_git(&tree);

    let moved = relocate(&tree, "ignored/a.md", "docs/a.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["index.md:1:5: ignored/a.md -> docs/a.md"]
    );
}

#[test]
fn reports_a_destination_with_a_space_as_unrewritable() {
    let tree = tree_of(&[("index.md", "see docs/a.md\n"), ("docs/a.md", "")]);
    let moved = relocate(&tree, "docs/a.md", "docs/my notes/a.md", 1);
    let report = report_of(&tree, vec![moved]);

    assert_eq!(
        report.lines,
        ["index.md:1:5: docs/a.md -> docs/my notes/a.md (unrewritable)"]
    );
    assert!(report.contents.is_empty());
}

#[test]
fn reports_a_rewrite_whose_surrounding_text_changes_its_reading_as_unrewritable() {
    let tree = tree_of(&[
        ("index.md", "_docs/a.md_ and docs/a.md\n"),
        ("docs/a.md", ""),
    ]);
    let moved = relocate(&tree, "docs/a.md", "_x/b.md", 1);
    let report = report_of(&tree, vec![moved]);

    assert_eq!(
        report.lines,
        [
            "index.md:1:2: docs/a.md -> _x/b.md (unrewritable)",
            "index.md:1:17: docs/a.md -> _x/b.md"
        ]
    );
    assert_eq!(
        report.contents,
        contents_of(&[("index.md", "_docs/a.md_ and _x/b.md\n")])
    );
}

#[test]
fn rejects_a_move_whose_source_still_exists_or_whose_destination_is_missing() {
    let tree = tree_of(&[
        ("index.md", "see docs/a.md\n"),
        ("docs/a.md", ""),
        ("docs/b.md", ""),
    ]);
    let existing = declare_move(&tree, "docs/a.md", "docs/b.md", 1);
    let missing = declare_move(&tree, "docs/gone.md", "docs/none.md", 2);

    assert_eq!(
        report_of(&tree, vec![existing, missing]).lines,
        [
            "declaration 1: source still exists",
            "declaration 2: destination missing"
        ]
    );
}

#[cfg(windows)]
#[test]
fn accepts_a_case_only_rename() {
    let tree = tree_of(&[("index.md", "see docs/A.md\n"), ("docs/A.md", "")]);
    let moved = relocate(&tree, "docs/A.md", "docs/a.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["index.md:1:5: docs/A.md -> docs/a.md"]
    );
}

#[test]
fn preserves_a_suffix_and_a_json_escaped_path() {
    let tree = tree_of(&[
        (
            "config.json",
            "{\"a\": \"docs\\\\a.md\", \"b\": \"docs/a.md:12\"}\n",
        ),
        ("docs/a.md", ""),
    ]);
    let moved = relocate(&tree, "docs/a.md", "lib/a.md", 1);
    let report = report_of(&tree, vec![moved]);

    assert_eq!(
        report.lines,
        [
            "config.json:1:8: docs\\\\a.md -> lib\\\\a.md",
            "config.json:1:27: docs/a.md:12 -> lib/a.md:12"
        ]
    );
    assert_eq!(
        report.contents,
        contents_of(&[(
            "config.json",
            "{\"a\": \"lib\\\\a.md\", \"b\": \"lib/a.md:12\"}\n"
        )])
    );
}
