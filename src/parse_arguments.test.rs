use super::*;
use clap::Parser;

fn command_of(arguments: &[&str]) -> Result<Command, String> {
    let arguments =
        Arguments::try_parse_from(std::iter::once("refs").chain(arguments.iter().copied()))
            .map_err(|error| error.to_string())?;

    parse_arguments(arguments)
}

fn scope_of(arguments: &[&str]) -> ScopeOptions {
    match command_of(arguments).unwrap() {
        Command::List { scope, .. } | Command::Fix { scope, .. } => scope,
    }
}

fn scope(no_ignore: bool, hidden: bool) -> ScopeOptions {
    ScopeOptions {
        no_ignore,
        no_ignore_vcs: false,
        hidden,
    }
}

#[test]
fn lists_the_working_directory_when_no_path_is_given() {
    assert_eq!(
        command_of(&[]),
        Ok(Command::List {
            scope: scope(false, false),
            paths: vec![PathBuf::from(".")],
            to: Vec::new(),
            dangling: false,
        })
    );
}

#[test]
fn fixes_with_the_paths_after_a_leading_dash() {
    assert_eq!(
        command_of(&["-", "docs", "--dry-run"]),
        Ok(Command::Fix {
            scope: scope(false, false),
            paths: vec![PathBuf::from("docs")],
            dry_run: true,
        })
    );

    assert_eq!(
        command_of(&["-"]),
        Ok(Command::Fix {
            scope: scope(false, false),
            paths: vec![PathBuf::from(".")],
            dry_run: false,
        })
    );
}

#[test]
fn rejects_a_dash_after_the_first_path() {
    assert!(command_of(&["docs", "-"]).is_err());
    assert!(command_of(&["-", "docs", "-"]).is_err());
}

#[test]
fn rejects_listing_flags_when_fixing() {
    assert!(command_of(&["-", "--to", "a.md"]).is_err());
    assert!(command_of(&["-", "--dangling"]).is_err());
}

#[test]
fn rejects_dry_run_when_listing() {
    assert!(command_of(&["--dry-run"]).is_err());
}

#[test]
fn stacks_unrestricted_as_rg_does() {
    assert_eq!(scope_of(&["-u"]), scope(true, false));
    assert_eq!(scope_of(&["-uu"]), scope(true, true));
    assert_eq!(scope_of(&["-uuu"]), scope(true, true));
    assert_eq!(scope_of(&["-u", "-u", "-"]), scope(true, true));
    assert!(command_of(&["-uuuu"]).is_err());
}
