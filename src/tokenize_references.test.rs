use super::*;

fn parts_of(content: &str) -> Vec<(String, String)> {
    tokenize_references(content)
        .into_iter()
        .map(|token| (token.path, token.suffix))
        .collect()
}

fn paths_of(content: &str) -> Vec<String> {
    parts_of(content)
        .into_iter()
        .map(|(path, _)| path)
        .collect()
}

fn part_of(path: &str, suffix: &str) -> (String, String) {
    (path.to_string(), suffix.to_string())
}

#[test]
fn splits_a_markdown_link_target_into_path_and_fragment() {
    let content = "see [a](../a.md#h) here";
    let tokens = tokenize_references(content);

    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].path, "../a.md");
    assert_eq!(tokens[0].suffix, "#h");
    assert_eq!(&content[tokens[0].start..tokens[0].end], "../a.md#h");
}

#[test]
fn keeps_a_backticked_path() {
    assert_eq!(paths_of("run `src/main.rs` first"), vec!["src/main.rs"]);
}

#[test]
fn splits_line_and_column_suffixes() {
    assert_eq!(
        parts_of("at main.rs:12 and a/b.rs:12:3"),
        vec![part_of("main.rs", ":12"), part_of("a/b.rs", ":12:3")]
    );
}

#[test]
fn splits_a_query_suffix() {
    assert_eq!(parts_of("page.html?x=1"), vec![part_of("page.html", "?x")]);
}

#[test]
fn keeps_a_drive_path_whole() {
    assert_eq!(
        parts_of(r"open C:\x\y.md now"),
        vec![part_of(r"C:\x\y.md", "")]
    );
}

#[test]
fn excludes_urls_globs_home_paths_and_git_coordinates() {
    let content = "https://example.com/a.md mailto:x@y.com src/**/*.md ~/x.md 8fbdb84e:src/a.md";

    assert!(tokenize_references(content).is_empty());
}

#[test]
fn trims_emphasis_mentions_and_option_prefixes() {
    assert_eq!(
        paths_of("**src/a.md** **docs/a.md**: --out=dist/x.js @src/b.md"),
        vec!["src/a.md", "docs/a.md", "dist/x.js", "src/b.md"]
    );
}

#[test]
fn keeps_underscores_and_inner_at_signs() {
    assert_eq!(
        paths_of("__init__.py _config.yml assets/icon@2x.png node_modules/@types/node/index.d.ts"),
        vec![
            "__init__.py",
            "_config.yml",
            "assets/icon@2x.png",
            "node_modules/@types/node/index.d.ts"
        ]
    );
}

#[test]
fn keeps_a_local_file_url() {
    assert_eq!(paths_of("file:///C:/x.md"), vec!["file:///C:/x.md"]);
}

#[test]
fn ignores_bare_names_and_dotfiles() {
    assert!(tokenize_references("LICENSE .gitignore #heading").is_empty());
}

#[test]
fn trims_trailing_periods_and_colons() {
    assert_eq!(
        paths_of("see docs/a.md. then src/b.md: done"),
        vec!["docs/a.md", "src/b.md"]
    );
}

#[test]
fn delimits_on_a_comma() {
    let tokens = tokenize_references("docs/a.md,src/b.md");

    assert_eq!(
        tokens
            .iter()
            .map(|token| (token.path.as_str(), token.start, token.end))
            .collect::<Vec<_>>(),
        vec![("docs/a.md", 0, 9), ("src/b.md", 10, 18)]
    );
}

#[test]
fn skips_a_leading_byte_order_mark() {
    let tokens = tokenize_references("\u{feff}docs/a.md");

    assert_eq!(tokens[0].path, "docs/a.md");
    assert_eq!(tokens[0].start, 3);
    assert_eq!(tokens[0].column, 1);
}

#[test]
fn reports_columns_of_two_tokens_on_one_line() {
    let tokens = tokenize_references("a.md b/c.md");

    assert_eq!(
        tokens
            .iter()
            .map(|token| (token.line, token.column))
            .collect::<Vec<_>>(),
        vec![(1, 1), (1, 6)]
    );
}

#[test]
fn counts_columns_in_bytes() {
    let tokens = tokenize_references("é docs/a.md");

    assert_eq!(tokens[0].column, 4);
    assert_eq!(tokens[0].start, 3);
}

#[test]
fn reads_crlf_lines() {
    let tokens = tokenize_references("x a.md\r\nb/c.md\r\n");

    assert_eq!(tokens[0].path, "a.md");
    assert_eq!((tokens[0].line, tokens[0].column), (1, 3));
    assert_eq!(tokens[1].path, "b/c.md");
    assert_eq!((tokens[1].line, tokens[1].column), (2, 1));
    assert_eq!(tokens[1].start, 8);
}

#[test]
fn excludes_a_file_url_with_a_host() {
    assert!(tokenize_references("file://host/x.md FILE://server/share/a.md").is_empty());
}

#[test]
fn keeps_a_trailing_parent_segment() {
    assert_eq!(
        paths_of(r"up ../.. and C:\x\.. end"),
        vec!["../..", r"C:\x\.."]
    );
}

#[test]
fn trims_a_trailing_current_segment_dot() {
    assert_eq!(paths_of("see src/components/."), vec!["src/components/"]);
}

#[test]
fn keeps_underscore_pairs() {
    assert_eq!(
        paths_of("__fixtures__/x/__mocks__"),
        vec!["__fixtures__/x/__mocks__"]
    );
}

#[test]
fn trims_a_single_underscore_emphasis_pair() {
    assert_eq!(paths_of("_a.md_ _docs/a.md_"), vec!["a.md", "docs/a.md"]);
}

#[test]
fn trims_a_trailing_run_of_dots() {
    assert_eq!(paths_of("see src/a.md..."), vec!["src/a.md"]);
}

#[test]
fn ignores_separator_only_paths() {
    assert!(tokenize_references("/ // ///").is_empty());
}

#[test]
fn excludes_scp_style_remotes() {
    assert!(tokenize_references("git@github.com:org/repo.git user@host:path/a.md").is_empty());
}

#[test]
fn excludes_file_urls_without_an_authority() {
    assert!(tokenize_references("file:x.md file:/x.md").is_empty());
}

#[test]
fn trims_one_backslash_from_an_odd_trailing_run() {
    let content = r#"{"build":"tsc -p \"tsconfig.build.json\"","doc":"open \"docs/a.md\""}"#;

    assert_eq!(paths_of(content), vec!["tsconfig.build.json", "docs/a.md"]);
    assert_eq!(paths_of(r#""C:\\x\\""#), vec![r"C:\\x\\"]);
    assert_eq!(
        paths_of(r#"{"cmd": "robocopy \"src\\dir\\\" x"}"#),
        vec![r"src\\dir\\"]
    );
}

#[test]
fn ignores_ellipses_and_dot_ended_segments() {
    assert!(tokenize_references("... refs [<path>...] docs/... a./b.md").is_empty());
}

#[test]
fn delimits_on_typographic_quotes() {
    assert_eq!(
        paths_of("\u{201c}src/a.md\u{201d} and docs/a.md\u{2019}s \u{2018}lib/b.md\u{2019}"),
        vec!["src/a.md", "docs/a.md", "lib/b.md"]
    );
}

#[test]
fn records_a_trimmed_leading_at_sign() {
    let tokens = tokenize_references("@src/a.md src/b.md @scope/pkg/file.js");

    assert_eq!(
        tokens
            .iter()
            .map(|token| token.at_prefixed)
            .collect::<Vec<_>>(),
        vec![true, false, true]
    );
}

#[test]
fn keeps_at_signs_in_paths_with_a_line_suffix() {
    assert_eq!(
        parts_of("node_modules/@types/node/index.d.ts:12:3 assets/icon@2x.png:4 me@x.md:12"),
        vec![
            part_of("node_modules/@types/node/index.d.ts", ":12:3"),
            part_of("assets/icon@2x.png", ":4"),
            part_of("me@x.md", ":12")
        ]
    );
}

#[test]
fn trims_emphasis_spanning_words() {
    assert_eq!(
        paths_of("**Read docs/a.md** *see src/b.md* _see lib/c.md_ ~~see old/d.md~~"),
        vec!["docs/a.md", "src/b.md", "lib/c.md", "old/d.md"]
    );
}

#[test]
fn keeps_emphasis_markers_that_touch_a_separator() {
    assert!(tokenize_references("**/dist/** *src/*").is_empty());
    assert_eq!(paths_of("_site/_"), vec!["_site/_"]);
}

#[test]
fn trims_negation_and_exclamation_marks() {
    assert_eq!(
        paths_of("!dist/keep.js see docs/a.md! and src/b.md\u{2026}"),
        vec!["dist/keep.js", "docs/a.md", "src/b.md"]
    );
}

#[test]
fn delimits_on_html_entities() {
    assert_eq!(
        paths_of("&quot;docs/a.md&quot; &apos;src/b.md&#39; &lt;lib/c.md&gt;"),
        vec!["docs/a.md", "src/b.md", "lib/c.md"]
    );
}

#[test]
fn keeps_a_bracketed_segment_between_separators() {
    assert_eq!(
        paths_of("app/(marketing)/about/page.tsx src/routes/[slug]/+page.svelte"),
        vec![
            "app/(marketing)/about/page.tsx",
            "src/routes/[slug]/+page.svelte"
        ]
    );
}

#[test]
fn delimits_brackets_outside_a_segment() {
    assert_eq!(
        paths_of("[a.md](docs/a.md) (src/b.md)"),
        vec!["a.md", "docs/a.md", "src/b.md"]
    );
}

#[test]
fn excludes_shell_and_batch_expansions() {
    let content = "$HOME/a.md ${ROOT}/bin/x.sh $(pwd)/../lib/y.js ${{ github.workspace }}/bin/z.sh %APPDATA%/x.json %~dp0\\tool.exe";

    assert_eq!(paths_of(content), vec!["github.workspace"]);
}

#[test]
fn keeps_a_percent_encoded_path_that_looks_like_a_batch_variable() {
    assert_eq!(paths_of("%C3%A9t%C3%A9.md"), vec!["%C3%A9t%C3%A9.md"]);
}
