use std::path::{Path, PathBuf};

use crate::walk_scope::ScopeOptions;

#[derive(Debug, clap::Parser)]
#[command(
    name = "refs",
    bin_name = "refs",
    version,
    about = "List file references and repair them from declared moves"
)]
pub struct Arguments {
    pub paths: Vec<PathBuf>,
    #[arg(long)]
    pub to: Vec<PathBuf>,
    #[arg(long)]
    pub dangling: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub no_ignore: bool,
    #[arg(long)]
    pub no_ignore_vcs: bool,
    #[arg(long)]
    pub hidden: bool,
    #[arg(short = 'u', action = clap::ArgAction::Count)]
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
