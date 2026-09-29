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
