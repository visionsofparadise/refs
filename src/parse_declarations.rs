use std::path::{Path, PathBuf};

use crate::resolve_reference::{is_beneath, key_of, locate_path};
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
    Move {
        from: String,
        to: String,
        hand_written: bool,
    },
    Delete(String),
    Copy(String, String),
    Created(String),
    Removed(String),
    RemovedDirectory(String),
}

struct PendingCopy {
    from: PathBuf,
    to: PathBuf,
    slot: usize,
    origin: Origin,
}

const UNRECOGNIZED: &str = "unrecognized";
const AMBIGUOUS: &str = "ambiguous";
const UNSUPPORTED_PATH: &str = "unsupported path";
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

fn resolve_path(text: &str, working_directory: &Path) -> Result<PathBuf, &'static str> {
    locate_path(text, working_directory).ok_or(UNSUPPORTED_PATH)
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

fn parse_name_status(line: &str) -> Option<Result<Parsed, &'static str>> {
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
        ('R', [from, to]) => Parsed::Move {
            from: from.clone(),
            to: to.clone(),
            hand_written: !line.contains('\t'),
        },
        ('D', [path]) => Parsed::Delete(path.clone()),
        _ => Parsed::Skip,
    }))
}

fn parse_gnu_move(line: &str) -> Option<Parsed> {
    let (copied, rest) = if let Some(rest) = line.strip_prefix("copied ") {
        (true, rest)
    } else if let Some(rest) = line.strip_prefix("renamed ") {
        (false, rest)
    } else if line.starts_with(['\'', '"']) || line.starts_with("$'") {
        (false, line)
    } else {
        return None;
    };

    let (from, rest) = unquote_shell(rest)?;
    let (to, rest) = unquote_shell(rest.strip_prefix(" -> ")?)?;

    if !rest.is_empty() {
        return None;
    }

    Some(if copied {
        Parsed::Copy(from, to)
    } else {
        Parsed::Move {
            from,
            to,
            hand_written: false,
        }
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

fn read_path(rest: &str) -> Result<String, &'static str> {
    match unquote_shell(rest) {
        Some((path, "")) => Ok(path),
        _ => Err(UNRECOGNIZED),
    }
}

fn strip_backup_tail(line: &str) -> &str {
    match line.rfind(" (backup: ") {
        Some(index) if line.ends_with(')') => &line[..index],
        _ => line,
    }
}

fn parse_line(line: &str) -> Result<Parsed, &'static str> {
    if let Some(result) = parse_name_status(line) {
        return result;
    }

    if let Some(rest) = line.strip_prefix("created directory ") {
        return read_path(rest).map(Parsed::Created);
    }

    if let Some(rest) = line.strip_prefix("removed directory ") {
        return read_path(rest).map(Parsed::RemovedDirectory);
    }

    if let Some(rest) = line.strip_prefix("removed ") {
        return read_path(rest).map(Parsed::Removed);
    }

    let line = strip_backup_tail(line);

    if let Some(parsed) = parse_gnu_move(line) {
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
        return split_exactly(rest, " to ").map(|(from, to)| Parsed::Move {
            from: from.to_string(),
            to: to.to_string(),
            hand_written: false,
        });
    }

    if let Some(rest) = line.strip_prefix("rm ") {
        return match rest
            .strip_prefix('\'')
            .and_then(|inner| inner.strip_suffix('\''))
        {
            Some(path) if !path.is_empty() => Ok(Parsed::Delete(path.to_string())),
            _ => Err(UNRECOGNIZED),
        };
    }

    split_exactly(line, " -> ").map(|(from, to)| Parsed::Move {
        from: from.to_string(),
        to: to.to_string(),
        hand_written: false,
    })
}

fn reject(origin: Origin, reason: &str) -> Rejected {
    Rejected {
        origin,
        reason: reason.to_string(),
    }
}

fn parse_records(input: &[u8], working_directory: &Path) -> (Vec<Declaration>, Vec<Rejected>) {
    let mut fields: Vec<&[u8]> = input.split(|byte| *byte == 0).collect();

    if fields
        .last()
        .is_some_and(|last| last.iter().all(u8::is_ascii_whitespace))
    {
        fields.pop();
    }

    let mut declarations = Vec::new();
    let mut rejected = Vec::new();
    let mut fields = fields.into_iter().peekable();
    let mut index = 0;

    while let Some(status) = fields.next() {
        index += 1;

        let status_text = String::from_utf8_lossy(status).into_owned();
        let count = if is_status(&status_text, "RC") { 2 } else { 1 };
        let known = is_status(&status_text, "RC") || is_status(&status_text, "DAMT");

        let paths: Vec<Option<&str>> = (0..count)
            .map_while(|_| fields.next())
            .map(|field| std::str::from_utf8(field).ok())
            .collect();
        let last = fields.peek().is_none();
        let paths: Vec<Option<&str>> = paths
            .into_iter()
            .enumerate()
            .map(|(position, path)| {
                if last && position + 1 == count {
                    path.map(|path| path.trim_end_matches(['\n', '\r']))
                } else {
                    path
                }
            })
            .collect();
        let text = std::iter::once(status_text.as_str())
            .chain(paths.iter().map(|path| path.unwrap_or("?")))
            .collect::<Vec<_>>()
            .join("\t");
        let origin = Origin { line: index, text };

        if !known {
            rejected.push(reject(origin, UNRECOGNIZED));

            continue;
        }

        if paths.len() != count || paths.iter().any(Option::is_none) {
            rejected.push(reject(origin, UNRECOGNIZED));

            continue;
        }

        let resolved: Result<Vec<PathBuf>, &str> = paths
            .iter()
            .flatten()
            .map(|path| resolve_path(path, working_directory))
            .collect();

        match (status_text.chars().next(), resolved.as_deref()) {
            (_, Err(reason)) => rejected.push(reject(origin, reason)),
            (Some('R'), Ok([from, to])) => declarations.push(Declaration::Move {
                from: from.clone(),
                to: to.clone(),
                origin,
            }),
            (Some('D'), Ok([path])) => declarations.push(Declaration::Delete {
                path: path.clone(),
                origin,
            }),
            _ => {}
        }
    }

    (declarations, rejected)
}

struct Sequence<'a> {
    slots: Vec<Option<Declaration>>,
    pending: Vec<PendingCopy>,
    completed: Vec<PathBuf>,
    created: Vec<PathBuf>,
    working_directory: &'a Path,
    is_directory: &'a dyn Fn(&Path) -> bool,
}

impl Sequence<'_> {
    fn add(&mut self, declaration: Option<Declaration>) -> usize {
        self.slots.push(declaration);

        self.slots.len() - 1
    }

    fn complete(&mut self, copy: PendingCopy) {
        self.completed.push(copy.from.clone());

        self.slots[copy.slot] = Some(Declaration::Move {
            from: copy.from,
            to: copy.to,
            origin: copy.origin,
        });
    }

    fn is_copied_beneath(&self, path: &Path) -> bool {
        self.pending.iter().any(|copy| is_beneath(&copy.from, path))
            || self.completed.iter().any(|source| is_beneath(source, path))
    }

    fn created_match_of(&self, path: &Path) -> Option<PathBuf> {
        let base = key_of(self.working_directory);
        let key = key_of(path);
        let relative = key.strip_prefix(base.as_slice())?;

        if relative.is_empty() {
            return None;
        }

        self.created
            .iter()
            .find(|created| {
                let created_key = key_of(created);

                created_key.len() > relative.len() && created_key.ends_with(relative)
            })
            .cloned()
    }

    fn is_beneath_copied_root(&self, path: &Path) -> bool {
        let base = self.working_directory;
        let Ok(relative) = path.strip_prefix(base) else {
            return false;
        };
        let Some(top) = relative.components().next() else {
            return false;
        };
        let root = base.join(top);

        root != path && self.is_copied_beneath(&root)
    }

    fn apply(&mut self, parsed: Parsed, origin: &Origin) -> Result<(), &'static str> {
        let working_directory = self.working_directory;
        let resolve = |text: &str| resolve_path(text, working_directory);
        let delete = |path: PathBuf| Declaration::Delete {
            path,
            origin: origin.clone(),
        };

        match parsed {
            Parsed::Skip => {}
            Parsed::Move {
                from,
                to,
                hand_written,
            } => {
                let from = resolve(&from)?;
                let mut to = resolve(&to)?;

                if hand_written && (self.is_directory)(&to) {
                    if let Some(name) = from.file_name() {
                        to = to.join(name);
                    }
                }

                self.add(Some(Declaration::Move {
                    from,
                    to,
                    origin: origin.clone(),
                }));
            }
            Parsed::Delete(path) => {
                self.add(Some(delete(resolve(&path)?)));
            }
            Parsed::Copy(from, to) => {
                let copy_from = resolve(&from)?;
                let copy_to = resolve(&to)?;
                let slot = self.add(None);

                self.pending.push(PendingCopy {
                    from: copy_from,
                    to: copy_to,
                    slot,
                    origin: origin.clone(),
                });
            }
            Parsed::Created(path) => {
                let path = resolve(&path)?;

                self.created.push(path);
            }
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
                    None => {
                        self.add(Some(delete(path)));
                    }
                }
            }
            Parsed::RemovedDirectory(path) => {
                let path = resolve(&path)?;
                let (beneath, others): (Vec<PendingCopy>, Vec<PendingCopy>) = self
                    .pending
                    .drain(..)
                    .partition(|copy| is_beneath(&copy.from, &path));
                let copied = !beneath.is_empty()
                    || self
                        .completed
                        .iter()
                        .any(|source| is_beneath(source, &path));

                self.pending = others;

                for copy in beneath {
                    self.complete(copy);
                }

                if copied {
                    return Ok(());
                }

                if let Some(destination) = self.created_match_of(&path) {
                    self.add(Some(Declaration::Move {
                        from: path,
                        to: destination,
                        origin: origin.clone(),
                    }));
                } else if !self.is_beneath_copied_root(&path) {
                    self.add(Some(delete(path)));
                }
            }
        }

        Ok(())
    }
}

fn parse_lines(
    input: &[u8],
    working_directory: &Path,
    is_directory: &dyn Fn(&Path) -> bool,
) -> (Vec<Declaration>, Vec<Rejected>) {
    let mut rejected = Vec::new();
    let mut sequence = Sequence {
        slots: Vec::new(),
        pending: Vec::new(),
        completed: Vec::new(),
        created: Vec::new(),
        working_directory,
        is_directory,
    };

    for (index, raw) in input.split(|byte| *byte == b'\n').enumerate() {
        let decoded = String::from_utf8_lossy(raw);
        let line = decoded.trim_end();

        if line.is_empty() {
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

        if let Err(reason) = parsed.and_then(|parsed| sequence.apply(parsed, &origin)) {
            rejected.push(reject(origin, reason));
        }
    }

    (sequence.slots.into_iter().flatten().collect(), rejected)
}

pub fn parse_declarations(
    input: &[u8],
    working_directory: &Path,
    is_directory: &dyn Fn(&Path) -> bool,
) -> (Vec<Declaration>, Vec<Rejected>) {
    let input = input.strip_prefix(&BOM).unwrap_or(input);

    if input.contains(&0) {
        parse_records(input, working_directory)
    } else {
        parse_lines(input, working_directory, is_directory)
    }
}

#[cfg(test)]
#[path = "parse_declarations.test.rs"]
mod tests;
