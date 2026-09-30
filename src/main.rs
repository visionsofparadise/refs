mod compose_declarations;
mod fix_references;
mod format_path;
mod list_references;
mod map_in_parallel;
mod parse_arguments;
mod parse_declarations;
mod path_text;
mod render_reference;
mod resolve_reference;
mod tokenize_references;
#[cfg(test)]
mod tree_of;
mod unquote;
mod walk_scope;
mod write_edits;

use clap::error::ErrorKind;
use clap::Parser;
use same_file::Handle;
use std::ffi::OsString;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use fix_references::{plan_fix, Finding, Outcome};
use format_path::format_path;
use list_references::{list_references, resolve_targets};
use parse_arguments::{parse_arguments, Arguments, Command};
use parse_declarations::parse_declarations;
use walk_scope::{walk_scope, Scope, ScopeOptions};
use write_edits::write_edits;

const SUCCESS: i32 = 0;
const FINDINGS: i32 = 1;
const FAILURE: i32 = 2;

struct Streams<'a> {
    stdin: &'a mut dyn Read,
    stdin_handle: Option<Handle>,
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

fn report_scope(scope: &Scope, streams: &mut Streams) {
    for message in scope.warnings.iter().chain(&scope.errors) {
        streams.report(message);
    }
}

fn line_of(finding: &Finding, withheld: bool, working_directory: &Path) -> String {
    let (replacement, kind) = match &finding.outcome {
        Outcome::Rewritten { replacement } if withheld => (replacement.clone(), " (not rewritten)"),
        Outcome::Rewritten { replacement } => (replacement.clone(), ""),
        Outcome::Deleted { target } => (format_path(target, working_directory), " (deleted)"),
        Outcome::OutOfScope { target } => {
            (format_path(target, working_directory), " (out of scope)")
        }
        Outcome::Unrewritable { target } => {
            (format_path(target, working_directory), " (unrewritable)")
        }
    };

    format!(
        "{}:{}:{}: {}{} -> {replacement}{kind}",
        format_path(&finding.file, working_directory),
        finding.token.line,
        finding.token.column,
        finding.token.path,
        finding.token.suffix,
    )
}

fn fix(
    options: &ScopeOptions,
    paths: &[PathBuf],
    dry_run: bool,
    working_directory: &Path,
    streams: &mut Streams,
) -> i32 {
    let mut input = Vec::new();

    if let Err(error) = streams.stdin.read_to_end(&mut input) {
        return streams.fail(&format!("stdin: {error}"));
    }

    let (declarations, mut rejected) = parse_declarations(
        &input,
        working_directory,
        &|path: &Path| path.is_dir(),
        &|path: &Path| path.exists(),
    );

    let scope = walk_scope(paths, options, working_directory);
    let plan = plan_fix(declarations, &scope, working_directory);

    let inputs: Vec<&PathBuf> = plan
        .edits
        .iter()
        .map(|edit| &edit.file)
        .filter(|file| is_input(file, streams.stdin_handle.as_ref()))
        .collect();

    for finding in &plan.findings {
        let withheld = inputs.contains(&&finding.file);

        let _ = writeln!(
            streams.stdout,
            "{}",
            line_of(finding, withheld, working_directory)
        );
    }

    for destination in &plan.unscanned {
        let _ = writeln!(
            streams.stdout,
            "{}: outbound references not repointed (out of scope)",
            format_path(destination, working_directory)
        );
    }

    let _ = streams.stdout.flush();

    rejected.extend(plan.rejected);

    for rejection in &rejected {
        streams.report(&format!(
            "skipped declaration line {}: {}: {}",
            rejection.origin.line, rejection.reason, rejection.origin.text
        ));
    }

    report_scope(&scope, streams);

    for error in &plan.errors {
        streams.report(error);
    }

    let mut failed = !scope.errors.is_empty() || !plan.errors.is_empty();

    for edit in &plan.edits {
        let file = format_path(&edit.file, working_directory);

        if inputs.contains(&&edit.file) {
            streams.report(&format!("{file}: declaration input not rewritten"));
        } else if !dry_run {
            if let Err(error) = write_edits(std::slice::from_ref(edit)) {
                streams.report(&format!("{file}: {error}"));

                failed = true;
            }
        }
    }

    if failed {
        return FAILURE;
    }

    let unrepaired = plan
        .findings
        .iter()
        .any(|finding| !matches!(finding.outcome, Outcome::Rewritten { .. }));

    if unrepaired || !inputs.is_empty() || !plan.unscanned.is_empty() || !rejected.is_empty() {
        return FINDINGS;
    }

    SUCCESS
}

fn is_input(file: &Path, stdin_handle: Option<&Handle>) -> bool {
    stdin_handle.is_some_and(|input| Handle::from_path(file).is_ok_and(|handle| handle == *input))
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

    report_scope(&scope, streams);

    for message in &listing.errors {
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

    let command = match parse_arguments(arguments) {
        Ok(command) => command,
        Err(message) => return streams.fail(&message),
    };

    let working_directory = match working_directory {
        Ok(directory) => directory,
        Err(error) => return streams.fail(&format!("working directory: {error}")),
    };

    match command {
        Command::List {
            scope,
            paths,
            to,
            dangling,
        } => list(&scope, &paths, &to, dangling, &working_directory, streams),
        Command::Fix {
            scope,
            paths,
            dry_run,
        } => fix(&scope, &paths, dry_run, &working_directory, streams),
    }
}

fn main() {
    let stdout = std::io::stdout();
    let mut stdout = BufWriter::new(stdout.lock());
    let mut stderr = std::io::stderr();
    let mut stdin = std::io::stdin().lock();

    let code = run(
        std::env::args_os().collect(),
        std::env::current_dir().and_then(dunce::canonicalize),
        &mut Streams {
            stdin: &mut stdin,
            stdin_handle: Handle::stdin().ok(),
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
