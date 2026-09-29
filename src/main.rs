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
use std::ffi::OsString;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use format_path::format_path;
use list_references::list_references;
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

struct Listing<'a> {
    options: &'a ScopeOptions,
    paths: &'a [PathBuf],
    to: &'a [PathBuf],
    dangling: bool,
    working_directory: &'a Path,
}

fn list(listing: &Listing, streams: &mut Streams) -> std::io::Result<i32> {
    let working_directory = listing.working_directory;
    let scope = walk_scope(listing.paths, listing.options, working_directory)?;
    let result = list_references(&scope, working_directory, listing.to, listing.dangling)?;

    for entry in &result.listed {
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

    for warning in &scope.warnings {
        streams.report(warning);
    }

    for error in scope.errors.iter().chain(&result.errors) {
        streams.report(error);
    }

    if !scope.errors.is_empty() || !result.errors.is_empty() {
        return Ok(FAILURE);
    }

    Ok(if listing.dangling && !result.listed.is_empty() {
        FINDINGS
    } else {
        SUCCESS
    })
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

            return streams.fail(rendered.trim_end().trim_start_matches("error: "));
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

    let listing = Listing {
        options: &options,
        paths: &paths,
        to: &to,
        dangling,
        working_directory: &working_directory,
    };

    list(&listing, streams).unwrap_or_else(|error| streams.fail(&error.to_string()))
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
