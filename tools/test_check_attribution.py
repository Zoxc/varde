#!/usr/bin/env python3
"""Tests of tools/check_attribution.py on small repositories made for each
case. Run by `tools/ci.sh attribution`, or `python3 tools/test_check_attribution.py`."""

import pathlib
import subprocess
import sys
import tempfile
import textwrap
import unittest

CHECK = pathlib.Path(__file__).resolve().parent / "check_attribution.py"

HEADER = "| Row | File | Project | Pin | Original file | License |\n|---|---|---|---|---|---|\n"

BAR = "def456" + "0" * 34
FOO = "abc123" + "0" * 34
DIGEST = "f" * 64


def check(files):
    """Runs the check on a fresh git repository holding `files` (path to
    text); returns its exit code and its standard error."""
    with tempfile.TemporaryDirectory() as root:
        root = pathlib.Path(root)
        subprocess.run(["git", "init", "-q"], cwd=root, check=True)
        for name, text in files.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(textwrap.dedent(text), encoding="utf-8")
        run = subprocess.run(
            [sys.executable, str(CHECK), "--root", str(root)],
            capture_output=True,
            text=True,
        )
        return run.returncode, run.stderr


def rs(first):
    """A derived Rust file whose header starts with `first`."""
    return (
        f"// {first}\n"
        "//   licensed under MIT (LICENSES/MIT.txt).\n"
        "// See THIRD_PARTY.md, row L1.\n"
        "fn a() {}\n"
    )


RS = rs(f"Derived from Bar (https://example.org/bar), commit {BAR}, file src/a.c,")
ROW = f"| L1 | a.rs | Bar | {BAR} | src/a.c | MIT |\n"


class CheckAttribution(unittest.TestCase):
    def test_a_matching_row_passes(self):
        code, err = check({"a.rs": RS, "THIRD_PARTY.md": HEADER + ROW})
        self.assertEqual(code, 0, err)

    def test_a_derived_file_without_a_row_fails(self):
        code, err = check({"a.rs": RS, "THIRD_PARTY.md": HEADER})
        self.assertEqual(code, 1, err)
        self.assertIn("no row for it", err)

    def test_a_bare_derived_from_comment_fails(self):
        # A comment merely starting "Derived from" is neither attribution
        # nor ignored.
        for first in [
            "Derived from Bar.",
            "Derived from Bar (https://example.org/bar)",
            f"Derived from Bar (https://example.org/bar), file src/a.c, commit {BAR}",
            "Derived from Bar (https://example.org/bar), commit def456, file src/a.c",
            "Derived from Bar (https://example.org/bar), release 1.2.0, file src/a.c",
            f"Derived from Bar, commit {BAR}, file src/a.c",
            f"Derived from Bar (https://example.org/bar), commit {BAR}, file src/a.c, lines 3-9",
        ]:
            code, err = check({"a.rs": rs(first), "THIRD_PARTY.md": HEADER + ROW})
            self.assertEqual(code, 1, first)
            self.assertIn("isn't an attribution line", err, first)
        # Without a row too.
        code, err = check({"a.rs": rs("Derived from Bar."), "THIRD_PARTY.md": HEADER})
        self.assertEqual(code, 1, err)
        self.assertIn("isn't an attribution line", err)

    def test_the_first_line_must_match_the_row(self):
        # The row's fields must be the first line's, not merely appear
        # somewhere in the block.
        for row in [
            f"| L1 | a.rs | Baz | {BAR} | src/a.c | MIT |\n",
            f"| L1 | a.rs | bar | {BAR} | src/a.c | MIT |\n",
            f"| L1 | a.rs | Bar | {FOO} | src/a.c | MIT |\n",
            f"| L1 | a.rs | Bar | {BAR} | src/b.c | MIT |\n",
            f"| L1 | a.rs | Bar | {BAR} | a.c | MIT |\n",
        ]:
            code, err = check({"a.rs": RS, "THIRD_PARTY.md": HEADER + row})
            self.assertEqual(code, 1, row)
            self.assertIn("names", err, row)
        later = (
            "// Derived from Bar (https://example.org/bar), commit "
            + FOO
            + ", file src/b.c,\n//   from commit "
            + BAR
            + ", file src/a.c.\n// See THIRD_PARTY.md, row L1.\n"
        )
        code, err = check({"a.rs": later, "THIRD_PARTY.md": HEADER + ROW})
        self.assertEqual(code, 1, err)

    def test_an_item_comment_and_a_sha256_pin_pass(self):
        # Mixed origins: one `///` comment per item; a source outside
        # version control is pinned by its file's sha256.
        text = (
            "/// Derived from Bar (https://example.org/bar), commit " + BAR + ", file src/a.c,\n"
            "///   licensed under MIT (LICENSES/MIT.txt). See THIRD_PARTY.md, row L1.\n"
            "fn a() {}\n"
            "\n"
            "/// Derived from Qux's Notes (https://example.org/~qux/notes.html), sha256 "
            + DIGEST
            + ", file notes.c\n"
            "fn b() {}\n"
        )
        rows = ROW + f"| L2 | a.rs | Qux's Notes | sha256 {DIGEST} | notes.c | public domain |\n"
        code, err = check({"a.rs": text, "THIRD_PARTY.md": HEADER + rows})
        self.assertEqual(code, 0, err)

    def test_a_row_names_a_file_of_any_kind(self):
        # A row's file is read whatever its suffix, not only the suffixes
        # scanned for unlisted derived files.
        script = f"""\
            #!/bin/sh
            # Derived from Foo (https://example.org/foo), commit {FOO}, file bin/fetch,
            #   licensed under MIT (LICENSES/MIT.txt).
            """
        code, err = check(
            {
                "tools/fetch.sh": script,
                "tools/foo.cfg": script,
                "THIRD_PARTY.md": HEADER
                + f"| L1 | tools/fetch.sh | Foo | {FOO} | bin/fetch | MIT |\n"
                + f"| L2 | tools/foo.cfg | Foo | {FOO} | bin/fetch | MIT |\n",
            }
        )
        self.assertEqual(code, 0, err)

    def test_empty_cells_fail(self):
        # An empty pin or original file is in every comment, so it would
        # pass unchecked.
        for row in [
            "| L1 | a.rs | Bar |  | src/a.c | MIT |",
            f"| L1 | a.rs | Bar | {BAR} | | MIT |",
            f"| L1 | a.rs | | {BAR} | src/a.c | MIT |",
        ]:
            code, err = check({"a.rs": RS, "THIRD_PARTY.md": HEADER + row + "\n"})
            self.assertEqual(code, 1, row)
            self.assertIn("empty", err, row)

    def test_a_short_row_is_reported(self):
        code, err = check({"a.rs": RS, "THIRD_PARTY.md": HEADER + "| L1 | a.rs | Bar |\n"})
        self.assertEqual(code, 1, err)
        self.assertNotIn("Traceback", err)
        self.assertIn("cells", err)


if __name__ == "__main__":
    unittest.main()
