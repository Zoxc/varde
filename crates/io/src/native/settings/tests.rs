use super::*;
use crate::settings::Theme;
use crate::tests::TempDir;

#[test]
fn write_and_load_through_a_file() {
    let dir = TempDir::new("settings");
    let store = dir.0.join("sub").join("settings.toml");
    assert_eq!(load(&store), Settings::default());
    let settings = Settings { theme: Theme::Dark };
    write(&store, &settings).unwrap();
    assert_eq!(load(&store), settings);
    // Written again over the first.
    write(&store, &Settings::default()).unwrap();
    assert_eq!(load(&store), Settings::default());
}

/// A file too large to be settings is taken as gone bad.
#[test]
fn a_huge_file_gives_the_defaults() {
    let dir = TempDir::new("settings-huge");
    let store = dir.0.join("settings.toml");
    let padding = " ".repeat(usize::try_from(MAX_BYTES).unwrap());
    std::fs::write(&store, format!("theme = \"dark\"\n{padding}")).unwrap();
    assert_eq!(load(&store), Settings::default());
    std::fs::write(&store, "theme = \"dark\"\n").unwrap();
    assert_eq!(load(&store).theme, Theme::Dark);
}
