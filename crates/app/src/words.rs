//! Wording shared by the status bar, the MCP replies and the MCP
//! panel: articles, counts.

/// "a" or "an", before `noun` (lowercase): "a face", "an edge".
pub(crate) fn article(noun: &str) -> &'static str {
    match starts_with_vowel(noun) {
        true => "an",
        false => "a",
    }
}

/// The article starting a sentence about a `noun`: "A shell", "An
/// offset".
pub(crate) fn capital_article(noun: &str) -> &'static str {
    match starts_with_vowel(noun) {
        true => "An",
        false => "A",
    }
}

/// `noun` (lowercase) after its article: "a face", "an edge".
#[cfg(not(target_arch = "wasm32"))] // Only the MCP panel and replies, native.
pub(crate) fn with_article(noun: &str) -> String {
    format!("{} {noun}", article(noun))
}

/// "1 face", "3 faces": `count` of `one`, or of `many` if not one.
#[cfg(not(target_arch = "wasm32"))] // Only the MCP panel and replies, native.
pub(crate) fn counted(count: usize, one: &str, many: &str) -> String {
    match count {
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    }
}

/// `text` with its first character lowercased: "Renamed ..." to
/// "renamed ...".
#[cfg(not(target_arch = "wasm32"))] // Only the MCP panel and replies, native.
pub(crate) fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    (chars.next().into_iter())
        .flat_map(char::to_lowercase)
        .chain(chars)
        .collect()
}

fn starts_with_vowel(noun: &str) -> bool {
    noun.starts_with(['a', 'e', 'i', 'o', 'u'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn articles() {
        assert_eq!(article("edge"), "an");
        assert_eq!(article("axis"), "an");
        assert_eq!(article("face"), "a");
        assert_eq!(capital_article("offset"), "An");
        assert_eq!(capital_article("shell"), "A");
        assert_eq!(with_article("edge"), "an edge");
        assert_eq!(with_article("corner"), "a corner");
    }

    #[test]
    fn counts() {
        assert_eq!(counted(1, "face", "faces"), "1 face");
        assert_eq!(counted(0, "face", "faces"), "0 faces");
        assert_eq!(counted(3, "body", "bodies"), "3 bodies");
    }

    #[test]
    fn lowercased() {
        assert_eq!(lowercase_first("Renamed Base"), "renamed Base");
        assert_eq!(lowercase_first(""), "");
    }
}
