use glam::DVec3;
use varde_kernel::{Display, Solid, Tolerance};
use varde_lane::thread::testing::next;

use super::*;
use crate::native::thread::spawn_at;
use crate::tests::TempDir;
use crate::{Chosen, Request, Response, SaveTo, Stores, Transport};

/// A 10 mm cube, welded.
fn cube() -> Vec<Body> {
    let tolerance = Tolerance::DEFAULT;
    let solid = Solid::cuboid(DVec3::ZERO, DVec3::splat(10.0), 1, &tolerance).unwrap();
    let mesh = solid.manifold_mesh(&Display::new(&tolerance)).unwrap();
    vec![Body {
        name: "Body 1".to_owned(),
        mesh,
    }]
}

/// The files in `dir`, by name, sorted.
fn listed(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_new_file_holds_the_package() {
    let dir = TempDir::new("export-new");
    let path = dir.0.join("plate.3mf");
    let bodies = cube();
    export(&path, false, "Plate", &bodies).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        three_mf::package("Plate", &bodies).unwrap()
    );
    assert_eq!(listed(&dir.0), ["plate.3mf"]);
}

#[test]
fn a_file_there_is_only_replaced_if_the_user_agreed() {
    let dir = TempDir::new("export-replace");
    let path = dir.0.join("plate.3mf");
    std::fs::write(&path, b"old").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let bodies = cube();
    assert_eq!(
        export(&path, false, "Plate", &bodies),
        Err("plate.3mf already exists".to_owned())
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    export(&path, true, "Plate", &bodies).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        three_mf::package("Plate", &bodies).unwrap()
    );
    // Nothing left next to it, and its permissions kept.
    assert_eq!(listed(&dir.0), ["plate.3mf"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_stays_and_its_target_is_written() {
    let dir = TempDir::new("export-link");
    let target = dir.0.join("target.3mf");
    std::fs::write(&target, b"old").unwrap();
    let link = dir.0.join("link.3mf");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let bodies = cube();
    export(&link, true, "Plate", &bodies).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(
        std::fs::read(&target).unwrap(),
        three_mf::package("Plate", &bodies).unwrap()
    );
}

#[test]
fn nothing_to_write_writes_nothing() {
    let dir = TempDir::new("export-none");
    let path = dir.0.join("plate.3mf");
    assert_eq!(
        export(&path, false, "Plate", &[]),
        Err("there are no visible bodies to export".to_owned())
    );
    let missing = dir.0.join("missing").join("plate.3mf");
    let error = export(&missing, false, "Plate", &cube()).unwrap_err();
    assert!(error.starts_with("couldn't write plate.3mf: "), "{error}");
    assert!(listed(&dir.0).is_empty());
}

#[test]
fn the_lane_writes_an_export_and_answers_with_its_path() {
    let dir = TempDir::new("export-lane");
    let (mut lane, mut responses) = spawn_at(Stores::default());
    let path = dir.0.join("plate.3mf");
    let bodies = cube();
    lane.send(Request::Export {
        to: SaveTo::Path {
            path: path.clone(),
            overwrite: false,
        },
        title: "Plate".to_owned(),
        bodies: bodies.clone(),
    });
    let Response::Exported { to, result } = next(&mut responses) else {
        panic!("not an export's answer");
    };
    assert_eq!(result, Ok(()));
    assert_eq!(to, Chosen::Path(path.clone()));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        three_mf::package("Plate", &bodies).unwrap()
    );
}
