//! Counting and listing things in a sentence.

/// `names` as a list in a sentence: "A", "A and B", "A, B and C".
pub(crate) fn listed<'a>(names: impl ExactSizeIterator<Item = &'a str>) -> String {
    let count = names.len();
    let mut list = String::new();
    for (at, name) in names.enumerate() {
        if at > 0 {
            list.push_str(if at + 1 == count { " and " } else { ", " });
        }
        list.push_str(name);
    }
    list
}

/// `n` and what it counts, `one` or `many` of them: "1 line", "2 lines".
pub(crate) fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What a status bar says of the regions an extrude or a revolve has
/// picked: "2 regions picked", or "pick the regions to `verb`".
pub(crate) fn regions_picked(n: usize, verb: &str) -> String {
    match n {
        0 => format!("pick the regions to {verb}"),
        n => format!("{} picked", counted(n, "region", "regions")),
    }
}

/// Each of `kinds` there is, [`counted`] by its `(n, one, many)`, in
/// order: "2 lines", "1 point".
pub(crate) fn counts<const N: usize>(kinds: [(usize, &str, &str); N]) -> Vec<String> {
    (kinds.into_iter())
        .filter(|&(n, ..)| n > 0)
        .map(|(n, one, many)| counted(n, one, many))
        .collect()
}
