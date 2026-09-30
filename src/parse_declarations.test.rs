use super::*;

fn root_of() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("C:\\wd")
    } else {
        PathBuf::from("/wd")
    }
}

fn path_of(path: &str) -> PathBuf {
    path.split('/')
        .fold(root_of(), |joined, name| joined.join(name))
}

fn foreign_path_of(path: &str) -> PathBuf {
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
    parse_declarations(input, &root_of(), is_directory, is_directory)
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

fn reasons_of(rejected: &[Rejected]) -> Vec<(usize, &str)> {
    rejected
        .iter()
        .map(|entry| (entry.origin.line, entry.reason.as_str()))
        .collect()
}

fn assert_move(input: &str, from: &str, to: &str) {
    let (declarations, rejected) = parse(input);

    assert!(rejected.is_empty(), "{input}: {rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![(path_of(from), path_of(to))],
        "{input}"
    );
    assert!(deletes_of(&declarations).is_empty(), "{input}");
}

fn assert_delete(input: &str, path: &str) {
    let (declarations, rejected) = parse(input);

    assert!(rejected.is_empty(), "{input}: {rejected:?}");
    assert_eq!(deletes_of(&declarations), vec![path_of(path)], "{input}");
    assert!(moves_of(&declarations).is_empty(), "{input}");
}

fn assert_rejected(input: &str, reason: &str) {
    let (declarations, rejected) = parse(input);

    assert!(declarations.is_empty(), "{input}: {declarations:?}");
    assert_eq!(rejected.len(), 1, "{input}");
    assert_eq!(rejected[0].reason, reason, "{input}");
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
fn decodes_c_quoted_paths_only_in_the_git_tab_form() {
    assert_move(
        "R100\t\"a b.md\"\t\"c\\303\\251.md\"",
        "a b.md",
        "c\u{e9}.md",
    );
    assert_delete("D\t\"a\\\"b\"", "a\"b");
    assert_rejected("R \"a b.md\" c.md", "unrecognized");
    assert_rejected("R \"my docs\\a.md\" \"my docs\\b.md\"", "unrecognized");
}

#[test]
fn splits_the_hand_written_space_form_raw() {
    assert_move("R a'b'c.md d.md", "a'b'c.md", "d.md");
    assert_move("R notes\\new.md x.md", "notes/new.md", "x.md");
    assert_move("R src\\a.md src\\b.md", "src/a.md", "src/b.md");
}

#[test]
fn ignores_added_modified_typed_unmerged_and_copied_entries() {
    let (declarations, rejected) = parse("A\ta\nM\tb\nT\tc\nU\tu\nC100\td\te\nM b\nU u\n");

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
            (path_of("a b.md"), path_of("a c.md")),
            (path_of("caf\u{e9}.md"), path_of("cafe2.md")),
            (path_of("dir/k.md"), path_of("d3/dir2/k.md")),
            (path_of("-dash.md"), path_of("dash.md")),
            (path_of("it's.md"), path_of("its2.md")),
            (path_of("nl\nx.md"), path_of("nl2.md")),
            (path_of("q\"x.md"), path_of("q2.md")),
            (path_of("sub/in.md"), path_of("sub/in2.md")),
            (path_of("x to y.md"), path_of("x to z.md")),
        ]
    );
    assert_eq!(
        deletes_of(&declarations),
        vec![
            path_of("back/slash.md"),
            path_of("gone b.md"),
            path_of("gone's.md"),
            path_of("tab\tx.md"),
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
            (path_of("a b.md"), path_of("a c.md")),
            (path_of("caf\u{e9}.md"), path_of("cafe2.md")),
            (path_of("nl\nx.md"), path_of("nl2.md")),
            (path_of("x to y.md"), path_of("x to z.md")),
        ]
    );
    assert_eq!(deletes_of(&declarations), vec![path_of("gone.md")]);
    assert_eq!(
        declarations[2],
        Declaration::Delete {
            path: path_of("gone.md"),
            origin: Origin {
                line: 3,
                text: "D\\tgone.md".to_string(),
                snapshot: Some(1),
            },
        }
    );
}

#[test]
fn parses_one_nul_stream_holding_every_status() {
    let (declarations, rejected) = parse_with(
        b"R100\0a.md\0b.md\0D\0c.md\0M\0m.md\0C075\0x.md\0y.md\0A\0n.md\0T\0t.md\0U\0u.md\0",
        &|_| false,
    );

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("a.md"), path_of("b.md"))]
    );
    assert_eq!(deletes_of(&declarations), vec![path_of("c.md")]);
}

#[test]
fn rejects_an_unknown_status_record_and_keeps_parsing() {
    let (declarations, rejected) =
        parse_with(b"X\0conflict.md\0R100\0a.md\0b.md\0D\0c.md\0", &|_| false);

    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("a.md"), path_of("b.md"))]
    );
    assert_eq!(deletes_of(&declarations), vec![path_of("c.md")]);
    assert_eq!(reasons_of(&rejected), vec![(1, "unrecognized")]);
    assert_eq!(rejected[0].origin.text, "X\\tconflict.md");
}

#[test]
fn keeps_nul_record_path_bytes_exactly() {
    let (declarations, _) = parse_with(b"D\0nl\n\0D\0a.md \0", &|_| false);

    assert_eq!(
        deletes_of(&declarations),
        vec![path_of("nl\n"), path_of("a.md ")]
    );
}

#[test]
fn skips_leading_nuls_empty_trailing_records_and_a_trailing_newline() {
    for input in [
        &b"\0D\0a\0R\0b\0c\0"[..],
        b"D\0a\0R\0b\0c\0\0\0",
        b"D\0a\0R\0b\0c\0\n",
    ] {
        let (declarations, rejected) = parse_with(input, &|_| false);

        assert!(rejected.is_empty(), "{input:?}: {rejected:?}");
        assert_eq!(deletes_of(&declarations), vec![path_of("a")], "{input:?}");
        assert_eq!(
            moves_of(&declarations),
            vec![(path_of("b"), path_of("c"))],
            "{input:?}"
        );
    }
}

#[test]
fn rejects_an_empty_nul_record_path_and_continues() {
    let (declarations, rejected) = parse_with(b"D\0\0R\0a\0b\0", &|_| false);

    assert_eq!(reasons_of(&rejected), vec![(1, "empty path")]);
    assert_eq!(moves_of(&declarations), vec![(path_of("a"), path_of("b"))]);
}

#[test]
fn rejects_a_truncated_record() {
    let (declarations, rejected) = parse_with(b"R\0a\0", &|_| false);

    assert!(declarations.is_empty());
    assert_eq!(rejected.len(), 1);
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
    assert_move("renamed foo.md -> bar.md", "foo.md", "bar.md");
}

#[test]
fn decodes_git_bash_output_with_raw_lead_bytes_and_escaped_continuations() {
    let (declarations, rejected) = parse_with(
        b"renamed './\xe6'$'\\227''\xa5\xe6'$'\\234''\xac.md' -> './out.md'\n\
          renamed './\xf0'$'\\237\\230\\200''.md' -> './emoji.md'\n\
          renamed './\xd0'$'\\226\\320\\226''.md' -> './cyr.md'\n\
          renamed './\xe2'$'\\202\\254''.md' -> './euro.md'\n\
          removed 'r-\xe6'$'\\227''\xa5\xe6'$'\\234''\xac.md'\n",
        &|_| false,
    );

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("\u{65e5}\u{672c}.md"), path_of("out.md")),
            (path_of("\u{1f600}.md"), path_of("emoji.md")),
            (path_of("\u{416}\u{416}.md"), path_of("cyr.md")),
            (path_of("\u{20ac}.md"), path_of("euro.md")),
        ]
    );
    assert_eq!(
        deletes_of(&declarations),
        vec![path_of("r-\u{65e5}\u{672c}.md")]
    );
}

#[test]
fn parses_the_captured_wsl_escape_output() {
    let (declarations, rejected) = parse(
        "renamed ''$'\\346\\227\\245\\346\\234\\254''.md' -> 'x.md'\n\
         renamed ''$'\\303\\251'' b.md' -> 'y b.md'\n",
    );

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("\u{65e5}\u{672c}.md"), path_of("x.md")),
            (path_of("\u{e9} b.md"), path_of("y b.md")),
        ]
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
    assert_delete("removed 'a.md' (backup: 'x')", "a.md");
}

#[test]
fn parses_a_quoted_name_containing_an_arrow() {
    assert_move("renamed 'a -> b' -> 'c'", "a -> b", "c");
    assert_move(
        "renamed './x -> y.md' -> './m8-x -> y.md'",
        "x -> y.md",
        "m8-x -> y.md",
    );
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
fn parses_the_captured_git_mv_output_for_files_and_a_directory() {
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
                 Renaming dir/sub/s.md to dir2/sub/s.md\n\
                 Renaming dir2 to d3/dir2\n\
                 Renaming dir2/k.md to d3/dir2/k.md\n\
                 Renaming dir2/sub/s.md to d3/dir2/sub/s.md\n";
    let (declarations, rejected) = parse_with(input.as_bytes(), &|_| true);

    assert_eq!(
        reasons_of(&rejected),
        vec![(4, "unrecognized"), (5, "unrecognized"), (7, "ambiguous")]
    );
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("a b.md"), path_of("a c.md")),
            (path_of("it's.md"), path_of("its2.md")),
            (path_of("q\"x.md"), path_of("q2.md")),
            (path_of("caf\u{e9}.md"), path_of("cafe2.md")),
            (path_of("-dash.md"), path_of("dash.md")),
            (path_of("dir"), path_of("dir2")),
            (path_of("dir/k.md"), path_of("dir2/k.md")),
            (path_of("dir/sub/s.md"), path_of("dir2/sub/s.md")),
            (path_of("dir2"), path_of("d3/dir2")),
            (path_of("dir2/k.md"), path_of("d3/dir2/k.md")),
            (path_of("dir2/sub/s.md"), path_of("d3/dir2/sub/s.md")),
        ]
    );
}

#[test]
fn parses_the_captured_git_rm_output() {
    let (declarations, rejected) = parse("rm 'gone b.md'\nrm 'gone's.md'\nrm 'tab\tx.md'\n");

    assert!(rejected.is_empty());
    assert_eq!(
        deletes_of(&declarations),
        vec![
            path_of("gone b.md"),
            path_of("gone's.md"),
            path_of("tab\tx.md")
        ]
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
fn rejects_ambiguous_lines() {
    for line in [
        "a -> b -> c",
        "Renaming a to b to c",
        "renamed a -> b -> c",
        "'a' -> 'b' -> 'c'",
    ] {
        let (declarations, rejected) = parse(line);

        assert!(declarations.is_empty(), "{line}");
        assert_eq!(rejected.len(), 1, "{line}");
        assert_eq!(rejected[0].reason, "ambiguous", "{line}");
        assert_eq!(
            rejected[0].origin,
            Origin {
                line: 1,
                text: line.to_string(),
                snapshot: None,
            }
        );
    }
}

#[test]
fn rejects_the_captured_malformed_lines() {
    let (declarations, rejected) = parse(
        "R a.md b.md c.md\nD\nR\nrenamed 'a' -> 'b' extra\nhello world\n\nD a\ncopied 'a' 'b'\n",
    );

    assert_eq!(deletes_of(&declarations), vec![path_of("a")]);
    assert_eq!(
        reasons_of(&rejected),
        vec![
            (1, "unrecognized"),
            (2, "unrecognized"),
            (3, "unrecognized"),
            (4, "unrecognized"),
            (5, "unrecognized"),
            (8, "unrecognized"),
        ]
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
fn rejects_an_empty_path_in_every_line_form() {
    for line in [
        "removed ''",
        "removed directory ''",
        "D \"\"",
        "D\t\"\"",
        "renamed '' -> 'x.md'",
        "rm ''",
        "R\t\ta.md",
    ] {
        assert_rejected(line, "empty path");
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
            path: path_of("a"),
            origin: Origin {
                line: 3,
                text: "D a".to_string(),
                snapshot: None,
            },
        }]
    );
}

#[test]
fn escapes_control_characters_in_an_origin_text() {
    let (_, rejected) = parse("rm 'tab\tx'\nD\ta\tb\tc\nbell\u{7} -> \n");

    assert_eq!(
        rejected
            .iter()
            .map(|entry| entry.origin.text.as_str())
            .collect::<Vec<_>>(),
        vec!["D\\ta\\tb\\tc", "bell\\x07 ->"]
    );

    let (_, rejected) = parse_with(b"X\0nl\nx\0", &|_| false);

    assert_eq!(rejected[0].origin.text, "X\\tnl\\nx");
}

#[test]
fn completes_a_cross_filesystem_file_move_at_its_copied_line() {
    let (declarations, rejected) = parse("copied 'a' -> 'b'\nD c\nremoved 'a'\n");

    assert!(rejected.is_empty());
    assert_eq!(
        declarations,
        vec![
            Declaration::Move {
                from: path_of("a"),
                to: path_of("b"),
                origin: Origin {
                    line: 1,
                    text: "copied 'a' -> 'b'".to_string(),
                    snapshot: None,
                },
            },
            Declaration::Delete {
                path: path_of("c"),
                origin: Origin {
                    line: 2,
                    text: "D c".to_string(),
                    snapshot: None,
                },
            },
        ]
    );
}

#[test]
fn reads_bare_word_gnu_copies_and_drops_a_copy_never_removed() {
    let (declarations, rejected) =
        parse("renamed foo.md -> bar.md\ncopied x.md -> y.md\nremoved x.md\n");

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("foo.md"), path_of("bar.md")),
            (path_of("x.md"), path_of("y.md"))
        ]
    );

    let (declarations, rejected) = parse("copied 'a' -> 'b'\n");

    assert!(declarations.is_empty());
    assert!(rejected.is_empty());
}

#[test]
fn treats_a_copy_of_one_source_to_two_destinations_as_one_move() {
    let (declarations, _) =
        parse("copied 'a.md' -> 'b.md'\ncopied 'a.md' -> 'c.md'\nremoved 'a.md'\n");

    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("a.md"), path_of("b.md"))]
    );
}

#[test]
fn maps_a_file_moved_out_of_a_directory_without_moving_its_siblings() {
    let (declarations, rejected) = parse(
        "copied 'docs/a.md' -> '/d/T/a.md'\n\
         removed 'docs/a.md'\n\
         removed directory 'docs/old'\n\
         removed 'docs/gone.md'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("docs/a.md"), foreign_path_of("/d/T/a.md"))]
    );
    assert_eq!(
        deletes_of(&declarations),
        vec![path_of("docs/old"), path_of("docs/gone.md")]
    );
}

#[test]
fn keeps_a_removal_after_a_retired_root_a_delete() {
    let (declarations, rejected) = parse(
        "created directory '/d/T/e'\n\
         created directory '/d/T/e/sub'\n\
         copied 'e/a.md' -> '/d/T/e/a.md'\n\
         removed directory 'e/sub'\n\
         removed 'e/a.md'\n\
         removed directory 'e'\n\
         removed directory 'sub'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("e"), foreign_path_of("/d/T/e")),
            (path_of("e/sub"), foreign_path_of("/d/T/e/sub")),
            (path_of("e/a.md"), foreign_path_of("/d/T/e/a.md")),
        ]
    );
    assert_eq!(deletes_of(&declarations), vec![path_of("sub")]);
}

#[test]
fn reads_a_directory_root_when_the_created_line_is_missing() {
    let (declarations, rejected) = parse(
        "copied 'e/a.md' -> '/d/T/e/a.md'\n\
         created directory '/d/T/e/sub'\n\
         removed 'e/a.md'\n\
         removed directory 'e'\n\
         removed directory 'sub'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("e/a.md"), foreign_path_of("/d/T/e/a.md")),
            (path_of("e"), foreign_path_of("/d/T/e")),
        ]
    );
    assert_eq!(deletes_of(&declarations), vec![path_of("sub")]);
}

#[test]
fn keeps_the_parent_of_a_moved_directory_a_delete() {
    let (declarations, rejected) = parse(
        "created directory '/d/T/b'\n\
         copied 'a/b/x.md' -> '/d/T/b/x.md'\n\
         removed 'a/b/x.md'\n\
         removed directory 'a/b'\n\
         removed 'a/other.md'\n\
         removed directory 'a'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("a/b"), foreign_path_of("/d/T/b")),
            (path_of("a/b/x.md"), foreign_path_of("/d/T/b/x.md")),
        ]
    );
    assert_eq!(
        deletes_of(&declarations),
        vec![path_of("a/other.md"), path_of("a")]
    );

    let (declarations, _) = parse(
        "copied 'a/b/x.md' -> '/d/T/b/x.md'\n\
         removed 'a/b/x.md'\n\
         removed directory 'a/b'\n\
         removed 'a/other.md'\n\
         removed directory 'a'\n",
    );

    assert_eq!(
        deletes_of(&declarations),
        vec![path_of("a/b"), path_of("a/other.md"), path_of("a")]
    );
}

#[test]
fn keeps_rm_recursive_output_as_deletes() {
    let (declarations, rejected) = parse(
        "removed 'r/a.md'\n\
         removed directory 'r/empty'\n\
         removed 'r/sub/b.md'\n\
         removed directory 'r/sub'\n\
         removed directory 'r'\n",
    );

    assert!(rejected.is_empty());
    assert!(moves_of(&declarations).is_empty());
    assert_eq!(
        deletes_of(&declarations),
        vec![
            path_of("r/a.md"),
            path_of("r/empty"),
            path_of("r/sub/b.md"),
            path_of("r/sub"),
            path_of("r"),
        ]
    );
}

#[test]
fn moves_a_directory_holding_only_empty_directories() {
    let (declarations, rejected) = parse(
        "created directory '/d/T/oe2'\n\
         created directory '/d/T/oe2/inner'\n\
         removed directory 'onlyempty/inner'\n\
         removed directory 'onlyempty'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("onlyempty"), foreign_path_of("/d/T/oe2")),
            (
                path_of("onlyempty/inner"),
                foreign_path_of("/d/T/oe2/inner")
            ),
        ]
    );
    assert!(deletes_of(&declarations).is_empty());
}

#[test]
fn moves_a_lone_empty_directory_named_by_a_created_line() {
    let (declarations, rejected) = parse(
        "created directory '/d/t/empty'\n\
         removed directory 'empty'\n",
    );

    assert!(rejected.is_empty());
    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("empty"), foreign_path_of("/d/t/empty"))]
    );
}

#[test]
fn parses_the_captured_cross_filesystem_output() {
    let input = "copied 'x.md' -> '/d/T/tmp.YkkZxcP6V4/x.md'\n\
                 removed 'x.md'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/e'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/e/sub'\n\
                 copied 'e/sub/b.md' -> '/d/T/tmp.YkkZxcP6V4/e/sub/b.md'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/e/empty'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/e/emptyonly'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/e/emptyonly/inner'\n\
                 copied 'e/a.md' -> '/d/T/tmp.YkkZxcP6V4/e/a.md'\n\
                 copied \"e/it's.md\" -> \"/d/T/tmp.YkkZxcP6V4/e/it's.md\"\n\
                 removed \"e/it's.md\"\n\
                 removed directory 'e/empty'\n\
                 removed 'e/sub/b.md'\n\
                 removed directory 'e/sub'\n\
                 removed directory 'e/emptyonly/inner'\n\
                 removed directory 'e/emptyonly'\n\
                 removed 'e/a.md'\n\
                 removed directory 'e'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/renamed'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/renamed/sub'\n\
                 copied 'f/sub/b.md' -> '/d/T/tmp.YkkZxcP6V4/renamed/sub/b.md'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/renamed/empty'\n\
                 copied 'f/a.md' -> '/d/T/tmp.YkkZxcP6V4/renamed/a.md'\n\
                 removed directory 'f/empty'\n\
                 removed 'f/sub/b.md'\n\
                 removed directory 'f/sub'\n\
                 removed 'f/a.md'\n\
                 removed directory 'f'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/oe2'\n\
                 created directory '/d/T/tmp.YkkZxcP6V4/oe2/inner'\n\
                 removed directory 'onlyempty/inner'\n\
                 removed directory 'onlyempty'\n\
                 copied '/d/T/tmp.YkkZxcP6V4/back.md' -> './back2.md'\n\
                 removed '/d/T/tmp.YkkZxcP6V4/back.md'\n\
                 copied 'bb.md' -> '/d/T/tmp.YkkZxcP6V4/bb.md' (backup: '/d/T/tmp.YkkZxcP6V4/bb.md~')\n\
                 removed 'bb.md'\n";
    let (declarations, rejected) = parse_with(input.as_bytes(), &|_| false);
    let target = |path: &str| foreign_path_of(&format!("/d/T/tmp.YkkZxcP6V4{path}"));

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("x.md"), target("/x.md")),
            (path_of("e"), target("/e")),
            (path_of("e/sub"), target("/e/sub")),
            (path_of("e/sub/b.md"), target("/e/sub/b.md")),
            (path_of("e/empty"), target("/e/empty")),
            (path_of("e/emptyonly"), target("/e/emptyonly")),
            (path_of("e/emptyonly/inner"), target("/e/emptyonly/inner")),
            (path_of("e/a.md"), target("/e/a.md")),
            (path_of("e/it's.md"), target("/e/it's.md")),
            (path_of("f"), target("/renamed")),
            (path_of("f/sub"), target("/renamed/sub")),
            (path_of("f/sub/b.md"), target("/renamed/sub/b.md")),
            (path_of("f/empty"), target("/renamed/empty")),
            (path_of("f/a.md"), target("/renamed/a.md")),
            (path_of("onlyempty"), target("/oe2")),
            (path_of("onlyempty/inner"), target("/oe2/inner")),
            (target("/back.md"), path_of("back2.md")),
            (path_of("bb.md"), target("/bb.md")),
        ]
    );
    assert!(deletes_of(&declarations).is_empty());
}

#[test]
fn moves_every_removal_beneath_a_copied_root_in_the_git_bash_capture() {
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
    let target = |path: &str| foreign_path_of(&format!("/d/refs-review-23961{path}"));

    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("x.md"), target("/x.md")),
            (path_of("d"), target("/d")),
            (path_of("d/sub"), target("/d/sub")),
            (path_of("d/sub/b.md"), target("/d/sub/b.md")),
            (path_of("d/a.md"), target("/d/a.md")),
        ]
    );
    assert!(deletes_of(&declarations).is_empty());
}

#[test]
fn rejects_a_removal_whose_copy_was_rejected_and_never_deletes_it() {
    let (declarations, rejected) = parse(
        "created directory 'x:y'\n\
         copied 'a.md' -> 'x:y.md'\n\
         removed 'a.md'\n\
         removed 'b.md'\n",
    );

    assert_eq!(
        reasons_of(&rejected),
        vec![(2, "unsupported path"), (3, "unsupported path")]
    );
    assert_eq!(deletes_of(&declarations), vec![path_of("b.md")]);
    assert!(moves_of(&declarations).is_empty());
}

#[test]
fn never_rejects_a_created_directory_line() {
    let (declarations, rejected) = parse("created directory ''\ncreated directory 'x:y'\n");

    assert!(declarations.is_empty());
    assert!(rejected.is_empty());
}

#[test]
fn moves_into_an_existing_directory_only_for_the_plain_space_form() {
    let parse_tree = |input: &[u8]| {
        parse_declarations(
            input,
            &root_of(),
            &|path: &Path| path == path_of("docs"),
            &|path: &Path| path == path_of("docs") || path == path_of("docs/a.md"),
        )
    };

    let (declarations, _) = parse_tree(b"R a.md docs\n");

    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("a.md"), path_of("docs/a.md"))]
    );

    let (declarations, _) = parse_tree(b"R a.md notes\n");

    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("a.md"), path_of("notes"))]
    );

    for line in [
        "R\ta.md\tdocs",
        "renamed 'a.md' -> 'docs'",
        "'a.md' -> 'docs'",
        "a.md -> docs",
        "Renaming a.md to docs",
    ] {
        let (declarations, _) = parse_tree(line.as_bytes());

        assert_eq!(
            moves_of(&declarations),
            vec![(path_of("a.md"), path_of("docs"))],
            "{line}"
        );
    }

    let (declarations, _) = parse_tree(b"R100\0a.md\0docs\0");

    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("a.md"), path_of("docs"))]
    );
}

#[test]
fn keeps_a_directory_rename_onto_an_existing_directory_without_the_entry() {
    let (declarations, _) = parse_declarations(
        b"R d1 docs\n",
        &root_of(),
        &|path: &Path| path == path_of("docs"),
        &|path: &Path| path == path_of("docs"),
    );

    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("d1"), path_of("docs"))]
    );
}

#[test]
fn keeps_a_directory_rename_whose_destination_shares_a_name() {
    let (declarations, _) = parse_with(b"renamed 'pkg' -> 'vendor/pkg'\n", &|_| true);

    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("pkg"), path_of("vendor/pkg"))]
    );

    let (declarations, _) = parse_with(b"Renaming src/lib to lib\n", &|_| true);

    assert_eq!(
        moves_of(&declarations),
        vec![(path_of("src/lib"), path_of("lib"))]
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

#[test]
fn groups_a_contiguous_listing_run_into_one_snapshot() {
    let (declarations, _) = parse(
        "R100	a.md	b.md
M	m.md
R100	b.md	c.md
renamed 'x' -> 'y'
D	gone.md
",
    );
    let snapshots: Vec<Option<usize>> = declarations
        .iter()
        .map(|declaration| match declaration {
            Declaration::Move { origin, .. } | Declaration::Delete { origin, .. } => {
                origin.snapshot
            }
        })
        .collect();

    assert_eq!(snapshots, [Some(1), Some(1), None, Some(5)]);

    let (declarations, _) = parse_with(b"R100 a.md b.md D c.md ", &|_| false);

    assert!(declarations.iter().all(|declaration| match declaration {
        Declaration::Move { origin, .. } | Declaration::Delete { origin, .. } =>
            origin.snapshot == Some(1),
    }));
}

#[test]
fn ends_a_snapshot_at_a_blank_line() {
    let (declarations, _) = parse("R100\ta.md\tb.md\n\nR100\tb.md\tc.md\n");
    let snapshots: Vec<Option<usize>> = declarations
        .iter()
        .map(|declaration| match declaration {
            Declaration::Move { origin, .. } | Declaration::Delete { origin, .. } => {
                origin.snapshot
            }
        })
        .collect();

    assert_eq!(snapshots, [Some(1), Some(3)]);
}

// Captured in WSL (coreutils 9.4, git 2.43, Ubuntu language packs) with
// `LANGUAGE=<language> LC_ALL=<locale>.UTF-8`: `mv -v` of a spaced name, into a directory and
// over a backup, `mv -v` of a directory and a file from ext4 to /mnt/c, `rm -v`, `rm -rv` and
// `git mv -v`. The /mnt/c destination prefix is shortened to /d/T/<language>. es and ar keep
// the messages their catalogues leave untranslated, and git has no ja or ar catalogue.
const CAPTURED_DE: &str = "\
    Datei umbenannt 'a b.md' -> 'c d.md'\n\
    Datei umbenannt \"it's.md\" -> \"d/it's.md\"\n\
    Datei umbenannt 'w.md' -> 'z.md' (Sicherung: 'z.md~')\n\
    Verzeichnis '/d/T/de/t2' angelegt\n\
    Verzeichnis '/d/T/de/t2/sub' angelegt\n\
    Datei kopiert 't/sub/y.md' -> '/d/T/de/t2/sub/y.md'\n\
    Datei kopiert 't/x.md' -> '/d/T/de/t2/x.md'\n\
    't/x.md' wurde entfernt\n\
    't/sub/y.md' wurde entfernt\n\
    Verzeichnis 't/sub' wurde entfernt\n\
    Verzeichnis 't' wurde entfernt\n\
    Datei kopiert 'f.md' -> '/d/T/de/f2.md'\n\
    'f.md' wurde entfernt\n\
    'c d.md' wurde entfernt\n\
    \"d/it's.md\" wurde entfernt\n\
    Verzeichnis 'd' wurde entfernt\n\
    Benenne g h.md nach i j.md um\n\
";

const CAPTURED_FR: &str = "\
    renommé 'a b.md' -> 'c d.md'\n\
    renommé \"it's.md\" -> \"d/it's.md\"\n\
    renommé 'w.md' -> 'z.md' (archive : 'z.md~')\n\
    répertoire '/d/T/fr/t2' créé\n\
    répertoire '/d/T/fr/t2/sub' créé\n\
    copié 't/sub/y.md' -> '/d/T/fr/t2/sub/y.md'\n\
    copié 't/x.md' -> '/d/T/fr/t2/x.md'\n\
    't/x.md' supprimé\n\
    't/sub/y.md' supprimé\n\
    répertoire 't/sub' supprimé\n\
    répertoire 't' supprimé\n\
    copié 'f.md' -> '/d/T/fr/f2.md'\n\
    'f.md' supprimé\n\
    'c d.md' supprimé\n\
    \"d/it's.md\" supprimé\n\
    répertoire 'd' supprimé\n\
    Renommage de g h.md en i j.md\n\
";

const CAPTURED_ES: &str = "\
    renamed 'a b.md' -> 'c d.md'\n\
    renamed \"it's.md\" -> \"d/it's.md\"\n\
    renamed 'w.md' -> 'z.md' (respaldo: 'z.md~')\n\
    created directory '/d/T/es/t2'\n\
    created directory '/d/T/es/t2/sub'\n\
    copied 't/sub/y.md' -> '/d/T/es/t2/sub/y.md'\n\
    copied 't/x.md' -> '/d/T/es/t2/x.md'\n\
    't/x.md' borrado\n\
    't/sub/y.md' borrado\n\
    removed directory 't/sub'\n\
    removed directory 't'\n\
    copied 'f.md' -> '/d/T/es/f2.md'\n\
    'f.md' borrado\n\
    'c d.md' borrado\n\
    \"d/it's.md\" borrado\n\
    removed directory 'd'\n\
    Renombrando g h.md a i j.md\n\
";

const CAPTURED_JA: &str = "\
    名前変更: 'a b.md' -> 'c d.md'\n\
    名前変更: \"it's.md\" -> \"d/it's.md\"\n\
    名前変更: 'w.md' -> 'z.md' (バックアップ: 'z.md~')\n\
    ディレクトリ '/d/T/ja/t2' を作成しました\n\
    ディレクトリ '/d/T/ja/t2/sub' を作成しました\n\
    コピー: 't/sub/y.md' -> '/d/T/ja/t2/sub/y.md'\n\
    コピー: 't/x.md' -> '/d/T/ja/t2/x.md'\n\
    't/x.md' を削除しました\n\
    't/sub/y.md' を削除しました\n\
    ディレクトリ 't/sub' を削除しました\n\
    ディレクトリ 't' を削除しました\n\
    コピー: 'f.md' -> '/d/T/ja/f2.md'\n\
    'f.md' を削除しました\n\
    'c d.md' を削除しました\n\
    \"d/it's.md\" を削除しました\n\
    ディレクトリ 'd' を削除しました\n\
    Renaming g h.md to i j.md\n\
";

const CAPTURED_AR: &str = "\
    renamed 'a b.md' -> 'c d.md'\n\
    renamed \"it's.md\" -> \"d/it's.md\"\n\
    renamed 'w.md' -> 'z.md' (نسخة احتياطية: 'z.md~')\n\
    أُنشئ الدليل '/d/T/ar/t2'\n\
    أُنشئ الدليل '/d/T/ar/t2/sub'\n\
    copied 't/sub/y.md' -> '/d/T/ar/t2/sub/y.md'\n\
    copied 't/x.md' -> '/d/T/ar/t2/x.md'\n\
    حُذِف 't/x.md'\n\
    حُذِف 't/sub/y.md'\n\
    حُذف الدليل 't/sub'\n\
    حُذف الدليل 't'\n\
    copied 'f.md' -> '/d/T/ar/f2.md'\n\
    حُذِف 'f.md'\n\
    حُذِف 'c d.md'\n\
    حُذِف \"d/it's.md\"\n\
    حُذف الدليل 'd'\n\
    Renaming g h.md to i j.md\n\
";

fn assert_captured_locale(input: &str, language: &str) {
    let (declarations, rejected) = parse(input);
    let target = |path: &str| foreign_path_of(&format!("/d/T/{language}{path}"));

    assert!(rejected.is_empty(), "{language}: {rejected:?}");
    assert_eq!(
        moves_of(&declarations),
        vec![
            (path_of("a b.md"), path_of("c d.md")),
            (path_of("it's.md"), path_of("d/it's.md")),
            (path_of("w.md"), path_of("z.md")),
            (path_of("t"), target("/t2")),
            (path_of("t/sub"), target("/t2/sub")),
            (path_of("t/sub/y.md"), target("/t2/sub/y.md")),
            (path_of("t/x.md"), target("/t2/x.md")),
            (path_of("f.md"), target("/f2.md")),
            (path_of("g h.md"), path_of("i j.md")),
        ],
        "{language}"
    );
    assert_eq!(
        deletes_of(&declarations),
        vec![path_of("c d.md"), path_of("d/it's.md"), path_of("d")],
        "{language}"
    );
}

#[test]
fn parses_the_captured_german_output() {
    assert_captured_locale(CAPTURED_DE, "de");
}

#[test]
fn parses_the_captured_french_output() {
    assert_captured_locale(CAPTURED_FR, "fr");
}

#[test]
fn parses_the_captured_spanish_output() {
    assert_captured_locale(CAPTURED_ES, "es");
}

#[test]
fn parses_the_captured_japanese_output() {
    assert_captured_locale(CAPTURED_JA, "ja");
}

#[test]
fn parses_the_captured_right_to_left_arabic_output() {
    assert_captured_locale(CAPTURED_AR, "ar");
}

fn line_of(template: &Template, arguments: [&str; 2]) -> String {
    template.arguments.iter().enumerate().fold(
        template.literals[0].to_string(),
        |line, (slot, argument)| line + arguments[*argument] + template.literals[slot + 1],
    )
}

#[test]
fn reads_every_generated_template_back() {
    let quoted = ["'a b.md'", "'d/c'\\''s.md'"];
    let raw = ["a b.md", "d/c's.md"];
    let [first, second] = raw.map(str::to_string);

    for (message, templates, reading) in TRANSLATED {
        let arguments = if reading == Reading::Shell {
            quoted
        } else {
            raw
        };
        let expected = match message {
            Message::Renamed | Message::Renaming => Parsed::Move {
                from: first.clone(),
                to: second.clone(),
                hand_written: false,
            },
            Message::Copied => Parsed::Copy(first.clone(), second.clone()),
            Message::Removed => Parsed::Removed(first.clone()),
            Message::RemovedDirectory => Parsed::RemovedDirectory(first.clone()),
            Message::CreatedDirectory => Parsed::Created(first.clone()),
        };

        for template in templates {
            let line = line_of(template, arguments);

            assert_eq!(parse_line(line.as_bytes()), Ok(expected.clone()), "{line}");
        }
    }

    for template in BACKUP {
        let line = format!("renamed 'a' -> 'b'{}", line_of(template, ["'b~'", ""]));

        assert_eq!(
            parse_line(line.as_bytes()),
            Ok(Parsed::Move {
                from: "a".to_string(),
                to: "b".to_string(),
                hand_written: false,
            }),
            "{line}"
        );
    }
}

#[test]
fn rejects_a_translated_line_that_splits_more_than_one_way() {
    assert_rejected("Benenne a nach b nach c um", "ambiguous");
}
