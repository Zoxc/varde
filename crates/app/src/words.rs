//! Wording shared by the status bar and the MCP replies.

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
    }
}
