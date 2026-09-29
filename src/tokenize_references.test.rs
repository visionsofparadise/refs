use super::*;

fn parts(content: &str) -> Vec<(String, String)> {
    tokenize_references(content)
        .into_iter()
        .map(|token| (token.path, token.suffix))
        .collect()
}

fn paths(content: &str) -> Vec<String> {
    parts(content).into_iter().map(|(path, _)| path).collect()
}

fn part(path: &str, suffix: &str) -> (String, String) {
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
    assert_eq!(paths("run `src/main.rs` first"), vec!["src/main.rs"]);
}

#[test]
fn splits_line_and_column_suffixes() {
    assert_eq!(
        parts("at main.rs:12 and a/b.rs:12:3"),
        vec![part("main.rs", ":12"), part("a/b.rs", ":12:3")]
    );
}

#[test]
fn splits_a_query_suffix() {
    assert_eq!(parts("page.html?x=1"), vec![part("page.html", "?x")]);
}

#[test]
fn keeps_a_drive_path_whole() {
    assert_eq!(parts(r"open C:\x\y.md now"), vec![part(r"C:\x\y.md", "")]);
}

#[test]
fn excludes_urls_globs_home_paths_and_git_coordinates() {
    let content = "https://example.com/a.md mailto:x@y.com src/**/*.md ~/x.md 8fbdb84e:src/a.md file://host/x.md";

    assert!(tokenize_references(content).is_empty());
}

#[test]
fn trims_emphasis_mentions_and_option_prefixes() {
    assert_eq!(
        paths("**src/a.md** **docs/a.md**: --out=dist/x.js @src/b.md"),
        vec!["src/a.md", "docs/a.md", "dist/x.js", "src/b.md"]
    );
}

#[test]
fn keeps_underscores_and_inner_at_signs() {
    assert_eq!(
        paths("__init__.py _config.yml assets/icon@2x.png node_modules/@types/node/index.d.ts"),
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
    assert_eq!(paths("file:///C:/x.md"), vec!["file:///C:/x.md"]);
}

#[test]
fn ignores_bare_names_and_dotfiles() {
    assert!(tokenize_references("LICENSE .gitignore #heading").is_empty());
}

#[test]
fn trims_trailing_punctuation() {
    assert_eq!(
        paths("see docs/a.md. then src/b.md, done"),
        vec!["docs/a.md", "src/b.md"]
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
