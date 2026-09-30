mod integration {
    use std::fs;
    use std::io::Write;
    use std::path::Path;
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
        assert_eq!(listed.status.code(), Some(0));

        fs::create_dir_all(root.join("guides")).unwrap();
        fs::rename(root.join("docs/guide.md"), root.join("guides/start.md")).unwrap();

        let declaration = "R\tdocs/guide.md\tguides/start.md\n";
        let previewed = refs_of(&root, &["-", "--dry-run"], declaration);

        assert_eq!(
            stdout_of(&previewed),
            "index.md:1:9: docs/guide.md -> guides/start.md\n"
        );
        assert_eq!(previewed.status.code(), Some(0));
        assert_eq!(
            fs::read_to_string(root.join("index.md")).unwrap(),
            "[guide](docs/guide.md)\n"
        );

        let fixed = refs_of(&root, &["-"], declaration);

        assert_eq!(
            stdout_of(&fixed),
            "index.md:1:9: docs/guide.md -> guides/start.md\n"
        );
        assert_eq!(fixed.status.code(), Some(0));
        assert_eq!(
            fs::read_to_string(root.join("index.md")).unwrap(),
            "[guide](guides/start.md)\n"
        );

        fs::remove_file(root.join("guides/start.md")).unwrap();

        let deleted = refs_of(&root, &["-"], "D\tguides/start.md\n");

        assert_eq!(
            stdout_of(&deleted),
            "index.md:1:9: guides/start.md -> guides/start.md (deleted)\n"
        );
        assert_eq!(deleted.status.code(), Some(1));
    }
}
