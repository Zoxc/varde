//! Writing bodies as a 3MF package, for printing.
//!
//! A 3MF file is an OPC package: a zip holding `[Content_Types].xml`,
//! `_rels/.rels`, which points at the model, and the model,
//! `3D/3dmodel.model`, in millimetres, with one `<object type="model">`
//! per body, its mesh's vertices and triangles, and a build item placing
//! each. The meshes are [`ManifoldMesh`]es, checked to be closed, oriented
//! manifolds, as the 3MF core specification asks of a model object.
//! Vertices are about the mesh's whole-numbered origin, which the build
//! item's transform moves them by, and are `f32` values, so readers that
//! keep single precision (most do) read the mesh that was checked.
//! Coordinates are written so they read back as the same `f64`s.
//!
//! The zip is written here (its parts deflated by `miniz_oxide`): local
//! headers, the central directory and its end record, no zip64, so the
//! package and each part must stay under 4 GiB, which [`write()`] checks
//! before it builds anything.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use varde_kernel::ManifoldMesh;

/// The extension of a 3MF file, without the dot.
pub const EXTENSION: &str = "3mf";

/// One body to write: its name and its mesh, in millimetres.
#[derive(Debug, Clone, Copy)]
pub struct Object<'a> {
    pub name: &'a str,
    pub mesh: &'a ManifoldMesh,
}

/// A body to write, owned, as [`Request::Export`](crate::Request::Export)
/// carries it to the lane: its mesh is checked again as it's decoded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub name: String,
    pub mesh: ManifoldMesh,
}

impl Body {
    pub fn object(&self) -> Object<'_> {
        Object {
            name: &self.name,
            mesh: &self.mesh,
        }
    }
}

/// [`write()`] of `bodies`.
pub fn package(title: &str, bodies: &[Body]) -> Result<Vec<u8>, Error> {
    let objects: Vec<Object<'_>> = bodies.iter().map(Body::object).collect();
    write(title, &objects)
}

/// Why no package was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// There is nothing to write.
    NoObjects,
    /// The package would be past what a zip without zip64 holds.
    TooLarge,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::NoObjects => "there are no visible bodies to export",
            Error::TooLarge => "the bodies' meshes are too large for a 3MF file",
        })
    }
}

impl std::error::Error for Error {}

/// The model part's path in the package.
pub const MODEL_PATH: &str = "3D/3dmodel.model";

/// The 3MF core namespace.
const CORE: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;

/// The most bytes the model may take: what a zip entry's size field
/// holds, less room for the other parts and the zip's own records.
const MAX_MODEL: u64 = u32::MAX as u64 - (1 << 16);

/// The most bytes a coordinate takes as [`number`] writes it: a sign, up
/// to 7 digits before the point (positions are within
/// [`ManifoldMesh::MAX_POSITION`]) and 17 significant digits, or a
/// mantissa of 17 digits and an exponent.
const MAX_NUMBER: u64 = 32;
/// The most bytes of one vertex line, without its numbers.
const VERTEX: u64 = 32;
/// The most bytes of one triangle line: three indices of up to 10 digits.
const TRIANGLE: u64 = 40 + 3 * 10;
/// The most bytes of one object's lines other than its vertices and
/// triangles, and of its build item, without its name and the numbers of
/// its transform.
const OBJECT: u64 = 256;
/// The most bytes of the model's lines outside the objects, without the
/// title.
const MODEL: u64 = 1024;

/// The package of `objects`, titled `title` (the design's name). Fails
/// with [`Error::NoObjects`] for none, and with [`Error::TooLarge`] if
/// the model could be past what the zip holds, worked out from the
/// meshes' sizes before anything is written.
pub fn write(title: &str, objects: &[Object<'_>]) -> Result<Vec<u8>, Error> {
    if objects.is_empty() {
        return Err(Error::NoObjects);
    }
    let bound = model_bound(title, objects).ok_or(Error::TooLarge)?;
    if bound > MAX_MODEL {
        return Err(Error::TooLarge);
    }
    let model = model(title, objects);
    debug_assert!(model.len() as u64 <= bound);
    let mut zip = Zip::default();
    zip.add("[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
    zip.add("_rels/.rels", RELS.as_bytes())?;
    zip.add(MODEL_PATH, model.as_bytes())?;
    zip.finish()
}

/// The most bytes the model of `objects` takes, or `None` past `u64`.
fn model_bound(title: &str, objects: &[Object<'_>]) -> Option<u64> {
    let text = |s: &str| (s.len() as u64).checked_mul(6);
    let mut total = MODEL.checked_add(text(title)?)?;
    for object in objects {
        let vertices = (object.mesh.positions().len() as u64)
            .checked_mul(VERTEX.checked_add(3 * MAX_NUMBER)?)?;
        let triangles = (object.mesh.triangles().len() as u64).checked_mul(TRIANGLE)?;
        total = total
            .checked_add(OBJECT)?
            .checked_add(3 * MAX_NUMBER)?
            .checked_add(text(object.name)?)?
            .checked_add(vertices)?
            .checked_add(triangles)?;
    }
    Some(total)
}

/// The model part: see the module's docs.
fn model(title: &str, objects: &[Object<'_>]) -> String {
    let mut out = String::new();
    // Writing to a `String` doesn't fail.
    let _ = write!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"{CORE}\">\n \
         <metadata name=\"Title\">{}</metadata>\n \
         <metadata name=\"Application\">Varde</metadata>\n \
         <resources>\n",
        escaped(title)
    );
    for (i, object) in objects.iter().enumerate() {
        let _ = write!(
            out,
            "  <object id=\"{}\" type=\"model\" name=\"{}\">\n   <mesh>\n    <vertices>\n",
            i + 1,
            escaped(object.name)
        );
        for p in object.mesh.positions() {
            out.push_str("     <vertex x=\"");
            number(&mut out, p[0]);
            out.push_str("\" y=\"");
            number(&mut out, p[1]);
            out.push_str("\" z=\"");
            number(&mut out, p[2]);
            out.push_str("\"/>\n");
        }
        out.push_str("    </vertices>\n    <triangles>\n");
        for [a, b, c] in object.mesh.triangles() {
            let _ = writeln!(out, "     <triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c}\"/>");
        }
        out.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    out.push_str(" </resources>\n <build>\n");
    for (i, object) in objects.iter().enumerate() {
        let _ = write!(out, "  <item objectid=\"{}\"", i + 1);
        let origin = object.mesh.origin();
        if origin != [0.0; 3] {
            out.push_str(" transform=\"1 0 0 0 1 0 0 0 1");
            for x in origin {
                out.push(' ');
                number(&mut out, x);
            }
            out.push('"');
        }
        out.push_str("/>\n");
    }
    out.push_str(" </build>\n</model>\n");
    out
}

/// Appends `x` as a decimal that reads back as the same `f64`: Rust's
/// shortest round-trip digits, with an exponent for small magnitudes,
/// which would otherwise be written with many zeros. Both forms are the
/// 3MF schema's `ST_Number`.
fn number(out: &mut String, x: f64) {
    // `ManifoldMesh` positions are finite.
    if x != 0.0 && x.abs() < 1e-4 {
        let _ = write!(out, "{x:e}");
    } else {
        let _ = write!(out, "{x}");
    }
}

/// `s` as XML text or an attribute value: the markup characters as
/// entities, and characters XML 1.0 doesn't allow left out.
fn escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c < ' ' || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

/// A zip being written: its entries' data and local headers, and their
/// central directory records.
#[derive(Default)]
struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    entries: u16,
}

/// The fixed DOS time and date entries get: midnight, 1 January 1980.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = (1 << 5) | 1;
/// Version 2.0, which deflate needs.
const VERSION: u16 = 20;
const DEFLATE: u16 = 8;

impl Zip {
    /// Appends the entry `name` (ASCII) holding `data`, deflated.
    fn add(&mut self, name: &str, data: &[u8]) -> Result<(), Error> {
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let crc = crc32fast::hash(data);
        let size = field(data.len())?;
        let packed_size = field(packed.len())?;
        let offset = field(self.out.len())?;
        let name_len = u16::try_from(name.len()).map_err(|_| Error::TooLarge)?;
        self.entries = (self.entries.checked_add(1))
            .filter(|&n| n < u16::MAX)
            .ok_or(Error::TooLarge)?;

        let out = &mut self.out;
        put32(out, 0x0403_4b50);
        put16(out, VERSION);
        put16(out, 0); // flags
        put16(out, DEFLATE);
        put16(out, DOS_TIME);
        put16(out, DOS_DATE);
        put32(out, crc);
        put32(out, packed_size);
        put32(out, size);
        put16(out, name_len);
        put16(out, 0); // extra field
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed);

        let central = &mut self.central;
        put32(central, 0x0201_4b50);
        put16(central, VERSION); // made by
        put16(central, VERSION); // needed
        put16(central, 0); // flags
        put16(central, DEFLATE);
        put16(central, DOS_TIME);
        put16(central, DOS_DATE);
        put32(central, crc);
        put32(central, packed_size);
        put32(central, size);
        put16(central, name_len);
        put16(central, 0); // extra field
        put16(central, 0); // comment
        put16(central, 0); // disk
        put16(central, 0); // internal attributes
        put32(central, 0); // external attributes
        put32(central, offset);
        central.extend_from_slice(name.as_bytes());
        Ok(())
    }

    /// The zip: the entries, the central directory and its end record.
    fn finish(mut self) -> Result<Vec<u8>, Error> {
        let offset = field(self.out.len())?;
        let size = field(self.central.len())?;
        let end = self.out.len().checked_add(self.central.len());
        field(end.ok_or(Error::TooLarge)?)?;
        self.out.append(&mut self.central);
        let out = &mut self.out;
        put32(out, 0x0605_4b50);
        put16(out, 0); // this disk
        put16(out, 0); // the central directory's disk
        put16(out, self.entries);
        put16(out, self.entries);
        put32(out, size);
        put32(out, offset);
        put16(out, 0); // comment
        Ok(self.out)
    }
}

/// `n` as a size or offset field of a zip without zip64: below
/// `u32::MAX`, which says the real value is in a zip64 record.
fn field(n: usize) -> Result<u32, Error> {
    u32::try_from(n)
        .ok()
        .filter(|&n| n < u32::MAX)
        .ok_or(Error::TooLarge)
}

fn put16(out: &mut Vec<u8>, x: u16) {
    out.extend_from_slice(&x.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, x: u32) {
    out.extend_from_slice(&x.to_le_bytes());
}

#[cfg(test)]
mod tests;
