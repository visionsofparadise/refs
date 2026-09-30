use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::declaration_messages::{
    Template, BACKUP, COPIED, CREATED_DIRECTORY, REMOVED, REMOVED_DIRECTORY, RENAMED, RENAMING,
};
use crate::match_template::{match_template, Reading};
use crate::resolve_reference::{is_beneath, key_of, locate_path};
use crate::unquote::{unquote_git, unquote_shell};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub line: usize,
    pub text: String,
    pub snapshot: Option<usize>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

struct Entry {
    origin: Origin,
    parsed: Result<Parsed, &'static str>,
}

struct PendingCopy {
    from: PathBuf,
    to: PathBuf,
    slot: usize,
    origin: Origin,
}

struct Created {
    path: PathBuf,
    slot: usize,
}

struct Mapping {
    from_root: PathBuf,
    to_root: PathBuf,
    is_directory: bool,
    retired: bool,
}

const UNRECOGNIZED: &str = "unrecognized";
const AMBIGUOUS: &str = "ambiguous";
const UNSUPPORTED_PATH: &str = "unsupported path";
const EMPTY_PATH: &str = "empty path";
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

#[derive(Clone, Copy)]
enum Message {
    Renamed,
    Copied,
    Removed,
    RemovedDirectory,
    CreatedDirectory,
    Renaming,
}

const TRANSLATED: [(Message, &[Template], Reading); 6] = [
    (Message::Renamed, RENAMED, Reading::Shell),
    (Message::Copied, COPIED, Reading::Shell),
    (Message::Removed, REMOVED, Reading::Shell),
    (Message::RemovedDirectory, REMOVED_DIRECTORY, Reading::Shell),
    (Message::CreatedDirectory, CREATED_DIRECTORY, Reading::Shell),
    (Message::Renaming, RENAMING, Reading::Raw),
];

fn resolve_path(text: &str, working_directory: &Path) -> Result<PathBuf, &'static str> {
    if text.is_empty() {
        return Err(EMPTY_PATH);
    }

    locate_path(text, working_directory).ok_or(UNSUPPORTED_PATH)
}

fn escape_text(text: &str) -> String {
    let mut escaped = String::new();

    for character in text.chars() {
        match character {
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            '\r' => escaped.push_str("\\r"),
            control if control.is_control() => {
                escaped.push_str(&format!("\\x{:02x}", u32::from(control)));
            }
            other => escaped.push(other),
        }
    }

    escaped
}

fn origin_of(line: usize, text: &str) -> Origin {
    Origin {
        line,
        text: escape_text(text),
        snapshot: None,
    }
}

fn is_status(field: &str, letters: &str) -> bool {
    let mut characters = field.chars();

    characters
        .next()
        .is_some_and(|first| letters.contains(first))
        && characters.all(|character| character.is_ascii_digit())
}

fn is_listing_record(line: &[u8]) -> bool {
    let status = line.split(|byte| *byte == b'\t').next().unwrap_or_default();

    line.contains(&b'\t') && is_status(&String::from_utf8_lossy(status), "RCDAMTU")
}

fn parse_name_status(text: &str) -> Option<Result<Parsed, &'static str>> {
    let tabbed = text.contains('\t');
    let fields: Vec<&str> = if tabbed {
        text.split('\t').collect()
    } else {
        text.split(' ').filter(|field| !field.is_empty()).collect()
    };
    let status = *fields.first()?;

    if fields.get(1) == Some(&"->") {
        return None;
    }

    let expected = if is_status(status, "RC") {
        3
    } else if is_status(status, "DAMTU") {
        2
    } else {
        return None;
    };

    if fields.len() != expected {
        return None;
    }

    let mut paths = Vec::new();

    for field in &fields[1..] {
        if tabbed {
            match unquote_git(field) {
                Some(path) => paths.push(path),
                None => return Some(Err(UNRECOGNIZED)),
            }
        } else if *field == "\"\"" {
            paths.push(String::new());
        } else {
            paths.push((*field).to_string());
        }
    }

    Some(Ok(match (status.chars().next()?, paths.as_slice()) {
        ('R', [from, to]) => Parsed::Move {
            from: from.clone(),
            to: to.clone(),
            hand_written: !tabbed,
        },
        ('D', [path]) => Parsed::Delete(path.clone()),
        _ => Parsed::Skip,
    }))
}

fn count_bytes(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn parse_gnu_move(line: &[u8]) -> Option<Parsed> {
    let (copied, rest) = if let Some(rest) = line.strip_prefix(b"copied ") {
        (true, rest)
    } else if let Some(rest) = line.strip_prefix(b"renamed ") {
        (false, rest)
    } else if line.starts_with(b"'") || line.starts_with(b"\"") || line.starts_with(b"$'") {
        (false, line)
    } else {
        return None;
    };

    let (from, rest) = unquote_shell(rest)?;
    let (to, rest) = unquote_shell(rest.strip_prefix(b" -> ")?)?;

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

fn read_path(rest: &[u8]) -> Result<String, &'static str> {
    match unquote_shell(rest) {
        Some((path, [])) => Ok(path),
        _ => Err(UNRECOGNIZED),
    }
}

fn strip_backup_tail(line: &[u8]) -> &[u8] {
    let quoted_end = line.ends_with(b"')") || line.ends_with(b"\")");

    if let Some(index) = (0..line.len())
        .rev()
        .find(|index| line[*index..].starts_with(b" (backup: "))
        .filter(|_| quoted_end)
    {
        return &line[..index];
    }

    for template in &BACKUP[1..] {
        let first = template.literals[0].as_bytes();
        let last = template.literals[template.arguments.len()].as_bytes();

        if !line.ends_with(last) {
            continue;
        }

        if let Some(index) = (0..line.len()).rev().find(|index| {
            line[*index..].starts_with(first)
                && !match_template(&line[*index..], template, Reading::Shell).is_empty()
        }) {
            return &line[..index];
        }
    }

    line
}

fn parsed_of(message: Message, arguments: Vec<String>) -> Parsed {
    let mut arguments = arguments.into_iter();
    let mut next = || arguments.next().unwrap_or_default();

    match message {
        Message::Renamed | Message::Renaming => Parsed::Move {
            from: next(),
            to: next(),
            hand_written: false,
        },
        Message::Copied => Parsed::Copy(next(), next()),
        Message::Removed => Parsed::Removed(next()),
        Message::RemovedDirectory => Parsed::RemovedDirectory(next()),
        Message::CreatedDirectory => Parsed::Created(next()),
    }
}

fn parse_translated(line: &[u8]) -> Option<Result<Parsed, &'static str>> {
    let mut readings: Vec<Parsed> = Vec::new();

    for (message, templates, reading) in TRANSLATED {
        for template in &templates[1..] {
            for arguments in match_template(line, template, reading) {
                let parsed = parsed_of(message, arguments);

                if !readings.contains(&parsed) {
                    readings.push(parsed);
                }
            }
        }
    }

    match readings.len() {
        0 => None,
        1 => readings.pop().map(Ok),
        _ => Some(Err(AMBIGUOUS)),
    }
}

fn parse_line(line: &[u8]) -> Result<Parsed, &'static str> {
    let line = strip_backup_tail(line);

    if let Some(rest) = line.strip_prefix(b"created directory ") {
        return Ok(read_path(rest).map_or(Parsed::Skip, Parsed::Created));
    }

    if let Some(rest) = line.strip_prefix(b"removed directory ") {
        return read_path(rest).map(Parsed::RemovedDirectory);
    }

    if let Some(rest) = line.strip_prefix(b"removed ") {
        return read_path(rest).map(Parsed::Removed);
    }

    if let Some(parsed) = parse_gnu_move(line) {
        return Ok(parsed);
    }

    if line.starts_with(b"copied ") || line.starts_with(b"renamed ") {
        return Err(if count_bytes(line, b" -> ") > 1 {
            AMBIGUOUS
        } else {
            UNRECOGNIZED
        });
    }

    if let Some(result) = parse_translated(line) {
        return result;
    }

    let text = std::str::from_utf8(line).map_err(|_| UNRECOGNIZED)?;

    if let Some(result) = parse_name_status(text) {
        return result;
    }

    if let Some(rest) = text.strip_prefix("Renaming ") {
        return split_exactly(rest, " to ").map(|(from, to)| Parsed::Move {
            from: from.to_string(),
            to: to.to_string(),
            hand_written: false,
        });
    }

    if let Some(rest) = text.strip_prefix("rm ") {
        return match rest
            .strip_prefix('\'')
            .and_then(|inner| inner.strip_suffix('\''))
        {
            Some(path) => Ok(Parsed::Delete(path.to_string())),
            None => Err(UNRECOGNIZED),
        };
    }

    split_exactly(text, " -> ").map(|(from, to)| Parsed::Move {
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
    let mut tokens: Vec<&[u8]> = input.split(|byte| *byte == 0).collect();

    while tokens.first().is_some_and(|token| token.is_empty()) {
        tokens.remove(0);
    }

    if tokens
        .last()
        .is_some_and(|token| !token.is_empty() && token.iter().all(u8::is_ascii_whitespace))
    {
        tokens.pop();
    }

    while tokens.last().is_some_and(|token| token.is_empty()) {
        tokens.pop();
    }

    let mut declarations = Vec::new();
    let mut rejected = Vec::new();
    let mut tokens = tokens.into_iter();
    let mut index = 0;

    while let Some(status) = tokens.next() {
        index += 1;

        let status_text = String::from_utf8_lossy(status).into_owned();
        let (count, known) = if is_status(&status_text, "RC") {
            (2, true)
        } else {
            (1, is_status(&status_text, "DAMTU"))
        };
        let count = if status.is_empty() { 0 } else { count };
        let paths: Vec<Option<&str>> = (0..count)
            .map_while(|_| tokens.next())
            .map(|token| std::str::from_utf8(token).ok())
            .collect();
        let text = std::iter::once(status_text.as_str())
            .chain(paths.iter().map(|path| path.unwrap_or("?")))
            .collect::<Vec<_>>()
            .join("\t");
        let origin = Origin {
            snapshot: Some(1),
            ..origin_of(index, &text)
        };

        if !known || paths.len() != count || paths.iter().any(Option::is_none) {
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

#[derive(Clone, Copy)]
struct Checks<'a> {
    is_directory: &'a dyn Fn(&Path) -> bool,
    exists: &'a dyn Fn(&Path) -> bool,
}
struct Sequence<'a> {
    slots: Vec<Option<Declaration>>,
    pending: Vec<PendingCopy>,
    created: Vec<Created>,
    announced: Vec<PathBuf>,
    mappings: Vec<Mapping>,
    rejected_copies: Vec<(String, &'static str)>,
    working_directory: &'a Path,
    checks: Checks<'a>,
}

impl Sequence<'_> {
    fn add(&mut self, declaration: Option<Declaration>) -> usize {
        self.slots.push(declaration);

        self.slots.len() - 1
    }

    fn mapping_of(&self, path: &Path) -> Option<usize> {
        self.mappings
            .iter()
            .enumerate()
            .filter(|(_, mapping)| {
                mapping.is_directory && !mapping.retired && is_beneath(path, &mapping.from_root)
            })
            .max_by_key(|(_, mapping)| key_of(&mapping.from_root).len())
            .map(|(index, _)| index)
    }

    fn destination_of(&self, path: &Path, mapping: usize) -> PathBuf {
        let mapping = &self.mappings[mapping];
        let depth = mapping.from_root.components().count();

        path.components()
            .skip(depth)
            .fold(mapping.to_root.clone(), |joined, part| joined.join(part))
    }

    fn place_directory_move(&mut self, from: PathBuf, to: PathBuf, origin: &Origin) {
        let declaration = Some(Declaration::Move {
            from,
            to: to.clone(),
            origin: origin.clone(),
        });
        let created = self
            .created
            .iter()
            .find(|created| {
                key_of(&created.path) == key_of(&to) && self.slots[created.slot].is_none()
            })
            .map(|created| created.slot);

        match created {
            Some(slot) => self.slots[slot] = declaration,
            None => {
                self.add(declaration);
            }
        }
    }

    fn map_copy(&mut self, from: &Path, to: &Path) {
        let from_key = key_of(from);
        let to_key = key_of(to);
        let common = from_key
            .iter()
            .rev()
            .zip(to_key.iter().rev())
            .take_while(|(left, right)| left == right)
            .count();
        let relative = if is_beneath(from, self.working_directory) {
            from_key.len() - key_of(self.working_directory).len()
        } else {
            from_key.len().saturating_sub(1)
        };
        let covering = self
            .announced
            .iter()
            .filter(|created| is_beneath(to, created) && key_of(created) != to_key)
            .map(|created| to_key.len() - key_of(created).len())
            .max();
        let trailing = common.min(relative.saturating_sub(1));
        let depth = match covering {
            Some(depth) if depth <= common => depth,
            _ => trailing,
        };
        let roots = (from.ancestors().nth(depth), to.ancestors().nth(depth));

        let (from_root, to_root, is_directory) = match roots {
            (Some(from_root), Some(to_root))
                if depth >= 1
                    && self
                        .announced
                        .iter()
                        .any(|created| is_beneath(created, to_root)) =>
            {
                (from_root.to_path_buf(), to_root.to_path_buf(), true)
            }
            _ => (from.to_path_buf(), to.to_path_buf(), false),
        };

        let known = self
            .mappings
            .iter()
            .any(|mapping| !mapping.retired && key_of(&mapping.from_root) == key_of(&from_root));

        if !known {
            self.mappings.push(Mapping {
                from_root,
                to_root,
                is_directory,
                retired: false,
            });
        }
    }

    fn retire(&mut self, path: &Path, is_directory: bool) {
        for mapping in &mut self.mappings {
            if mapping.is_directory == is_directory && key_of(&mapping.from_root) == key_of(path) {
                mapping.retired = true;
            }
        }
    }

    fn map_created_run(&mut self, entries: &[Entry], position: usize, path: &Path) -> bool {
        let working = key_of(self.working_directory);
        let orphans: Vec<&Created> = self
            .created
            .iter()
            .filter(|created| {
                self.slots[created.slot].is_none()
                    && !self
                        .mappings
                        .iter()
                        .any(|mapping| is_beneath(&created.path, &mapping.to_root))
            })
            .collect();
        let roots: Vec<&Created> = orphans
            .iter()
            .copied()
            .filter(|created| {
                !orphans.iter().any(|other| {
                    key_of(&other.path) != key_of(&created.path)
                        && is_beneath(&created.path, &other.path)
                })
            })
            .collect();

        for root in roots {
            let root_key = key_of(&root.path);
            let mirrored: BTreeSet<Vec<String>> = orphans
                .iter()
                .filter(|created| {
                    key_of(&created.path) != root_key && is_beneath(&created.path, &root.path)
                })
                .map(|created| key_of(&created.path)[root_key.len()..].to_vec())
                .collect();

            for candidate in path.ancestors() {
                let candidate_key = key_of(candidate);

                if candidate_key.len() <= working.len() || !candidate_key.starts_with(&working) {
                    break;
                }

                let mut removed = BTreeSet::new();
                let mut found = false;

                for entry in &entries[position..] {
                    let Ok(Parsed::RemovedDirectory(text)) = &entry.parsed else {
                        continue;
                    };
                    let Ok(removed_path) = resolve_path(text, self.working_directory) else {
                        continue;
                    };
                    let removed_key = key_of(&removed_path);

                    if removed_key == candidate_key {
                        found = true;

                        break;
                    }

                    if removed_key.starts_with(&candidate_key) {
                        removed.insert(removed_key[candidate_key.len()..].to_vec());
                    }
                }

                if found && removed == mirrored {
                    let to_root = root.path.clone();

                    self.mappings.push(Mapping {
                        from_root: candidate.to_path_buf(),
                        to_root,
                        is_directory: true,
                        retired: false,
                    });

                    return true;
                }
            }
        }

        false
    }

    fn apply(
        &mut self,
        entries: &[Entry],
        position: usize,
        parsed: &Parsed,
    ) -> Result<(), &'static str> {
        let origin = &entries[position].origin;
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
                let from = resolve(from)?;
                let mut to = resolve(to)?;

                if *hand_written && (self.checks.is_directory)(&to) {
                    if let Some(name) = from.file_name() {
                        let inside = to.join(name);

                        if (self.checks.exists)(&inside) {
                            to = inside;
                        }
                    }
                }

                self.add(Some(Declaration::Move {
                    from,
                    to,
                    origin: origin.clone(),
                }));
            }
            Parsed::Delete(path) => {
                self.add(Some(delete(resolve(path)?)));
            }
            Parsed::Copy(from, to) => {
                let resolved = resolve(from).and_then(|from| Ok((from, resolve(to)?)));
                let (copy_from, copy_to) = match resolved {
                    Ok(paths) => paths,
                    Err(reason) => {
                        self.rejected_copies.push((from.clone(), reason));

                        return Err(reason);
                    }
                };

                self.map_copy(&copy_from, &copy_to);

                let slot = self.add(None);

                self.pending.push(PendingCopy {
                    from: copy_from,
                    to: copy_to,
                    slot,
                    origin: origin.clone(),
                });
            }
            Parsed::Created(path) => {
                if let Ok(path) = resolve(path) {
                    let slot = self.add(None);

                    self.created.push(Created { path, slot });
                }
            }
            Parsed::Removed(text) => {
                if let Some((_, reason)) =
                    self.rejected_copies.iter().find(|(from, _)| from == text)
                {
                    return Err(reason);
                }

                let path = resolve(text)?;
                let matching = self
                    .pending
                    .iter()
                    .position(|copy| key_of(&copy.from) == key_of(&path));

                if let Some(matching) = matching {
                    let copy = self.pending.remove(matching);

                    self.slots[copy.slot] = Some(Declaration::Move {
                        from: copy.from,
                        to: copy.to,
                        origin: copy.origin,
                    });

                    self.retire(&path, false);
                } else if let Some(mapping) = self.mapping_of(&path) {
                    let to = self.destination_of(&path, mapping);

                    self.add(Some(Declaration::Move {
                        from: path,
                        to,
                        origin: origin.clone(),
                    }));
                } else {
                    self.add(Some(delete(path)));
                }
            }
            Parsed::RemovedDirectory(text) => {
                let path = resolve(text)?;

                if self.mapping_of(&path).is_none() {
                    self.map_created_run(entries, position, &path);
                }

                match self.mapping_of(&path) {
                    Some(mapping) => {
                        let to = self.destination_of(&path, mapping);
                        let is_root = key_of(&self.mappings[mapping].from_root) == key_of(&path);

                        self.place_directory_move(path.clone(), to, origin);

                        if is_root {
                            self.retire(&path, true);
                        }
                    }
                    None => {
                        self.add(Some(delete(path)));
                    }
                }
            }
        }

        Ok(())
    }
}

fn parse_lines(
    input: &[u8],
    working_directory: &Path,
    checks: Checks,
) -> (Vec<Declaration>, Vec<Rejected>) {
    let mut snapshot = None;
    let entries: Vec<Entry> = input
        .split(|byte| *byte == b'\n')
        .enumerate()
        .filter_map(|(index, raw)| {
            let line = raw.trim_ascii_end();

            if line.is_empty() {
                snapshot = None;

                return None;
            }

            snapshot = is_listing_record(line).then(|| snapshot.unwrap_or(index + 1));

            Some(Entry {
                origin: Origin {
                    snapshot,
                    ..origin_of(index + 1, &String::from_utf8_lossy(line))
                },
                parsed: parse_line(line),
            })
        })
        .collect();
    let announced = entries
        .iter()
        .filter_map(|entry| match &entry.parsed {
            Ok(Parsed::Created(text)) => resolve_path(text, working_directory).ok(),
            _ => None,
        })
        .collect();
    let mut sequence = Sequence {
        slots: Vec::new(),
        pending: Vec::new(),
        created: Vec::new(),
        announced,
        mappings: Vec::new(),
        rejected_copies: Vec::new(),
        working_directory,
        checks,
    };
    let mut rejected = Vec::new();

    for (position, entry) in entries.iter().enumerate() {
        let outcome = match &entry.parsed {
            Ok(parsed) => sequence.apply(&entries, position, parsed),
            Err(reason) => Err(*reason),
        };

        if let Err(reason) = outcome {
            rejected.push(reject(entry.origin.clone(), reason));
        }
    }

    (sequence.slots.into_iter().flatten().collect(), rejected)
}

pub fn parse_declarations(
    input: &[u8],
    working_directory: &Path,
    is_directory: &dyn Fn(&Path) -> bool,
    exists: &dyn Fn(&Path) -> bool,
) -> (Vec<Declaration>, Vec<Rejected>) {
    let input = input.strip_prefix(&BOM).unwrap_or(input);

    if input.contains(&0) {
        parse_records(input, working_directory)
    } else {
        parse_lines(
            input,
            working_directory,
            Checks {
                is_directory,
                exists,
            },
        )
    }
}

#[cfg(test)]
#[path = "parse_declarations.test.rs"]
mod tests;
