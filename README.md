# refs

`refs` keeps file references correct when files move. It finds paths written in any text file, and repoints the ones a move broke.

## Install

Download from the [latest release](https://github.com/visionsofparadise/refs/releases/latest). Installers exist for Windows, macOS and Linux x64 (AppImage, .deb). Linux arm64 gets a standalone binary.

## Usage

```text
refs [OPTIONS] [<path>...]
refs - [--dry-run] [OPTIONS] [<path>...]
```

The first form lists references. The second reads move and delete declarations from stdin and rewrites the references they broke. `<path>` defaults to `.`.

| Option              | Effect                                                                                      |
| ------------------- | ------------------------------------------------------------------------------------------- |
| `--to <path>`       | List only references to `<path>` or beneath it. Repeatable.                                 |
| `--dangling`        | List only references whose target does not exist.                                           |
| `--dry-run`         | Print what a fix would do without writing.                                                  |
| `--no-ignore`       | Disable `.gitignore`, the global gitignore, `.git/info/exclude`, `.ignore` and `.rgignore`. |
| `--no-ignore-vcs`   | Disable the three git sources only; `.ignore` and `.rgignore` still apply.                  |
| `--hidden`          | Include hidden files; `.git`, `.hg`, `.svn`, `.jj` and `.bzr` stay excluded.                |
| `-u`, `-uu`, `-uuu` | `-u` is `--no-ignore`, `-uu` adds `--hidden`, `-uuu` equals `-uu`.                          |
| `-h`, `--help`      | Print help.                                                                                 |
| `-V`, `--version`   | Print the version.                                                                          |

### Declarations

| Source                           | Example line                    |
| -------------------------------- | ------------------------------- |
| GNU `mv -v`                      | `renamed 'a.md' -> 'b.md'`      |
| BSD `mv -v`                      | `a.md -> b.md`                  |
| `rm -v`                          | `removed 'a.md'`                |
| `git mv -v`                      | `Renaming a.md to b.md`         |
| `git rm`                         | `rm 'a.md'`                     |
| `git diff -M --name-status [-z]` | `R100 a.md b.md`, tab-separated |
| hand-written                     | `R a.md b.md`, `D a.md`         |

Declarations describe moves that already happened. git paths are relative to the repository root. `mv -v` across filesystems prints copied and removed lines, which are understood as a move. `mv`, `rm` and `git mv` output is recognized in any language.

### Output

```text
<file>:<line>:<column>: <reference> -> <target>[ (dangling)]
<file>:<line>:<column>: <old> -> <new>
<file>:<line>:<column>: <reference> -> <target> (deleted|out of scope|unrewritable)
<destination>: outbound references not repointed (out of scope)
```

Listing prints the first line per reference. A fix prints the second per rewrite and the third per reference it reports instead. Rewrites keep the reference's form: relative or absolute, separators, escaping, suffixes such as `#section` or `:12`. A relative reference to another drive becomes absolute.

The fourth line names a moved file outside the scanned paths, whose own references are not repointed. A rewrite of a declaration file fed from inside the scope prints with `(not rewritten)` appended and leaves the file untouched. Stderr carries `refs: skipped declaration line <n>: <reason>: <text>` for each skipped declaration and `refs: <file>: declaration input not rewritten`.

### Exit status

| Code | Meaning                                                                      |
| ---- | ---------------------------------------------------------------------------- |
| `0`  | Success.                                                                     |
| `1`  | `--dangling` printed a line, or a fix reported a reference or a declaration. |
| `2`  | Usage or I/O error.                                                          |

### Examples

The starting tree, where the guide links to `setup.md` and the other files link to the guide:

```text
.
├── README.md         [Guide](docs/guide.md)
├── config.json       {"guide": "docs/guide.md"}
├── docs
│   ├── guide.md      # Guide, blank line, See [setup](setup.md).
│   └── setup.md      # Setup
└── manual
```

What links to the guide:

```sh
refs --to docs/guide.md
```

```text
README.md:1:9: docs/guide.md -> docs/guide.md
config.json:1:12: docs/guide.md -> docs/guide.md
```

Move it and repair its links, including its own link to `setup.md`:

```sh
mv -v docs/guide.md manual/ | refs -
```

```text
README.md:1:9: docs/guide.md -> manual/guide.md
config.json:1:12: docs/guide.md -> manual/guide.md
manual/guide.md:3:13: setup.md -> ../docs/setup.md
```

Delete a file and see what referenced it (exit status `1`):

```sh
rm -v docs/setup.md | refs -
```

```text
manual/guide.md:3:13: ../docs/setup.md -> docs/setup.md (deleted)
```

From the starting tree committed in a git repository with `core.autocrlf=false`, repair after a `git mv`, run from the repository root:

```sh
git mv docs/guide.md GUIDE.md
git diff -M --cached --name-status -z | refs -
```

```text
GUIDE.md:3:13: setup.md -> docs/setup.md
README.md:1:9: docs/guide.md -> GUIDE.md
config.json:1:12: docs/guide.md -> GUIDE.md
```

## License

[MIT](LICENSE)

The translated message templates embedded in `src/declaration_messages.rs` come from GNU coreutils and git translation catalogues under the GPL; their licenses and credits are in [TRANSLATION-NOTICES.txt](TRANSLATION-NOTICES.txt).
