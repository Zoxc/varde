//! Picking sketch regions for an operation, in the viewport: what the
//! extrude and the revolve share. Before the source sketch is chosen,
//! every candidate's regions are shaded on its own plane; after, the
//! source's are the base layer, those picked filled stronger and
//! outlined. The region under the cursor is filled over them. Picking
//! casts the cursor's ray onto each candidate's plane and asks
//! [`Profiles::region_at`] there, the nearest hit winning; no GPU picking.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use iced::Rectangle;
use varde_document::{FeatureId, Placement};
use varde_render::{Camera, GridPlane, SketchLayer, Space as LayerSpace};
use varde_sketch::Profiles;

use super::sketch::{fill_region, fill_region_in, line};
use crate::operation_panel::Candidate;
use crate::projection::Projector;
use crate::theme::SketchColors;

/// How wide the outline of picked regions is, in pixels.
const OUTLINE_WIDTH: f32 = 1.8;
/// How opaque the fill of a picked region is.
const PICKED_ALPHA: f32 = 0.35;

/// The sketches whose regions an operation picks, and those picked.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Regions<'s, 'a> {
    /// The source sketch once there is one, the only candidate then;
    /// else every visible sketch with regions.
    pub(crate) candidates: &'s [Candidate<'a>],
    pub(crate) source: Option<FeatureId>,
    /// The regions picked, by their index in the source's profiles.
    pub(crate) picked: &'a BTreeSet<usize>,
}

/// What the viewport keeps of the regions between events and frames.
#[derive(Default)]
pub(crate) struct Input {
    /// The region under the cursor as it last moved, and its sketch.
    pub(crate) hover: Option<(FeatureId, usize)>,
    /// The base layer last built, and what from.
    base: RefCell<Option<Base>>,
}

/// The base layer as built, and what it was built from.
struct Base {
    /// Compared by pointer: the app finds them again only when the sketch
    /// changes.
    profiles: Arc<Profiles>,
    picked: BTreeSet<usize>,
    colors: SketchColors,
    layer: Arc<SketchLayer>,
}

impl<'s, 'a> Regions<'s, 'a> {
    /// The source sketch, if it's chosen.
    pub(crate) fn source(&self) -> Option<&'s Candidate<'a>> {
        let source = self.source?;
        self.candidates.iter().find(|c| c.feature == source)
    }

    /// Where the layers are drawn: on the source sketch's plane, or XY
    /// before there is one, when each candidate is drawn on its own.
    pub(crate) fn plane(&self) -> GridPlane {
        self.source()
            .and_then(|source| grid_plane(source.plane.placement()))
            .unwrap_or(GridPlane::XY)
    }

    /// The region under the screen position `at`, and its sketch: of the
    /// candidates' regions the cursor's ray meets, the nearest.
    pub(crate) fn region_under(
        &self,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
    ) -> Option<(FeatureId, usize)> {
        let mut nearest: Option<(f64, FeatureId, usize)> = None;
        for candidate in self.candidates {
            let placement = candidate.plane.placement();
            let Some(projector) = Projector::new(camera, placement, bounds.width, bounds.height)
            else {
                continue;
            };
            let Some(cursor) = projector.cursor(at) else {
                continue;
            };
            let Some(region) = candidate.profiles.region_at(cursor.at) else {
                continue;
            };
            let depth = projector.depth(cursor.at);
            if nearest.is_none_or(|(nearest, ..)| depth < nearest) {
                nearest = Some((depth, candidate.feature, region));
            }
        }
        nearest.map(|(_, feature, region)| (feature, region))
    }

    /// Adds to the live layer `live` the region `hovered`, and before
    /// there's a source every candidate's regions, each on its own plane.
    pub(crate) fn live(
        &self,
        hovered: Option<(FeatureId, usize)>,
        colors: SketchColors,
        live: &mut SketchLayer,
    ) {
        let source = self.source();
        for candidate in self.candidates {
            let hovered = hovered
                .filter(|(feature, _)| *feature == candidate.feature)
                .and_then(|(_, region)| candidate.profiles.regions.get(region));
            if source.is_some() {
                if let Some(region) = hovered {
                    fill_region(live, region, colors.region_hovered);
                }
                continue;
            }
            let Some(plane) = grid_plane(candidate.plane.placement()) else {
                continue;
            };
            let space = LayerSpace::On(plane);
            for region in &candidate.profiles.regions {
                fill_region_in(live, space, region, colors.region);
            }
            if let Some(region) = hovered {
                fill_region_in(live, space, region, colors.region_hovered);
            }
        }
    }

    /// The source sketch's regions shaded, and those picked filled and
    /// outlined, kept in `input` until they change.
    pub(crate) fn base_layer(&self, input: &Input, colors: SketchColors) -> Arc<SketchLayer> {
        let Some(source) = self.source() else {
            return Arc::default();
        };
        let mut base = input.base.borrow_mut();
        let current = base.as_ref().is_some_and(|base| {
            Arc::ptr_eq(&base.profiles, source.profiles)
                && base.picked == *self.picked
                && base.colors == colors
        });
        if let Some(base) = base.as_ref().filter(|_| current) {
            return base.layer.clone();
        }
        let mut layer = SketchLayer::default();
        let picked = colors.selected.scale_alpha(PICKED_ALPHA);
        for (index, region) in source.profiles.regions.iter().enumerate() {
            if self.picked.contains(&index) {
                fill_region(&mut layer, region, picked);
                for polyline in &region.outline {
                    let mut closed = polyline.clone();
                    closed.extend(polyline.first().copied());
                    layer.polyline(
                        LayerSpace::Sketch,
                        &closed,
                        line(colors.selected, OUTLINE_WIDTH, false),
                    );
                }
            } else {
                fill_region(&mut layer, region, colors.region);
            }
        }
        let layer = Arc::new(layer);
        *base = Some(Base {
            profiles: source.profiles.clone(),
            picked: self.picked.clone(),
            colors,
            layer: layer.clone(),
        });
        layer
    }
}

/// The renderer's plane of `placement`, unless it's too far out for it.
pub(crate) fn grid_plane(placement: Placement) -> Option<GridPlane> {
    GridPlane::new(
        placement.origin.as_vec3(),
        placement.x.as_vec3(),
        placement.y.as_vec3(),
    )
}
