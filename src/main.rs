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
#[cfg(test)]
mod tree_of;
#[allow(dead_code)]
mod unquote;
mod walk_scope;

use clap::error::ErrorKind;
use clap::Parser;
use std::ffi::OsString;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use format_path::format_path;
use list_references::{list_references, resolve_targets};
use parse_arguments::{parse_arguments, Arguments, Command};
use walk_scope::{walk_scope, ScopeOptions};

const SUCCESS: i32 = 0;
const FINDINGS: i32 = 1;
const FAILURE: i32 = 2;

struct Streams<'a> {
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
}

impl Streams<'_> {
    fn report(&mut self, message: &str) {
        let _ = writeln!(self.stderr, "refs: {message}");
    }

    fn fail(&mut self, message: &str) -> i32 {
        self.report(message);

        FAILURE
    }
}

fn list(
    options: &ScopeOptions,
    paths: &[PathBuf],
    to: &[PathBuf],
    dangling: bool,
    working_directory: &Path,
    streams: &mut Streams,
) -> i32 {
    let to = match resolve_targets(to, working_directory) {
        Ok(to) => to,
        Err(errors) => {
            for error in &errors {
                streams.report(error);
            }

            return FAILURE;
        }
    };

    let scope = walk_scope(paths, options, working_directory);
    let listing = list_references(&scope, working_directory, &to, dangling);

    for entry in &listing.listed {
        let _ = writeln!(
            streams.stdout,
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

    let _ = streams.stdout.flush();

    for message in scope
        .warnings
        .iter()
        .chain(&scope.errors)
        .chain(&listing.errors)
    {
        streams.report(message);
    }

    if !scope.errors.is_empty() || !listing.errors.is_empty() {
        return FAILURE;
    }

    if dangling && !listing.listed.is_empty() {
        return FINDINGS;
    }

    SUCCESS
}

fn run(
    arguments: Vec<OsString>,
    working_directory: std::io::Result<PathBuf>,
    streams: &mut Streams,
) -> i32 {
    let arguments = match Arguments::try_parse_from(arguments) {
        Ok(arguments) => arguments,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = write!(streams.stdout, "{}", error.render());

            return SUCCESS;
        }
        Err(error) => {
            let rendered = error.render().to_string();
            let first_line = rendered.lines().next().unwrap_or_default();

            return streams.fail(first_line.trim_start_matches("error: "));
        }
    };

    let (options, paths, to, dangling) = match parse_arguments(arguments) {
        Ok(Command::List {
            scope,
            paths,
            to,
            dangling,
        }) => (scope, paths, to, dangling),
        Ok(Command::Fix { .. }) => return streams.fail("- is not implemented yet"),
        Err(message) => return streams.fail(&message),
    };

    let working_directory = match working_directory {
        Ok(directory) => directory,
        Err(error) => return streams.fail(&format!("working directory: {error}")),
    };

    list(&options, &paths, &to, dangling, &working_directory, streams)
}

fn main() {
    let stdout = std::io::stdout();
    let mut stdout = BufWriter::new(stdout.lock());
    let mut stderr = std::io::stderr();

    let code = run(
        std::env::args_os().collect(),
        std::env::current_dir().and_then(dunce::canonicalize),
        &mut Streams {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );

    let _ = stdout.flush();

    std::process::exit(code);
}

#[cfg(test)]
#[path = "main.integration.test.rs"]
mod integration;
