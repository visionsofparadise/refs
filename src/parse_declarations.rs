use std::path::{Path, PathBuf};

use crate::resolve_reference::{is_beneath, key_of, normalize_path, parse_absolute};
use crate::unquote::{unquote_git, unquote_shell};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declaration {
    Move {
        from: PathBuf,
        to: PathBuf,
        origin: Origin,
    },
    Delete {
        path: PathBuf,
        origin: Origin,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub origin: Origin,
    pub reason: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Skip,
    Move(String, String),
    Delete(String),
    Copy(String, String),
    Removed(String),
    RemovedDirectory(String),
}

struct PendingCopy {
    from: PathBuf,
    to: PathBuf,
    origin: Origin,
}

const UNRECOGNIZED: &str = "unrecognized";
const AMBIGUOUS: &str = "ambiguous";

fn resolve_path(text: &str, working_directory: &Path) -> Option<PathBuf> {
    if text.is_empty() {
        return None;
    }

    Some(match parse_absolute(text) {
        Some((path, _)) => normalize_path(&path),
        None => normalize_path(&working_directory.join(text)),
    })
}

fn move_of(from: PathBuf, to: PathBuf, origin: Origin) -> Declaration {
    let into_directory = match from.file_name() {
        Some(name) if to.is_dir() => {
            let inside = to.join(name);

            inside.exists().then_some(inside)
        }
        _ => None,
    };

    Declaration::Move {
        from,
        to: into_directory.unwrap_or(to),
        origin,
    }
}

fn split_fields(line: &str) -> Vec<&str> {
    if line.contains('\t') {
        return line.split('\t').collect();
    }

    let mut fields = Vec::new();
    let mut rest = line.trim_start_matches(' ');

    while !rest.is_empty() {
        let bytes = rest.as_bytes();
        let mut end = 0;
        let mut quoted = false;

        while end < bytes.len() {
            match bytes[end] {
                b'\\' if quoted && bytes.get(end + 1).is_some_and(u8::is_ascii) => end += 2,
                b'"' => {
                    quoted = !quoted;
                    end += 1;
                }
                b' ' if !quoted => break,
                _ => end += 1,
            }
        }

        let end = end.min(bytes.len());

        fields.push(&rest[..end]);

        rest = rest[end..].trim_start_matches(' ');
    }

    fields
}

fn is_status(field: &str, letters: &str) -> bool {
    let mut characters = field.chars();

    characters
        .next()
        .is_some_and(|first| letters.contains(first))
        && characters.all(|character| character.is_ascii_digit())
}

fn name_status(line: &str) -> Option<Result<Parsed, &'static str>> {
    let fields = split_fields(line);
    let status = *fields.first()?;

    if fields.get(1) == Some(&"->") {
        return None;
    }

    let expected = if is_status(status, "RC") {
        3
    } else if is_status(status, "DAMT") {
        2
    } else {
        return None;
    };

    if fields.len() != expected {
        return None;
    }

    let mut paths = Vec::new();

    for field in &fields[1..] {
        match unquote_git(field) {
            Some(path) => paths.push(path),
            None => return Some(Err(UNRECOGNIZED)),
        }
    }

    Some(Ok(match (status.chars().next()?, paths.as_slice()) {
        ('R', [from, to]) => Parsed::Move(from.clone(), to.clone()),
        ('D', [path]) => Parsed::Delete(path.clone()),
        _ => Parsed::Skip,
    }))
}

fn gnu_move(line: &str) -> Option<Parsed> {
    let (copied, rest) = if let Some(rest) = line.strip_prefix("copied ") {
        (true, rest)
    } else {
        (false, line.strip_prefix("renamed ").unwrap_or(line))
    };

    let (from, rest) = unquote_shell(rest)?;
    let (to, rest) = unquote_shell(rest.strip_prefix(" -> ")?)?;

    if !rest.is_empty() {
        return None;
    }

    Some(if copied {
        Parsed::Copy(from, to)
    } else {
        Parsed::Move(from, to)
    })
}

fn split_exactly<'a>(text: &'a str, separator: &str) -> Result<(&'a str, &'a str), &'static str> {
    let mut parts = text.split(separator);

    match (parts.next(), parts.next(), parts.next()) {
        (Some(from), Some(to), None) if !from.is_empty() && !to.is_empty() => Ok((from, to)),
        (_, Some(_), Some(_)) => Err(AMBIGUOUS),
        _ => Err(UNRECOGNIZED),
    }
}

fn single_path(rest: &str) -> Result<String, &'static str> {
    match unquote_shell(rest) {
        Some((path, "")) => Ok(path),
        _ => Err(UNRECOGNIZED),
    }
}

fn parse_line(line: &str) -> Result<Parsed, &'static str> {
    if let Some(result) = name_status(line) {
        return result;
    }

    if line.starts_with("created directory ") {
        return Ok(Parsed::Skip);
    }

    if let Some(rest) = line.strip_prefix("removed directory ") {
        return single_path(rest).map(Parsed::RemovedDirectory);
    }

    if let Some(rest) = line.strip_prefix("removed ") {
        return single_path(rest).map(Parsed::Removed);
    }

    if let Some(parsed) = gnu_move(line) {
        return Ok(parsed);
    }

    if line.starts_with("copied ") || line.starts_with("renamed ") {
        return Err(if line.matches(" -> ").count() > 1 {
            AMBIGUOUS
        } else {
            UNRECOGNIZED
        });
    }

    if let Some(rest) = line.strip_prefix("Renaming ") {
        return split_exactly(rest, " to ")
            .map(|(from, to)| Parsed::Move(from.to_string(), to.to_string()));
    }

    if let Some(rest) = line.strip_prefix("rm ") {
        return single_path(rest).map(Parsed::Delete);
    }

    split_exactly(line, " -> ").map(|(from, to)| Parsed::Move(from.to_string(), to.to_string()))
}

fn parse_records(input: &[u8], working_directory: &Path) -> (Vec<Declaration>, Vec<Rejected>) {
    let mut declarations = Vec::new();
    let mut rejected = Vec::new();
    let mut fields = input.split(|byte| *byte == 0).peekable();
    let mut index = 0;

    while let Some(status) = fields.next() {
        if status.is_empty() && fields.peek().is_none() {
            break;
        }

        index += 1;

        let status_text = String::from_utf8_lossy(status).into_owned();
        let count = if is_status(&status_text, "RC") {
            2
        } else if is_status(&status_text, "DAMT") {
            1
        } else {
            rejected.push(Rejected {
                origin: Origin {
                    line: index,
                    text: status_text,
                },
                reason: UNRECOGNIZED.to_string(),
            });

            break;
        };

        let paths: Vec<Option<&str>> = (0..count)
            .map_while(|_| fields.next())
            .map(|field| std::str::from_utf8(field).ok())
            .collect();
        let text = std::iter::once(status_text.as_str())
            .chain(paths.iter().map(|path| path.unwrap_or("?")))
            .collect::<Vec<_>>()
            .join("\t");
        let origin = Origin { line: index, text };

        let resolved: Option<Vec<PathBuf>> = if paths.len() == count {
            paths
                .iter()
                .map(|path| path.and_then(|path| resolve_path(path, working_directory)))
                .collect()
        } else {
            None
        };

        match (status_text.chars().next(), resolved.as_deref()) {
            (Some('R'), Some([from, to])) => {
                declarations.push(move_of(from.clone(), to.clone(), origin));
            }
            (Some('D'), Some([path])) => declarations.push(Declaration::Delete {
                path: path.clone(),
                origin,
            }),
            (_, Some(_)) => {}
            (_, None) => rejected.push(Rejected {
                origin,
                reason: UNRECOGNIZED.to_string(),
            }),
        }
    }

    (declarations, rejected)
}

struct Sequence {
    declarations: Vec<Declaration>,
    pending: Vec<PendingCopy>,
    completed: Vec<PathBuf>,
}

impl Sequence {
    fn complete(&mut self, copy: PendingCopy) {
        self.completed.push(copy.from.clone());
        self.declarations
            .push(move_of(copy.from, copy.to, copy.origin));
    }

    fn apply(
        &mut self,
        parsed: Parsed,
        origin: &Origin,
        working_directory: &Path,
    ) -> Result<(), &'static str> {
        let resolve = |text: &str| resolve_path(text, working_directory).ok_or(UNRECOGNIZED);
        let delete = |path: PathBuf| Declaration::Delete {
            path,
            origin: origin.clone(),
        };

        match parsed {
            Parsed::Skip => {}
            Parsed::Move(from, to) => {
                let declaration = move_of(resolve(&from)?, resolve(&to)?, origin.clone());

                self.declarations.push(declaration);
            }
            Parsed::Delete(path) => self.declarations.push(delete(resolve(&path)?)),
            Parsed::Copy(from, to) => self.pending.push(PendingCopy {
                from: resolve(&from)?,
                to: resolve(&to)?,
                origin: origin.clone(),
            }),
            Parsed::Removed(path) => {
                let path = resolve(&path)?;
                let matching = self
                    .pending
                    .iter()
                    .position(|copy| key_of(&copy.from) == key_of(&path));

                match matching {
                    Some(position) => {
                        let copy = self.pending.remove(position);

                        self.complete(copy);
                    }
                    None => self.declarations.push(delete(path)),
                }
            }
            Parsed::RemovedDirectory(path) => {
                let path = resolve(&path)?;
                let (beneath, others): (Vec<PendingCopy>, Vec<PendingCopy>) = self
                    .pending
                    .drain(..)
                    .partition(|copy| is_beneath(&copy.from, &path));
                let moved = !beneath.is_empty()
                    || self
                        .completed
                        .iter()
                        .any(|source| is_beneath(source, &path));

                self.pending = others;

                for copy in beneath {
                    self.complete(copy);
                }

                if !moved {
                    self.declarations.push(delete(path));
                }
            }
        }

        Ok(())
    }
}

fn parse_lines(input: &[u8], working_directory: &Path) -> (Vec<Declaration>, Vec<Rejected>) {
    let mut rejected = Vec::new();
    let mut sequence = Sequence {
        declarations: Vec::new(),
        pending: Vec::new(),
        completed: Vec::new(),
    };

    for (index, raw) in input.split(|byte| *byte == b'\n').enumerate() {
        let decoded = String::from_utf8_lossy(raw);
        let line = decoded.strip_suffix('\r').unwrap_or(&decoded);

        if line.trim().is_empty() {
            continue;
        }

        let origin = Origin {
            line: index + 1,
            text: line.to_string(),
        };
        let parsed = if std::str::from_utf8(raw).is_ok() {
            parse_line(line)
        } else {
            Err(UNRECOGNIZED)
        };

        if let Err(reason) =
            parsed.and_then(|parsed| sequence.apply(parsed, &origin, working_directory))
        {
            rejected.push(Rejected {
                origin,
                reason: reason.to_string(),
            });
        }
    }

    (sequence.declarations, rejected)
}

pub fn parse_declarations(
    input: &[u8],
    working_directory: &Path,
) -> (Vec<Declaration>, Vec<Rejected>) {
    if input.contains(&0) {
        parse_records(input, working_directory)
    } else {
        parse_lines(input, working_directory)
    }
}

#[cfg(test)]
#[path = "parse_declarations.test.rs"]
mod tests;
