use super::*;

fn directory() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("C:\\wd")
    } else {
        PathBuf::from("/wd")
    }
}

fn moved(from: &str, to: &str) -> (PathBuf, PathBuf) {
    (directory().join(from), directory().join(to))
}

fn parse(input: &str) -> (Vec<Declaration>, Vec<Rejected>) {
    parse_declarations(input.as_bytes(), &directory())
}

fn moves_of(declarations: &[Declaration]) -> Vec<(PathBuf, PathBuf)> {
    declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Move { from, to, .. } => Some((from.clone(), to.clone())),
            Declaration::Delete { .. } => None,
        })
        .collect()
}

fn deletes_of(declarations: &[Declaration]) -> Vec<PathBuf> {
    declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Delete { path, .. } => Some(path.clone()),
            Declaration::Move { .. } => None,
        })
        .collect()
}

fn assert_move(input: &str, from: &str, to: &str) {
    let (declarations, rejected) = parse(input);

    assert!(rejected.is_empty(), "{input}: {rejected:?}");
    assert_eq!(moves_of(&declarations), vec![moved(from, to)], "{input}");
    assert!(deletes_of(&declarations).is_empty(), "{input}");
}

fn assert_delete(input: &str, path: &str) {
    let (declarations, rejected) = parse(input);

    assert!(rejected.is_empty(), "{input}: {rejected:?}");
    assert_eq!(
        deletes_of(&declarations),
        vec![directory().join(path)],
        "{input}"
    );
    assert!(moves_of(&declarations).is_empty(), "{input}");
}

#[test]
fn parses_git_name_status_moves_and_deletes() {
    assert_move("R100\told/a.md\tnew/a.md", "old/a.md", "new/a.md");
    assert_move("R\ta\tb", "a", "b");
    assert_move("R a b", "a", "b");
    assert_move("R087 a b", "a", "b");
    assert_delete("D\tgone/a.md", "gone/a.md");
    assert_delete("D a", "a");
}

#[test]
fn decodes_c_quoted_git_paths() {
    assert_move(
        "R100\t\"a b.md\"\t\"c\\303\\251.md\"",
        "a b.md",
        "c\u{e9}.md",
    );
    assert_move("R \"a b.md\" c.md", "a b.md", "c.md");
    assert_delete("D \"a\\\"b\"", "a\"b");
}

#[test]
fn ignores_added_modified_typed_and_copied_entries() {
    let (declarations, rejected) = parse("A\ta\nM\tb\nT\tc\nC100\td\te\nM b\n");

    assert!(declarations.is_empty());
    assert!(rejected.is_empty());
}

#[test]
fn parses_gnu_mv_verbose() {
    assert_move("renamed 'old' -> 'new'", "old", "new");
    assert_move("'old' -> 'new'", "old", "new");
    assert_move("renamed 'a b' -> 'c'\\''d'", "a b", "c'd");
}

#[test]
fn parses_a_quoted_name_containing_an_arrow() {
    assert_move("renamed 'a -> b' -> 'c'", "a -> b", "c");
}

#[test]
fn parses_gnu_rm_verbose() {
    assert_delete("removed 'x/a.md'", "x/a.md");
    assert_delete("removed directory 'x'", "x");
}

#[test]
fn parses_git_mv_and_rm() {
    assert_move("Renaming old.md to new.md", "old.md", "new.md");
    assert_delete("rm 'gone.md'", "gone.md");
}

#[test]
fn parses_bsd_mv_verbose() {
    assert_move("old.md -> new.md", "old.md", "new.md");
    assert_move("a b -> c d", "a b", "c d");
}

#[test]
fn rejects_ambiguous_lines() {
    for line in ["a -> b -> c", "Renaming a to b to c", "renamed a -> b -> c"] {
        let (declarations, rejected) = parse(line);

        assert!(declarations.is_empty(), "{line}");
        assert_eq!(rejected.len(), 1, "{line}");
        assert_eq!(rejected[0].reason, "ambiguous", "{line}");
        assert_eq!(
            rejected[0].origin,
            Origin {
                line: 1,
                text: line.to_string()
            }
        );
    }
}

#[test]
fn rejects_unrecognized_lines_and_keeps_going() {
    let (declarations, rejected) = parse("hello world\n\nD a\ncopied 'a' 'b'\n");

    assert_eq!(deletes_of(&declarations), vec![directory().join("a")]);
    assert_eq!(rejected.len(), 2);
    assert_eq!(rejected[0].reason, "unrecognized");
    assert_eq!(rejected[0].origin.line, 1);
    assert_eq!(rejected[1].origin.line, 4);
}

#[test]
fn skips_created_directory_and_blank_lines_and_reads_crlf() {
    let (declarations, rejected) = parse("created directory 'e'\r\n\r\nD a\r\n");

    assert!(rejected.is_empty());
    assert_eq!(deletes_of(&declarations), vec![directory().join("a")]);
    assert_eq!(
        declarations[0],
        Declaration::Delete {
            path: directory().join("a"),
            origin: Origin {
                line: 3,
                text: "D a".to_string()
            },
        }
    );
}

#[test]
fn completes_a_cross_filesystem_copy_when_removed() {
    let (declarations, rejected) = parse("copied 'a' -> 'b'\nremoved 'a'\n");

    assert!(rejected.is_empty());
    assert_eq!(moves_of(&declarations), vec![moved("a", "b")]);
    assert!(deletes_of(&declarations).is_empty());
}

#[test]
fn completes_a_cross_filesystem_directory_move_per_file() {
    let (declarations, rejected) = parse(
        "created directory 'e'\n\
         copied 'd/a.md' -> 'e/a.md'\n\
         copied 'd/b.md' -> 'e/b.md'\n\
         removed 'd/a.md'\n\
         removed 'd/b.md'\n\
         removed directory 'd'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![moved("d/a.md", "e/a.md"), moved("d/b.md", "e/b.md")]
    );
    assert!(deletes_of(&declarations).is_empty());
}

#[test]
fn completes_pending_copies_at_a_removed_directory() {
    let (declarations, rejected) = parse(
        "copied 'd/a.md' -> 'e/a.md'\n\
         removed directory 'd'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(moves_of(&declarations), vec![moved("d/a.md", "e/a.md")]);
    assert!(deletes_of(&declarations).is_empty());
}

#[test]
fn drops_a_lone_copy() {
    let (declarations, rejected) = parse("copied 'a' -> 'b'\n");

    assert!(declarations.is_empty());
    assert!(rejected.is_empty());
}

#[test]
fn keeps_rm_recursive_output_as_deletes() {
    let (declarations, _) = parse("removed 'd/a.md'\nremoved directory 'd'\n");

    assert_eq!(
        deletes_of(&declarations),
        vec![directory().join("d/a.md"), directory().join("d")]
    );
}

#[test]
fn parses_name_status_records_separated_by_nul() {
    let input = b"R100\0old/a.md\0new/a.md\0D\0gone.md\0M\0kept.md\0C075\0x\0y\0A\0added.md\0";
    let (declarations, rejected) = parse_declarations(input, &directory());

    assert!(rejected.is_empty());
    assert_eq!(moves_of(&declarations), vec![moved("old/a.md", "new/a.md")]);
    assert_eq!(deletes_of(&declarations), vec![directory().join("gone.md")]);
    assert_eq!(
        declarations[1],
        Declaration::Delete {
            path: directory().join("gone.md"),
            origin: Origin {
                line: 2,
                text: "D\tgone.md".to_string()
            },
        }
    );
}

#[test]
fn rejects_an_unknown_or_truncated_record() {
    let (declarations, rejected) = parse_declarations(b"D\0a\0X\0b\0", &directory());

    assert_eq!(deletes_of(&declarations), vec![directory().join("a")]);
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].origin.line, 2);

    let (declarations, rejected) = parse_declarations(b"R\0a\0", &directory());

    assert!(declarations.is_empty());
    assert_eq!(rejected.len(), 1);
}

#[test]
fn normalizes_declaration_paths() {
    assert_move("R a/../b/./c.md d.md", "b/c.md", "d.md");
}

#[test]
fn moves_into_an_existing_directory_holding_the_source_name() {
    let temporary = tempfile::TempDir::new().unwrap();
    let root = dunce::canonicalize(temporary.path()).unwrap();

    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::create_dir_all(root.join("empty")).unwrap();
    std::fs::write(root.join("docs").join("a.md"), "").unwrap();

    let (declarations, rejected) = parse_declarations(b"R\ta.md\tdocs\nR\tb.md\tempty\n", &root);

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![
            (root.join("a.md"), root.join("docs").join("a.md")),
            (root.join("b.md"), root.join("empty")),
        ]
    );
}

#[cfg(windows)]
#[test]
fn reads_msys_and_drive_declaration_paths() {
    let (declarations, rejected) = parse("R\t/c/x/a.md\tC:\\x\\b.md\n");

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![(PathBuf::from("C:\\x\\a.md"), PathBuf::from("C:\\x\\b.md"))]
    );
}

#[cfg(not(windows))]
#[test]
fn reads_absolute_declaration_paths() {
    let (declarations, rejected) = parse("R\t/x/a.md\t/x/b.md\n");

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![(PathBuf::from("/x/a.md"), PathBuf::from("/x/b.md"))]
    );
}
