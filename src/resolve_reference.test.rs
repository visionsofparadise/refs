use super::*;

fn absolute(names: &[&str]) -> PathBuf {
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
    resolve_reference(path, &absolute(&["work", "docs"]), &absolute(&["work"]))
}

fn targets(path: &str) -> Vec<PathBuf> {
    resolve(path)
        .0
        .into_iter()
        .map(|candidate| candidate.target)
        .collect()
}

fn style(path: &str) -> PathStyle {
    resolve(path).1
}

#[test]
fn orders_the_file_relative_candidate_before_the_working_directory_candidate() {
    assert_eq!(
        resolve("a.md").0,
        vec![
            Candidate {
                target: absolute(&["work", "docs", "a.md"]),
                form: PathForm::FileRelative,
            },
            Candidate {
                target: absolute(&["work", "a.md"]),
                form: PathForm::WorkingDirectoryRelative,
            },
        ]
    );
}

#[test]
fn deduplicates_candidates_when_the_referrer_is_the_working_directory() {
    let work = absolute(&["work"]);
    let (candidates, _) = resolve_reference("a.md", &work, &work);

    assert_eq!(
        candidates,
        vec![Candidate {
            target: absolute(&["work", "a.md"]),
            form: PathForm::FileRelative,
        }]
    );
}

#[test]
fn pops_parent_segments() {
    assert_eq!(
        targets("../x/../a.md"),
        vec![absolute(&["work", "a.md"]), absolute(&["a.md"])]
    );
}

#[test]
fn splits_on_backslashes() {
    assert_eq!(
        targets("sub\\a.md")[0],
        absolute(&["work", "docs", "sub", "a.md"])
    );
}

#[test]
fn normalizes_without_climbing_above_the_root() {
    assert_eq!(
        normalize_path(&absolute(&["..", "a", ".", "b"])),
        absolute(&["a", "b"])
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
    assert_eq!(targets("D:/x/../y.md"), vec![PathBuf::from("D:\\y.md")]);
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
fn yields_no_candidate_for_an_unanchored_rooted_path() {
    assert!(targets("/docs/a.md").is_empty());
    assert!(targets("\\\\server\\share\\a.md").is_empty());
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
    assert!(targets("file:///C:/x.md").is_empty());
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
    let base = absolute(&["src", "a"]);

    assert!(is_beneath(&absolute(&["src", "a", "x.md"]), &base));
    assert!(is_beneath(&base, &base));
    assert!(!is_beneath(&absolute(&["src", "ab"]), &base));
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
    assert_eq!(style("sub\\a.md").separator, '\\');
    assert_eq!(style("sub/a.md").separator, '/');
    assert_eq!(style("a.md").separator, '/');
}

#[test]
fn detects_and_decodes_doubled_backslashes() {
    assert!(style("sub\\\\a.md").doubled_backslashes);
    assert!(!style("sub\\a.md").doubled_backslashes);
    assert_eq!(
        targets("sub\\\\a.md")[0],
        absolute(&["work", "docs", "sub", "a.md"])
    );
}

#[test]
fn detects_a_dot_prefix() {
    assert!(style("./a.md").dot_prefix);
    assert!(style(".\\a.md").dot_prefix);
    assert!(!style("../a.md").dot_prefix);
}

#[test]
fn detects_a_trailing_separator() {
    assert!(style("sub/").trailing_separator);
    assert!(!style("sub/a.md").trailing_separator);
}

#[test]
fn detects_and_decodes_percent_encoding() {
    assert!(style("my%20file.md").percent_encoded);
    assert!(!style("100%.md").percent_encoded);
    assert_eq!(
        targets("%C3%A9t%C3%A9.md")[0],
        absolute(&["work", "docs", "été.md"])
    );
}

#[test]
fn keeps_percent_encoding_that_does_not_decode_to_utf8() {
    assert!(!style("a%FF.md").percent_encoded);
    assert_eq!(
        targets("a%FF.md")[0],
        absolute(&["work", "docs", "a%FF.md"])
    );
}

#[cfg(windows)]
#[test]
fn detects_and_strips_a_drive_file_url() {
    let (candidates, style) = resolve("file:///C:/x/a.md");

    assert!(style.file_scheme);
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
