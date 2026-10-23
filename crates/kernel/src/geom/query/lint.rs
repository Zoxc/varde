//! The lint keeping the non-decision queries out of decision code: no
//! source file of the crate outside the queries' own module and the
//! callers allowed ([`ALLOWED`]: the measures, and finding named entities
//! again) refers to the module. A reference is any path through it
//! (`query::…`), a path naming it (`geom::query`), or a `use` declaration
//! naming `query` at all; comments, strings and character literals are
//! skipped. So the module can't be reached from elsewhere by an import, a
//! path or a re-export, and an unrelated item named `query` is refused
//! only in a path or an import (rename it there). Other crates can't reach
//! the module's items, which are the crate's own.

use std::path::{Path, PathBuf};

/// The source paths, relative to the crate's `src` and with `/`
/// separators, that may refer to the queries: a file, or a directory when
/// it ends in `/`.
const ALLOWED: &[&str] = &[
    "geom/query.rs",
    "geom/query/",
    "measure.rs",
    "measure/",
    "names/resolve.rs",
    "names/resolve/",
];

/// Whether the file at `rel` (relative to `src`, `/` separators) may
/// refer to the queries.
fn allowed(rel: &str) -> bool {
    ALLOWED.iter().any(|a| {
        if a.ends_with('/') {
            rel.starts_with(a)
        } else {
            rel == *a
        }
    })
}

/// `src` with its comments, string literals and character literals
/// blanked out (each character but a newline turned into a space), so
/// lines keep their numbers.
fn code_only(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    while i < c.len() {
        let rest = &c[i..];
        if rest.starts_with(&['/', '/']) {
            while i < c.len() && c[i] != '\n' {
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if rest.starts_with(&['/', '*']) {
            // Block comments nest.
            let mut depth = 0usize;
            while i < c.len() {
                if c[i..].starts_with(&['/', '*']) {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if c[i..].starts_with(&['*', '/']) {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
        } else if let Some(n) = raw_string(rest) {
            for &ch in &rest[..n] {
                blank(&mut out, ch);
            }
            i += n;
        } else if c[i] == '"' || (c[i] == 'b' && rest.get(1) == Some(&'"') && !ident_before(&c, i))
        {
            // A string, a byte string: up to the closing quote, escapes
            // skipped.
            let start = i;
            i += if c[i] == 'b' { 2 } else { 1 };
            while i < c.len() && c[i] != '"' {
                i += if c[i] == '\\' { 2 } else { 1 };
            }
            i = (i + 1).min(c.len());
            for &ch in &c[start..i] {
                blank(&mut out, ch);
            }
        } else if c[i] == '\'' {
            // A character literal (`'x'`, `'\n'`, `'\u{..}'`), not a
            // lifetime or a label.
            let len = if rest.get(1) == Some(&'\\') {
                rest.iter()
                    .skip(2)
                    .position(|&ch| ch == '\'')
                    .map(|p| p + 3)
            } else if rest.get(2) == Some(&'\'') {
                Some(3)
            } else {
                None
            };
            match len {
                Some(n) => {
                    for &ch in &rest[..n] {
                        blank(&mut out, ch);
                    }
                    i += n;
                }
                None => {
                    out.push('\'');
                    i += 1;
                }
            }
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// Whether the character before `i` continues an identifier.
fn ident_before(c: &[char], i: usize) -> bool {
    i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_')
}

/// The length of the raw string (`r"…"`, `r#"…"#`, `br"…"`) `rest` starts
/// with, if it starts with one.
fn raw_string(rest: &[char]) -> Option<usize> {
    let mut i = match rest {
        ['r', ..] => 1,
        ['b', 'r', ..] => 2,
        _ => return None,
    };
    let hashes = rest[i..].iter().take_while(|&&ch| ch == '#').count();
    i += hashes;
    if rest.get(i) != Some(&'"') {
        return None;
    }
    i += 1;
    while i < rest.len() {
        if rest[i] == '"'
            && rest[i + 1..]
                .iter()
                .take(hashes)
                .filter(|&&ch| ch == '#')
                .count()
                == hashes
        {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    Some(rest.len())
}

/// The identifiers of `code` (comments and literals blanked) with their
/// byte offsets.
fn idents(code: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (k, ch) in code
        .char_indices()
        .chain(std::iter::once((code.len(), ' ')))
    {
        let part = ch.is_alphanumeric() || ch == '_';
        match (start, part) {
            (None, true) => start = Some(k),
            (Some(s), false) => {
                out.push((s, &code[s..k]));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The 1-based lines of `src` that refer to the queries' module (module
/// docs).
fn references(src: &str) -> Vec<usize> {
    let code = code_only(src);
    let line = |at: usize| code[..at].matches('\n').count() + 1;
    let ids = idents(&code);
    let mut lines = Vec::new();
    // Paths: `query::…` and `geom::query`.
    for (k, &(at, id)) in ids.iter().enumerate() {
        if id != "query" {
            continue;
        }
        let after = code[at + id.len()..].trim_start();
        let before = code[..at].trim_end();
        let through = after.starts_with("::");
        let named = before.ends_with("::") && k > 0 && ids[k - 1].1 == "geom";
        if through || named {
            lines.push(line(at));
        }
    }
    // `use` declarations naming `query`.
    for (k, &(at, id)) in ids.iter().enumerate() {
        if id != "use" {
            continue;
        }
        let end = code[at..].find(';').map_or(code.len(), |e| at + e);
        if ids[k + 1..]
            .iter()
            .take_while(|&&(a, _)| a < end)
            .any(|&(_, x)| x == "query")
        {
            lines.push(line(at));
        }
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// The Rust files under `dir`, sorted.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("a readable source directory")
        .map(|e| e.expect("a directory entry").path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn only_the_measures_refer_to_the_queries() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 20, "the crate's sources: {}", files.len());
    let mut found = Vec::new();
    let mut seen_self = false;
    for f in &files {
        let rel = f
            .strip_prefix(&src)
            .expect("under src")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let text = std::fs::read_to_string(f).expect("a readable source file");
        seen_self |= rel == "geom/query.rs";
        if allowed(&rel) {
            continue;
        }
        for l in references(&text) {
            found.push(format!("src/{rel}:{l}"));
        }
    }
    assert!(seen_self, "the lint found the queries' own module");
    assert!(
        found.is_empty(),
        "decision code refers to the non-decision queries (geom::query), \
         which only the measures may read:\n{}",
        found.join("\n")
    );
}

#[test]
fn references_are_found_in_paths_and_imports_only() {
    let refs = |s: &str| references(s);
    assert_eq!(refs("use crate::geom::query;"), [1]);
    assert_eq!(refs("use crate::geom::query::winding;"), [1]);
    assert_eq!(refs("use super::{\n    tile,\n    query as q,\n};"), [1]);
    assert_eq!(
        refs("use crate::geom::*;\nfn f() { query::winding(); }"),
        [2]
    );
    assert_eq!(refs("let m = crate::geom::query ::winding(a);"), [1]);
    assert_eq!(refs("let m = crate::geom :: query;"), [1]);
    assert_eq!(refs("pub use query::Membership;"), [1]);
    // Not references: a variable, a field, a function of another module,
    // and anything in comments or literals.
    assert!(refs("let query = 1; s.query(q); calibration::query(&[], p, 0);").is_empty());
    assert!(refs("// use crate::geom::query;\n/* query::x /* nested */ query::y */").is_empty());
    assert!(refs(r#"let s = "query::winding"; let t = r"geom::query";"#).is_empty());
    assert!(refs("let s = r#\"query::\"#; let c = ':'; let l: &'a str;").is_empty());
    assert!(refs("let b = b\"use query;\";").is_empty());
    // Lines keep their numbers past comments and literals.
    assert_eq!(refs("/* a\n b */\nlet s = \"x\ny\";\nquery::f();"), [5]);
}

#[test]
fn the_allowed_paths_are_the_measures_and_resolve() {
    assert!(allowed("geom/query.rs"));
    assert!(allowed("geom/query/tests.rs"));
    assert!(allowed("measure/mass.rs"));
    assert!(allowed("names/resolve.rs"));
    assert!(!allowed("geom/query_more.rs"));
    assert!(!allowed("geom.rs"));
    assert!(!allowed("boolean/snap.rs"));
    assert!(!allowed("measurement.rs"));
}
