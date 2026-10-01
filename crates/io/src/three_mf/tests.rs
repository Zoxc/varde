use glam::DVec3;
use varde_kernel::{Budget, Display, ManifoldMesh, Op, Solid, Tolerance, boolean};

use super::*;

const TOL: Tolerance = Tolerance::DEFAULT;

/// A zip's entries, read back by its central directory, each checked
/// against its local header and CRC.
fn unzip(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16_at = |at: usize| u16::from_le_bytes([zip[at], zip[at + 1]]) as usize;
    let u32_at =
        |at: usize| u32::from_le_bytes([zip[at], zip[at + 1], zip[at + 2], zip[at + 3]]) as usize;
    // No comment: the end record is the last 22 bytes.
    let end = zip.len() - 22;
    assert_eq!(u32_at(end), 0x0605_4b50);
    let (entries, size, offset) = (u16_at(end + 10), u32_at(end + 12), u32_at(end + 16));
    assert_eq!(u16_at(end + 8), entries);
    assert_eq!(offset + size, end);
    let mut out = Vec::new();
    let mut at = offset;
    for _ in 0..entries {
        assert_eq!(u32_at(at), 0x0201_4b50);
        let method = u16_at(at + 10);
        let (crc, packed, unpacked) = (u32_at(at + 16), u32_at(at + 20), u32_at(at + 24));
        let (name_len, extra, comment) = (u16_at(at + 28), u16_at(at + 30), u16_at(at + 32));
        let local = u32_at(at + 42);
        let name = std::str::from_utf8(&zip[at + 46..at + 46 + name_len]).unwrap();
        at += 46 + name_len + extra + comment;

        assert_eq!(u32_at(local), 0x0403_4b50);
        assert_eq!(u16_at(local + 8), method);
        assert_eq!(u32_at(local + 14), crc);
        assert_eq!(u32_at(local + 18), packed);
        assert_eq!(u32_at(local + 22), unpacked);
        let start = local + 30 + u16_at(local + 26) + u16_at(local + 28);
        let data = &zip[start..start + packed];
        let data = match method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(data).unwrap(),
            _ => panic!("method {method}"),
        };
        assert_eq!(data.len(), unpacked);
        assert_eq!(crc32fast::hash(&data), crc as u32);
        out.push((name.to_owned(), data));
    }
    assert_eq!(at, end);
    out
}

/// An object read back from the model: its name and mesh.
struct Read {
    name: String,
    mesh: ManifoldMesh,
}

/// The package read back and checked against the 3MF core rules that
/// matter here: its content types and relationship, the model's
/// namespace and unit, objects of type model with meshes that are
/// manifolds facing out, and a build item for each. Returns the title and
/// the objects.
fn read(zip: &[u8]) -> (String, Vec<Read>) {
    let parts = unzip(zip);
    let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["[Content_Types].xml", "_rels/.rels", MODEL_PATH]);
    let text = |i: usize| std::str::from_utf8(&parts[i].1).unwrap();

    let types = roxmltree::Document::parse(text(0)).unwrap();
    let defaults: Vec<(&str, &str)> = (types.root_element().children())
        .filter(|n| n.has_tag_name("Default"))
        .map(|n| {
            (
                n.attribute("Extension").unwrap(),
                n.attribute("ContentType").unwrap(),
            )
        })
        .collect();
    assert!(defaults.contains(&(
        "model",
        "application/vnd.ms-package.3dmanufacturing-3dmodel+xml"
    )));
    assert!(defaults.contains(&(
        "rels",
        "application/vnd.openxmlformats-package.relationships+xml"
    )));

    let rels = roxmltree::Document::parse(text(1)).unwrap();
    let rel = (rels.root_element().children())
        .find(|n| n.has_tag_name("Relationship"))
        .unwrap();
    assert_eq!(rel.attribute("Target"), Some("/3D/3dmodel.model"));
    assert_eq!(
        rel.attribute("Type"),
        Some("http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel")
    );

    let model = roxmltree::Document::parse(text(2)).unwrap();
    let root = model.root_element();
    assert_eq!(root.tag_name().namespace(), Some(CORE));
    assert_eq!(root.tag_name().name(), "model");
    assert_eq!(root.attribute("unit"), Some("millimeter"));
    let title = (root.children())
        .find(|n| n.has_tag_name((CORE, "metadata")) && n.attribute("name") == Some("Title"))
        .and_then(|n| n.text())
        .unwrap_or_default()
        .to_owned();
    let mut objects = Vec::new();
    let mut ids = Vec::new();
    for object in child(root, "resources")
        .children()
        .filter(|n| n.is_element())
    {
        assert!(object.has_tag_name((CORE, "object")));
        assert_eq!(object.attribute("type"), Some("model"));
        ids.push(object.attribute("id").unwrap().to_owned());
        let mesh = child(object, "mesh");
        let coordinate = |v: roxmltree::Node<'_, '_>, axis: &str| -> f64 {
            v.attribute(axis).unwrap().parse().unwrap()
        };
        let positions: Vec<[f64; 3]> = (child(mesh, "vertices").children())
            .filter(|n| n.is_element())
            .map(|v| ["x", "y", "z"].map(|axis| coordinate(v, axis)))
            .collect();
        let triangles: Vec<[u32; 3]> = (child(mesh, "triangles").children())
            .filter(|n| n.is_element())
            .map(|t| ["v1", "v2", "v3"].map(|v| t.attribute(v).unwrap().parse().unwrap()))
            .collect();
        objects.push(Read {
            name: object.attribute("name").unwrap().to_owned(),
            mesh: ManifoldMesh::new(positions, triangles).unwrap(),
        });
    }
    let items: Vec<String> = (child(root, "build").children())
        .filter(|n| n.is_element())
        .map(|n| n.attribute("objectid").unwrap().to_owned())
        .collect();
    assert_eq!(items, ids);
    (title, objects)
}

/// The first child of `n` tagged `tag` in the core namespace.
fn child<'a, 'input>(n: roxmltree::Node<'a, 'input>, tag: &str) -> roxmltree::Node<'a, 'input> {
    n.children()
        .find(|c| c.has_tag_name((CORE, tag)))
        .unwrap_or_else(|| panic!("no {tag}"))
}

/// A 20 × 12 × 4 block with a hole of radius 3 through it, and a boss
/// on top, at the default tolerance: holes and curved faces.
fn block() -> Solid {
    let block = Solid::cuboid(DVec3::ZERO, DVec3::new(20.0, 12.0, 4.0), 1, &TOL).unwrap();
    let hole = Solid::cylinder(DVec3::new(6.0, 6.0, -1.0), 3.0, 6.0, 2, &TOL).unwrap();
    let boss = Solid::cylinder(DVec3::new(15.0, 6.0, 2.0), 2.5, 5.0, 3, &TOL).unwrap();
    let drilled = boolean(&block, &hole, Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
    boolean(&drilled, &boss, Op::Union, &TOL, &Budget::DEFAULT).unwrap()
}

#[test]
fn bodies_read_back_as_written() {
    let display = Display::new(&TOL);
    let block = block();
    let rod = Solid::cylinder(DVec3::new(-30.0, 0.0, 0.0), 4.0, 25.0, 4, &TOL).unwrap();
    let cube = Solid::cuboid(DVec3::new(0.0, 30.0, 0.0), DVec3::splat(5.0), 5, &TOL).unwrap();
    let solids = [&block, &rod, &cube];
    let meshes: Vec<ManifoldMesh> = (solids.iter())
        .map(|s| s.manifold_mesh(&display).unwrap())
        .collect();
    let names = ["Body 1", "Rod & <\"axle\">", "Cube\u{1}\u{fffe}"];
    let objects: Vec<Object<'_>> = (names.iter().zip(&meshes))
        .map(|(name, mesh)| Object { name, mesh })
        .collect();
    let zip = write("Bracket 'v2'", &objects).unwrap();

    let (title, read) = read(&zip);
    assert_eq!(title, "Bracket 'v2'");
    assert_eq!(read.len(), 3);
    for (((back, solid), mesh), name) in read.iter().zip(solids).zip(&meshes).zip(names) {
        // The same vertices to the bit, and the same triangles.
        assert_eq!(&back.mesh, mesh);
        assert_eq!(back.name, name.replace(['\u{1}', '\u{fffe}'], ""));
        // Facing out, enclosing the solid's volume within the chord over
        // its area.
        let bounds = solid.bounds3().unwrap();
        let chord = display.chord((bounds.max - bounds.min).length());
        let (volume, exact) = (back.mesh.volume(), solid.volume());
        assert!(volume > 0.0);
        assert!(
            (volume - exact).abs() <= chord * solid.area(),
            "{volume} vs {exact}"
        );
    }
    // The block: 960 less the hole, plus the boss above the top.
    let pi = std::f64::consts::PI;
    let expected = 960.0 - pi * 9.0 * 4.0 + pi * 6.25 * 3.0;
    assert!((block.volume() - expected).abs() < 1e-9 * expected);
    assert!((read[0].mesh.volume() - expected).abs() < 0.01 * expected);
}

#[test]
fn small_and_negative_coordinates_read_back() {
    // Corners at tiny, negative and long coordinates.
    let cube = Solid::cuboid(
        DVec3::new(-1.234_567_890_123_456_7e-7, -0.1, 123_456.789_012_345_6),
        DVec3::new(1.0 / 3.0, 0.1, 7.0),
        1,
        &TOL,
    )
    .unwrap();
    let mesh = cube.manifold_mesh(&Display::new(&TOL)).unwrap();
    let zip = write(
        "",
        &[Object {
            name: "",
            mesh: &mesh,
        }],
    )
    .unwrap();
    let (_, read) = read(&zip);
    assert_eq!(read[0].mesh, mesh);
}

#[test]
fn nothing_to_write_is_refused() {
    assert_eq!(write("x", &[]), Err(Error::NoObjects));
}

#[test]
fn coordinates_are_written_within_their_bound() {
    for x in [
        -ManifoldMesh::MAX_POSITION,
        -1_999_999.999_999_999_8,
        -1.234_567_890_123_456_7e-300,
        -9.999_999_999_999_999e-5,
        1e-4,
        0.000_123_456_789_012_345_67,
        -0.0,
        f64::MIN_POSITIVE,
        5e-324,
    ] {
        let mut out = String::new();
        number(&mut out, x);
        assert!(out.len() as u64 <= MAX_NUMBER, "{out}");
        assert_eq!(out.parse::<f64>().unwrap().to_bits(), x.to_bits(), "{out}");
    }
}

#[test]
fn the_model_stays_within_its_bound() {
    let mesh = block().manifold_mesh(&Display::new(&TOL)).unwrap();
    let name = "\"&\"".repeat(10);
    let objects = [Object {
        name: &name,
        mesh: &mesh,
    }; 2];
    let text = model(&name, &objects);
    assert!(text.len() as u64 <= model_bound(&name, &objects).unwrap());
}

/// A mesh crossing to the IO worker is checked again on the way in.
#[test]
fn meshes_are_checked_when_deserialized() {
    let mesh = block().manifold_mesh(&Display::new(&TOL)).unwrap();
    let bytes = postcard::to_stdvec(&mesh).unwrap();
    assert_eq!(postcard::from_bytes::<ManifoldMesh>(&bytes).unwrap(), mesh);
    // The same parts with one triangle turned round.
    let mut triangles = mesh.triangles().to_vec();
    triangles[0].swap(1, 2);
    let bytes = postcard::to_stdvec(&(mesh.positions(), &triangles)).unwrap();
    assert!(postcard::from_bytes::<ManifoldMesh>(&bytes).is_err());
}
