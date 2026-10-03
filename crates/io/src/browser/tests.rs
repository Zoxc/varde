use super::*;

#[test]
fn design_names_are_what_download_names_make() {
    for name in [
        "a.vrdp",
        "bracket (2).vrdp",
        "x.VRDP",
        "ünïcode.vrdp",
        "a.b.vrdp",
    ] {
        assert!(is_design_name(name), "{name}");
    }
    for name in [
        "",
        ".vrdp",
        "a",
        "a.txt",
        ".a.vrdp",
        ".a.vrdp.autosave",
        ".a.vrdp.1f.tmp",
        "a/b.vrdp",
        "a\\b.vrdp",
        "a\0.vrdp",
        "downloads.toml",
    ] {
        assert!(!is_design_name(name), "{name:?}");
    }
    for typed in ["bracket", "bracket.vrdp", "..bracket", "a/b", "", "CON"] {
        let name = file_name(typed);
        assert!(is_design_name(&name), "{typed:?} -> {name:?}");
    }
    assert_eq!(file_name("bracket"), "bracket.vrdp");
    assert_eq!(file_name("bracket.vrdp"), "bracket.vrdp");
    assert_eq!(file_name("a/b"), "a_b.vrdp");
    assert_eq!(file_name(""), "Untitled.vrdp");
}
