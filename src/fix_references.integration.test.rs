use super::*;
use crate::parse_declarations::{parse_declarations, Origin};
use crate::tree_of::{link_directory, tree_of, Tree};
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
        snapshot: None,
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
    let removed = path_of(tree, path);

    if removed.is_dir() {
        fs::remove_dir_all(removed).unwrap();
    } else {
        fs::remove_file(removed).unwrap();
    }

    Declaration::Delete {
        path: path_of(tree, path),
        origin: origin_of(line),
    }
}

fn in_snapshot(declaration: Declaration, snapshot: usize) -> Declaration {
    match declaration {
        Declaration::Move { from, to, origin } => Declaration::Move {
            from,
            to,
            origin: Origin {
                snapshot: Some(snapshot),
                ..origin
            },
        },
        Declaration::Delete { path, origin } => Declaration::Delete {
            path,
            origin: Origin {
                snapshot: Some(snapshot),
                ..origin
            },
        },
    }
}

fn parsed_of(tree: &Tree, text: &str) -> Vec<Declaration> {
    let (declarations, rejected) = parse_declarations(
        text.as_bytes(),
        &tree.root,
        &|path: &Path| path.is_dir(),
        &|path: &Path| path.exists(),
    );

    assert!(rejected.is_empty(), "{rejected:?}");

    declarations
}

fn rename(tree: &Tree, from: &str, to: &str) {
    let destination = path_of(tree, to);

    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::rename(path_of(tree, from), destination).unwrap();
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
    let plan = plan_fix(declarations, &scope, root);

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
fn rewrites_a_swap_through_a_temporary_name_both_ways() {
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
            "index.md:1:1: docs/a.md -> docs/b.md",
            "index.md:1:11: docs/b.md -> docs/a.md"
        ]
    );
    assert_eq!(
        report.contents,
        contents_of(&[("index.md", "docs/b.md docs/a.md\n")])
    );
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
fn rewrites_only_the_renamed_components_case() {
    let tree = tree_of(&[
        (
            "index.md",
            "see DOCS/A.md
",
        ),
        ("docs/A.md", ""),
    ]);
    let moved = relocate(&tree, "docs/A.md", "docs/a.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["index.md:1:5: DOCS/A.md -> DOCS/a.md"]
    );
}

#[test]
fn drops_an_exact_round_trip() {
    let tree = tree_of(&[
        (
            "index.md",
            "see docs/A.md and [x](docs/a.md)
",
        ),
        ("docs/a.md", ""),
    ]);
    let first = relocate(&tree, "docs/a.md", "docs/b.md", 1);
    let second = relocate(&tree, "docs/b.md", "docs/a.md", 2);

    assert!(report_of(&tree, vec![first, second]).lines.is_empty());
}

#[test]
fn rewrites_a_root_files_references_after_it_moves_down() {
    let tree = tree_of(&[
        (
            "CONTRIBUTING.md",
            "[g](g.md) [i](images/p.png)
",
        ),
        ("g.md", ""),
        ("images/p.png", ""),
    ]);
    let moved = relocate(&tree, "CONTRIBUTING.md", "docs/CONTRIBUTING.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        [
            "docs/CONTRIBUTING.md:1:5: g.md -> ../g.md",
            "docs/CONTRIBUTING.md:1:15: images/p.png -> ../images/p.png"
        ]
    );
}

#[test]
fn rewrites_a_sibling_reference_after_the_target_moves_up_to_the_root() {
    let tree = tree_of(&[
        ("docs/a.md", ""),
        (
            "docs/x.md",
            "see [a](a.md)
",
        ),
    ]);
    let moved = relocate(&tree, "docs/a.md", "a.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["docs/x.md:1:9: a.md -> ../a.md"]
    );
}

#[test]
fn turns_an_overwritten_destination_into_a_delete_of_its_source() {
    let tree = tree_of(&[
        (
            "i.md",
            "d/a.md d/b.md
",
        ),
        ("d/a.md", "a"),
        ("d/b.md", "b"),
    ]);
    let first = relocate(&tree, "d/a.md", "d/c.md", 1);
    let second = relocate(&tree, "d/b.md", "d/c.md", 2);

    assert_eq!(
        report_of(&tree, vec![first, second]).lines,
        [
            "i.md:1:1: d/a.md -> d/a.md (deleted)",
            "i.md:1:8: d/b.md -> d/c.md"
        ]
    );
}

#[test]
fn applies_one_listings_records_simultaneously() {
    let tree = tree_of(&[
        (
            "i.md",
            "a.md b.md
",
        ),
        ("a.md", "a"),
        ("b.md", "b"),
    ]);

    fs::rename(path_of(&tree, "b.md"), path_of(&tree, "c.md")).unwrap();
    fs::rename(path_of(&tree, "a.md"), path_of(&tree, "b.md")).unwrap();

    let declarations = vec![
        in_snapshot(declare_move(&tree, "a.md", "b.md", 1), 1),
        in_snapshot(declare_move(&tree, "b.md", "c.md", 2), 1),
    ];

    assert_eq!(
        report_of(&tree, declarations).lines,
        ["i.md:1:1: a.md -> b.md", "i.md:1:6: b.md -> c.md"]
    );
}

#[test]
fn carries_an_earlier_destination_along_with_its_moved_ancestor() {
    let tree = tree_of(&[
        (
            "i.md",
            "y.md d1/x.md
",
        ),
        ("y.md", ""),
        ("d1/x.md", ""),
    ]);
    let first = relocate(&tree, "d1", "d2", 1);
    let second = relocate(&tree, "y.md", "d2/y.md", 2);
    let third = relocate(&tree, "d2", "d3", 3);

    assert_eq!(
        report_of(&tree, vec![first, second, third]).lines,
        ["i.md:1:1: y.md -> d3/y.md", "i.md:1:6: d1/x.md -> d3/x.md"]
    );
}

#[test]
fn deletes_an_earlier_moves_source_when_an_ancestor_of_its_destination_is_deleted() {
    let tree = tree_of(&[
        (
            "i.md", "y.md
",
        ),
        ("y.md", ""),
    ]);
    let moved = relocate(&tree, "y.md", "d/y.md", 1);
    let deleted = remove(&tree, "d", 2);

    assert_eq!(
        report_of(&tree, vec![moved, deleted]).lines,
        ["i.md:1:1: y.md -> y.md (deleted)"]
    );
}

#[test]
fn reports_an_unreadable_file_and_fixes_the_rest() {
    let tree = tree_of(&[
        (
            "index.md",
            "see docs/a.md
",
        ),
        (
            "locked.md",
            "see docs/a.md
",
        ),
        ("docs/a.md", ""),
    ]);
    let moved = relocate(&tree, "docs/a.md", "docs/b.md", 1);
    let scope = walk_scope(&[PathBuf::from(".")], &ScopeOptions::default(), &tree.root);

    let Some(_lock) = crate::tree_of::lock_file(&tree.root.join("locked.md")) else {
        eprintln!("skipped: the file stays readable for this user");

        return;
    };

    let plan = plan_fix(vec![moved], &scope, &tree.root);

    assert_eq!(plan.errors.len(), 1, "{:?}", plan.errors);
    assert!(
        plan.errors[0].starts_with("locked.md: "),
        "{:?}",
        plan.errors
    );
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(
        plan.edits[0].content,
        "see docs/b.md
"
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

#[test]
fn deletes_the_content_a_move_overwrites_inside_a_moved_directory() {
    let tree = tree_of(&[("i.md", "d1/y.md f.md\n"), ("d1/y.md", "y"), ("f.md", "f")]);
    let first = relocate(&tree, "d1", "d2", 1);
    let second = relocate(&tree, "f.md", "d2/y.md", 2);

    assert_eq!(
        report_of(&tree, vec![first, second]).lines,
        [
            "i.md:1:1: d1/y.md -> d1/y.md (deleted)",
            "i.md:1:9: f.md -> d2/y.md"
        ]
    );
}

#[test]
fn ignores_the_per_file_lines_git_mv_prints_after_a_directory() {
    let tree = tree_of(&[("i.md", "a.md d1/b.md\n"), ("a.md", ""), ("d1/b.md", "")]);

    rename(&tree, "a.md", "d1/a.md");
    rename(&tree, "d1", "d2");

    let declarations = parsed_of(
        &tree,
        "Renaming a.md to d1/a.md\nRenaming d1 to d2\nRenaming d1/a.md to d2/a.md\nRenaming d1/b.md to d2/b.md\n",
    );

    assert_eq!(
        report_of(&tree, declarations).lines,
        ["i.md:1:1: a.md -> d2/a.md", "i.md:1:6: d1/b.md -> d2/b.md"]
    );
}

#[test]
fn composes_a_cross_filesystem_directory_move_after_a_move_into_it() {
    let tree = tree_of(&[("i.md", "a.md d1/b.md\n"), ("a.md", ""), ("d1/b.md", "")]);

    rename(&tree, "a.md", "d1/a.md");
    rename(&tree, "d1", "d2");

    let declarations = parsed_of(
        &tree,
        "renamed 'a.md' -> 'd1/a.md'\ncreated directory 'd2'\ncopied 'd1/a.md' -> 'd2/a.md'\ncopied 'd1/b.md' -> 'd2/b.md'\nremoved 'd1/a.md'\nremoved 'd1/b.md'\nremoved directory 'd1'\n",
    );

    assert_eq!(
        report_of(&tree, declarations).lines,
        ["i.md:1:1: a.md -> d2/a.md", "i.md:1:6: d1/b.md -> d2/b.md"]
    );
}

#[cfg(windows)]
#[test]
fn composes_a_case_rename_through_a_temporary_directory() {
    let tree = tree_of(&[("i.md", "Docs/readme.md\n"), ("Docs/readme.md", "")]);
    let first = relocate(&tree, "Docs/readme.md", "Docs/README.md", 1);
    let second = relocate(&tree, "Docs", "tmp", 2);
    let third = relocate(&tree, "tmp", "docs", 3);

    assert_eq!(
        report_of(&tree, vec![first, second, third]).lines,
        ["i.md:1:1: Docs/readme.md -> docs/README.md"]
    );
}

#[cfg(windows)]
#[test]
fn rewrites_a_staged_case_rename() {
    let tree = tree_of(&[("i.md", "Docs/readme.md\n"), ("Docs/readme.md", "")]);

    rename(&tree, "Docs/readme.md", "Docs/README.md");
    rename(&tree, "Docs", "docs");

    let declarations = parsed_of(&tree, "R100\tDocs/readme.md\tdocs/README.md\n");

    assert_eq!(
        report_of(&tree, declarations).lines,
        ["i.md:1:1: Docs/readme.md -> docs/README.md"]
    );
}

#[test]
fn composes_a_listing_against_an_earlier_move() {
    let tree = tree_of(&[("i.md", "a.md x.md\n"), ("a.md", "a"), ("x.md", "x")]);

    rename(&tree, "a.md", "b.md");
    rename(&tree, "b.md", "c.md");
    rename(&tree, "x.md", "b.md");

    let declarations = parsed_of(
        &tree,
        "renamed 'a.md' -> 'b.md'\nR100\tx.md\tb.md\nR100\tb.md\tc.md\n",
    );

    assert_eq!(
        report_of(&tree, declarations).lines,
        ["i.md:1:1: a.md -> c.md", "i.md:1:6: x.md -> b.md"]
    );
}

#[test]
fn reports_a_reference_a_sibling_now_shadows_as_unrewritable() {
    let tree = tree_of(&[
        ("README.md", ""),
        ("lib/README.md", ""),
        ("docs/x.md", "README.md\n"),
    ]);
    let moved = relocate(&tree, "docs/x.md", "lib/x.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["lib/x.md:1:1: README.md -> README.md (unrewritable)"]
    );
}

#[test]
fn reports_a_reference_a_new_arrival_would_capture_as_unrewritable() {
    let tree = tree_of(&[
        ("README.md", ""),
        ("other.md", ""),
        ("docs/x.md", "README.md\n"),
    ]);
    let moved = relocate(&tree, "other.md", "docs/README.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["docs/x.md:1:1: README.md -> README.md (unrewritable)"]
    );
}

#[test]
fn rewrites_a_move_declared_through_an_in_tree_link() {
    let tree = tree_of(&[("i.md", "[a](lnk/a.md)\n"), ("real/a.md", "")]);

    if !link_directory(&tree.root.join("real"), &tree.root.join("lnk")) {
        eprintln!("skipped: links cannot be created here");

        return;
    }

    let moved = relocate(&tree, "lnk/a.md", "lnk/b.md", 1);

    assert_eq!(
        report_of(&tree, vec![moved]).lines,
        ["i.md:1:5: lnk/a.md -> lnk/b.md"]
    );
}

#[test]
fn repairs_an_intermediate_name_and_deletes_beneath_a_moved_directory() {
    let tree = tree_of(&[
        ("i.md", "b.md d2/x.md\n"),
        ("a.md", ""),
        ("d1/x.md", ""),
        ("d1/z.md", ""),
    ]);
    let first = relocate(&tree, "a.md", "b.md", 1);
    let second = relocate(&tree, "b.md", "c.md", 2);
    let third = relocate(&tree, "d1", "d2", 3);
    let fourth = remove(&tree, "d2/x.md", 4);

    assert_eq!(
        report_of(&tree, vec![first, second, third, fourth]).lines,
        [
            "i.md:1:1: b.md -> c.md",
            "i.md:1:6: d2/x.md -> d2/x.md (deleted)"
        ]
    );
}

#[test]
fn infers_a_directory_from_a_listing_that_moved_its_files() {
    let tree = tree_of(&[("i.md", "src/a/ src/a\n"), ("src/a/x.md", "")]);

    rename(&tree, "src/a/x.md", "lib/x.md");
    fs::remove_dir(path_of(&tree, "src/a")).unwrap();

    let declarations = parsed_of(&tree, "R100\tsrc/a/x.md\tlib/x.md\n");

    assert_eq!(
        report_of(&tree, declarations).lines,
        ["i.md:1:1: src/a/ -> lib/", "i.md:1:8: src/a -> ./lib"]
    );
}

#[test]
fn reports_a_directory_whose_files_were_all_deleted() {
    let tree = tree_of(&[
        ("i.md", "d1/s/ d1/a.md\n"),
        ("d1/a.md", ""),
        ("d1/s/s.md", ""),
    ]);

    rename(&tree, "d1", "d2");
    fs::remove_dir_all(path_of(&tree, "d2/s")).unwrap();

    let declarations = parsed_of(
        &tree,
        "Renaming d1 to d2\nRenaming d1/a.md to d2/a.md\nRenaming d1/s/s.md to d2/s/s.md\nrm 'd2/s/s.md'\n",
    );

    assert_eq!(
        report_of(&tree, declarations).lines,
        [
            "i.md:1:1: d1/s/ -> d1/s (deleted)",
            "i.md:1:7: d1/a.md -> d2/a.md"
        ]
    );
}

#[test]
fn accepts_a_name_refilled_after_its_content_moved_away() {
    let tree = tree_of(&[("i.md", "a.md n.md\n"), ("a.md", "a"), ("n.md", "n")]);
    let first = relocate(&tree, "a.md", "a-old.md", 1);
    let second = relocate(&tree, "n.md", "a.md", 2);

    assert_eq!(
        report_of(&tree, vec![first, second]).lines,
        ["i.md:1:1: a.md -> a-old.md", "i.md:1:6: n.md -> a.md"]
    );
}
