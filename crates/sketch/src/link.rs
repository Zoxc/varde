//! Links: geometry a sketch takes from outside it, projected onto its
//! plane or cut by it, which follows what it comes from ([`Link`]).
//!
//! A link's points and curves are ordinary items of the sketch, in its
//! lists, with ids from its counter, so constraints, dimensions, snapping
//! and drawing take them as they take any. What makes them a link's is
//! the [`Link`] listing them: the solver takes them as constants (as the
//! origin and axes), an edit may not change them (only the link's own
//! edits do: [`SketchEdit::AddLink`](crate::SketchEdit::AddLink),
//! [`SketchEdit::Relink`](crate::SketchEdit::Relink),
//! [`SketchEdit::SetLinkProfiles`](crate::SketchEdit::SetLinkProfiles),
//! and deleting the link, which takes them with it), and its curves count
//! for profiles only where the link says so (they're construction
//! geometry otherwise). What a link comes from isn't the sketch's: the
//! document keeps it beside the sketch.
//!
//! A link's geometry is held as it was last found ([`LinkShape`]), so the
//! sketch solves and draws without the model. Finding it again moves it
//! in place, keeping the ids of what goes on and so what's on them
//! (every id with the same form: as many points, curves of the same
//! kinds made from them in the same way); what doesn't go on is deleted
//! and what's new added ([`Sketch::relink`]).

mod fit;

pub use fit::{FitError, SampledChain};

use std::collections::{BTreeMap, HashSet};

use glam::{DAffine2, DVec2};
use serde::{Deserialize, Serialize};

use crate::{Curve, EditError, Id, MAX_SPLINE_POINTS, OutOfIds, Sketch, Spline, SplineKind, angle};

/// The most links a sketch may hold.
pub const MAX_LINKS: usize = 1000;

/// The most points one link may make: a few splines of
/// [`MAX_SPLINE_POINTS`] each, or many lines.
pub const MAX_LINK_POINTS: usize = 10 * MAX_SPLINE_POINTS;

/// The most curves one link may make.
pub const MAX_LINK_CURVES: usize = MAX_LINK_POINTS / 2;

/// How a link makes its geometry from what it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkKind {
    /// Projected square onto the sketch's plane.
    Project,
    /// Where the sketch's plane cuts it.
    Intersect,
}

impl LinkKind {
    /// "Projected", "Intersected": what the sketch's list calls its
    /// links of the kind.
    pub fn name(self) -> &'static str {
        match self {
            LinkKind::Project => "Projected",
            LinkKind::Intersect => "Intersected",
        }
    }
}

/// Geometry a sketch takes from outside it, see the module's docs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// Its id, from the sketch's counter: it names the link, never an
    /// item in the sketch's lists.
    pub id: Id,
    pub kind: LinkKind,
    /// The points it made, in the order its [`LinkShape`] has them, so
    /// increasing.
    pub points: Vec<Id>,
    /// The curves it made, likewise, made of its points alone.
    pub curves: Vec<Id>,
    /// Whether its curves count for profiles: they're construction
    /// geometry where they don't.
    pub profiles: bool,
}

impl Link {
    /// The points and curves it made, its points first.
    pub fn items(&self) -> impl Iterator<Item = Id> + '_ {
        self.points.iter().chain(&self.curves).copied()
    }
}

/// A link's geometry without ids: its points, and its curves naming them
/// by placeholders, `Id`s numbering the points from 0 ([`LinkShape::point`]),
/// so made from points of the shape alone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkShape {
    pub points: Vec<DVec2>,
    pub curves: Vec<Curve>,
}

impl LinkShape {
    /// Adds a point at `at`, giving its placeholder.
    pub fn point(&mut self, at: DVec2) -> Id {
        let id = Id(u32::try_from(self.points.len()).unwrap_or(u32::MAX));
        self.points.push(at);
        id
    }

    /// Adds `curve`, made of the shape's points by their placeholders.
    pub fn curve(&mut self, curve: Curve) {
        self.curves.push(curve);
    }

    /// Whether it has nothing.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty() && self.curves.is_empty()
    }

    /// Whether it's a shape a sketch could take: at most
    /// [`MAX_LINK_POINTS`] points and [`MAX_LINK_CURVES`] curves, every
    /// coordinate and radius finite and within `max` (radii above
    /// zero), every curve made of its points, splines as they can be
    /// ([`Spline::fits`]). What a lane sends is checked by it.
    pub fn fits(&self, max: f64) -> bool {
        let within = |v: f64| v.is_finite() && v.abs() <= max;
        if self.points.len() > MAX_LINK_POINTS || self.curves.len() > MAX_LINK_CURVES {
            return false;
        }
        if !(self.points.iter()).all(|p| within(p.x) && within(p.y)) {
            return false;
        }
        let count = self.points.len();
        self.curves.iter().all(|curve| {
            let points = curve.points().all(|id| (id.0 as usize) < count);
            let own = match curve {
                &Curve::Circle { radius, .. } => radius > 0.0 && within(radius),
                Curve::Spline(spline) => spline.fits() && spline.handles.is_empty(),
                Curve::Line { start, end } => start != end,
                Curve::Arc { center, start, end } => {
                    center != start && center != end && start != end
                }
            };
            points && own
        })
    }

    /// Whether `other` has the same form: as many points, and the same
    /// curves made from them in the same way (a spline of the same kind
    /// with as many knots), whatever their places, radii and knots.
    pub fn same_form(&self, other: &LinkShape) -> bool {
        self.points.len() == other.points.len()
            && self.curves.len() == other.curves.len()
            && (self.curves.iter().zip(&other.curves)).all(|pair| match pair {
                (Curve::Circle { center: a, .. }, Curve::Circle { center: b, .. }) => a == b,
                (Curve::Spline(a), Curve::Spline(b)) => {
                    a.kind == b.kind
                        && a.closed == b.closed
                        && a.points == b.points
                        && a.handles == b.handles
                        && a.knots.len() == b.knots.len()
                }
                (a, b) => a == b,
            })
    }

    /// Whether `other` is the same shape to within `within`: the same
    /// form, each point and radius within that, each knot within a
    /// billionth. What tells a link found again from the one held, so
    /// rounding doesn't change the sketch.
    pub fn close_to(&self, other: &LinkShape, within: f64) -> bool {
        let near = |a: f64, b: f64, by: f64| (a - b).abs() <= by;
        self.same_form(other)
            && (self.points.iter().zip(&other.points))
                .all(|(a, b)| near(a.x, b.x, within) && near(a.y, b.y, within))
            && (self.curves.iter().zip(&other.curves)).all(|pair| match pair {
                (&Curve::Circle { radius: a, .. }, &Curve::Circle { radius: b, .. }) => {
                    near(a, b, within)
                }
                (Curve::Spline(a), Curve::Spline(b)) => {
                    (a.knots.iter().zip(&b.knots)).all(|(&a, &b)| near(a, b, 1e-9))
                }
                _ => true,
            })
    }

    /// The shape mapped by `map`, an affine map of the plane that keeps
    /// circles circles (a turn, a reflection or a shift): circles keep
    /// their radii, and arcs run the other way round where it reflects,
    /// so they stay counter-clockwise.
    fn mapped_rigid(&self, map: &DAffine2, reflects: bool) -> LinkShape {
        let curves = (self.curves.iter())
            .map(|curve| match *curve {
                Curve::Arc { center, start, end } if reflects => Curve::Arc {
                    center,
                    start: end,
                    end: start,
                },
                ref other => other.clone(),
            })
            .collect();
        LinkShape {
            points: (self.points.iter())
                .map(|&p| map.transform_point2(p))
                .collect(),
            curves,
        }
    }
}

/// The most pairs of points [`Sketch::follow`] weighs to match the
/// points left nearest first: past it, they're matched in turn.
const NEAREST_PAIRS: usize = 1 << 16;

/// A shape found for a link as relinking gives it the link, see
/// [`Sketch::follow`].
struct Followed {
    /// The shape, reordered.
    shape: LinkShape,
    /// The id each of its points keeps, in its order, `None` for a new one.
    points: Vec<Option<Id>>,
    /// Likewise its curves'.
    curves: Vec<Option<Id>>,
}

/// Why a sketch's own point or curve gives no shape projected into
/// another sketch.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectError {
    /// It isn't a point or a curve of the sketch.
    Missing,
    /// What it projects to can't be held within the tolerance.
    Fit(FitError),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::Missing => f.write_str("it isn't in its sketch any more"),
            ProjectError::Fit(why) => why.fmt(f),
        }
    }
}

/// How many places along a circle a projection that doesn't keep it one
/// fits a spline through: enough for an ellipse within a fit tolerance's
/// share of its size.
const CIRCLE_SAMPLES: usize = 256;

impl Sketch {
    /// The link `id`.
    pub fn link(&self, id: Id) -> Option<&Link> {
        (self.links.binary_search_by_key(&id, |link| link.id).ok()).map(|index| &self.links[index])
    }

    fn link_mut(&mut self, id: Id) -> Option<&mut Link> {
        (self.links.binary_search_by_key(&id, |link| link.id).ok())
            .map(|index| &mut self.links[index])
    }

    /// The link that made the point or curve `item`, if one did.
    pub fn link_of(&self, item: Id) -> Option<&Link> {
        (self.links.iter()).find(|link| link.points.contains(&item) || link.curves.contains(&item))
    }

    /// Whether a link made the point or curve `item`.
    pub fn is_linked(&self, item: Id) -> bool {
        self.link_of(item).is_some()
    }

    /// The ids of every point and curve a link made.
    pub fn linked(&self) -> HashSet<Id> {
        self.links.iter().flat_map(Link::items).collect()
    }

    /// The geometry `link` holds now, as a shape (placeholders for its
    /// points in its order). `None` if it names what the sketch hasn't.
    pub fn link_shape(&self, link: &Link) -> Option<LinkShape> {
        let index: BTreeMap<Id, Id> = (link.points.iter().enumerate())
            .map(|(i, &id)| Some((id, Id(u32::try_from(i).ok()?))))
            .collect::<Option<_>>()?;
        let points = (link.points.iter())
            .map(|&id| self.point(id).map(|point| point.at))
            .collect::<Option<Vec<_>>>()?;
        let curves = (link.curves.iter())
            .map(|&id| {
                let entry = self.curve(id)?;
                entry
                    .curve
                    .map_points(|point| index.get(&point).copied().ok_or(()))
                    .ok()
            })
            .collect::<Option<Vec<_>>>()?;
        Some(LinkShape { points, curves })
    }

    /// Adds a link of `kind` making nothing yet, as a link starts: what
    /// it comes from is found later ([`SketchEdit::Relink`](crate::SketchEdit::Relink)).
    /// Its id is the highest, so it goes last.
    pub fn add_link(&mut self, kind: LinkKind) -> Result<Id, OutOfIds> {
        let id = self.new_id()?;
        self.links.push(Link {
            id,
            kind,
            points: Vec::new(),
            curves: Vec::new(),
            profiles: false,
        });
        Ok(id)
    }

    /// Gives the link `id` the geometry `shape`, keeping what ids it can
    /// ([`Sketch::follow`]): its points and curves that go on are moved
    /// in place, or reshaped (a spline found with more points), keeping
    /// what's on them; those that don't are deleted, with what's on them
    /// ([`Sketch::delete`]), and what's new is added. Of the same form,
    /// every id stays. `Target` for no such link, `OutOfIds` past the
    /// last id.
    pub(crate) fn relink(&mut self, id: Id, shape: &LinkShape) -> Result<(), EditError> {
        let link = self.link(id).ok_or(EditError::Target(id))?.clone();
        let followed = self.follow(&link, shape).ok_or(EditError::Target(id))?;
        let mut points = Vec::with_capacity(followed.points.len());
        for (&kept, &at) in followed.points.iter().zip(&followed.shape.points) {
            match kept {
                Some(point) => {
                    self.point_mut(point).ok_or(EditError::Target(point))?.at = at;
                    points.push(point);
                }
                None => points.push(self.add_point(at)?),
            }
        }
        let mut curves = Vec::with_capacity(followed.curves.len());
        for (&kept, curve) in followed.curves.iter().zip(&followed.shape.curves) {
            let curve = curve
                .map_points(|placeholder| points.get(placeholder.0 as usize).copied().ok_or(()))
                .map_err(|()| EditError::Target(id))?;
            match kept {
                Some(kept) => {
                    self.curve_mut(kept).ok_or(EditError::Target(kept))?.curve = curve;
                    curves.push(kept);
                }
                None => curves.push(self.add_curve(curve, !link.profiles)?),
            }
        }
        let now: HashSet<Id> = points.iter().chain(&curves).copied().collect();
        let gone: Vec<Id> = link.items().filter(|item| !now.contains(item)).collect();
        let held = self.link_mut(id).ok_or(EditError::Target(id))?;
        held.points = points;
        held.curves = curves;
        // Nothing of the link is made from what's gone any more.
        self.delete(&gone);
        Ok(())
    }

    /// Whether the link `id` holds `shape` as relinking it would give it
    /// (as `Sketch::follow` matches them), to within `within` ([`LinkShape::close_to`]):
    /// every item found keeping an id, each where it was found. What
    /// tells a stale link, so one relinked is in step from then on.
    pub fn link_follows(&self, id: Id, shape: &LinkShape, within: f64) -> bool {
        let Some(link) = self.link(id) else {
            return false;
        };
        let (Some(held), Some(followed)) = (self.link_shape(link), self.follow(link, shape)) else {
            return false;
        };
        (followed.points.iter().all(Option::is_some))
            && (followed.curves.iter().all(Option::is_some))
            && followed.points.len() == link.points.len()
            && followed.curves.len() == link.curves.len()
            && held.close_to(&followed.shape, within)
    }

    /// `shape`, found for `link`, matched to what it holds: each curve
    /// keeps the id of the one of the same kind as many before it of
    /// that kind (the third line found the third line held), each point
    /// that of the point it's made from in the same role on a curve so
    /// kept (an end, a center, a spline's point by its place in the
    /// list), the rest of the points those of the points left, the
    /// nearest pairs first (in turn past [`NEAREST_PAIRS`]);
    /// what's left of either is new or gone. Reordered so the ids kept
    /// come first, increasing, then the new items as they were found,
    /// as the link lists them. Of the same form every id is kept, in
    /// order. `None` if the link names what the sketch hasn't.
    fn follow(&self, link: &Link, shape: &LinkShape) -> Option<Followed> {
        let held = self.link_shape(link)?;
        let mut points: Vec<Option<Id>> = vec![None; shape.points.len()];
        let mut curves: Vec<Option<Id>> = vec![None; shape.curves.len()];
        let mut taken = vec![false; held.points.len()];
        // Where each kind's next curve held is looked for from.
        let mut from: Vec<(crate::Kind, usize)> = Vec::new();
        for (found, curve) in shape.curves.iter().enumerate() {
            let kind = curve.kind();
            let start = match from.iter().find(|(k, _)| *k == kind) {
                Some(&(_, at)) => at,
                None => 0,
            };
            let Some(index) = (start..held.curves.len()).find(|&i| held.curves[i].kind() == kind)
            else {
                from.retain(|(k, _)| *k != kind);
                from.push((kind, held.curves.len()));
                continue;
            };
            from.retain(|(k, _)| *k != kind);
            from.push((kind, index + 1));
            curves[found] = Some(link.curves[index]);
            for (old, new) in held.curves[index].points().zip(curve.points()) {
                let (old, new) = (old.0 as usize, new.0 as usize);
                if points.get(new).is_some_and(Option::is_none) && taken.get(old) == Some(&false) {
                    points[new] = Some(link.points[old]);
                    taken[old] = true;
                }
            }
        }
        // The rest nearest first.
        let left: Vec<usize> = (0..shape.points.len())
            .filter(|&new| points[new].is_none())
            .collect();
        let free: Vec<usize> = (0..held.points.len()).filter(|&old| !taken[old]).collect();
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        if left.len().saturating_mul(free.len()) <= NEAREST_PAIRS {
            for &new in &left {
                for &old in &free {
                    let apart = shape.points[new].distance_squared(held.points[old]);
                    pairs.push((apart, new, old));
                }
            }
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then((a.1, a.2).cmp(&(b.1, b.2))));
        } else {
            // Too many to weigh each pair: in turn.
            pairs.extend(left.iter().zip(&free).map(|(&new, &old)| (0.0, new, old)));
        }
        for (_, new, old) in pairs {
            if points[new].is_none() && !taken[old] {
                points[new] = Some(link.points[old]);
                taken[old] = true;
            }
        }
        // Kept ids first, increasing, then the new ones in the order found.
        let order = |ids: &[Option<Id>]| {
            let mut order: Vec<usize> = (0..ids.len()).collect();
            order.sort_by_key(|&i| (ids[i].is_none(), ids[i], i));
            order
        };
        let point_order = order(&points);
        let curve_order = order(&curves);
        let mut placeholder = vec![Id(0); shape.points.len()];
        for (to, &was) in point_order.iter().enumerate() {
            placeholder[was] = Id(u32::try_from(to).ok()?);
        }
        let curves_found = (curve_order.iter())
            .map(|&i| {
                shape.curves[i]
                    .map_points(|p| placeholder.get(p.0 as usize).copied().ok_or(()))
                    .ok()
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Followed {
            shape: LinkShape {
                points: point_order.iter().map(|&i| shape.points[i]).collect(),
                curves: curves_found,
            },
            points: point_order.iter().map(|&i| points[i]).collect(),
            curves: curve_order.iter().map(|&i| curves[i]).collect(),
        })
    }

    /// Makes the link `id`'s curves count for profiles, or not: they're
    /// construction geometry where they don't.
    pub(crate) fn set_link_profiles(&mut self, id: Id, profiles: bool) -> Result<(), EditError> {
        let link = self.link_mut(id).ok_or(EditError::Target(id))?;
        link.profiles = profiles;
        let curves = link.curves.clone();
        for curve in curves {
            self.curve_mut(curve)
                .ok_or(EditError::Target(curve))?
                .construction = !profiles;
        }
        Ok(())
    }

    /// The point or curve `item` as another sketch takes it projected
    /// through `map`, the affine map from this sketch's plane into the
    /// other's (taking a place here to the place there it's projected
    /// onto). Exact where it can be: a point a point, a line a line (what
    /// fillets and chamfers leave of it, [`Sketch::cut_back`]; a point
    /// where it's square to the other plane), a circle or an arc
    /// one where `map` keeps them (the planes are parallel), a spline by
    /// its control points mapped, which an affine map keeps exact.
    /// Otherwise fitted within `fit` ([`LinkShape::fit`]), its kind
    /// told within `exact`: a circle seen edge on is a line, a tilted one
    /// a spline.
    pub fn project_item(
        &self,
        item: Id,
        map: &DAffine2,
        exact: f64,
        fit: f64,
    ) -> Result<LinkShape, ProjectError> {
        let at = |id: Id| {
            self.point(id)
                .map(|point| point.at)
                .ok_or(ProjectError::Missing)
        };
        let m = map.matrix2;
        // Whether `map` keeps lengths: its columns unit and square to
        // each other, to within rounding.
        let rigid = (m.x_axis.length_squared() - 1.0).abs() <= 1e-12
            && (m.y_axis.length_squared() - 1.0).abs() <= 1e-12
            && m.x_axis.dot(m.y_axis).abs() <= 1e-12;
        let reflects = m.determinant() < 0.0;
        if let Some(point) = self.point(item) {
            let mut shape = LinkShape::default();
            shape.point(map.transform_point2(point.at));
            return Ok(shape);
        }
        let entry = self.curve(item).ok_or(ProjectError::Missing)?;
        let mut own = LinkShape::default();
        let sampled = |places: Vec<DVec2>, closed: bool| {
            let places = places.iter().map(|&p| map.transform_point2(p)).collect();
            LinkShape::fit(&[SampledChain { places, closed }], exact, fit)
                .map_err(ProjectError::Fit)
        };
        match &entry.curve {
            &Curve::Line { start, end } => {
                // What a fillet or chamfer at an end leaves of it.
                let (s, e) = (at(start)?, at(end)?);
                let [from, to] = self.cut_back().get(&item).copied().unwrap_or([0.0, 1.0]);
                let (a, b) = (
                    map.transform_point2(s.lerp(e, from)),
                    map.transform_point2(s.lerp(e, to.max(from))),
                );
                if a.distance(b) <= exact {
                    own.point((a + b) * 0.5);
                } else {
                    let start = own.point(a);
                    let end = own.point(b);
                    own.curve(Curve::Line { start, end });
                }
                Ok(own)
            }
            &Curve::Circle { center, radius } => {
                let c = at(center)?;
                if rigid {
                    let center = own.point(c);
                    own.curve(Curve::Circle { center, radius });
                    return Ok(own.mapped_rigid(map, reflects));
                }
                let places = (0..CIRCLE_SAMPLES)
                    .map(|i| {
                        let turn = std::f64::consts::TAU * i as f64 / CIRCLE_SAMPLES as f64;
                        c + angle::from_angle(turn) * radius
                    })
                    .collect();
                sampled(places, true)
            }
            &Curve::Arc { center, start, end } => {
                let (c, s, e) = (at(center)?, at(start)?, at(end)?);
                if rigid {
                    let center = own.point(c);
                    let start = own.point(s);
                    let end = own.point(e);
                    own.curve(Curve::Arc { center, start, end });
                    return Ok(own.mapped_rigid(map, reflects));
                }
                let radius = s.distance(c);
                let from = angle::to_angle(s - c);
                let sweep = crate::arc_sweep(s - c, e - c);
                let count = CIRCLE_SAMPLES;
                let places = (0..=count)
                    .map(|i| {
                        let turn = from + sweep * i as f64 / count as f64;
                        c + angle::from_angle(turn) * radius
                    })
                    .collect();
                sampled(places, false)
            }
            Curve::Spline(spline) => {
                let shape = self.spline_shape(spline).ok_or(ProjectError::Missing)?;
                let control = shape.control();
                if control.len() <= MAX_SPLINE_POINTS {
                    let mut points = Vec::with_capacity(control.len());
                    for &p in control {
                        points.push(own.point(map.transform_point2(p)));
                    }
                    let projected = Spline {
                        kind: SplineKind::Control,
                        points,
                        closed: spline.closed,
                        handles: Vec::new(),
                        knots: shape.knots().to_vec(),
                    };
                    let sound =
                        crate::BSpline::new(&projected.knots, own.points.clone(), projected.closed)
                            .is_some_and(|mapped| mapped.path().is_sound());
                    if projected.fits() && sound {
                        own.curve(Curve::Spline(projected));
                        return Ok(own);
                    }
                }
                let mut places = self.flatten(&entry.curve).ok_or(ProjectError::Missing)?;
                if spline.closed {
                    places.pop();
                }
                sampled(places, spline.closed)
            }
        }
    }
}

impl Sketch {
    /// That every link holds as [`Sketch::check`] wants it, see there.
    pub(crate) fn check_links(&self) -> Result<(), crate::SketchError> {
        use crate::SketchError;
        if self.links.len() > MAX_LINKS {
            return Err(SketchError::TooMany {
                list: crate::List::Links,
                count: self.links.len(),
                limit: MAX_LINKS,
            });
        }
        let mut seen: HashSet<Id> = HashSet::new();
        let mut before: Option<Id> = None;
        for link in &self.links {
            let id = link.id;
            if before.is_some_and(|before| id <= before) {
                return Err(SketchError::Order {
                    id,
                    before: before.unwrap_or(id),
                });
            }
            before = Some(id);
            if id.0 >= self.next_id {
                return Err(SketchError::NextId(id));
            }
            if self.kind(id).is_some() {
                return Err(SketchError::Shared(id));
            }
            let increasing = |ids: &[Id]| ids.windows(2).all(|pair| pair[0] < pair[1]);
            if link.points.len() > MAX_LINK_POINTS
                || link.curves.len() > MAX_LINK_CURVES
                || !increasing(&link.points)
                || !increasing(&link.curves)
            {
                return Err(SketchError::Link(id));
            }
            for &point in &link.points {
                if self.point_index(point).is_none() || !seen.insert(point) {
                    return Err(SketchError::Link(id));
                }
            }
            for &curve in &link.curves {
                let Some(entry) = self.curve(curve) else {
                    return Err(SketchError::Link(id));
                };
                if !seen.insert(curve)
                    || entry.corner.is_some()
                    || entry.construction == link.profiles
                    // Its points are increasing, as seen above.
                    || entry
                        .curve
                        .points()
                        .any(|point| link.points.binary_search(&point).is_err())
                    || matches!(&entry.curve, Curve::Spline(spline) if !spline.handles.is_empty())
                {
                    return Err(SketchError::Link(id));
                }
            }
        }
        // No curve of the user's is made from a link's point.
        let shared = (self.curves.iter())
            .filter(|entry| !seen.contains(&entry.id))
            .find(|entry| entry.curve.points().any(|point| seen.contains(&point)));
        if let Some(entry) = shared {
            let link = self.link_of(
                entry
                    .curve
                    .points()
                    .find(|p| seen.contains(p))
                    .unwrap_or(entry.id),
            );
            return Err(SketchError::Link(link.map_or(entry.id, |link| link.id)));
        }
        Ok(())
    }

    /// That every link of `before` is in `self` as it was, with its
    /// points and curves, but those named in `deleted` (gone whole):
    /// what an edit other than a link's own may not change. The first
    /// item changed, or the link, as [`EditError::Linked`].
    pub(crate) fn links_kept(&self, before: &Sketch, deleted: &[Id]) -> Result<(), EditError> {
        for link in &before.links {
            let Some(now) = self.link(link.id) else {
                if deleted.contains(&link.id) {
                    continue;
                }
                return Err(EditError::Linked(link.id));
            };
            // Its items first: one deleted leaves the link's lists changed
            // too, but the item is what the edit took.
            for &point in &link.points {
                if self.point(point) != before.point(point) {
                    return Err(EditError::Linked(point));
                }
            }
            for &curve in &link.curves {
                if self.curve(curve) != before.curve(curve) {
                    return Err(EditError::Linked(curve));
                }
            }
            if now != link {
                return Err(EditError::Linked(link.id));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
