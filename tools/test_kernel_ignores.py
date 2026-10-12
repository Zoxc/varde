#!/usr/bin/env python3
"""Tests of tools/kernel_ignores.py's reading of the tags. Run by
`tools/ci.sh kernel-ignores`, or `python3 tools/test_kernel_ignores.py`."""

import sys
import textwrap
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from kernel_ignores import CAPABILITIES, report, scan  # noqa: E402


def tags_of(text):
    return scan({"crates/a/src/lib.rs": textwrap.dedent(text)})


class Scan(unittest.TestCase):
    def test_attributes_count_with_their_test(self):
        tags, problems = tags_of("""\
            #[test]
            #[ignore = "kernel: extrude"]
            fn first() {}

            #[ignore = "kernel: curved boolean"]
            #[cfg(unix)]
            #[test]
            pub async fn r#second() {}

            #[test]
            #[ignore=
                "kernel: revolve"]
            fn third<T>() {}
            """)
        self.assertEqual(problems, [])
        self.assertEqual(tags, [("crates/a/src/lib.rs", 2, "extrude", "first"),
                                ("crates/a/src/lib.rs", 5, "curved boolean", "r#second"),
                                ("crates/a/src/lib.rs", 11, "revolve", "third")])

    def test_prose_does_not_count(self):
        tags, problems = tags_of("""\
            //! Tests are `#[ignore = "kernel: <capability>"]` until it lands.
            /// Exact for any kernel: every boolean of the empty solid.
            /* #[ignore = "kernel: extrude"] */
            #[test]
            #[ignore = "not the kernel"]
            fn t() {
                let _ = "kernel: extrude";
                let _ = r#"#[ignore = "kernel: extrude"]"#;
                let _ = r#"ignore = "kernel: extrud""#;
            }
            """)
        self.assertEqual((tags, problems), ([], []))

    def test_unknown_or_malformed_tags_are_problems(self):
        for attr in ['#[ignore = "kernel: extrud"]', '#[ignore = "kernel:extrude"]',
                     '#[ignore = "kernel: extrude, revolve"]',
                     '#[cfg_attr(miri, ignore = "kernel: extrude")]',
                     '#[ignore = r"kernel: extrude"]', '#[ignore = r#"kernel: extrude"#]']:
            with self.subTest(attr=attr):
                tags, problems = tags_of(f"#[test]\n{attr}\nfn t() {{}}\n")
                self.assertEqual(tags, [])
                self.assertEqual(len(problems), 1, problems)

    def test_tags_off_a_test_are_problems(self):
        for text in ['#[ignore = "kernel: extrude"]\nfn helper() {}\n',
                     '#[test]\n#[ignore = "kernel: extrude"]\nmod m {}\n',
                     '#[ignore = "kernel: extrude"]\n#[should_panic]\nfn t() {}\n']:
            with self.subTest(text=text):
                tags, problems = tags_of(text)
                self.assertEqual(tags, [])
                self.assertEqual(len(problems), 1, problems)

    def test_report_lists_every_capability(self):
        lines = report([("crates/app/src/x.rs", 1, "extrude", "a"),
                        ("crates/app/src/y.rs", 1, "extrude", "b"),
                        ("crates/regen/src/z.rs", 1, "extrude", "c")])
        self.assertEqual(lines[0], "kernel-ignores: 3 tests wait on the kernel")
        self.assertEqual(len(lines), 1 + len(CAPABILITIES))
        extrude = lines[1 + CAPABILITIES.index("extrude")]
        self.assertEqual(extrude.split()[:2], ["extrude", "3"])
        self.assertIn("(app 2, regen 1)", extrude)
        self.assertEqual(lines[1 + CAPABILITIES.index("revolve")].split(), ["revolve", "0"])


if __name__ == "__main__":
    unittest.main()
