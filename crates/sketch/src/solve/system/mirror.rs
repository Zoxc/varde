//! Which equations a mirror image restates: [`Constraint::Symmetric`]
//! pairs make an arc's radius equation, or a fillet's or a chamfer's, one
//! its image's imply, which the analysis would otherwise call redundant.

use std::collections::{HashMap, HashSet};

use crate::{Constraint, Curve, CurveEntry, Id, Sketch};

/// The arcs and corners a mirror image restates, from [`mirrored`].
pub(super) struct Mirrored {
    /// The arcs whose radius equation is a mirror image's, see
    /// [`mirrored_arcs`].
    pub arcs: HashSet<Id>,
    /// The corners whose equations are implied by a mirror image's, see
    /// [`mirrored_corners`].
    pub corners: HashMap<Id, (Id, Vec<Id>)>,
}

/// The arcs and corners of `sketch` a mirror image restates.
pub(super) fn mirrored(sketch: &Sketch) -> Mirrored {
    let images = images(sketch);
    Mirrored {
        arcs: mirrored_arcs(sketch, &images),
        corners: mirrored_corners(sketch, &images),
    }
}

/// Each point's mirror images: the points [`Constraint::Symmetric`] with
/// it, and the line each is about.
type Images = HashMap<Id, Vec<(Id, Id)>>;

/// The mirror images of the points of `sketch`, see [`Images`].
fn images(sketch: &Sketch) -> Images {
    let mut images: Images = HashMap::new();
    for entry in &sketch.constraints {
        if let Constraint::Symmetric { a, b, about } = entry.constraint {
            images.entry(a).or_default().push((b, about));
            images.entry(b).or_default().push((a, about));
        }
    }
    images
}

/// Whether the point `b` is the mirror image of `a` in the line `about`.
fn mirror(images: &Images, a: Id, b: Id, about: Id) -> bool {
    images
        .get(&a)
        .is_some_and(|images| images.contains(&(b, about)))
}

/// Whether `copy` is the mirror image of `other`, point for point by
/// `same`: a line's ends each the image of the other's same end, an arc's
/// centre the other's centre's and each end the other's other end, as a
/// mirror image runs the other way round.
fn image_of(copy: &Curve, other: &Curve, same: impl Fn(Id, Id) -> bool) -> bool {
    counterparts(copy, other).is_some_and(|pairs| pairs.into_iter().all(|(a, b)| same(a, b)))
}

/// The points of `copy` with those of `other` each is to be the image of,
/// see [`image_of`]: `None` for curves of different kinds, or circles.
fn counterparts(copy: &Curve, other: &Curve) -> Option<Vec<(Id, Id)>> {
    match (copy, other) {
        (
            &Curve::Arc { center, start, end },
            &Curve::Arc {
                center: c,
                start: s,
                end: e,
            },
        ) => Some(vec![(center, c), (start, e), (end, s)]),
        (&Curve::Line { start, end }, &Curve::Line { start: s, end: e }) => {
            Some(vec![(start, s), (end, e)])
        }
        _ => None,
    }
}

/// The arcs of `sketch` whose radius equation follows from another's:
/// each of its points is held [`Constraint::Symmetric`] about one line
/// with a point of an arc with a lower id (see [`image_of`]), as
/// [`SketchEdit::Mirror`](crate::SketchEdit::Mirror) makes them. A mirror
/// image keeps distances, so with those its equation restates the
/// other's, and the analysis would find the two redundant: it's left out.
/// The other's may be left out too for a third arc's, whose id is lower
/// again, so one of them always keeps its own.
fn mirrored_arcs(sketch: &Sketch, images: &Images) -> HashSet<Id> {
    let mut mirrored = HashSet::new();
    if images.is_empty() {
        return mirrored;
    }
    let mut arcs: HashMap<Id, Vec<&CurveEntry>> = HashMap::new();
    for entry in &sketch.curves {
        if let Curve::Arc { center, .. } = entry.curve {
            arcs.entry(center).or_default().push(entry);
        }
    }
    for entry in &sketch.curves {
        let Curve::Arc { center, .. } = entry.curve else {
            continue;
        };
        let mut centers = images.get(&center).into_iter().flatten();
        let found = centers.any(|&(other_center, about)| {
            let others = arcs.get(&other_center).into_iter().flatten();
            others.into_iter().any(|other| {
                other.id < entry.id
                    && image_of(&entry.curve, &other.curve, |a, b| {
                        mirror(images, a, b, about)
                    })
            })
        });
        if found {
            mirrored.insert(entry.id);
        }
    }
    mirrored
}

/// The fillets and chamfers of `sketch` whose corner's equations follow
/// from another's: the mirror image in one line, `equal` as it is, as
/// [`SketchEdit::Mirror`](crate::SketchEdit::Mirror) copies one with its
/// lines, of one with a lower id. Their points are each held
/// [`Constraint::Symmetric`] about the line with the other's (see
/// [`image_of`]) or are the same point, on the line, and so are its lines'
/// ends and its corner's; a line on the mirror line may be its own image.
/// Its equations would restate the other's, so they're left out, as a
/// mirrored arc's radius is ([`mirrored_arcs`]). But only where the points
/// shared, their own images, are on the line, which nothing else need
/// hold: each is given, by the line mirrored about, as the points to hold
/// on it in their place (see [`System::implied`](super::System::implied)).
fn mirrored_corners(sketch: &Sketch, images: &Images) -> HashMap<Id, (Id, Vec<Id>)> {
    let mut mirrored = HashMap::new();
    if images.is_empty() {
        return mirrored;
    }
    // The same point, or its image in `about`.
    let same = |about: Id| move |a: Id, b: Id| a == b || mirror(images, a, b, about);
    let curve = |id: Id| sketch.curve(id).map(|entry| &entry.curve);
    // The points of the line `a` and those of `b` each is the image of:
    // its own, if it's its own image.
    let line_pairs = |a: Id, b: Id| {
        let (a, b) = (curve(a)?, curve(b)?);
        match a.ends() {
            Some([start, end]) if a == b => Some(vec![(start, start), (end, end)]),
            _ => counterparts(a, b),
        }
    };
    // The corners by their ends, to find the other from an image.
    let corners = || sketch.curves.iter().filter(|entry| entry.corner.is_some());
    let mut by_end: HashMap<Id, Vec<&CurveEntry>> = HashMap::new();
    for entry in corners() {
        for end in entry.curve.ends().into_iter().flatten() {
            by_end.entry(end).or_default().push(entry);
        }
    }
    // The points shared, if `copy` is the image of `other` in `about`.
    let corner_image = |copy: &CurveEntry, other: &CurveEntry, about: Id| {
        let (corner, of) = (copy.corner?, other.corner?);
        // A mirrored arc's start is on the image of the other's `b`.
        let (a, b) = match copy.curve {
            Curve::Arc { .. } => (of.b, of.a),
            _ => (of.a, of.b),
        };
        let mut pairs = counterparts(&copy.curve, &other.curve)?;
        pairs.push((corner.at, of.at));
        pairs.extend(line_pairs(corner.a, a)?);
        pairs.extend(line_pairs(corner.b, b)?);
        if corner.equal != of.equal || !pairs.iter().all(|&(p, q)| same(about)(p, q)) {
            return None;
        }
        // Those of the line itself are on it already.
        let own = curve(about).and_then(Curve::ends);
        let mut shared: Vec<Id> = pairs
            .into_iter()
            .filter(|&(p, q)| p == q && !own.is_some_and(|own| own.contains(&p)))
            .map(|(p, _)| p)
            .collect();
        shared.sort_unstable();
        shared.dedup();
        Some(shared)
    };
    for entry in corners() {
        let found = entry.curve.ends().into_iter().flatten().find_map(|end| {
            let images = images.get(&end).into_iter().flatten();
            images.into_iter().find_map(|&(image, about)| {
                let others = by_end.get(&image).into_iter().flatten();
                others
                    .into_iter()
                    .filter(|other| other.id < entry.id)
                    .find_map(|other| Some((about, corner_image(entry, other, about)?)))
            })
        });
        if let Some(found) = found {
            mirrored.insert(entry.id, found);
        }
    }
    mirrored
}
