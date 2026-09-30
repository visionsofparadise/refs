use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn root_of() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("C:\\w")
    } else {
        PathBuf::from("/w")
    }
}

fn path_of(name: &str) -> PathBuf {
    name.split('/')
        .filter(|part| !part.is_empty())
        .fold(root_of(), |joined, part| joined.join(part))
}

fn origin_of(line: usize) -> Origin {
    Origin {
        line,
        text: format!("declaration {line}"),
        snapshot: None,
    }
}

fn move_of(from: &str, to: &str, line: usize) -> Declaration {
    Declaration::Move {
        from: path_of(from),
        to: path_of(to),
        origin: origin_of(line),
    }
}

fn delete_of(path: &str, line: usize) -> Declaration {
    Declaration::Delete {
        path: path_of(path),
        origin: origin_of(line),
    }
}

fn is_under(path: &str, base: &str) -> bool {
    path == base || path.starts_with(&format!("{base}/"))
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn ancestors_of(path: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut current = parent_of(path);

    while !current.is_empty() {
        ancestors.push(current.to_string());

        current = parent_of(current);
    }

    ancestors
}

#[derive(Clone, Default)]
struct Tree {
    files: BTreeMap<String, usize>,
    directories: BTreeSet<String>,
}

impl Tree {
    fn place(&mut self, path: &str, content: usize) {
        self.directories.extend(ancestors_of(path));
        self.files.insert(path.to_string(), content);
    }

    fn is_directory(&self, path: &str) -> bool {
        self.directories.contains(path)
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path) || self.is_directory(path)
    }

    fn relocate(&mut self, from: &str, to: &str) {
        let files: Vec<(String, usize)> = self
            .files
            .iter()
            .filter(|(path, _)| is_under(path, from))
            .map(|(path, content)| (path.clone(), *content))
            .collect();
        let directories: Vec<String> = self
            .directories
            .iter()
            .filter(|path| is_under(path, from))
            .cloned()
            .collect();

        self.files.remove(to);

        for (path, _) in &files {
            self.files.remove(path);
        }

        for path in &directories {
            self.directories.remove(path);
        }

        self.directories.extend(ancestors_of(to));

        for path in directories {
            self.directories
                .insert(format!("{to}{}", &path[from.len()..]));
        }

        for (path, content) in files {
            self.files
                .insert(format!("{to}{}", &path[from.len()..]), content);
        }
    }

    fn remove(&mut self, path: &str, line: &mut usize) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        let files: Vec<String> = self
            .files
            .keys()
            .filter(|file| is_under(file, path))
            .cloned()
            .collect();
        let mut directories: Vec<String> = self
            .directories
            .iter()
            .filter(|directory| is_under(directory, path))
            .cloned()
            .collect();

        directories.sort_by_key(|directory| std::cmp::Reverse(directory.len()));

        for file in files {
            self.files.remove(&file);

            *line += 1;

            declarations.push(delete_of(&file, *line));
        }

        for directory in directories {
            self.directories.remove(&directory);

            *line += 1;

            declarations.push(delete_of(&directory, *line));
        }

        declarations
    }

    fn content_of(&self, content: usize) -> Option<&String> {
        self.files
            .iter()
            .find(|(_, held)| **held == content)
            .map(|(path, _)| path)
    }
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);

        self.0 >> 33
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next()).unwrap_or_default() % bound.max(1)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }
}

enum Expected {
    At(PathBuf),
    Deleted,
}

fn forward_of(composition: &Composition, path: &Path) -> Expected {
    let moved = composition
        .moves
        .iter()
        .filter(|found| is_beneath(path, &found.from))
        .max_by_key(|found| key_of(&found.from).len());
    let deleted = composition
        .deletes
        .iter()
        .filter(|deleted| is_beneath(path, deleted))
        .map(|deleted| key_of(deleted).len())
        .max();

    if deleted.is_some() && deleted > moved.map(|found| key_of(&found.from).len()) {
        return Expected::Deleted;
    }

    Expected::At(moved.map_or_else(
        || path.to_path_buf(),
        |found| rebase(path, &found.from, &found.to),
    ))
}

fn compose_in(tree: &Tree, declarations: Vec<Declaration>) -> Composition {
    let root = root_of();
    let name_of = |path: &Path| {
        path.strip_prefix(&root)
            .map(|relative| {
                relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default()
    };

    compose_declarations(
        declarations,
        &Checks {
            exists: &|path| tree.exists(&name_of(path)),
            is_directory: &|path| tree.is_directory(&name_of(path)),
        },
    )
}

fn scenario_of(seed: u64) -> (Tree, Vec<String>, Vec<Declaration>) {
    let mut random = Random(seed);
    let mut tree = Tree::default();
    let mut originals = Vec::new();

    for (index, directory) in ["", "a", "a/b", "c", "c/d"].iter().enumerate() {
        for copy in 0..=random.below(2) {
            let name = format!("{directory}/f{index}{copy}.md");
            let name = name.trim_start_matches('/').to_string();

            tree.place(&name, originals.len());
            originals.push(name);
        }
    }

    let mut named: BTreeSet<String> = BTreeSet::new();
    let mut declarations = Vec::new();
    let mut line = 0;

    for counter in 1..=(2 + random.below(6)) {
        let files: Vec<String> = tree.files.keys().cloned().collect();
        let directories: Vec<String> = tree.directories.iter().cloned().collect();
        let mut places = vec![String::new()];

        places.extend(directories.iter().cloned());

        let join = |parent: &str, name: String| {
            if parent.is_empty() {
                name
            } else {
                format!("{parent}/{name}")
            }
        };

        let operation = match random.below(7) {
            0 | 1 => random.pick(&files).map(|from| {
                let parent = random.pick(&places).cloned().unwrap_or_default();

                (from.clone(), join(&parent, format!("g{counter}.md")))
            }),
            2 => {
                let known: Vec<&String> = files
                    .iter()
                    .filter(|file| named.iter().any(|name| is_under(file, name)))
                    .collect();

                match (random.pick(&files), random.pick(&known)) {
                    (Some(from), Some(to)) if from != *to => Some((from.clone(), (*to).clone())),
                    _ => None,
                }
            }
            3 => random.pick(&directories).and_then(|from| {
                let parent = random.pick(&places).cloned().unwrap_or_default();

                (!is_under(&parent, from))
                    .then(|| (from.clone(), join(&parent, format!("d{counter}"))))
            }),
            4 => {
                let gone: Vec<&String> = originals
                    .iter()
                    .filter(|original| !tree.exists(original))
                    .filter(|original| {
                        ancestors_of(original)
                            .iter()
                            .all(|ancestor| !tree.files.contains_key(ancestor))
                    })
                    .collect();

                match (random.pick(&files), random.pick(&gone)) {
                    (Some(from), Some(to)) if from != *to => Some((from.clone(), (*to).clone())),
                    _ => None,
                }
            }
            5 => {
                if let Some(path) = random.pick(&files) {
                    named.insert(path.clone());
                    declarations.extend(tree.remove(&path.clone(), &mut line));
                }

                None
            }
            _ => {
                if let Some(path) = random.pick(&directories) {
                    named.insert(path.clone());
                    declarations.extend(tree.remove(&path.clone(), &mut line));
                }

                None
            }
        };

        if let Some((from, to)) = operation {
            if tree.exists(&to) && !tree.files.contains_key(&to) {
                continue;
            }

            named.insert(from.clone());
            named.insert(to.clone());
            tree.relocate(&from, &to);

            line += 1;

            declarations.push(move_of(&from, &to, line));
        }
    }

    (tree, originals, declarations)
}

fn failures_of(seed: u64) -> Vec<String> {
    let (tree, originals, declarations) = scenario_of(seed);
    let composition = compose_in(&tree, declarations);
    let mut failures: Vec<String> = composition
        .rejected
        .iter()
        .map(|rejection| format!("seed {seed}: rejected {rejection:?}"))
        .collect();

    for (content, original) in originals.iter().enumerate() {
        let expected = forward_of(&composition, &path_of(original));

        match (tree.content_of(content), expected) {
            (Some(now), Expected::At(mapped)) if key_of(&mapped) == key_of(&path_of(now)) => {}
            (None, Expected::Deleted) => {}
            (None, Expected::At(mapped))
                if !tree.exists(&mapped.to_string_lossy()) && mapped == path_of(original) => {}
            (now, _) => failures.push(format!("seed {seed}: {original} is now {now:?}")),
        }
    }

    failures
}

#[test]
fn composes_random_move_and_delete_sequences_to_the_content_they_leave() {
    let failures: Vec<String> = (0..2000).flat_map(failures_of).collect();

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn composition_of(
    declarations: Vec<Declaration>,
    present: &[&str],
    folders: &[&str],
) -> Composition {
    let mut tree = Tree::default();

    for (index, path) in present.iter().enumerate() {
        tree.place(path, index);
    }

    tree.directories
        .extend(folders.iter().map(|folder| (*folder).to_string()));

    compose_in(&tree, declarations)
}

fn moves_of(composition: &Composition) -> Vec<(PathBuf, PathBuf)> {
    composition
        .moves
        .iter()
        .map(|found| (found.from.clone(), found.to.clone()))
        .collect()
}

#[test]
fn yields_nothing_for_a_round_trip() {
    let composition = composition_of(
        vec![
            move_of("docs/a.md", "docs/sub/a.md", 1),
            move_of("docs/sub/a.md", "docs/a.md", 2),
            move_of("c/d", "d", 3),
            move_of("d", "c/d", 4),
        ],
        &["docs/a.md", "c/d/f.md"],
        &["docs/sub"],
    );

    assert!(composition.moves.is_empty());
    assert!(composition.deletes.is_empty());
    assert!(composition.rejected.is_empty());
}

#[test]
fn never_emits_an_intermediate_beneath_another_original() {
    let composition = composition_of(
        vec![
            move_of("d", "e", 1),
            move_of("x.md", "d/f.md", 2),
            move_of("d/f.md", "y.md", 3),
        ],
        &["e/f.md", "y.md"],
        &["d"],
    );

    assert_eq!(
        moves_of(&composition),
        [
            (path_of("d"), path_of("e")),
            (path_of("x.md"), path_of("y.md"))
        ]
    );
}

#[test]
fn keeps_a_refilled_directorys_own_move() {
    let composition = composition_of(
        vec![move_of("d", "e", 1), move_of("x.md", "d/x.md", 2)],
        &["e/f.md", "d/x.md"],
        &[],
    );

    assert!(
        composition.rejected.is_empty(),
        "{:?}",
        composition.rejected
    );
    assert_eq!(
        moves_of(&composition),
        [
            (path_of("d"), path_of("e")),
            (path_of("x.md"), path_of("d/x.md"))
        ]
    );
}

#[test]
fn reports_no_delete_where_a_directory_moved_into_a_moved_directory() {
    let composition = composition_of(
        vec![
            move_of("a", "x", 1),
            move_of("a/k.md", "x/k.md", 2),
            move_of("d", "x/d", 3),
            move_of("d/f.md", "x/d/f.md", 4),
            move_of("x/d", "d2", 5),
            move_of("x/d/f.md", "d2/f.md", 6),
        ],
        &["x/k.md", "d2/f.md"],
        &[],
    );

    assert!(
        composition.deletes.is_empty(),
        "{:?} {:?}",
        composition.deletes,
        moves_of(&composition)
    );
    assert!(
        composition.rejected.is_empty(),
        "{:?}",
        composition.rejected
    );
    assert!(moves_of(&composition).contains(&(path_of("d"), path_of("d2"))));
}

#[test]
fn treats_a_recreated_vacated_path_as_a_new_arrival() {
    let composition = composition_of(
        vec![
            move_of("a", "D", 1),
            move_of("D/g.md", "a/g.md", 2),
            move_of("a", "Z/a", 3),
            move_of("q", "y", 4),
            delete_of("q/b", 5),
            delete_of("q", 6),
        ],
        &["D/h.md", "Z/a/g.md", "y/b/f.md"],
        &[],
    );

    assert!(composition.deletes.is_empty(), "{:?}", composition.deletes);
    assert!(
        composition.rejected.is_empty(),
        "{:?}",
        composition.rejected
    );
    assert!(moves_of(&composition).contains(&(path_of("a"), path_of("D"))));
    assert!(moves_of(&composition).contains(&(path_of("a/g.md"), path_of("Z/a/g.md"))));
    assert!(!moves_of(&composition)
        .iter()
        .any(|(from, to)| *from == path_of("a") && *to == path_of("Z/a")));
}

#[test]
fn blames_the_failing_declaration_and_keeps_the_earlier_move() {
    let composition = composition_of(
        vec![move_of("a.md", "b.md", 1), move_of("b.md", "c.md", 2)],
        &["b.md"],
        &[],
    );

    assert_eq!(moves_of(&composition), [(path_of("a.md"), path_of("b.md"))]);
    assert_eq!(
        composition
            .rejected
            .iter()
            .map(|rejection| (rejection.origin.line, rejection.reason.as_str()))
            .collect::<Vec<_>>(),
        [(2, "destination missing")]
    );
}
