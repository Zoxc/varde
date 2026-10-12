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


RS = """\
    // Derived from Bar (https://example.org/bar), commit def456, file src/a.c,
    //   licensed under MIT (LICENSES/MIT.txt).
    // See THIRD_PARTY.md, row L1.
    fn a() {}
    """


class CheckAttribution(unittest.TestCase):
    def test_a_matching_row_passes(self):
        row = "| L1 | a.rs | Bar | def456 | src/a.c | MIT |\n"
        code, err = check({"a.rs": RS, "THIRD_PARTY.md": HEADER + row})
        self.assertEqual(code, 0, err)

    def test_a_derived_file_without_a_row_fails(self):
        code, err = check({"a.rs": RS, "THIRD_PARTY.md": HEADER})
        self.assertEqual(code, 1, err)
        self.assertIn("no row for it", err)

    def test_a_row_names_a_file_of_any_kind(self):
        # A row's file is read whatever its suffix, not only the suffixes
        # scanned for unlisted derived files.
        script = """\
            #!/bin/sh
            # Derived from Foo (https://example.org/foo), commit abc123, file bin/fetch,
            #   licensed under MIT (LICENSES/MIT.txt).
            """
        code, err = check(
            {
                "tools/fetch.sh": script,
                "tools/foo.cfg": script,
                "THIRD_PARTY.md": HEADER
                + "| L1 | tools/fetch.sh | Foo | abc123 | bin/fetch | MIT |\n"
                + "| L2 | tools/foo.cfg | Foo | abc123 | bin/fetch | MIT |\n",
            }
        )
        self.assertEqual(code, 0, err)

    def test_empty_cells_fail(self):
        # An empty pin or original file is in every comment, so it would
        # pass unchecked.
        for row in [
            "| L1 | a.rs | Bar |  | src/a.c | MIT |",
            "| L1 | a.rs | Bar | def456 | | MIT |",
            "| L1 | a.rs | | def456 | src/a.c | MIT |",
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
