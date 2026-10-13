//! The crates whose maths must have the same bits natively and on the web
//! each refuse std's and glam's platform maths in their own `clippy.toml`
//! (clippy reads only the nearest one). The lists are copies; this keeps
//! them in step.

use std::collections::BTreeSet;
use std::path::Path;

/// The `path = "..."` values of a `clippy.toml`'s `disallowed-methods`.
fn paths(crate_dir: &str) -> BTreeSet<String> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_dir)
        .join("clippy.toml");
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("reading {}: {e}", file.display()));
    text.lines()
        .filter_map(|line| {
            let rest = line.split_once("path = \"")?.1;
            Some(rest.split_once('"')?.0.to_owned())
        })
        .collect()
}

#[test]
fn platform_maths_lists_agree() {
    let sketch = paths("sketch");
    assert!(!sketch.is_empty(), "no paths in sketch's clippy.toml");
    for other in ["kernel-compat", "view"] {
        let theirs = paths(other);
        assert_eq!(
            sketch.symmetric_difference(&theirs).collect::<Vec<_>>(),
            Vec::<&String>::new(),
            "sketch's and {other}'s clippy.toml list different methods"
        );
    }
}
