use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

pub struct Tree {
    _directory: TempDir,
    pub root: PathBuf,
}

impl Tree {
    pub fn is_case_insensitive(&self) -> bool {
        let probe = self.root.join("case-probe.tmp");

        fs::write(&probe, "").unwrap();

        let insensitive = self.root.join("CASE-PROBE.TMP").exists();

        fs::remove_file(probe).unwrap();

        insensitive
    }
}

pub fn tree_of_bytes(files: &[(&str, &[u8])]) -> Tree {
    let directory = TempDir::new().unwrap();
    let root = dunce::canonicalize(directory.path()).unwrap();

    for (name, content) in files {
        let path = root.join(name);

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    Tree {
        _directory: directory,
        root,
    }
}

pub fn tree_of(files: &[(&str, &str)]) -> Tree {
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, content)| (*name, content.as_bytes()))
        .collect();

    tree_of_bytes(&files)
}
