use super::*;

fn absolute_path_of(names: &[&str]) -> PathBuf {
    let mut path = if cfg!(windows) {
        PathBuf::from("C:\\")
    } else {
        PathBuf::from("/")
    };

    for name in names {
        path.push(name);
    }

    path
}

fn resolve(path: &str) -> (Vec<Candidate>, PathStyle) {
    resolve_reference(
        path,
        &absolute_path_of(&["work", "docs"]),
        &absolute_path_of(&["work"]),
    )
}

fn targets_of(path: &str) -> Vec<PathBuf> {
    resolve(path)
        .0
        .into_iter()
        .map(|candidate| candidate.target)
        .collect()
}

fn style_of(path: &str) -> PathStyle {
    resolve(path).1
}

#[test]
fn orders_the_file_relative_candidate_before_the_working_directory_candidate() {
    assert_eq!(
        resolve("a.md").0,
        vec![
            Candidate {
                target: absolute_path_of(&["work", "docs", "a.md"]),
                form: PathForm::FileRelative,
            },
            Candidate {
                target: absolute_path_of(&["work", "a.md"]),
                form: PathForm::WorkingDirectoryRelative,
            },
        ]
    );
}

#[test]
fn deduplicates_candidates_when_the_referrer_is_the_working_directory() {
    let work = absolute_path_of(&["work"]);
    let (candidates, _) = resolve_reference("a.md", &work, &work);

    assert_eq!(
        candidates,
        vec![Candidate {
            target: absolute_path_of(&["work", "a.md"]),
            form: PathForm::FileRelative,
        }]
    );
}

#[test]
fn pops_parent_segments() {
    assert_eq!(
        targets_of("../x/../a.md"),
        vec![
            absolute_path_of(&["work", "a.md"]),
            absolute_path_of(&["a.md"])
        ]
    );
}

#[test]
fn splits_on_backslashes() {
    assert_eq!(
        targets_of("sub\\a.md")[0],
        absolute_path_of(&["work", "docs", "sub", "a.md"])
    );
}

#[test]
fn normalizes_without_climbing_above_the_root() {
    assert_eq!(
        normalize_path(&absolute_path_of(&["..", "a", ".", "b"])),
        absolute_path_of(&["a", "b"])
    );
    assert_eq!(
        normalize_path(Path::new("../a/./b/..")),
        PathBuf::from("../a")
    );
}

#[cfg(windows)]
#[test]
fn resolves_drive_and_msys_paths() {
    assert_eq!(
        resolve("C:\\x\\y.md").0,
        vec![Candidate {
            target: PathBuf::from("C:\\x\\y.md"),
            form: PathForm::Absolute(AbsoluteStyle::Drive),
        }]
    );
    assert_eq!(targets_of("D:/x/../y.md"), vec![PathBuf::from("D:\\y.md")]);
    assert_eq!(
        resolve("/c/x/y.md").0,
        vec![Candidate {
            target: PathBuf::from("C:\\x\\y.md"),
            form: PathForm::Absolute(AbsoluteStyle::Msys),
        }]
    );
}

#[cfg(windows)]
#[test]
fn yields_no_candidate_for_a_rooted_path_without_a_drive() {
    assert!(targets_of("/docs/a.md").is_empty());
    assert!(targets_of(r"\docs\a.md").is_empty());
    assert!(targets_of(r"\server\share\a.md").is_empty());
}

#[cfg(windows)]
#[test]
fn yields_no_candidate_for_a_colon_outside_the_drive_position() {
    assert!(targets_of("C:a.md").is_empty());
    assert!(targets_of("C:/x/a:b.md").is_empty());
    assert!(targets_of("/c/x:y.md").is_empty());
}

#[cfg(windows)]
#[test]
fn requires_a_separator_after_an_msys_letter() {
    assert!(targets_of("/c").is_empty());
    assert!(targets_of("/c.md").is_empty());
}

#[cfg(windows)]
#[test]
fn records_the_written_drive_letter_case() {
    assert!(style_of("c:/x/a.md").lowercase_drive);
    assert!(!style_of("C:/x/a.md").lowercase_drive);
    assert!(style_of("/c/x/a.md").lowercase_drive);
    assert!(!style_of("/C/x/a.md").lowercase_drive);
    assert_eq!(targets_of("c:/x/a.md"), vec![PathBuf::from(r"C:\x\a.md")]);
}

#[cfg(windows)]
#[test]
fn decodes_a_file_url_before_reading_its_drive() {
    let (candidates, style) = resolve("file:///c%3A/Users/a.md");

    assert_eq!(candidates[0].target, PathBuf::from(r"C:\Users\a.md"));
    assert_eq!(candidates[0].form, PathForm::Absolute(AbsoluteStyle::Drive));
    assert!(style.lowercase_drive);
    assert!(style.encoded_drive_colon);
}

#[cfg(windows)]
#[test]
fn never_reads_a_file_url_as_msys() {
    assert!(targets_of("file:///c/x/a.md").is_empty());
}

#[cfg(not(windows))]
#[test]
fn resolves_posix_absolute_paths() {
    assert_eq!(
        resolve("/x/../y/a.md").0,
        vec![Candidate {
            target: PathBuf::from("/y/a.md"),
            form: PathForm::Absolute(AbsoluteStyle::Posix),
        }]
    );
}

#[cfg(not(windows))]
#[test]
fn yields_no_candidate_for_a_drive_file_url_off_windows() {
    assert!(targets_of("file:///C:/x.md").is_empty());
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn folds_case_in_keys() {
    assert_eq!(
        key_of(Path::new("Docs/A.md")),
        key_of(Path::new("docs/a.md"))
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn keeps_case_in_keys() {
    assert_ne!(
        key_of(Path::new("Docs/A.md")),
        key_of(Path::new("docs/a.md"))
    );
}

#[test]
fn compares_beneath_by_whole_components() {
    let base = absolute_path_of(&["src", "a"]);

    assert!(is_beneath(&absolute_path_of(&["src", "a", "x.md"]), &base));
    assert!(is_beneath(&base, &base));
    assert!(!is_beneath(&absolute_path_of(&["src", "ab"]), &base));
}

#[test]
fn shapes_dangling_paths_by_separator_and_extension() {
    assert!(is_dangling_shaped("docs/a.md"));
    assert!(is_dangling_shaped("docs\\a.md"));
    assert!(!is_dangling_shaped("and/or"));
    assert!(!is_dangling_shaped("1/2"));
    assert!(!is_dangling_shaped("index.md"));
    assert!(!is_dangling_shaped("docs/.hidden"));
}

#[test]
fn detects_the_separator() {
    assert_eq!(style_of("sub\\a.md").separator, '\\');
    assert_eq!(style_of("sub/a.md").separator, '/');
    assert_eq!(style_of("a.md").separator, '/');
}

#[test]
fn detects_and_decodes_doubled_backslashes() {
    assert!(style_of("sub\\\\a.md").doubled_backslashes);
    assert!(!style_of("sub\\a.md").doubled_backslashes);
    assert_eq!(
        targets_of("sub\\\\a.md")[0],
        absolute_path_of(&["work", "docs", "sub", "a.md"])
    );
}

#[test]
fn detects_a_dot_prefix() {
    assert!(style_of("./a.md").dot_prefix);
    assert!(style_of(".\\a.md").dot_prefix);
    assert!(!style_of("../a.md").dot_prefix);
}

#[test]
fn detects_a_trailing_separator() {
    assert!(style_of("sub/").trailing_separator);
    assert!(!style_of("sub/a.md").trailing_separator);
}

#[test]
fn detects_and_decodes_percent_encoding() {
    assert!(style_of("my%20file.md").percent_encoded);
    assert!(!style_of("100%.md").percent_encoded);
    assert_eq!(
        targets_of("%C3%A9t%C3%A9.md")[0],
        absolute_path_of(&["work", "docs", "été.md"])
    );
}

#[test]
fn keeps_percent_encoding_that_does_not_decode_to_utf8() {
    assert!(!style_of("a%FF.md").percent_encoded);
    assert_eq!(
        targets_of("a%FF.md")[0],
        absolute_path_of(&["work", "docs", "a%FF.md"])
    );
}

#[cfg(windows)]
#[test]
fn detects_and_strips_a_drive_file_url() {
    let (candidates, style) = resolve("file:///C:/x/a.md");

    assert!(style.file_scheme);
    assert_eq!(style.scheme, "file");
    assert_eq!(candidates[0].target, PathBuf::from("C:\\x\\a.md"));
    assert_eq!(candidates[0].form, PathForm::Absolute(AbsoluteStyle::Drive));
}

#[cfg(not(windows))]
#[test]
fn detects_and_strips_a_posix_file_url() {
    let (candidates, style) = resolve("file:///x/a.md");

    assert!(style.file_scheme);
    assert_eq!(candidates[0].target, PathBuf::from("/x/a.md"));
}

#[test]
fn records_the_written_scheme_case() {
    assert_eq!(style_of("FILE:///x/a.md").scheme, "FILE");
    assert_eq!(style_of("a.md").scheme, "");
}

#[test]
fn yields_no_candidate_for_a_relative_path_with_a_colon() {
    assert!(targets_of("a:b/c.md").is_empty());
}

#[cfg(not(windows))]
#[test]
fn yields_no_candidate_for_a_drive_path_off_windows() {
    assert!(targets_of(r"C:\Users\x.md").is_empty());
    assert!(targets_of("C:/x.md").is_empty());
}

#[test]
fn yields_no_candidate_when_decoding_yields_a_separator() {
    assert!(targets_of("docs%2Fa.md").is_empty());
    assert!(targets_of("docs%5ca.md").is_empty());
}

#[test]
fn records_the_hex_case() {
    assert!(style_of("%c3%a9.md").lowercase_hex);
    assert!(!style_of("%C3%A9.md").lowercase_hex);
    assert!(!style_of("a%20b.md").lowercase_hex);
}

#[cfg(not(windows))]
#[test]
fn takes_the_separator_after_the_root() {
    assert_eq!(style_of(r"/x\a.md").separator, '\\');
    assert_eq!(style_of("/a.md").separator, '/');
}

#[cfg(windows)]
#[test]
fn takes_the_separator_after_a_drive_or_msys_root() {
    assert_eq!(style_of(r"/c\x\a.md").separator, '\\');
    assert_eq!(style_of(r"C:\x/a.md").separator, '/');
    assert_eq!(style_of(r"C:\a.md").separator, '\\');
}

#[cfg(windows)]
#[test]
fn records_no_encoded_drive_colon_for_a_literal_colon() {
    assert!(!style_of("file:///C:/x/a.md").encoded_drive_colon);
    assert!(!style_of("file:///C:/my%20dir/a.md").encoded_drive_colon);
}
