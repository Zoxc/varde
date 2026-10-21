#!/usr/bin/env python3
"""Cross-checks the attribution of code derived from other projects.

Every source file with a `Derived from` comment needs a row in
THIRD_PARTY.md naming it, and every row's file needs a `Derived from`
comment naming the row's project, pin and original file. Either missing
fails.

THIRD_PARTY.md holds one Markdown table whose header names at least these
columns (others are free):

    | Row | File | Project | Pin | Original file | License |

`File` is our file's path from the repository root, `Project` the
project's name, `Pin` the full commit hash ported from (or, for a source
outside version control, `sha256 <hash>` of the original file), and
`Original file` the path in that project; none of the named columns may
be empty. A row's file may be of any kind; unlisted derived files are
looked for in source files only (by suffix). A file may have several rows
(one per origin) and a row may name its `Row` id in the file's comment
("See THIRD_PARTY.md, row L4"); if a comment names a row, that row must
exist and be for this file.

A `Derived from` block is a comment line starting with those words and
the comment lines right after it. Its first line must be, in full,

    // Derived from <project> (<url>), commit <40-hex hash>, file <path>

(a trailing comma allowed, any comment marker: `///` for one item of a
file with several origins), or with `sha256 <64-hex hash>` in place of
the commit for a source outside version control; a row matches a block
whose project, pin and file are exactly the row's. A comment starting
with "Derived from" in any other shape fails. File header template:

    // Derived from <project> (<url>), commit <full hash>, file <path>,
    //   <original copyright line>, licensed under <license> (LICENSES/<file>).
    // Translated to Rust and modified: <main changes>.
    // See THIRD_PARTY.md, row <Ln>.

Usage: tools/check_attribution.py [--root DIR]. Exits 1 on a mismatch.
"""

import argparse
import pathlib
import re
import subprocess
import sys

SOURCE_SUFFIXES = {".rs", ".wgsl", ".js", ".py", ".sh", ".c", ".h", ".cpp", ".hpp"}
COMMENT = re.compile(r"^\s*(//[/!]?|#|\*|/\*)")
DERIVED = re.compile(r"^\s*(//[/!]?|#|\*|/\*)\s*Derived from\b")
FIRST_LINE = re.compile(
    r"^\s*(?://[/!]?|#|\*|/\*)\s*Derived from (?P<project>\S.*?) \((?P<url>[^\s()]+)\), "
    r"(?P<pin>commit [0-9a-f]{40}|sha256 [0-9a-f]{64}), file (?P<file>[^\s,]+),?\s*$"
)
ROW_REF = re.compile(r"THIRD_PARTY\.md,\s*row\s+([A-Za-z0-9_-]+)")
COLUMNS = ("Row", "File", "Project", "Pin", "Original file")


def source_files(root):
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
        cwd=root, check=True, capture_output=True, text=True,
    ).stdout.splitlines()
    # This script and its tests quote the comments they look for.
    here = pathlib.Path(__file__).resolve()
    me = {here, here.with_name("test_" + here.name)}
    for name in sorted(out):
        path = root / name
        if path.suffix in SOURCE_SUFFIXES and path.is_file() and path.resolve() not in me:
            yield name, path


def derived_blocks(text):
    """The `Derived from` comment blocks of a file, each as a triple: the
    line number and the match of its first line (None if that line isn't
    a full attribution line) and the block as one string."""
    lines = text.splitlines()
    blocks = []
    i = 0
    while i < len(lines):
        if DERIVED.match(lines[i]):
            start = i + 1
            first = FIRST_LINE.match(lines[i])
            block = [lines[i]]
            i += 1
            while i < len(lines) and COMMENT.match(lines[i]) and not DERIVED.match(lines[i]):
                block.append(lines[i])
                i += 1
            blocks.append((start, first, " ".join(COMMENT.sub("", l).strip() for l in block)))
        else:
            i += 1
    return blocks


def pin_of(first):
    """The pin of an attribution line as THIRD_PARTY.md writes it: the bare
    commit hash, or `sha256 <hash>`."""
    return first["pin"].removeprefix("commit ")


def table_rows(path, errors):
    """The rows of THIRD_PARTY.md's table, as dicts by column name; a row
    with too few cells or an empty one of COLUMNS goes to `errors`."""
    if not path.is_file():
        return []
    header = None
    rows = []
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.lstrip().startswith("|"):
            header = None
            continue
        cells = [c.strip().strip("`") for c in line.strip().strip("|").split("|")]
        if header is None:
            if all(c in cells for c in COLUMNS):
                header = cells
            continue
        if set(line.replace("|", "").strip()) <= set("-: "):
            continue
        if len(cells) < len(header):
            errors.append(f"THIRD_PARTY.md:{n}: {len(cells)} cells, the header has {len(header)}")
            continue
        row = dict(zip(header, cells))
        empty = [c for c in COLUMNS if not row[c]]
        if empty:
            errors.append(f"THIRD_PARTY.md:{n}: empty {', '.join(empty)}")
            continue
        row["_line"] = n
        rows.append(row)
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parent.parent)
    root = parser.parse_args().root.resolve()
    errors = []

    rows = table_rows(root / "THIRD_PARTY.md", errors)
    by_file = {}
    by_id = {}
    for row in rows:
        by_file.setdefault(row["File"], []).append(row)
        if row["Row"] in by_id:
            errors.append(f"THIRD_PARTY.md:{row['_line']}: row {row['Row']} listed twice")
        by_id[row["Row"]] = row

    def blocks_of(path):
        try:
            return derived_blocks(path.read_text(encoding="utf-8"))
        except (UnicodeDecodeError, OSError):
            return []

    derived = {}
    for name, path in source_files(root):
        blocks = blocks_of(path)
        if blocks:
            derived[name] = blocks

    for name, blocks in derived.items():
        if name not in by_file:
            errors.append(f"{name}: says 'Derived from' but THIRD_PARTY.md has no row for it")
        for line, first, block in blocks:
            if first is None:
                errors.append(
                    f"{name}:{line}: a 'Derived from' comment that isn't an attribution line: "
                    f"'Derived from <project> (<url>), commit <40-hex hash>, file <path>'"
                )
            for ref in ROW_REF.findall(block):
                row = by_id.get(ref)
                if row is None:
                    errors.append(f"{name}: names THIRD_PARTY.md row {ref}, which doesn't exist")
                elif row["File"] != name:
                    errors.append(f"{name}: names THIRD_PARTY.md row {ref}, which is for {row['File']}")

    for row in rows:
        where = f"THIRD_PARTY.md:{row['_line']} (row {row['Row']})"
        path = root / row["File"]
        blocks = derived.get(row["File"]) or (blocks_of(path) if path.is_file() else [])
        if not path.is_file():
            errors.append(f"{where}: file {row['File']} doesn't exist")
        elif not blocks:
            errors.append(f"{where}: {row['File']} has no 'Derived from' comment")
        elif not any(
            first is not None
            and first["project"] == row["Project"]
            and pin_of(first) == row["Pin"]
            and first["file"] == row["Original file"]
            for _, first, _ in blocks
        ):
            errors.append(
                f"{where}: no 'Derived from' comment in {row['File']} names "
                f"{row['Project']}, {row['Pin']} and {row['Original file']}"
            )

    for e in errors:
        print(e, file=sys.stderr)
    print(f"attribution: {len(derived)} derived file(s), {len(rows)} THIRD_PARTY.md row(s), "
          f"{len(errors)} problem(s)")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
