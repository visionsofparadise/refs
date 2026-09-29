use std::path::{Path, PathBuf};

use crate::walk_scope::ScopeOptions;

#[derive(Debug, clap::Parser)]
#[command(
    name = "refs",
    bin_name = "refs",
    version,
    about = "List file references and repair them from declared moves",
    after_help = "Listing: refs [PATH]... prints each reference found as <file>:<line>:<column>: <token> -> <target>.\nFixing: refs - [PATH]... reads moves and deletes from stdin (git diff --name-status, mv -v, rm -v or git mv -v output) and repairs the references those moves broke."
)]
pub struct Arguments {
    #[arg(
        value_name = "PATH",
        help = "Files and directories to scan [default: .]; a first path of - reads declarations from stdin and fixes references"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Keep references whose target is this path or lies beneath it (repeatable)"
    )]
    pub to: Vec<PathBuf>,
    #[arg(
        long,
        help = "Keep dangling references only; exit 1 when any is printed"
    )]
    pub dangling: bool,
    #[arg(long, help = "With -, print the rewrites without writing them")]
    pub dry_run: bool,
    #[arg(
        long,
        help = "Respect no ignore file: .gitignore, .ignore, .rgignore, the global gitignore or .git/info/exclude"
    )]
    pub no_ignore: bool,
    #[arg(
        long,
        help = "Respect no git ignore source (.gitignore, the global gitignore, .git/info/exclude); .ignore and .rgignore still apply"
    )]
    pub no_ignore_vcs: bool,
    #[arg(long, help = "Scan hidden files and directories")]
    pub hidden: bool,
    #[arg(
        short = 'u',
        action = clap::ArgAction::Count,
        help = "Reduce filtering as rg does: -u is --no-ignore, -uu adds --hidden, -uuu equals -uu"
    )]
    pub unrestricted: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    List {
        scope: ScopeOptions,
        paths: Vec<PathBuf>,
        to: Vec<PathBuf>,
        dangling: bool,
    },
    Fix {
        scope: ScopeOptions,
        paths: Vec<PathBuf>,
        dry_run: bool,
    },
}

fn is_stdin(path: &Path) -> bool {
    path.as_os_str() == "-"
}

pub fn parse_arguments(arguments: Arguments) -> Result<Command, String> {
    if arguments.unrestricted > 3 {
        return Err("-u is accepted at most three times".to_string());
    }

    let scope = ScopeOptions {
        no_ignore: arguments.no_ignore || arguments.unrestricted >= 1,
        no_ignore_vcs: arguments.no_ignore_vcs,
        hidden: arguments.hidden || arguments.unrestricted >= 2,
    };

    let fix = arguments.paths.first().is_some_and(|path| is_stdin(path));
    let mut paths = arguments.paths;

    if fix {
        paths.remove(0);
    }

    if paths.iter().any(|path| is_stdin(path)) {
        return Err("- is accepted only as the first path".to_string());
    }

    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }

    if !fix {
        if arguments.dry_run {
            return Err("--dry-run applies only to fixing with -".to_string());
        }

        return Ok(Command::List {
            scope,
            paths,
            to: arguments.to,
            dangling: arguments.dangling,
        });
    }

    if !arguments.to.is_empty() {
        return Err("--to applies only to listing".to_string());
    }

    if arguments.dangling {
        return Err("--dangling applies only to listing".to_string());
    }

    Ok(Command::Fix {
        scope,
        paths,
        dry_run: arguments.dry_run,
    })
}

#[cfg(test)]
#[path = "parse_arguments.test.rs"]
mod tests;
