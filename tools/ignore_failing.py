#!/usr/bin/env python3
"""Ignores the tests that fail for want of a kernel capability.

While the kernel facade (`crates/kernel-compat`) stubs an operation, the
tests that need it fail. This marks each failing test with

    #[ignore = "kernel: <capability>"]

so the suite stays green and `git grep 'kernel: <capability>'` lists what
a capability's landing must restore (AGENTS.md says what the tags mean).

It runs `cargo test --workspace --no-fail-fast` (or reads a saved log of
one with `--log`), collects the failing tests, finds each one's function
in the source and gives it the tag, or changes the capability of a tag it
already has. A test with another `#[ignore]` keeps it. The capability is
the most advanced one (in CAPABILITIES' order) that the test's body,
the helpers it calls (and the one it failed in), its file's name and its
failure output point at, so a test is only
un-ignored once everything it needs exists; OVERRIDES settles the ones
the heuristic gets wrong. Tests in UNRELATED fail for reasons other than
the kernel and are left alone. Doc tests are only reported.

Usage:
    tools/ignore_failing.py [--log FILE] [--dry-run] [-v]

`--dry-run` prints what it would do; `-v` lists every test with its
capability. Cargo's variables apply to the run (CARGO_BUILD_JOBS=4...).
To re-tag the ignored tests, give it the log of
`cargo test --workspace --no-fail-fast -- --include-ignored`; a tagged
test that passes there is reported, for its tag to go.
Exits 1 if a failing test couldn't be found or tagged, a tagged test
passes, or the log is broken: the build failed, a test binary stopped
before its summary, or one didn't run (a run cut short, or not of the
whole workspace).
"""

import argparse
import functools
import json
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The capabilities, from the first the kernel gives to the last.
CAPABILITIES = [
    "solids",
    "extrude",
    "revolve",
    "tessellate",
    "measure",
    "export",
    "boolean",
    "curved boolean",
    "tangency",
    "touches",
    "assemble",
    "resolve",
    "distance",
    "feature",
]
RANK = {c: i for i, c in enumerate(CAPABILITIES)}

# What in a test's code (comments left out) points at a capability.
CODE_HINTS = [
    ("solids", r"\bcuboid\b|\bcylinder\b|Solid::new\b"),
    ("extrude", r"[Ee]xtrude|Document::example\b|\bexample\(\)"),
    ("revolve", r"[Rr]evolve"),
    ("tessellate", r"render_mesh|RenderMesh|tessellat|triangle|\.mesh\(\)|MeshUpload"
     r"|thumbnail|Renderer\b"),
    ("measure", r"\bmeasure|\bvolume\b|moments?\b"),
    ("export", r"\bexport|three_mf|ThreeMf|\b3mf\b"),
    ("boolean", r"\bboolean\b|\bunion\b|\bdifference\b|\bintersect|Operation::(Join|Cut|Intersect)"
     r"|\bCombine\b|BodyOp\b"),
    # A join, cut or intersect of an extrude, revolve, sweep or loft asks
    # which bodies its tool touches first (a combine doesn't).
    ("touches", r"\btouch(es|ed)\b|touched_features|\badd_join\b|Operation::(Join|Cut|Intersect)\b"
     r"|OperationKind::(Join|Cut|Intersect)\b|\"operation\":\s*\"(join|cut)\""),
    ("assemble", r"\bassemble|\btransformed\b|FeatureKind::(Move|Mirror|Pattern|Align|Scale)\b"
     r"|\b(Move|Mirror|Pattern|Align|Scale) \{|Look::Start(Move|Mirror|\w*Pattern|Align|Scale)\b"
     r"|MotionKind::\w+|MotionLook::"
     r"|\"(move|mirror|pattern|align|scale)\",\s*json!"),
    ("resolve", r"FaceKey\b|EdgeKey\b|\bTopology\b|\btopology\(\)|FaceRef\b|EdgeRef\b"
     r"|[Bb]ody_?[Tt]opology|topology_call|Picked::(Edge|Vertex)\b|\w+_keys\(|tangent_chain"
     r"|Plane::Face\b|AxisRef::(Edge|Face)\b|PointRef::(?!Origin)|DirRef::Normal\b"
     r"|Entity::(Face|Edge|Corner|EdgePoint)\b|\b(face|edge|corner)_pick\b|Look::PickPlane\b"
     r"|StartMeasure\b|\bprobed\b|LinkKind\b|\badd_link\b"),
    ("feature", r"[Ff]illet|[Cc]hamfer|\bShell\b|\bshell\(|[Oo]ffset_?[Ff]ace|[Ff]ace_?[Dd]raft"
     r"|[Dd]raft_faces|[Ss]weep|[Ll]oft|\bSplit\b|split::|split_tool|\btaper"),
]

# The same, for words too common to count in a helper the test calls
# (which may do much else): only in the test's own code.
WEAK_HINTS = [
    ("tessellate", r"[Pp]review|\.mesh\b|part_ends|\bscene\b|\btables\b|\brender\("),
    ("tangency", r"tangen"),
    ("resolve", r"\bresolve"),
    ("distance", r"\"Distance \d|\.distance\b(?!\s*\()|measure::distance"),
]

# Picking in the model, also only in the test's own code. In the app a pick mostly becomes a reference or a
# selection kept by its faces' names ("resolve"); elsewhere (the pick
# tables, the viewport's rays) it needs the model drawn ("tessellate"),
# and the names it needs show in the hints above.
PICK = r"\bpick(ed|s|ing)?\b|pick_\w+"

# What a test's failure output points at: the stubs' NotImplemented
# messages and the regeneration's "isn't supported yet".
OUTPUT_HINTS = [
    ("solids", r"\"boxes\"|\"cylinders\"|solids of patch meshes"),
    ("extrude", r"extruding"),
    ("revolve", r"revolving"),
    ("measure", r"measuring"),
    ("boolean", r"\bbooleans\b"),
    ("touches", r"whether solids touch"),
    ("assemble", r"moving a solid|assembling solids"),
    ("feature", r"tapered extrudes|shelling|offsetting faces|drafting faces|chamfering"
     r"|filleting|sweeping|lofting|splitting|split tool"),
]

# A test file's name (its stem, or a directory on its path) that points at
# a capability.
FILE_HINTS = {
    "revolve": "revolve",
    "combine": "boolean",
    "boolean_evidence": "boolean",
    "merging": "boolean",
    "overlaps": "touches",
    "measure": "measure",
    "export": "export",
    "three_mf": "export",
    "thumbnail": "tessellate",
    "preview": "tessellate",
    "chamfer": "feature",
    "fillet": "feature",
    "shell": "feature",
    "offset_face": "feature",
    "face_draft": "feature",
    "sweep": "feature",
    "loft": "feature",
    "split": "feature",
    "edge_facts": "resolve",
    "taper": "feature",
}
# The same, for regeneration's tests of the features that move bodies.
REGEN_MOTION_FILES = {"motion", "scale", "pattern", "align"}

# Capabilities the heuristic gets wrong: "<crate>::<test path>" -> capability.
OVERRIDES = {
    # Bodies merged by a join or cut by a cut, behind helpers: asking
    # which bodies the tool touches comes first.
    "varde-app::doc::delete::tests::a_cut_left_a_body_to_work_on_isnt_warned_of": "touches",
    "varde-app::doc::feed::tests::merged_bodies_aren_t_given_out_across_a_replacement": "touches",
    "varde-app::doc::feed::tests::merged_bodies_follow_the_model_shown": "touches",
    # Sketches on a face, links projecting its edges.
    "varde-app::doc::sketch::tests::faces::change::picking_a_plane_ends_when_the_document_turns_read_only": "resolve",
    "varde-app::mcp::tests::sketches::the_users_edit_behind_a_link_timed_out": "resolve",
    "varde-app::mcp::tests::sketches::undo_passes_over_a_link_waiting_on_the_solver": "resolve",
    "varde-app::mcp::tests::sketches::add_link_waiting_on_the_solver_times_out": "resolve",
    "varde-app::mcp::tests::sketches::add_link_projects_another_sketchs_curve": "resolve",
    "varde-app::mcp::tests::sketches::add_link_projects_an_edge": "resolve",
    # The camera framing the model's mesh.
    "varde-app::tests::a_view_cube_face_looks_at_the_model_as_home_does": "tessellate",
    "varde-app::tests::home_frames_the_model": "tessellate",
    "varde-app::tests::an_opened_document_frames_its_first_model_unless_the_camera_moved": "tessellate",
    "varde-view::viewport::tests::the_pivot_click_is_as_set": "tessellate",
    "varde-render::hover_and_selection_are_drawn_over_the_faded_model": "tessellate",
    # The scene drawn, behind helpers (the mesh cache, the reply's parts,
    # the highlight, what the model hides).
    "varde-regen::tests::dragging_a_draft_reruns_only_the_draft": "tessellate",
    "varde-regen::wire::tests::cut_short_parts_the_tables_go_by_are_refused": "tessellate",
    "varde-app::tests::a_body_row_hovered_lights_the_body": "tessellate",
    "varde-view::viewport::sketch_pick::tests::the_nearest_curve_is_picked_and_what_the_model_hides_only_through_it": "tessellate",
    # A topology query left waiting needs only the body built.
    "varde-app::mcp::tests::quitting_answers_the_calls_waiting": "extrude",
    # Through-all spans the solids' bounds.
    "varde-regen::history::tests::through_all_spans_the_bodies_with_a_margin": "measure",
}

# Tests that fail for reasons other than the kernel: "<crate>::<test path>".
UNRELATED = set()

CURVED = r"[Rr]evolve|\bcylinder\b|Curve::(Arc|Circle)|\bcircle\b|\barc\b|[Ff]illet"

TAG = re.compile(r'#\[ignore\s*=\s*"kernel: ([^"]*)"\]')


def run_tests():
    """Runs the workspace's tests; their output."""
    cmd = ["cargo", "test", "--workspace", "--no-fail-fast"]
    print("running", " ".join(cmd), file=sys.stderr)
    proc = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          text=True, check=False)
    return proc.stdout


class Packages:
    """The workspace's packages: `dirs`, package name -> its directory;
    `targets`, test binary name (a target's name, `-` as `_`) -> the
    (package, root file) pairs of the targets tested under that name; and
    `expected`, the binaries a whole run tests: ("unit", package, root
    file) and ("doc", lib name)."""

    def __init__(self):
        out = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"],
                             cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True).stdout
        self.dirs, self.targets, self.expected = {}, defaultdict(set), set()
        for p in json.loads(out)["packages"]:
            pdir = Path(p["manifest_path"]).parent
            self.dirs[p["name"]] = pdir
            for t in p["targets"]:
                if t.get("required-features"):
                    continue
                name = t["name"].replace("-", "_")
                if t["test"]:
                    root = str(Path(t["src_path"]).relative_to(pdir))
                    self.targets[name].add((p["name"], root))
                    self.expected.add(("unit", p["name"], root))
                if t["doctest"]:
                    self.expected.add(("doc", name))


RUNNING = re.compile(r"^\s+Running (?:unittests )?(\S+) \((\S+)\)")
DOCTESTS = re.compile(r"^\s+Doc-tests (\S+)")
STDOUT = re.compile(r"^---- (.+?) stdout ----$")


# A doc test's name has spaces: `src/lib.rs - Item (line 3)`.
RESULT = re.compile(r"^test (.+?)(?: - should panic)? \.\.\. (ok|FAILED|ignored)")
SUMMARY = re.compile(r"^test result: ")


def binary_packages(binary, root, pkgs):
    """The packages a test binary may have been built for, by its path:
    the build directory's (`build/<package>/<hash>/out/`), or else those
    with a target of its name and root file."""
    m = re.search(r"/build/([^/]+)/[0-9a-f]+/out/", binary)
    if m and m.group(1) in pkgs.dirs:
        return {m.group(1)}
    return {p for p, r in pkgs.targets.get(re.sub(r"-[0-9a-f]+$", "", Path(binary).name), ())
            if r == root}


class Run:
    """What a cargo test log says: the failing tests (package, root file,
    test path, output), the passing ones (package, root file, test path)
    and what's wrong with the log itself."""

    def __init__(self, log, pkgs):
        self.failed, self.passed, self.broken = [], [], []
        pkg = root = None
        binary = None  # the binary running, until its summary
        current = None
        failing = set()
        ran = set()
        for line in log.splitlines():
            m = RUNNING.match(line) or DOCTESTS.match(line)
            if m:
                if binary:
                    self.broken.append(f"{binary} stopped before its summary (crashed?)")
                if line.lstrip().startswith("Doc-tests"):
                    root, pkg, binary = None, m.group(1), f"doc tests of {m.group(1)}"
                    ran.add(("doc", pkg))
                else:
                    root, binary = m.group(1), m.group(2)
                    found = binary_packages(m.group(2), root, pkgs)
                    # Two packages' targets alike (old build layouts)
                    # leave its package unknown, its tests not tagged.
                    pkg = next(iter(found)) if len(found) == 1 else None
                    ran |= {("unit", p, root) for p in found}
                current = None
                continue
            m = RESULT.match(line)
            if m and binary:
                if m.group(2) == "ok":
                    self.passed.append((pkg, root, m.group(1)))
                elif m.group(2) == "FAILED":
                    failing.add((pkg, root, m.group(1)))
                continue
            m = STDOUT.match(line)
            if m:
                current = [pkg, root, m.group(1), []]
                self.failed.append(current)
                continue
            if SUMMARY.match(line):
                binary = current = None
                continue
            if current is not None:
                if line == "failures:":
                    current = None
                else:
                    current[3].append(line)
        if binary:
            self.broken.append(f"{binary} stopped before its summary (crashed?)")
        self.failed = [(p, r, t, "\n".join(o)) for p, r, t, o in self.failed]
        # Every test said to fail has its output, and the other way round.
        sections = {(p, r, t) for p, r, t, _ in self.failed}
        for p, r, t in sorted(failing ^ sections, key=str):
            self.broken.append(f"{p}::{t}: failed without its output, or the other way round")
        if not re.search(r"^test result: ", log, re.M):
            self.broken.append("no test ran: did the build fail? (see cargo's output)")
        else:
            # A run cut short between two binaries, or one of only some
            # packages, misses the failures of the binaries it didn't run.
            for b in sorted(pkgs.expected - ran):
                what = f"doc tests of {b[1]}" if b[0] == "doc" else f"{b[1]}'s {b[2]}"
                self.broken.append(f"{what} didn't run: the run stopped early, or wasn't"
                                   " of the whole workspace")


@functools.cache
def code_of(file):
    """File `file`'s code with comments blanked, as it was first read
    (the files are only written once all are read)."""
    return strip_code(file.read_text())


CHAR = re.compile(r"'(\\.[^']*|[^\\'])'")
# A raw string's start (`r"`, `br#"`), not the end of an identifier.
RAW = re.compile(r'(?<![\w#])b?r(#*)"')
STRING_REST = re.compile(r'(?:[^"\\]|\\.)*"', re.S)
RAW_BEFORE = re.compile(r'(?<![\w#])b?r#*$')


def literal_start(code, i):
    """The start of the literal whose opening quote is at `i`: before it
    if it's a raw string's (`r#"`)."""
    if code[i] == '"':
        m = RAW_BEFORE.search(code, max(0, i - 40), i)
        if m:
            return m.start()
    return i


def literal_end(code, i):
    """The index past the string or char literal starting at `i` in
    `code`, or None if none does (a lifetime's `'` among them)."""
    c, n = code[i], len(code)
    raw = RAW.match(code, i) if c in "br" else None
    if raw or c == '"':
        hashes = len(raw.group(1)) if raw else 0
        close = '"' + "#" * hashes
        if raw:
            j = code.find(close, raw.end())
            return n if j < 0 else j + len(close)
        m = STRING_REST.match(code, i + 1)
        return m.end() if m else n
    if c == "'":
        m = CHAR.match(code, i)
        if m:
            return m.end()
    return None


LEXEME = re.compile(r"//|/\*|[\"']")


def strip_code(text):
    """`text` with comments blanked, string contents kept (they name things)."""
    out = []
    i, n = 0, len(text)
    while True:
        m = LEXEME.search(text, i)
        if not m:
            out.append(text[i:])
            return "".join(out)
        j = m.start()
        out.append(text[i:j])
        if m.group() == "//":
            k = text.find("\n", j)
            k = n if k < 0 else k
            out.append(" " * (k - j))
            i = k
        elif m.group() == "/*":
            # Block comments nest.
            depth, k = 0, j
            while k < n:
                if text.startswith("/*", k):
                    depth, k = depth + 1, k + 2
                elif text.startswith("*/", k):
                    depth, k = depth - 1, k + 2
                    if depth == 0:
                        break
                else:
                    k += 1
            out.append(re.sub(r"[^\n]", " ", text[j:k]))
            i = k
        else:
            # A raw string's prefix is already out.
            k = literal_end(text, literal_start(text, j))
            k = j + 1 if k is None else min(k, n)
            out.append(text[j:k])
            i = k


# What a scan of comment-free code stops at: braces and literals' starts.
STOP = re.compile(r"[{}\"']")


def block_end(code, open_at):
    """The index past the `}` closing the `{` at `open_at` in comment-free
    `code`, skipping string and char literals."""
    depth = 0
    i, n = open_at, len(code)
    while (m := STOP.search(code, i)) is not None:
        i = m.start()
        end = literal_end(code, literal_start(code, i))
        if end is not None:
            i = end
            continue
        if code[i] == "{":
            depth += 1
        elif code[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def nested(code, lo, hi):
    """The spans of the `{}` blocks in `code[lo:hi]` (the outermost):
    what's in them isn't at the module's level."""
    spans = []
    i = lo
    while (m := STOP.search(code, i, hi)) is not None:
        i = m.start()
        end = literal_end(code, literal_start(code, i))
        if end is not None:
            i = end
        elif code[i] == "{":
            spans.append((i, block_end(code, i)))
            i = spans[-1][1]
        else:
            i += 1
    return spans


def at_top(spans, at):
    """Whether index `at` is outside all `spans`."""
    return not any(a <= at < b for a, b in spans)


def ident(name):
    """A pattern for identifier `name`, raw (`r#match`) or not."""
    return r"(?:r#)?" + re.escape(name.removeprefix("r#")) + r"\b"


# Outer attributes, `]` and `"` in their strings allowed, and a
# visibility, just before an item.
ATTRS_BEFORE = re.compile(r'((?:#\[(?:[^\[\]"]|"(?:[^"\\]|\\.)*"|\[[^\]]*\])*\]\s*)*)'
                          r'(?:pub(?:\([^)]*\))?\s+)?$')
PATH_ATTR = re.compile(r'#\[path\s*=\s*"([^"]+)"\s*\]')


def find_module(code, name, lo, hi):
    """The inline `mod name {` (its body's span) or `mod name;` (None) at
    the level of `code[lo:hi]`, with the `#[path]` given; False if neither."""
    pat = re.compile(r"\bmod\s+" + ident(name) + r"\s*([;{])")
    spans = nested(code, lo, hi)
    for m in pat.finditer(code, lo, hi):
        if not at_top(spans, m.start()):
            continue
        if m.group(1) == "{":
            start = m.end() - 1
            return ("inline", start, block_end(code, start))
        attrs = ATTRS_BEFORE.search(code, max(lo, m.start() - 2000), m.start())
        path = attrs and PATH_ATTR.search(attrs.group(1))
        return ("file", path and path.group(1))
    return False


def locate(crate_dir, root, path):
    """The file and the function's line index (0-based) of test `path`
    in the crate whose target's root is `root`, or a reason it's not found."""
    *mods, name = path.split("::")
    file = crate_dir / root
    # Where the current module's child modules' files live (for a crate
    # root, a mod.rs or a file a `#[path]` names, its own directory), and
    # whether it's in an inline module of `file`.
    children, inline = file.parent, False
    code = code_of(file)
    lo, hi = 0, len(code)
    for mod in mods:
        found = find_module(code, mod, lo, hi)
        if found is False:
            return None, f"no module {mod} in {file.relative_to(ROOT)}"
        plain = mod.removeprefix("r#")
        if found[0] == "inline":
            lo, hi = found[1] + 1, found[2] - 1
            children, inline = children / plain, True
            continue
        if found[1]:
            # Out of an inline module, a `#[path]` is from the file's
            # directory, whatever kind of file it is.
            nxt = (children if inline else file.parent) / found[1]
        else:
            nxt = children / f"{plain}.rs"
            if not nxt.exists():
                nxt = children / plain / "mod.rs"
        if not nxt.exists():
            return None, f"no file for module {mod} from {file.relative_to(ROOT)}"
        children = nxt.parent if found[1] or nxt.name == "mod.rs" else nxt.parent / nxt.stem
        file, inline = nxt, False
        code = code_of(file)
        lo, hi = 0, len(code)
    fn = re.compile(r"^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:(?:const|async|unsafe)\s+)*fn\s+"
                    + ident(name) + r"\s*[<(]", re.M)
    spans = nested(code, lo, hi)
    hits = [m for m in fn.finditer(code, lo, hi) if at_top(spans, m.start())]
    if not hits:
        return None, f"no fn {name} in {file.relative_to(ROOT)} (made by a macro?)"
    at = hits[0].start()
    return (file, code.count("\n", 0, at), code), None


def signature_end(code, at):
    """The index of the `{` opening the body of the function whose
    signature starts at `at`, or -1 if it has none (it ends at a `;`):
    the first of either outside the signature's brackets, as in
    `-> [u32; 3]`."""
    depth = 0
    for i in range(at, len(code)):
        c = code[i]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif depth == 0 and c in "{;":
            return i if c == "{" else -1
    return -1


def body_of(code, line):
    """The source of the function starting at `line` of comment-free `code`."""
    start = sum(len(l) + 1 for l in code.split("\n")[:line])
    return code[start:block_end(code, signature_end(code, start))]


FN = re.compile(r"\bfn\s+((?:r#)?\w+)\s*[<(]")
# A call of a function: only the helpers a test calls count, not the
# ones its variables, fields or other tests share a name with.
CALL = re.compile(r"\b([a-z_][a-z0-9_]*)\s*\(")
TEST_ATTR = re.compile(r"#\[(\w+::)*test\b[^\]]*\]\s*(#\[[^\]]*\]\s*|///[^\n]*\s*)*$")


class Helpers:
    """The functions of a crate's test code, by name, so a test's
    helpers count as its body: what a test needs is often behind them."""

    def __init__(self, crate_dir):
        self.by_file = defaultdict(dict)  # file -> name -> [body]
        for file in sorted(crate_dir.rglob("*.rs")):
            rel = str(file.relative_to(crate_dir))
            if "test" not in rel:
                continue
            code = code_of(file)
            for m in FN.finditer(code):
                brace = signature_end(code, m.start())
                # Tests aren't helpers.
                if brace < 0 or TEST_ATTR.search(code, max(0, m.start() - 300), m.start()):
                    continue
                body = code[m.start():block_end(code, brace)]
                self.by_file[file].setdefault(m.group(1).removeprefix("r#"), []).append(body)

    def at(self, file, line):
        """The body of the helper in test file `file` holding 1-based
        `line`, or ""."""
        if file not in self.by_file:
            return ""
        code = code_of(file)
        offset = sum(len(l) + 1 for l in code.split("\n")[:line - 1])
        best = ""
        for m in FN.finditer(code, 0, offset):
            brace = signature_end(code, m.start())
            # The innermost: the last to start that holds it.
            if brace >= 0 and block_end(code, brace) > offset:
                best = m.group(1).removeprefix("r#") in self.by_file[file] and code[m.start():block_end(code, brace)]
        return best or ""

    def called(self, file, body, depth=1):
        """The bodies of the helpers `body` calls, `depth` deep, from
        `file` or else from the files of its parent modules."""
        near = [f for f in self.by_file if f == file or file.is_relative_to(f.parent)
                and f.name in ("tests.rs", "mod.rs") or f.with_suffix("") == file.parent]
        near.sort(key=lambda f: (f != file, -len(f.parts)))
        seen = set()
        parts = []
        todo = [body]
        for _ in range(depth):
            nxt = []
            for text in todo:
                for word in set(CALL.findall(text)) - seen:
                    seen.add(word)
                    for f in near:
                        if word in self.by_file[f]:
                            nxt.extend(self.by_file[f][word])
                            break
            parts.extend(nxt)
            todo = nxt
        return "\n".join(parts)


def classify(key, file, own, called, output):
    """The capability test `key` needs, and what pointed at it, from its
    own code `own`, the helpers it calls `called` and its failure."""
    if key in OVERRIDES:
        return OVERRIDES[key], ["override"]
    found = set()
    # A field set to nothing names what the test doesn't use.
    unset = r"\b\w+\s*:\s*None\b|\"\w+\"\s*:\s*(0|\"0[^\"]*\")"
    own, called = re.sub(unset, "", own), re.sub(unset, "", called)
    body = own + "\n" + called
    for cap, pat in CODE_HINTS:
        if re.search(pat, body):
            found.add(cap)
    for cap, pat in WEAK_HINTS:
        if re.search(pat, own):
            found.add(cap)
    for cap, pat in OUTPUT_HINTS:
        if re.search(pat, output):
            found.add(cap)
    rel = file.relative_to(ROOT)
    if re.search(PICK, own):
        found.add("resolve" if rel.parts[:2] == ("crates", "app") else "tessellate")
    parts = [p.removesuffix(".rs") for p in rel.parts]
    for part in parts:
        if part in FILE_HINTS:
            found.add(FILE_HINTS[part])
    if "regen" in parts and "history" in parts and parts[-1] in REGEN_MOTION_FILES:
        found.add("assemble")
    if "boolean" in found and re.search(CURVED, body):
        found.add("curved boolean")
    if not found:
        found.add("solids")
        return "solids", ["default"]
    best = max(found, key=RANK.get)
    return best, sorted(found, key=RANK.get)


PANIC = re.compile(r"panicked at ([^:\s]+\.rs):(\d+):\d+:")


STRING = re.compile(r'"(?:[^"\\]|\\.)*"')


def attributes(lines, code_lines, fn_line):
    """The outer attributes of the item at `fn_line`: (first line, text)
    for each, bottom up. `lines` is the source and `code_lines` the same
    with comments blanked; comments and doc comments between them are
    skipped, a blank line or anything else ends them."""
    found = []
    i, depth, text = fn_line - 1, 0, []
    while i >= 0:
        code = STRING.sub('""', code_lines[i]).strip()
        if depth == 0 and not code:
            if not lines[i].strip():
                break
            i -= 1
            continue
        depth += code.count("]") + code.count(")") - code.count("[") - code.count("(")
        text.insert(0, code_lines[i].strip())
        if depth < 0:
            break
        if depth == 0:
            if not code.startswith("#["):
                break
            found.append((i, " ".join(text)))
            text = []
        i -= 1
    return found


def tag(lines, code_lines, fn_line, cap):
    """Gives the function at `fn_line` the capability's tag in `lines`
    (a list, changed in place; `code_lines` the original's lines with
    comments blanked, the same above the lines changed): 'added',
    'changed', 'kept' (the tag is there), 'other' (another #[ignore],
    perhaps in a `cfg_attr`) or None (no #[test] found)."""
    attrs = attributes(lines, code_lines, fn_line)
    for at, text in attrs:
        m = TAG.search(text)
        if m:
            if m.group(1) == cap:
                return "kept"
            lines[at] = TAG.sub(f'#[ignore = "kernel: {cap}"]', lines[at])
            return "changed"
    if any(re.match(r"#\[\s*(ignore\b|cfg_attr\b.*\bignore\b)", t) for _, t in attrs):
        return "other"
    tests = [at for at, text in attrs if re.match(r"#\[\s*(\w+::)*test\b", text)]
    if not tests:
        return None
    indent = re.match(r"\s*", lines[tests[0]]).group(0)
    lines.insert(tests[0] + 1, f'{indent}#[ignore = "kernel: {cap}"]')
    return "added"


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--log", help="read this saved cargo test output instead of running")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()

    log = Path(args.log).read_text() if args.log else run_tests()
    pkgs = Packages()
    dirs = pkgs.dirs
    run = Run(log, pkgs)
    if run.broken:
        print("\n".join(log.splitlines()[-30:]), file=sys.stderr)
        for b in run.broken:
            print("broken log:", b, file=sys.stderr)
        return 1
    failed = run.failed
    problems = still_tagged(run.passed, dirs)
    notes = []
    if not failed:
        print("no failing tests")
        for p in problems:
            print("passes:", p, file=sys.stderr)
        return 1 if problems else 0

    helpers = {}
    edits = defaultdict(list)  # file -> [(line, cap, key)]
    for pkg, root, path, output in failed:
        key = f"{pkg}::{path}"
        if key in UNRELATED:
            problems.append(f"{key}: fails, not for the kernel (UNRELATED)")
            continue
        if pkg is None or pkg not in dirs or root is None:
            problems.append(f"{key}: a doc test or an unknown binary, left alone")
            continue
        where, why = locate(dirs[pkg], root, path)
        if where is None:
            problems.append(f"{key}: {why}")
            continue
        file, line, code = where
        if pkg not in helpers:
            helpers[pkg] = Helpers(dirs[pkg])
        own = body_of(code, line)
        called = helpers[pkg].called(file, own)
        # The helper it failed in, if any, and what that calls.
        m = PANIC.search(output)
        if m:
            site = helpers[pkg].at(ROOT / m.group(1), int(m.group(2)))
            called += "\n" + site + "\n" + helpers[pkg].called(ROOT / m.group(1), site)
        cap, hints = classify(key, file, own, called, output)
        edits[file].append((line, cap, key, hints))

    counts = Counter()
    for file, items in sorted(edits.items()):
        lines = file.read_text().split("\n")
        code_lines = strip_code("\n".join(lines)).split("\n")
        # A test in a file two binaries hold (a `#[path]`, an integration
        # test's shared module) fails in both: tag it once, with the most
        # advanced capability.
        merged = {}
        for line, cap, key, hints in items:
            if line not in merged or RANK[cap] > RANK[merged[line][0]]:
                merged[line] = (cap, key, hints)
        # Bottom up, so inserted lines don't move the ones still to do.
        for line, (cap, key, hints) in sorted(merged.items(), reverse=True):
            done = tag(lines, code_lines, line, cap)
            if done is None:
                problems.append(f"{key}: no #[test] above its fn")
                continue
            if done == "other":
                # Run with --include-ignored: its own reason stays.
                notes.append(f"{key}: has another #[ignore], kept")
                continue
            crate = file.relative_to(ROOT).parts[1]
            counts[(crate, cap)] += 1
            if args.verbose:
                print(f"{done:8} {cap:15} {key}  [{', '.join(hints)}]")
        text = "\n".join(lines)
        if not args.dry_run and text != file.read_text():
            file.write_text(text)

    print(f"{sum(counts.values())} of {len(failed)} failing tests tagged"
          + (" (dry run)" if args.dry_run else ""))
    by_cap = Counter()
    for (crate, cap), n in sorted(counts.items(), key=lambda x: (RANK[x[0][1]], x[0][0])):
        by_cap[cap] += n
        print(f"  kernel: {cap:15} {crate:10} {n}")
    for cap in CAPABILITIES:
        if by_cap[cap]:
            print(f"  kernel: {cap:15} total      {by_cap[cap]}")
    for n in notes:
        print("note:", n, file=sys.stderr)
    for p in problems:
        print("not tagged:", p, file=sys.stderr)
    return 1 if any("UNRELATED" not in p for p in problems) else 0


def still_tagged(passed, dirs):
    """The tests in `passed` that still have a kernel tag, as problems:
    a run with `-- --include-ignored` finds the tags a landed capability
    leaves behind."""
    found = []
    names = defaultdict(list)
    for pkg, root, path in passed:
        if pkg in dirs and root:
            names[path.rsplit("::", 1)[-1].removeprefix("r#")].append((pkg, root, path))
    # The names of the functions tagged: the first after each tag.
    tagged = set()
    files = subprocess.run(["git", "grep", "-l", "kernel: ", "--", "*.rs"], cwd=ROOT,
                           stdout=subprocess.PIPE, text=True, check=False).stdout.split()
    for f in files:
        code = code_of(ROOT / f)
        for m in TAG.finditer(code):
            fn = FN.search(code, m.end())
            if fn:
                tagged.add(fn.group(1).removeprefix("r#"))
    for name in sorted(tagged & names.keys()):
        for pkg, root, path in names[name]:
            where, _ = locate(dirs[pkg], root, path)
            if where is None:
                continue
            file, line, code = where
            attrs = attributes(file.read_text().split("\n"), code.split("\n"), line)
            if any(TAG.search(text) for _, text in attrs):
                found.append(f"{pkg}::{path}: passes but is tagged, in {file.relative_to(ROOT)}")
    return found


if __name__ == "__main__":
    sys.exit(main())
