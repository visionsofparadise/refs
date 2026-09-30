# refs

`refs` keeps file references correct when files move. It finds paths written in any text file, and repoints the ones a move broke.

## Install

Download the installer for your platform from the [latest release](https://github.com/visionsofparadise/refs/releases/latest).

Supports: Windows x64/arm64, macOS arm64/x64, Linux x64/arm64

## Usage

```text
refs [OPTIONS] [<path>...]
refs - [--dry-run] [OPTIONS] [<path>...]
```

The first form lists references. The second reads move and delete declarations from stdin and rewrites the references they broke. `<path>` defaults to `.`.

| Option            | Effect                                                      |
| ----------------- | ----------------------------------------------------------- |
| `--to <path>`     | List only references to `<path>` or beneath it. Repeatable. |
| `--dangling`      | List only references whose target does not exist.           |
| `--dry-run`       | Print what a fix would do without writing.                  |
| `--no-ignore`     | Ignore `.gitignore`, `.ignore` and `.rgignore`.             |
| `--no-ignore-vcs` | Ignore `.gitignore` only.                                   |
| `--hidden`        | Include hidden files.                                       |
| `-u`, `-uu`       | `--no-ignore`, then also `--hidden`.                        |
| `-h`, `--help`    | Print help.                                                 |
| `-V`, `--version` | Print the version.                                          |

### Declarations

| Source                           | Example line                    |
| -------------------------------- | ------------------------------- |
| `mv -v`                          | `renamed 'a.md' -> 'b.md'`      |
| `rm -v`                          | `removed 'a.md'`                |
| `git mv -v`                      | `Renaming a.md to b.md`         |
| `git rm`                         | `rm 'a.md'`                     |
| `git diff -M --name-status [-z]` | `R100 a.md b.md`, tab-separated |
| hand-written                     | `R a.md b.md`, `D a.md`         |

Declarations describe moves that already happened. git paths are relative to the repository root.

### Output

```text
<file>:<line>:<column>: <reference> -> <target>[ (dangling)]
<file>:<line>:<column>: <old> -> <new>
<file>:<line>:<column>: <reference> -> <target> (deleted|out of scope|unrewritable)
```

Listing prints the first line per reference. A fix prints the second per rewrite and the third per reference it reports instead. Rewrites keep the reference's form: relative or absolute, separators, escaping, suffixes such as `#section` or `:12`.

### Exit status

| Code | Meaning                                                                      |
| ---- | ---------------------------------------------------------------------------- |
| `0`  | Success.                                                                     |
| `1`  | `--dangling` printed a line, or a fix reported a reference or a declaration. |
| `2`  | Usage or I/O error.                                                          |

### Examples

A tree with `README.md`, `config.json`, `docs/guide.md` and `docs/setup.md`, where the guide links to `setup.md`.

What links to the guide:

```sh
refs --to docs/guide.md
```

```text
README.md:1:18: docs/guide.md -> docs/guide.md
config.json:1:12: docs/guide.md -> docs/guide.md
```

Move it and repair its links, including its own link to `setup.md`:

```sh
mv -v docs/guide.md manual/ | refs -
```

```text
README.md:1:18: docs/guide.md -> manual/guide.md
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

From the original tree in a git repository, repair after a `git mv`, run from the repository root:

```sh
git mv docs/guide.md GUIDE.md
git diff -M --cached --name-status -z | refs -
```

```text
README.md:1:18: docs/guide.md -> GUIDE.md
```

## License

[MIT](LICENSE)
