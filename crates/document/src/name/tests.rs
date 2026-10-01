use std::path::PathBuf;

use super::*;

#[test]
fn a_design_downloads_as_its_name_with_the_extension() {
    assert_eq!(download_name("bracket"), "bracket.vrdp");
    assert_eq!(download_name("bracket.vrdp"), "bracket.vrdp");
    assert_eq!(download_name("bracket.VRDP"), "bracket.VRDP");
    assert_eq!(download_name("v1.2 bracket"), "v1.2 bracket.vrdp");
    assert_eq!(download_name("ünïcode"), "ünïcode.vrdp");
}

#[test]
fn a_download_name_holds_nothing_a_file_name_cant() {
    assert_eq!(download_name("a/b\\c:d"), "a_b_c_d.vrdp");
    assert_eq!(download_name("what?*<>|\""), "what______.vrdp");
    assert_eq!(download_name("tab\there\n"), "tab_here_.vrdp");
    assert_eq!(download_name("../up"), "_up.vrdp");
    assert_eq!(download_name(" .hidden. "), "hidden.vrdp");
    assert_eq!(download_name(".vrdp"), "vrdp.vrdp");
    assert_eq!(download_name(""), "Untitled.vrdp");
    assert_eq!(download_name(" . "), "Untitled.vrdp");
}

#[test]
fn with_extension_adds_vrdp_unless_there() {
    assert_eq!(
        with_extension("/d/a.vrdp".into()),
        (PathBuf::from("/d/a.vrdp"), true)
    );
    assert_eq!(
        with_extension("/d/a.VRDP".into()),
        (PathBuf::from("/d/a.VRDP"), true)
    );
    assert_eq!(
        with_extension("/d/a.b".into()),
        (PathBuf::from("/d/a.b.vrdp"), false)
    );
    assert_eq!(
        with_extension("/d/a".into()),
        (PathBuf::from("/d/a.vrdp"), false)
    );
    // Like a download's name, the extension alone is a name, not one.
    assert_eq!(
        with_extension("/d/.vrdp".into()),
        (PathBuf::from("/d/.vrdp.vrdp"), false)
    );
}

#[test]
fn an_export_is_named_with_its_own_extension() {
    assert_eq!(download_name_with("bracket", "3mf"), "bracket.3mf");
    assert_eq!(download_name_with("bracket.3MF", "3mf"), "bracket.3MF");
    assert_eq!(
        download_name_with("bracket.vrdp", "3mf"),
        "bracket.vrdp.3mf"
    );
    assert_eq!(download_name_with("a/b", "3mf"), "a_b.3mf");
    assert_eq!(download_name_with("", "3mf"), "Untitled.3mf");
    assert_eq!(
        with_extension_of("/d/a".into(), "3mf"),
        (PathBuf::from("/d/a.3mf"), false)
    );
    assert_eq!(
        with_extension_of("/d/a.3mf".into(), "3mf"),
        (PathBuf::from("/d/a.3mf"), true)
    );
    assert_eq!(
        with_extension_of("/d/a.vrdp".into(), "3mf"),
        (PathBuf::from("/d/a.vrdp.3mf"), false)
    );
}

#[test]
fn a_download_name_is_no_device_on_windows() {
    assert_eq!(download_name("CON"), "CON_.vrdp");
    assert_eq!(download_name("con.tar"), "con.tar_.vrdp");
    assert_eq!(download_name("Nul.vrdp"), "Nul_.vrdp");
    assert_eq!(download_name_with("lpt9 ", "3mf"), "lpt9_.3mf");
    assert_eq!(download_name_with("COM¹", "3mf"), "COM¹_.3mf");
    assert_eq!(download_name("conveyor"), "conveyor.vrdp");
    assert_eq!(download_name("COM10"), "COM10.vrdp");
    assert_eq!(download_name("my con"), "my con.vrdp");
}

#[test]
fn a_long_download_name_is_cut_to_fit() {
    let long = "é".repeat(200);
    for name in [download_name(&long), download_name_with(&long, "3mf")] {
        assert!(name.len() <= 255, "{}", name.len());
        assert!(name.starts_with("éé"));
    }
    assert_eq!(download_name_with(&long, "3mf").len(), 250 + 4);
    // Its own extension kept, a cut that ends in a dot trimmed.
    let dotted = format!("{}.{}.VRDP", "a".repeat(249), "b".repeat(20));
    let name = download_name(&dotted);
    assert_eq!(name, format!("{}.VRDP", "a".repeat(249)));
    assert_eq!(download_name(&"x".repeat(300)).len(), 255);
}
