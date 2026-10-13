//! A combine's note in the status bar. Its panel is in `combine.rs`.

/// A combine's status bar info: its target, tools and operation,
/// "Body 1 with Body 2 · Cut", and whether it keeps its tools.
pub(crate) fn combine_info(
    document: &varde_document::Document,
    combine: &varde_document::Combine,
) -> String {
    let name = |body| {
        document
            .body(body)
            .map_or("a body", |body| body.name.as_str())
    };
    let tools: Vec<&str> = combine.tools.iter().map(|&tool| name(tool)).collect();
    let kept = if combine.keep_tools {
        " · tools kept"
    } else {
        ""
    };
    format!(
        "{} with {} · {}{kept}",
        name(combine.target),
        tools.join(", "),
        combine.op.label()
    )
}
