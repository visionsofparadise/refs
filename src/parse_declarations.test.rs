use super::*;

fn directory() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("C:\\wd")
    } else {
        PathBuf::from("/wd")
    }
}

fn under(path: &str) -> PathBuf {
    path.split('/')
        .fold(directory(), |joined, name| joined.join(name))
}

fn elsewhere(path: &str) -> PathBuf {
    let rest = path.strip_prefix("/d").unwrap();

    if cfg!(windows) {
        PathBuf::from(format!("D:{}", rest.replace('/', "\\")))
    } else {
        PathBuf::from(path)
    }
}

fn parse_with(
    input: &[u8],
    is_directory: &dyn Fn(&Path) -> bool,
) -> (Vec<Declaration>, Vec<Rejected>) {
    parse_declarations(input, &directory(), is_directory)
}

fn parse(input: &str) -> (Vec<Declaration>, Vec<Rejected>) {
    parse_with(input.as_bytes(), &|_| false)
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
    assert_eq!(
        moves_of(&declarations),
        vec![(under(from), under(to))],
        "{input}"
    );
    assert!(deletes_of(&declarations).is_empty(), "{input}");
}

fn assert_delete(input: &str, path: &str) {
    let (declarations, rejected) = parse(input);

    assert!(rejected.is_empty(), "{input}: {rejected:?}");
    assert_eq!(deletes_of(&declarations), vec![under(path)], "{input}");
    assert!(moves_of(&declarations).is_empty(), "{input}");
}

fn reasons_of(rejected: &[Rejected]) -> Vec<(usize, &str)> {
    rejected
        .iter()
        .map(|entry| (entry.origin.line, entry.reason.as_str()))
        .collect()
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
fn parses_the_captured_git_name_status_output() {
    let input = "R100\ta b.md\ta c.md\n\
                 D\t\"back\\\\slash.md\"\n\
                 R100\t\"caf\\303\\251.md\"\tcafe2.md\n\
                 R100\tdir/k.md\td3/dir2/k.md\n\
                 R100\t-dash.md\tdash.md\n\
                 D\tgone b.md\n\
                 D\tgone's.md\n\
                 R100\tit's.md\tits2.md\n\
                 R100\t\"nl\\nx.md\"\tnl2.md\n\
                 R100\t\"q\\\"x.md\"\tq2.md\n\
                 R100\tsub/in.md\tsub/in2.md\n\
                 D\t\"tab\\tx.md\"\n\
                 R100\tx to y.md\tx to z.md\n";
    let (declarations, rejected) = parse_with(input.as_bytes(), &|_| true);

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (under("a b.md"), under("a c.md")),
            (under("caf\u{e9}.md"), under("cafe2.md")),
            (under("dir/k.md"), under("d3/dir2/k.md")),
            (under("-dash.md"), under("dash.md")),
            (under("it's.md"), under("its2.md")),
            (under("nl\nx.md"), under("nl2.md")),
            (under("q\"x.md"), under("q2.md")),
            (under("sub/in.md"), under("sub/in2.md")),
            (under("x to y.md"), under("x to z.md")),
        ]
    );
    assert_eq!(
        deletes_of(&declarations),
        vec![
            under("back/slash.md"),
            under("gone b.md"),
            under("gone's.md"),
            under("tab\tx.md"),
        ]
    );
}

#[test]
fn parses_the_captured_nul_separated_records() {
    let input = b"R100\0a b.md\0a c.md\0R100\0caf\xc3\xa9.md\0cafe2.md\0D\0gone.md\0R100\0nl\nx.md\0nl2.md\0R100\0x to y.md\0x to z.md\0";
    let (declarations, rejected) = parse_with(input, &|_| true);

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (under("a b.md"), under("a c.md")),
            (under("caf\u{e9}.md"), under("cafe2.md")),
            (under("nl\nx.md"), under("nl2.md")),
            (under("x to y.md"), under("x to z.md")),
        ]
    );
    assert_eq!(deletes_of(&declarations), vec![under("gone.md")]);
    assert_eq!(
        declarations[2],
        Declaration::Delete {
            path: under("gone.md"),
            origin: Origin {
                line: 3,
                text: "D\tgone.md".to_string()
            },
        }
    );
}

#[test]
fn rejects_an_unmerged_record_and_keeps_parsing() {
    let (declarations, rejected) =
        parse_with(b"U\0conflict.md\0R100\0a.md\0b.md\0D\0c.md\0", &|_| false);

    assert_eq!(
        moves_of(&declarations),
        vec![(under("a.md"), under("b.md"))]
    );
    assert_eq!(deletes_of(&declarations), vec![under("c.md")]);
    assert_eq!(reasons_of(&rejected), vec![(1, "unrecognized")]);
    assert_eq!(rejected[0].origin.text, "U\tconflict.md");
}

#[test]
fn rejects_a_truncated_record_and_ignores_a_trailing_newline() {
    let (declarations, rejected) = parse_with(b"R\0a\0", &|_| false);

    assert!(declarations.is_empty());
    assert_eq!(rejected.len(), 1);

    let (declarations, rejected) = parse_with(b"D\0a\0\n", &|_| false);

    assert!(rejected.is_empty());
    assert_eq!(deletes_of(&declarations), vec![under("a")]);

    let (declarations, rejected) = parse_with(b"D\0a\n", &|_| false);

    assert!(rejected.is_empty());
    assert_eq!(deletes_of(&declarations), vec![under("a")]);
}

#[test]
fn skips_added_modified_and_copied_records() {
    let (declarations, rejected) = parse_with(b"M\0kept.md\0C075\0x\0y\0A\0added.md\0", &|_| false);

    assert!(declarations.is_empty());
    assert!(rejected.is_empty());
}

#[test]
fn parses_gnu_mv_verbose() {
    assert_move("renamed 'old' -> 'new'", "old", "new");
    assert_move("'old' -> 'new'", "old", "new");
    assert_move("renamed 'a b' -> 'c'\\''d'", "a b", "c'd");
    assert_move("renamed $'a\\nb.md' -> 'c.md'", "a\nb.md", "c.md");
    assert_move(
        "renamed 'caf'$'\\303\\251''.md' -> 'out.md'",
        "caf\u{e9}.md",
        "out.md",
    );
}

#[test]
fn parses_the_gnu_double_quoted_form() {
    assert_move(
        "renamed \"it's.md\" -> \"out/it's.md\"",
        "it's.md",
        "out/it's.md",
    );
    assert_delete("removed \"d/it's.md\"", "d/it's.md");
}

#[test]
fn ignores_a_gnu_backup_tail() {
    assert_move("renamed 'w.md' -> 'z.md' (backup: 'z.md~')", "w.md", "z.md");
    assert_move(
        "renamed 'w3.md' -> 'z.md' (backup: 'z.md.~1~')",
        "w3.md",
        "z.md",
    );
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
fn parses_git_mv_and_rm_taking_git_paths_raw() {
    assert_move("Renaming old.md to new.md", "old.md", "new.md");
    assert_move("Renaming a b.md to a c.md", "a b.md", "a c.md");
    assert_move("Renaming it's.md to its2.md", "it's.md", "its2.md");
    assert_move("Renaming q\"x.md to q2.md", "q\"x.md", "q2.md");
    assert_delete("rm 'gone.md'", "gone.md");
    assert_delete("rm 'gone b.md'", "gone b.md");
    assert_delete("rm 'gone's.md'", "gone's.md");
    assert_delete("rm 'tab\tx.md'", "tab\tx.md");
}

#[test]
fn parses_the_captured_git_mv_output() {
    let input = "Renaming a b.md to a c.md\n\
                 Renaming it's.md to its2.md\n\
                 Renaming q\"x.md to q2.md\n\
                 Renaming nl\n\
                 x.md to nl2.md\n\
                 Renaming caf\u{e9}.md to cafe2.md\n\
                 Renaming x to y.md to x to z.md\n\
                 Renaming -dash.md to dash.md\n\
                 Renaming dir to dir2\n\
                 Renaming dir/k.md to dir2/k.md\n\
                 Renaming dir2 to d3/dir2\n\
                 Renaming dir2/k.md to d3/dir2/k.md\n";
    let (declarations, rejected) = parse_with(input.as_bytes(), &|_| true);

    assert_eq!(
        reasons_of(&rejected),
        vec![(4, "unrecognized"), (5, "unrecognized"), (7, "ambiguous")]
    );
    assert_eq!(
        moves_of(&declarations),
        vec![
            (under("a b.md"), under("a c.md")),
            (under("it's.md"), under("its2.md")),
            (under("q\"x.md"), under("q2.md")),
            (under("caf\u{e9}.md"), under("cafe2.md")),
            (under("-dash.md"), under("dash.md")),
            (under("dir"), under("dir2")),
            (under("dir/k.md"), under("dir2/k.md")),
            (under("dir2"), under("d3/dir2")),
            (under("dir2/k.md"), under("d3/dir2/k.md")),
        ]
    );
}

#[test]
fn parses_the_captured_git_rm_output() {
    let (declarations, rejected) = parse("rm 'gone b.md'\nrm 'gone's.md'\nrm 'tab\tx.md'\n");

    assert!(rejected.is_empty());
    assert_eq!(
        deletes_of(&declarations),
        vec![under("gone b.md"), under("gone's.md"), under("tab\tx.md")]
    );
}

#[test]
fn parses_bsd_mv_verbose_splitting_raw() {
    assert_move("old.md -> new.md", "old.md", "new.md");
    assert_move("a b -> c d", "a b", "c d");
    assert_move("src\\a.md -> src\\b.md", "src/a.md", "src/b.md");
    assert_move("a'b'c.md -> d.md", "a'b'c.md", "d.md");
}

#[test]
fn splits_the_plain_space_form_raw() {
    assert_move("R a'b'c.md d.md", "a'b'c.md", "d.md");
    assert_move("R src\\a.md src\\b.md", "src/a.md", "src/b.md");
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

    assert_eq!(deletes_of(&declarations), vec![under("a")]);
    assert_eq!(
        reasons_of(&rejected),
        vec![(1, "unrecognized"), (4, "unrecognized")]
    );
}

#[test]
fn rejects_a_path_the_grammar_gives_no_candidate() {
    let (declarations, rejected) = parse("R a:b.md c.md\nD C:rel.md\n");

    assert!(declarations.is_empty());
    assert_eq!(
        reasons_of(&rejected),
        vec![(1, "unsupported path"), (2, "unsupported path")]
    );

    #[cfg(windows)]
    {
        let (declarations, rejected) = parse("R /tmp/x.md y.md\nD \\\\server\\share\\a.md\nD /c\n");

        assert!(declarations.is_empty());
        assert_eq!(rejected.len(), 3);
        assert!(rejected
            .iter()
            .all(|entry| entry.reason == "unsupported path"));
    }

    #[cfg(not(windows))]
    {
        let (declarations, rejected) = parse("D C:\\x\\a.md\n");

        assert!(declarations.is_empty());
        assert_eq!(reasons_of(&rejected), vec![(1, "unsupported path")]);
    }
}

#[test]
fn ignores_a_bom_trailing_whitespace_carriage_returns_and_blank_lines() {
    let (declarations, rejected) = parse_with(
        b"\xef\xbb\xbfcreated directory 'e'\r\n\r\nD a  \r\n",
        &|_| false,
    );

    assert!(rejected.is_empty());
    assert_eq!(
        declarations,
        vec![Declaration::Delete {
            path: under("a"),
            origin: Origin {
                line: 3,
                text: "D a".to_string()
            },
        }]
    );
}

#[test]
fn completes_a_cross_filesystem_copy_at_its_copied_line() {
    let (declarations, rejected) = parse("copied 'a' -> 'b'\nD c\nremoved 'a'\n");

    assert!(rejected.is_empty());
    assert_eq!(
        declarations,
        vec![
            Declaration::Move {
                from: under("a"),
                to: under("b"),
                origin: Origin {
                    line: 1,
                    text: "copied 'a' -> 'b'".to_string()
                },
            },
            Declaration::Delete {
                path: under("c"),
                origin: Origin {
                    line: 2,
                    text: "D c".to_string()
                },
            },
        ]
    );
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
        vec![
            (under("d/a.md"), under("e/a.md")),
            (under("d/b.md"), under("e/b.md"))
        ]
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
    assert_eq!(
        moves_of(&declarations),
        vec![(under("d/a.md"), under("e/a.md"))]
    );
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

    assert_eq!(deletes_of(&declarations), vec![under("d/a.md"), under("d")]);
}

#[test]
fn parses_the_captured_cross_filesystem_output_with_an_empty_directory() {
    let input = "created directory '/d/tmp.CvyuD3uQK8/d'\n\
                 created directory '/d/tmp.CvyuD3uQK8/d/sub'\n\
                 copied 'd/sub/b.md' -> '/d/tmp.CvyuD3uQK8/d/sub/b.md'\n\
                 created directory '/d/tmp.CvyuD3uQK8/d/empty'\n\
                 copied 'd/a.md' -> '/d/tmp.CvyuD3uQK8/d/a.md'\n\
                 copied \"d/it's.md\" -> \"/d/tmp.CvyuD3uQK8/d/it's.md\"\n\
                 removed \"d/it's.md\"\n\
                 removed directory 'd/empty'\n\
                 removed 'd/sub/b.md'\n\
                 removed directory 'd/sub'\n\
                 removed 'd/a.md'\n\
                 removed directory 'd'\n\
                 copied 'x.md' -> '/d/tmp.CvyuD3uQK8/y.md'\n\
                 removed 'x.md'\n\
                 copied '/d/tmp.CvyuD3uQK8/y.md' -> './z.md'\n\
                 removed '/d/tmp.CvyuD3uQK8/y.md'\n\
                 renamed 'pkg' -> 'vendor/pkg'\n\
                 renamed 'w.md' -> 'z.md' (backup: 'z.md~')\n\
                 renamed 'w3.md' -> 'z.md' (backup: 'z.md.~1~')\n\
                 renamed 'caf'$'\\303\\251''.md' -> 'out.md'\n\
                 renamed 'caf\u{e9}.md' -> 'out2.md'\n\
                 renamed 'q.md' -> 'q2.md'\n\
                 removed directory 'vendor/pkg/pkg'\n\
                 removed directory 'vendor/pkg'\n\
                 removed directory 'vendor'\n";
    let (declarations, rejected) = parse_with(input.as_bytes(), &|_| true);
    let target = |path: &str| elsewhere(&format!("/d/tmp.CvyuD3uQK8{path}"));

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (under("d/sub/b.md"), target("/d/sub/b.md")),
            (under("d/a.md"), target("/d/a.md")),
            (under("d/it's.md"), target("/d/it's.md")),
            (under("d/empty"), target("/d/empty")),
            (under("x.md"), target("/y.md")),
            (target("/y.md"), under("z.md")),
            (under("pkg"), under("vendor/pkg")),
            (under("w.md"), under("z.md")),
            (under("w3.md"), under("z.md")),
            (under("caf\u{e9}.md"), under("out.md")),
            (under("caf\u{e9}.md"), under("out2.md")),
            (under("q.md"), under("q2.md")),
        ]
    );
    assert_eq!(
        deletes_of(&declarations),
        vec![
            under("vendor/pkg/pkg"),
            under("vendor/pkg"),
            under("vendor")
        ]
    );
}

#[test]
fn parses_the_captured_cross_filesystem_output_of_git_bash() {
    let input = "copied 'x.md' -> '/d/refs-review-23961/x.md'\n\
                 removed 'x.md'\n\
                 created directory '/d/refs-review-23961/d'\n\
                 created directory '/d/refs-review-23961/d/sub'\n\
                 copied 'd/sub/b.md' -> '/d/refs-review-23961/d/sub/b.md'\n\
                 copied 'd/a.md' -> '/d/refs-review-23961/d/a.md'\n\
                 removed 'd/a.md'\n\
                 removed 'd/sub/b.md'\n\
                 removed directory 'd/sub'\n\
                 removed directory 'd'\n";
    let (declarations, rejected) = parse(input);
    let target = |path: &str| elsewhere(&format!("/d/refs-review-23961{path}"));

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (under("x.md"), target("/x.md")),
            (under("d/sub/b.md"), target("/d/sub/b.md")),
            (under("d/a.md"), target("/d/a.md")),
        ]
    );
    assert!(deletes_of(&declarations).is_empty());
}

#[test]
fn drops_a_removed_directory_beneath_a_copied_root_with_no_created_line() {
    let (declarations, rejected) = parse(
        "copied 'd/a.md' -> '/d/t/d/a.md'\n\
         removed 'd/a.md'\n\
         removed directory 'd/empty'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(moves_of(&declarations).len(), 1);
    assert!(deletes_of(&declarations).is_empty());
}

#[test]
fn moves_an_empty_top_level_directory_named_by_a_created_line() {
    let (declarations, rejected) = parse(
        "created directory '/d/t/empty'\n\
         removed directory 'empty'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![(under("empty"), elsewhere("/d/t/empty"))]
    );
}

#[test]
fn moves_into_an_existing_directory_only_for_the_plain_space_form() {
    let directory_of = |path: &Path| path == under("docs");

    let (declarations, _) = parse_with(b"R a.md docs\n", &directory_of);

    assert_eq!(
        moves_of(&declarations),
        vec![(under("a.md"), under("docs/a.md"))]
    );

    let (declarations, _) = parse_with(b"R a.md notes\n", &directory_of);

    assert_eq!(
        moves_of(&declarations),
        vec![(under("a.md"), under("notes"))]
    );

    for line in [
        "R\ta.md\tdocs",
        "renamed 'a.md' -> 'docs'",
        "'a.md' -> 'docs'",
        "a.md -> docs",
        "Renaming a.md to docs",
    ] {
        let (declarations, _) = parse_with(line.as_bytes(), &directory_of);

        assert_eq!(
            moves_of(&declarations),
            vec![(under("a.md"), under("docs"))],
            "{line}"
        );
    }

    let (declarations, _) = parse_with(b"R100\0a.md\0docs\0", &directory_of);

    assert_eq!(
        moves_of(&declarations),
        vec![(under("a.md"), under("docs"))]
    );
}

#[test]
fn keeps_a_directory_rename_whose_destination_shares_a_name() {
    let (declarations, _) = parse_with(b"renamed 'pkg' -> 'vendor/pkg'\n", &|_| true);

    assert_eq!(
        moves_of(&declarations),
        vec![(under("pkg"), under("vendor/pkg"))]
    );

    let (declarations, _) = parse_with(b"Renaming src/lib to lib\n", &|_| true);

    assert_eq!(
        moves_of(&declarations),
        vec![(under("src/lib"), under("lib"))]
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

#[test]
fn normalizes_declaration_paths() {
    assert_move("R a/../b/./c.md d.md", "b/c.md", "d.md");
}
