use std::path::{Path, PathBuf};

use crate::parse_declarations::{Declaration, Origin, Rejected};
use crate::resolve_reference::{is_beneath, key_of};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Default)]
pub struct Composition {
    pub moves: Vec<Move>,
    pub deletes: Vec<PathBuf>,
    pub rejected: Vec<Rejected>,
}

const SOURCE_STILL_EXISTS: &str = "source still exists";
const DESTINATION_MISSING: &str = "destination missing";
const PATH_STILL_EXISTS: &str = "path still exists";

struct Item {
    original: PathBuf,
    location: Option<PathBuf>,
    visited: Vec<PathBuf>,
    origin: Origin,
    destroyed_by: Option<Origin>,
}

struct Step {
    from: PathBuf,
    to: PathBuf,
    origin: Origin,
}

#[derive(Default)]
struct Model {
    items: Vec<Item>,
}

pub fn rebase(path: &Path, base: &Path, onto: &Path) -> PathBuf {
    path.components()
        .skip(key_of(base).len())
        .fold(onto.to_path_buf(), |joined, part| joined.join(part))
}

fn is_same(left: &Path, right: &Path) -> bool {
    key_of(left) == key_of(right)
}

impl Model {
    fn holder_of(&self, path: &Path) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| Some((index, item.location.as_ref()?)))
            .filter(|(_, location)| is_beneath(path, location))
            .max_by_key(|(_, location)| key_of(location).len())
            .map(|(index, _)| index)
    }

    fn whereabouts_of(&self, path: &Path) -> Option<PathBuf> {
        self.items
            .iter()
            .filter_map(|item| {
                let location = item.location.as_ref()?;
                let passed = item.visited.iter().rev().skip(1);

                passed
                    .filter(|visited| is_beneath(path, visited))
                    .max_by_key(|visited| key_of(visited).len())
                    .map(|visited| (key_of(visited).len(), rebase(path, visited, location)))
            })
            .max_by_key(|(length, _)| *length)
            .map(|(_, whereabouts)| whereabouts)
    }

    fn materialize(&mut self, path: &Path, origin: &Origin) -> usize {
        let item = match self.holder_of(path) {
            Some(holder) => {
                let parent = &self.items[holder];
                let location = parent.location.clone().unwrap_or_default();

                if is_same(&location, path) {
                    return holder;
                }

                Item {
                    original: rebase(path, &location, &parent.original),
                    location: Some(path.to_path_buf()),
                    visited: parent
                        .visited
                        .iter()
                        .map(|visited| rebase(path, &location, visited))
                        .collect(),
                    origin: origin.clone(),
                    destroyed_by: None,
                }
            }
            None => Item {
                original: path.to_path_buf(),
                location: Some(path.to_path_buf()),
                visited: vec![path.to_path_buf()],
                origin: origin.clone(),
                destroyed_by: None,
            },
        };

        self.items.push(item);

        self.items.len() - 1
    }

    fn destroy(&mut self, path: &Path, origin: &Origin, vacated: &dyn Fn(&Path) -> bool) {
        if !vacated(path) && self.holder_of(path).is_some() {
            let index = self.materialize(path, origin);

            self.items[index].location = None;
            self.items[index].destroyed_by = Some(origin.clone());
        }

        for item in &mut self.items {
            let beneath = item
                .location
                .as_ref()
                .is_some_and(|location| is_beneath(location, path) && !vacated(location));

            if beneath {
                item.location = None;
                item.destroyed_by = Some(origin.clone());
            }
        }
    }

    fn delete(&mut self, path: &Path, origin: &Origin, vacated: &dyn Fn(&Path) -> bool) {
        if !vacated(path) && self.holder_of(path).is_none() {
            self.items.push(Item {
                original: path.to_path_buf(),
                location: None,
                visited: vec![path.to_path_buf()],
                origin: origin.clone(),
                destroyed_by: Some(origin.clone()),
            });
        }

        self.destroy(path, origin, vacated);
    }

    fn apply(&mut self, group: Vec<Declaration>) {
        let mut steps = Vec::new();
        let mut deletes = Vec::new();

        for declaration in group {
            match declaration {
                Declaration::Move { from, to, origin } => {
                    let redundant = self.holder_of(&from).is_none()
                        && self
                            .whereabouts_of(&from)
                            .is_some_and(|whereabouts| is_same(&whereabouts, &to));

                    if !redundant {
                        steps.push(Step { from, to, origin });
                    }
                }
                Declaration::Delete { path, origin } => deletes.push((path, origin)),
            }
        }

        for step in &steps {
            self.materialize(&step.from, &step.origin);
        }

        let sources: Vec<PathBuf> = steps.iter().map(|step| step.from.clone()).collect();
        let vacated = |path: &Path| sources.iter().any(|source| is_beneath(path, source));

        let carried: Vec<(usize, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let location = item.location.as_ref()?;

                steps
                    .iter()
                    .enumerate()
                    .filter(|(_, step)| is_beneath(location, &step.from))
                    .max_by_key(|(_, step)| key_of(&step.from).len())
                    .map(|(step, _)| (index, step))
            })
            .collect();

        for step in &steps {
            self.destroy(&step.to, &step.origin, &vacated);
        }

        for (path, origin) in &deletes {
            self.delete(path, origin, &vacated);
        }

        for (index, step) in carried {
            let item = &mut self.items[index];
            let step = &steps[step];

            if let Some(location) = &item.location {
                let moved = rebase(location, &step.from, &step.to);

                item.location = Some(moved.clone());

                item.visited.push(moved);
            }
        }
    }
}

fn snapshot_of(declaration: &Declaration) -> Option<usize> {
    match declaration {
        Declaration::Move { origin, .. } | Declaration::Delete { origin, .. } => origin.snapshot,
    }
}

fn reject(rejected: &mut Vec<Rejected>, origin: &Origin, reason: &str) {
    rejected.push(Rejected {
        origin: origin.clone(),
        reason: reason.to_string(),
    });
}

pub fn compose_declarations(
    declarations: Vec<Declaration>,
    exists: &dyn Fn(&Path) -> bool,
) -> Composition {
    let mut model = Model::default();
    let mut group: Vec<Declaration> = Vec::new();

    for declaration in declarations {
        let joins = snapshot_of(&declaration).is_some()
            && group
                .last()
                .is_some_and(|last| snapshot_of(last) == snapshot_of(&declaration));

        if !joins && !group.is_empty() {
            model.apply(std::mem::take(&mut group));
        }

        group.push(declaration);
    }

    model.apply(group);

    let refilled = |path: &Path| model.holder_of(path).is_some();
    let mut composition = Composition::default();
    let mut sources: Vec<Vec<String>> = Vec::new();
    let mut moves: Vec<(PathBuf, PathBuf, &Origin)> = Vec::new();
    let mut deletes: Vec<(PathBuf, &Origin)> = Vec::new();

    for item in &model.items {
        match (&item.location, &item.destroyed_by) {
            (Some(location), _) if *location != item.original => {
                moves.push((item.original.clone(), location.clone(), &item.origin));
            }
            (None, Some(destroyed_by)) => deletes.push((item.original.clone(), destroyed_by)),
            _ => {}
        }

        sources.push(key_of(&item.original));
    }

    for item in &model.items {
        let passed = item.visited.iter().skip(1);
        let passed: Vec<&PathBuf> = match &item.location {
            Some(_) => passed.take(item.visited.len().saturating_sub(2)).collect(),
            None => passed.collect(),
        };

        for visited in passed {
            if exists(visited) || sources.contains(&key_of(visited)) {
                continue;
            }

            sources.push(key_of(visited));

            match (&item.location, &item.destroyed_by) {
                (Some(location), _) => {
                    moves.push((visited.clone(), location.clone(), &item.origin))
                }
                (None, Some(destroyed_by)) => deletes.push((visited.clone(), destroyed_by)),
                _ => {}
            }
        }
    }

    for (from, to, origin) in moves {
        if !is_same(&from, &to) && !exists(&to) {
            reject(&mut composition.rejected, origin, DESTINATION_MISSING);
        } else if !is_same(&from, &to) && exists(&from) && !refilled(&from) {
            reject(&mut composition.rejected, origin, SOURCE_STILL_EXISTS);
        } else {
            composition.moves.push(Move { from, to });
        }
    }

    for (path, origin) in deletes {
        if exists(&path) && !refilled(&path) {
            reject(&mut composition.rejected, origin, PATH_STILL_EXISTS);
        } else {
            composition.deletes.push(path);
        }
    }

    composition
        .rejected
        .sort_by_key(|rejection| rejection.origin.line);
    composition.rejected.dedup();

    composition
}
