#!/usr/bin/env python3
"""Counts the tests waiting on the kernel, and fails on those waiting on
a capability that has landed.

A test waiting on the kernel is `#[ignore = "kernel: <capability>"]`
(AGENTS.md, tools/ignore_failing.py). This reads the attributes in the
workspace's Rust files (git's, tracked or not, but not those it
ignores), comments and the insides of string literals left out, and
prints how many tests wait on each capability, per crate. Given capabilities that have
landed, it lists every test still tagged with one and exits 1.

Usage:
    tools/kernel_ignores.py [CAPABILITY...]

Exits 2 on an argument naming no capability, on an ignore that names
the kernel but isn't `#[ignore = "kernel: <capability>"]` exactly with a
known capability (in a `cfg_attr`, a raw string, without the space...),
on such a tag that isn't on a `#[test]` function (rustc doesn't say),
or on a file it can't read as UTF-8. `tools/ci.sh kernel-ignores` runs
it with the capabilities listed as landed there.
"""

import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from ignore_failing import (  # noqa: E402
    ATTRS_BEFORE, CAPABILITIES, ROOT, TAG, literal_end, literal_start, strip_code)

# Any ignore reason naming the kernel, well formed or not, a raw string's
# too; TAG is the well-formed one.
LOOSE = re.compile(r'\bignore\s*=\s*b?r?#*"\s*kernel\s*:[^"]*"')
QUOTE = re.compile(r"[\"']")
ATTR = r'#\[(?:[^\[\]"]|"(?:[^"\\]|\\.)*"|\[[^\]]*\])*\]'
# What may follow a tag up to its function's name: more attributes, the
# function's qualifiers.
TO_FN = re.compile(r'(\s*(?:' + ATTR + r'\s*)*)(?:pub(?:\([^)]*\))?\s+)?'
                   r'(?:(?:const|async|unsafe|extern\s+"[^"]*")\s+)*fn\s+((?:r#)?\w+)')
TEST = re.compile(r"#\[\s*(\w+::)*test\s*\]")


def blank_literals(code):
    """Comment-free `code` with its string and char literals blanked
    (lines kept), so what's left is outside them."""
    out, i = [], 0
    while (m := QUOTE.search(code, i)) is not None:
        start = literal_start(code, m.start())
        end = literal_end(code, start)
        if end is None:
            out.append(code[i:m.end()])
            i = m.end()
            continue
        out.append(code[i:start])
        out.append(re.sub(r"[^\n]", " ", code[start:end]))
        i = end
    out.append(code[i:])
    return "".join(out)


def scan(files):
    """The tags in `files` (path to text): a list of (path, line,
    capability, test name), and a list of problems."""
    tags, problems = [], []
    for path, text in files.items():
        code = strip_code(text)
        outside = blank_literals(code)
        for m in LOOSE.finditer(code):
            if outside[m.start()] == " ":
                continue
            line = code.count("\n", 0, m.start()) + 1
            start = code.rfind("#[", 0, m.start())
            tag = TAG.match(code, start) if start >= 0 else None
            cap = tag.group(1) if tag else None
            if cap not in CAPABILITIES:
                problems.append(f"{path}:{line}: {m.group(0)} is not in "
                                f'#[ignore = "kernel: <capability>"] with a known capability')
                continue
            fn = TO_FN.match(code, tag.end())
            before = ATTRS_BEFORE.search(code, max(0, start - 2000), start)
            if not fn or not TEST.search(before.group(1) + fn.group(1)):
                problems.append(f"{path}:{line}: {tag.group(0)} is not on a #[test] function")
                continue
            tags.append((path, line, cap, fn.group(2)))
    return tags, problems


def rust_files():
    """The workspace's Rust files git knows of or doesn't ignore, path to
    text, and the paths of those that aren't UTF-8."""
    names = subprocess.run(["git", "ls-files", "-z", "--cached", "--others",
                            "--exclude-standard", "--", "*.rs"], cwd=ROOT,
                           stdout=subprocess.PIPE, check=True).stdout
    files, bad = {}, []
    for name in sorted(set(names.split(b"\0")) - {b""}):
        path = ROOT / name.decode("utf-8", "surrogateescape")
        # Deleted but not yet staged.
        if path.is_file():
            try:
                files[str(path.relative_to(ROOT))] = path.read_bytes().decode("utf-8")
            except UnicodeDecodeError:
                bad.append(f"{path.relative_to(ROOT)}: not UTF-8")
    return files, bad


def crate_of(path):
    parts = Path(path).parts
    return parts[1] if len(parts) > 1 and parts[0] == "crates" else parts[0]


def report(tags):
    """The count per capability per crate, as lines."""
    by_cap = {c: Counter() for c in CAPABILITIES}
    for path, _, cap, _ in tags:
        by_cap[cap][crate_of(path)] += 1
    width = max(map(len, CAPABILITIES))
    out = [f"kernel-ignores: {len(tags)} tests wait on the kernel"]
    for cap in CAPABILITIES:
        crates = by_cap[cap]
        each = ", ".join(f"{c} {n}" for c, n in sorted(crates.items()))
        out.append(f"  {cap:{width}} {sum(crates.values()):4}" + (f"  ({each})" if each else ""))
    return out


def main(landed):
    unknown = [c for c in landed if c not in CAPABILITIES]
    if unknown:
        print(f"kernel-ignores: unknown capabilities {unknown}; known: {CAPABILITIES}",
              file=sys.stderr)
        return 2
    files, problems = rust_files()
    tags, more = scan(files)
    problems += more
    for p in problems:
        print("kernel-ignores:", p, file=sys.stderr)
    if problems:
        return 2
    print("\n".join(report(tags)), flush=True)
    waiting = [t for t in tags if t[2] in landed]
    for path, line, cap, fn in waiting:
        print(f"{path}:{line}: {fn} waits on {cap}, which has landed", file=sys.stderr)
    if waiting:
        print(f"kernel-ignores: {len(waiting)} tests still tagged with a landed capability "
              f"({', '.join(landed)}): un-ignore them and make them pass", file=sys.stderr)
        return 1
    if landed:
        print(f"kernel-ignores: no test waits on {', '.join(landed)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
