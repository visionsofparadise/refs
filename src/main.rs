mod format_path;
mod list_references;
mod parse_arguments;
#[allow(dead_code)]
mod parse_declarations;
mod path_text;
#[allow(dead_code)]
mod render_reference;
mod resolve_reference;
mod tokenize_references;
#[allow(dead_code)]
mod unquote;
mod walk_scope;

use clap::error::ErrorKind;
use clap::Parser;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use format_path::format_path;
use list_references::list_references;
use parse_arguments::{parse_arguments, Arguments, Command};
use walk_scope::{walk_scope, ScopeOptions};

const SUCCESS: i32 = 0;
const FINDINGS: i32 = 1;
const FAILURE: i32 = 2;

fn report(message: &str) -> i32 {
    eprintln!("refs: {message}");

    FAILURE
}

fn list(
    options: &ScopeOptions,
    paths: &[PathBuf],
    to: &[PathBuf],
    dangling: bool,
    working_directory: &Path,
) -> std::io::Result<i32> {
    let scope = walk_scope(paths, options, working_directory)?;
    let listed = list_references(&scope, working_directory, to, dangling)?;
    let mut stdout = BufWriter::new(std::io::stdout().lock());

    for entry in &listed {
        let _ = writeln!(
            stdout,
            "{}:{}:{}: {}{} -> {}{}",
            format_path(&entry.file, working_directory),
            entry.token.line,
            entry.token.column,
            entry.token.path,
            entry.token.suffix,
            format_path(&entry.target, working_directory),
            if entry.dangling { " (dangling)" } else { "" }
        );
    }

    let _ = stdout.flush();

    Ok(if dangling && !listed.is_empty() {
        FINDINGS
    } else {
        SUCCESS
    })
}

fn run() -> i32 {
    let arguments = match Arguments::try_parse() {
        Ok(arguments) => arguments,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();

            return SUCCESS;
        }
        Err(error) => {
            let rendered = error.render().to_string();

            return report(rendered.trim_end().trim_start_matches("error: "));
        }
    };

    let (options, paths, to, dangling) = match parse_arguments(arguments) {
        Ok(Command::List {
            scope,
            paths,
            to,
            dangling,
        }) => (scope, paths, to, dangling),
        Ok(Command::Fix { .. }) => return report("- is not implemented yet"),
        Err(message) => return report(&message),
    };

    let working_directory = match std::env::current_dir().and_then(dunce::canonicalize) {
        Ok(directory) => directory,
        Err(error) => return report(&format!("working directory: {error}")),
    };

    list(&options, &paths, &to, dangling, &working_directory)
        .unwrap_or_else(|error| report(&error.to_string()))
}

fn main() {
    std::process::exit(run());
}
