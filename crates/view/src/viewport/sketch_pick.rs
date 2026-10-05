//! Picking finished sketches' curves and points in the viewport, each
//! where its sketch is placed in the world: a split's line, a sweep's
//! path, a loft's rails and points. Each sketch is hit tested on its own
//! plane ([`SketchLines`]), and of the sketches with something under the
//! cursor the nearest by depth wins.
//!
//! Given the model shown (`hidden_by`), what it hides isn't picked, as
//! the model's own edges aren't ([`PickIndex::hides`]): a curve or point
//! is picked only where its point under the cursor shows. Without it,
//! what's behind the model is picked too, as through the faded model
//! behind the sketch being edited.
//!
//! With the model ([`ModelPicking::sketches`]), a curve or point under the
//! cursor wins over a face there, which is under it everywhere, and over
//! an edge or a vertex farther from the eye ([`wins`]). What's hovered and
//! selected of them is drawn over the scene ([`draw_items`]).
//!
//! [`ModelPicking::sketches`]: super::ModelPicking::sketches

use glam::{DVec2, DVec3};
use iced::Rectangle;
use varde_document::FeatureId;
use varde_render::{Camera, PointStyle, SketchLayer, Space};
use varde_sketch::{Id, Sketch};

use super::regions::grid_plane;
use super::sketch::{line, srgba};
use crate::hit;
use crate::pick::{Pick, PickIndex, Picked};
use crate::projection::Projector;
use crate::theme::SketchColors;
use crate::{OverlapItem, SketchItem, SketchLines};

/// How near the cursor a curve or point must show to be under it, in
/// pixels: a sketch's hit tolerance.
pub(crate) const HIT_PIXELS: f64 = 6.0;

/// A sketch's curve or point under the cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SketchHit {
    /// The sketch feature.
    pub(crate) sketch: FeatureId,
    /// The curve or point.
    pub(crate) item: Id,
    /// Where on it, in the world: a curve's point nearest the cursor's
    /// point on its plane, or the point.
    pub(crate) at: DVec3,
    /// How far in front of the eye `at` is, or of the target in an
    /// orthographic view: nearer is smaller.
    pub(crate) depth: f64,
}

impl SketchHit {
    /// The sketch and the item, as the sessions picking curves have them.
    pub(crate) fn of(self) -> (FeatureId, Id) {
        (self.sketch, self.item)
    }
}

/// Which points of a sketch are picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Points {
    /// Only those on their own, no curve's: a loft's sections.
    Loose,
    /// All of them, curves' ends and centres too.
    All,
}

/// The curve of `lines`' sketches under the screen position `at` in a
/// viewport of `bounds` seen by `camera`: of those within [`HIT_PIXELS`]
/// on each sketch's plane, the nearest by depth that the model
/// `hidden_by` doesn't hide.
pub(crate) fn curve_under(
    lines: &[SketchLines<'_>],
    at: DVec2,
    camera: &Camera,
    bounds: Rectangle,
    hidden_by: Option<&PickIndex>,
) -> Option<SketchHit> {
    let mut nearest: Option<SketchHit> = None;
    for candidate in lines {
        let Some(projector) =
            Projector::new(camera, candidate.placement, bounds.width, bounds.height)
        else {
            continue;
        };
        let Some(cursor) = projector.cursor(at) else {
            continue;
        };
        let tolerance = HIT_PIXELS * cursor.pixel;
        let Some(curve) = hit::hit_curve(candidate.sketch, cursor.at, tolerance) else {
            continue;
        };
        let depth = projector.depth(cursor.at);
        if nearest.is_some_and(|nearest| depth >= nearest.depth) {
            continue;
        }
        let on = (candidate.sketch.nearest_on(curve, cursor.at)).unwrap_or(cursor.at);
        let world = candidate.placement.to_world(on);
        if hidden(hidden_by, camera, bounds, world) {
            continue;
        }
        nearest = Some(SketchHit {
            sketch: candidate.feature,
            item: curve,
            at: world,
            depth,
        });
    }
    nearest
}

/// The point of `lines`' sketches under the screen position `at`, of
/// those `points` takes: of those showing within [`HIT_PIXELS`] of it
/// that the model `hidden_by` doesn't hide, the nearest on the screen.
pub(crate) fn point_under(
    lines: &[SketchLines<'_>],
    at: DVec2,
    camera: &Camera,
    bounds: Rectangle,
    points: Points,
    hidden_by: Option<&PickIndex>,
) -> Option<SketchHit> {
    let mut nearest: Option<(f64, SketchHit)> = None;
    for candidate in lines {
        let Some(projector) =
            Projector::new(camera, candidate.placement, bounds.width, bounds.height)
        else {
            continue;
        };
        let sketch = candidate.sketch;
        let all: Box<dyn Iterator<Item = (Id, DVec2)>> = match points {
            Points::Loose => Box::new(loose(sketch)),
            Points::All => Box::new(sketch.points.iter().map(|point| (point.id, point.at))),
        };
        for (id, point) in all {
            let Some(shown) = projector.project(point) else {
                continue;
            };
            let distance = shown.distance(at);
            if distance > HIT_PIXELS || nearest.is_some_and(|(nearest, _)| distance >= nearest) {
                continue;
            }
            let world = candidate.placement.to_world(point);
            if hidden(hidden_by, camera, bounds, world) {
                continue;
            }
            let hit = SketchHit {
                sketch: candidate.feature,
                item: id,
                at: world,
                depth: projector.depth(point),
            };
            nearest = Some((distance, hit));
        }
    }
    nearest.map(|(_, hit)| hit)
}

/// The point or curve of `lines`' sketches under the screen position
/// `at`: any point ([`point_under`]), else a curve ([`curve_under`]), as
/// a click in the sketch being edited hits points first.
pub(crate) fn item_under(
    lines: &[SketchLines<'_>],
    at: DVec2,
    camera: &Camera,
    bounds: Rectangle,
    hidden_by: Option<&PickIndex>,
) -> Option<SketchHit> {
    point_under(lines, at, camera, bounds, Points::All, hidden_by)
        .or_else(|| curve_under(lines, at, camera, bounds, hidden_by))
}

/// The curves and points of `lines`' sketches showing within `reach`
/// pixels of the screen position `at`, each where it's nearest the
/// cursor, that the model `hidden_by` doesn't hide, nearest the eye
/// first: what's listed of them where the button was held.
pub(crate) fn items_near(
    lines: &[SketchLines<'_>],
    at: DVec2,
    camera: &Camera,
    bounds: Rectangle,
    reach: f64,
    hidden_by: Option<&PickIndex>,
) -> Vec<SketchHit> {
    let mut found = Vec::new();
    for candidate in lines {
        let Some(projector) =
            Projector::new(camera, candidate.placement, bounds.width, bounds.height)
        else {
            continue;
        };
        let sketch = candidate.sketch;
        let mut take = |item: Id, on: DVec2| {
            let world = candidate.placement.to_world(on);
            if !hidden(hidden_by, camera, bounds, world) {
                found.push(SketchHit {
                    sketch: candidate.feature,
                    item,
                    at: world,
                    depth: projector.depth(on),
                });
            }
        };
        for point in &sketch.points {
            if (projector.project(point.at)).is_some_and(|shown| shown.distance(at) <= reach) {
                take(point.id, point.at);
            }
        }
        let Some(cursor) = projector.cursor(at) else {
            continue;
        };
        let tolerance = reach * cursor.pixel;
        for entry in &sketch.curves {
            let near = hit::curve_distance(sketch, &entry.curve, cursor.at)
                .is_some_and(|distance| distance <= tolerance);
            if near {
                let on = (sketch.nearest_on(entry.id, cursor.at)).unwrap_or(cursor.at);
                take(entry.id, on);
            }
        }
    }
    found.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    found
}

/// The rows listed where the button was held: the model's `picks`, as
/// [`PickIndex::overlaps`] orders them (vertices and edges nearest the
/// eye first, then faces), with the sketches' `hits` among them, each
/// before the faces and before the first vertex or edge it [`wins`]
/// over; at most `most`.
pub(crate) fn listed_with(
    hits: Vec<SketchHit>,
    picks: Vec<Pick>,
    camera: &Camera,
    size: [f32; 2],
    most: usize,
) -> Vec<OverlapItem> {
    let item = |hit: SketchHit| {
        OverlapItem::Sketch(SketchItem {
            sketch: hit.sketch,
            item: hit.item,
        })
    };
    let mut hits = hits.into_iter().peekable();
    let mut listed = Vec::new();
    for pick in picks {
        while let Some(hit) = hits.next_if(|hit| wins(hit, Some(pick), camera, size)) {
            listed.push(item(hit));
        }
        listed.push(OverlapItem::Model(pick));
    }
    listed.extend(hits.map(item));
    listed.truncate(most);
    listed
}

/// How much farther from the eye than an edge or a vertex of the model
/// a sketch's item may be and still win over it, in view heights: as far
/// as the renderer pulls edges towards the camera, so one lying on an
/// edge wins.
const SAME_DEPTH: f64 = 0.002;

/// How wide a finished sketch's curve hovered and selected (or picked by
/// a session) is drawn, in pixels, and how big its point.
pub(crate) const HOVERED_WIDTH: f32 = 3.0;
pub(crate) const SELECTED_WIDTH: f32 = 2.5;
const HOVERED_RADIUS: f32 = 5.5;
const SELECTED_RADIUS: f32 = 4.5;

/// Whether the sketch's item `hit` wins over `pick`, what of the model is
/// under the cursor too, seen by `camera` in a viewport `size` big:
/// always over nothing or a face, which the cursor is over everywhere
/// while the item is a few pixels' target; over an edge or a vertex
/// where it's no farther from the eye, to [`SAME_DEPTH`].
pub(crate) fn wins(hit: &SketchHit, pick: Option<Pick>, camera: &Camera, size: [f32; 2]) -> bool {
    let Some(pick) = pick.filter(|pick| !matches!(pick.target, Picked::Face(_))) else {
        return true;
    };
    let Some(projector) = Projector::world(camera, size[0], size[1]) else {
        return true;
    };
    let slack = SAME_DEPTH * f64::from(camera.view_height());
    hit.depth <= projector.world_depth(pick.at) + slack
}

/// Draws on `live`, each on its sketch's plane, the curve or point of
/// `lines`' sketches `hovered`, in the hovered colour, and those
/// `marked`, in the selected colour: over a sketch's own drawing, or the
/// model's.
pub(crate) fn draw_items(
    live: &mut SketchLayer,
    lines: &[SketchLines<'_>],
    hovered: Option<SketchItem>,
    marked: &[SketchItem],
    colors: SketchColors,
) {
    for candidate in lines {
        let lit = |item: Id| {
            let named = SketchItem {
                sketch: candidate.feature,
                item,
            };
            if hovered == Some(named) {
                Some(true)
            } else {
                marked.contains(&named).then_some(false)
            }
        };
        if hovered.is_none_or(|hovered| hovered.sketch != candidate.feature)
            && !marked
                .iter()
                .any(|marked| marked.sketch == candidate.feature)
        {
            continue;
        }
        let sketch = candidate.sketch;
        if let Some(plane) = grid_plane(candidate.placement) {
            for entry in &sketch.curves {
                let Some(hot) = lit(entry.id) else {
                    continue;
                };
                let Some(points) = sketch.flatten(&entry.curve) else {
                    continue;
                };
                let style = if hot {
                    line(colors.hovered, HOVERED_WIDTH, false)
                } else {
                    line(colors.selected, SELECTED_WIDTH, false)
                };
                live.polyline(Space::On(plane), &points, style);
            }
        }
        for point in &sketch.points {
            let Some(hot) = lit(point.id) else {
                continue;
            };
            let color = if hot { colors.hovered } else { colors.selected };
            let style = PointStyle {
                radius: if hot { HOVERED_RADIUS } else { SELECTED_RADIUS },
                rim_width: 1.5,
                rim: srgba(color),
                fill: srgba(colors.point_fill),
                fixed: false,
            };
            let at = candidate.placement.to_world(point.at);
            live.world_point(at.as_vec3(), style);
        }
    }
}

/// Whether the model `hidden_by`, if any, hides the world point `at`.
fn hidden(hidden_by: Option<&PickIndex>, camera: &Camera, bounds: Rectangle, at: DVec3) -> bool {
    hidden_by.is_some_and(|index| index.hides(camera, [bounds.width, bounds.height], at))
}

/// The points on their own (no curve's) of `sketch`, with where they are.
pub(crate) fn loose(sketch: &Sketch) -> impl Iterator<Item = (Id, DVec2)> + '_ {
    let used: std::collections::BTreeSet<Id> = (sketch.curves.iter())
        .flat_map(|entry| entry.curve.points())
        .collect();
    (sketch.points.iter())
        .filter(move |point| !used.contains(&point.id))
        .map(|point| (point.id, point.at))
}

#[cfg(test)]
mod tests;
