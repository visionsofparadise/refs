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

#[derive(Clone)]
struct Visit {
    path: PathBuf,
    origin: Origin,
}

struct Item {
    original: PathBuf,
    arrival: bool,
    location: Option<PathBuf>,
    visited: Vec<Visit>,
    destroyed_by: Option<Origin>,
    overwritten_by: Option<usize>,
}

struct Step {
    from: PathBuf,
    to: PathBuf,
    origin: Origin,
}

enum Occupant {
    Held(usize),
    Vacated(Option<PathBuf>),
    Unknown,
}

#[derive(Default)]
struct Model {
    items: Vec<Item>,
}

pub struct Checks<'a> {
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub is_directory: &'a dyn Fn(&Path) -> bool,
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

    fn occupant_of(&self, path: &Path) -> Occupant {
        let held = self.holder_of(path).map(|index| {
            let location = self.items[index].location.as_ref();

            (location.map_or(0, |location| key_of(location).len()), index)
        });

        let vacated = self
            .items
            .iter()
            .filter_map(|item| {
                let location = item.location.as_ref();
                let passed = item
                    .visited
                    .iter()
                    .rev()
                    .skip(usize::from(location.is_some()));

                passed
                    .filter(|visit| is_beneath(path, &visit.path))
                    .max_by_key(|visit| key_of(&visit.path).len())
                    .map(|visit| {
                        let whereabouts =
                            location.map(|location| rebase(path, &visit.path, location));

                        (key_of(&visit.path).len(), whereabouts)
                    })
            })
            .max_by_key(|(length, whereabouts)| (*length, whereabouts.is_some()));

        match (held, vacated) {
            (Some((held, index)), Some((vacated, _))) if held >= vacated => Occupant::Held(index),
            (Some((_, index)), None) => Occupant::Held(index),
            (_, Some((_, whereabouts))) => Occupant::Vacated(whereabouts),
            (None, None) => Occupant::Unknown,
        }
    }

    fn materialize(&mut self, path: &Path, origin: &Origin) -> (usize, bool) {
        let fresh = |arrival: bool| Item {
            original: path.to_path_buf(),
            arrival,
            location: Some(path.to_path_buf()),
            visited: vec![Visit {
                path: path.to_path_buf(),
                origin: origin.clone(),
            }],
            destroyed_by: None,
            overwritten_by: None,
        };

        let item = match self.occupant_of(path) {
            Occupant::Held(holder) => {
                let parent = &self.items[holder];
                let location = parent.location.clone().unwrap_or_default();

                if is_same(&location, path) {
                    return (holder, false);
                }

                Item {
                    original: rebase(path, &location, &parent.original),
                    arrival: parent.arrival,
                    location: Some(path.to_path_buf()),
                    visited: parent
                        .visited
                        .iter()
                        .map(|visit| Visit {
                            path: rebase(path, &location, &visit.path),
                            origin: visit.origin.clone(),
                        })
                        .collect(),
                    destroyed_by: None,
                    overwritten_by: None,
                }
            }
            Occupant::Vacated(_) => fresh(true),
            Occupant::Unknown => fresh(false),
        };

        self.items.push(item);

        (self.items.len() - 1, true)
    }

    fn destroy(
        &mut self,
        path: &Path,
        origin: &Origin,
        vacated: &dyn Fn(&Path) -> bool,
        overwriter: Option<usize>,
    ) {
        if !vacated(path) && matches!(self.occupant_of(path), Occupant::Held(_)) {
            let (index, split) = self.materialize(path, origin);
            let item = &mut self.items[index];

            item.location = None;
            item.destroyed_by = Some(origin.clone());
            item.overwritten_by = overwriter.filter(|_| split);
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
        let occupant = self.occupant_of(path);

        if !vacated(path) && !matches!(occupant, Occupant::Held(_)) {
            self.items.push(Item {
                original: path.to_path_buf(),
                arrival: matches!(occupant, Occupant::Vacated(_)),
                location: None,
                visited: vec![Visit {
                    path: path.to_path_buf(),
                    origin: origin.clone(),
                }],
                destroyed_by: Some(origin.clone()),
                overwritten_by: None,
            });
        }

        self.destroy(path, origin, vacated, None);
    }

    fn apply(&mut self, group: Vec<Declaration>) {
        let mut steps = Vec::new();
        let mut deletes = Vec::new();

        for declaration in group {
            match declaration {
                Declaration::Move { from, to, origin } => {
                    let redundant = matches!(
                        self.occupant_of(&from),
                        Occupant::Vacated(Some(whereabouts)) if is_same(&whereabouts, &to)
                    );

                    if !redundant {
                        steps.push(Step { from, to, origin });
                    }
                }
                Declaration::Delete { path, origin } => deletes.push((path, origin)),
            }
        }

        let movers: Vec<usize> = steps
            .iter()
            .map(|step| self.materialize(&step.from, &step.origin).0)
            .collect();

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

        for (step, mover) in steps.iter().zip(&movers) {
            self.destroy(&step.to, &step.origin, &vacated, Some(*mover));
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

                item.visited.push(Visit {
                    path: moved,
                    origin: step.origin.clone(),
                });
            }
        }
    }

    fn is_refilled(&self, path: &Path, owner: &Item) -> bool {
        self.items
            .iter()
            .filter(|item| !std::ptr::eq(*item, owner))
            .any(|item| {
                let skip = usize::from(!item.arrival);

                item.visited
                    .iter()
                    .skip(skip)
                    .any(|visit| is_beneath(&visit.path, path) || is_beneath(path, &visit.path))
            })
    }

    fn is_void(&self, item: &Item, checks: &Checks) -> bool {
        item.overwritten_by.is_some_and(|overwriter| {
            self.items[overwriter]
                .location
                .as_ref()
                .is_some_and(|location| (checks.is_directory)(location))
        })
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

struct Outcome<'a> {
    item: &'a Item,
    trail: Vec<&'a Visit>,
    destroyed: bool,
}

enum Settled<'a> {
    Changed(Outcome<'a>),
    Returned(&'a Path),
    Unchanged,
}

fn settle<'a>(
    model: &'a Model,
    item: &'a Item,
    checks: &Checks,
    rejected: &mut Vec<Rejected>,
) -> Settled<'a> {
    if item.arrival || model.is_void(item, checks) {
        return Settled::Unchanged;
    }

    let exists = checks.exists;
    let mut trail: Vec<&Visit> = item.visited.iter().collect();

    if item.location.is_none() {
        if exists(&item.original) && !model.is_refilled(&item.original, item) {
            if let Some(origin) = &item.destroyed_by {
                reject(rejected, origin, PATH_STILL_EXISTS);
            }

            return Settled::Unchanged;
        }

        return Settled::Changed(Outcome {
            item,
            trail,
            destroyed: true,
        });
    }

    let case_only =
        |visit: &Visit| is_same(&visit.path, &item.original) && visit.path != item.original;

    while trail.len() > 1 && trail.last().is_some_and(|visit| !exists(&visit.path)) {
        if let Some(visit) = trail.pop() {
            reject(rejected, &visit.origin, DESTINATION_MISSING);
        }
    }

    let Some(last) = trail.last() else {
        return Settled::Unchanged;
    };

    if last.path == item.original {
        return if trail.len() > 1 {
            Settled::Returned(&item.original)
        } else {
            Settled::Unchanged
        };
    }

    if exists(&item.original) && !model.is_refilled(&item.original, item) && !case_only(last) {
        reject(rejected, &trail[1].origin, SOURCE_STILL_EXISTS);

        return Settled::Unchanged;
    }

    Settled::Changed(Outcome {
        item,
        trail,
        destroyed: false,
    })
}

pub fn compose_declarations(declarations: Vec<Declaration>, checks: &Checks) -> Composition {
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

    let mut composition = Composition::default();
    let mut outcomes: Vec<Outcome> = Vec::new();
    let mut returned: Vec<&Path> = Vec::new();

    for item in &model.items {
        match settle(&model, item, checks, &mut composition.rejected) {
            Settled::Changed(outcome) => outcomes.push(outcome),
            Settled::Returned(original) => returned.push(original),
            Settled::Unchanged => {}
        }
    }

    let originals: Vec<&PathBuf> = model
        .items
        .iter()
        .filter(|item| !item.arrival)
        .map(|item| &item.original)
        .collect();

    let mut sources: Vec<Vec<String>> = Vec::new();

    for outcome in &outcomes {
        let original = &outcome.item.original;

        sources.push(key_of(original));

        match outcome.trail.last() {
            Some(last) if !outcome.destroyed => composition.moves.push(Move {
                from: original.clone(),
                to: last.path.clone(),
            }),
            _ => composition.deletes.push(original.clone()),
        }
    }

    for outcome in &outcomes {
        let original = &outcome.item.original;
        let skip = if outcome.destroyed { 1 } else { 2 };
        let passed = &outcome.trail[1..outcome.trail.len().saturating_sub(skip - 1).max(1)];

        for visit in passed {
            let beneath_original = originals
                .iter()
                .any(|other| *other != original && is_beneath(&visit.path, other));

            if (checks.exists)(&visit.path)
                || beneath_original
                || sources.contains(&key_of(&visit.path))
            {
                continue;
            }

            sources.push(key_of(&visit.path));

            match outcome.trail.last() {
                Some(last) if !outcome.destroyed => composition.moves.push(Move {
                    from: visit.path.clone(),
                    to: last.path.clone(),
                }),
                _ => composition.deletes.push(visit.path.clone()),
            }
        }
    }

    for original in returned {
        let covered = composition
            .moves
            .iter()
            .map(|found| &found.from)
            .chain(&composition.deletes)
            .any(|covering| is_beneath(original, covering));

        if covered {
            composition.moves.push(Move {
                from: original.to_path_buf(),
                to: original.to_path_buf(),
            });
        }
    }

    composition
        .rejected
        .sort_by_key(|rejection| rejection.origin.line);
    composition.rejected.dedup();

    composition
}

#[cfg(test)]
#[path = "compose_declarations.test.rs"]
mod tests;
