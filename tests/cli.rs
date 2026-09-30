mod integration {
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};

    fn refs_of(root: &Path, arguments: &[&str], input: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_refs"))
            .args(arguments)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();

        child.wait_with_output().unwrap()
    }

    fn stdout_of(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }

    fn stderr_of(output: &Output) -> String {
        String::from_utf8(output.stderr.clone()).unwrap()
    }

    fn snapshot_of(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];

        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();

                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.insert(path.clone(), fs::read(&path).unwrap());
                }
            }
        }

        files
    }

    #[test]
    fn lists_then_repairs_a_move_and_reports_a_delete() {
        let directory = tempfile::TempDir::new().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();

        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("index.md"), "[guide](docs/guide.md)\n").unwrap();
        fs::write(root.join("docs/guide.md"), "").unwrap();

        let listed = refs_of(&root, &[], "");

        assert_eq!(
            stdout_of(&listed),
            "index.md:1:9: docs/guide.md -> docs/guide.md\n"
        );
        assert_eq!(stderr_of(&listed), "");
        assert_eq!(listed.status.code(), Some(0));

        fs::create_dir_all(root.join("guides")).unwrap();
        fs::rename(root.join("docs/guide.md"), root.join("guides/start.md")).unwrap();

        let declaration = "R\tdocs/guide.md\tguides/start.md\n";
        let before_preview = snapshot_of(&root);
        let previewed = refs_of(&root, &["-", "--dry-run"], declaration);

        assert_eq!(
            stdout_of(&previewed),
            "index.md:1:9: docs/guide.md -> guides/start.md\n"
        );
        assert_eq!(stderr_of(&previewed), "");
        assert_eq!(previewed.status.code(), Some(0));
        assert_eq!(snapshot_of(&root), before_preview);

        let fixed = refs_of(&root, &["-"], declaration);

        assert_eq!(
            stdout_of(&fixed),
            "index.md:1:9: docs/guide.md -> guides/start.md\n"
        );
        assert_eq!(stderr_of(&fixed), "");
        assert_eq!(fixed.status.code(), Some(0));
        assert_eq!(
            fs::read_to_string(root.join("index.md")).unwrap(),
            "[guide](guides/start.md)\n"
        );

        fs::remove_file(root.join("guides/start.md")).unwrap();

        let before_delete = snapshot_of(&root);
        let deleted = refs_of(&root, &["-"], "D\tguides/start.md\n");

        assert_eq!(
            stdout_of(&deleted),
            "index.md:1:9: guides/start.md -> guides/start.md (deleted)\n"
        );
        assert_eq!(stderr_of(&deleted), "");
        assert_eq!(deleted.status.code(), Some(1));
        assert_eq!(snapshot_of(&root), before_delete);
    }
}
