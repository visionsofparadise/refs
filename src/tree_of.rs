use std::fs;
use std::path::{Path, PathBuf};
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

#[cfg(unix)]
pub fn link_directory(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
pub fn link_directory(target: &Path, link: &Path) -> bool {
    std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|output| output.status.success())
}

pub struct Lock {
    _file: Option<fs::File>,
}

#[cfg(windows)]
pub fn lock_file(path: &Path) -> Option<Lock> {
    use std::os::windows::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .ok()
        .map(|file| Lock { _file: Some(file) })
}

#[cfg(unix)]
pub fn lock_file(path: &Path) -> Option<Lock> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).ok()?;

    if fs::read(path).is_ok() {
        return None;
    }

    Some(Lock { _file: None })
}
