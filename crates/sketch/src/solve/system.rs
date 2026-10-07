//! A sketch as the solver sees it: variables, equations over them, and
//! the connected components they split into.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use glam::DVec2;

use crate::angle;
use crate::origin::axis;
use crate::sets::Sets;
use crate::spline::bezier::Path;
use crate::{
    Constraint, Corner, Curve, CurveEntry, Dimension, Id, Joint, Measure, OffsetPair, Side, Sketch,
    SplineKind,
};

use super::equation::{
    EdgeTo, Equation, Heading, LineSlots, PairRead, PairSlots, PointSlots, Refit, Residual,
    RoundSlots, Slot, SplineAt, SplineSlots,
};

/// How [`Constraint::Fix`] is modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fixing {
    /// What's fixed is constants, not variables: the solver can't move
    /// it, and it cuts the sketch into smaller components.
    Constants,
    /// What's fixed is variables held by equations of their own, so the
    /// analysis can name a `Fix` as part of a redundancy.
    Equations,
}

/// The variables of a sketch and the equations over them.
pub(crate) struct System {
    /// Each variable's value.
    pub values: Vec<f64>,
    pub equations: Vec<Equation>,
    /// Each point's coordinates, in the order of the sketch's points.
    pub points: Vec<PointSlots>,
    /// Each circle's radius, in the order of the sketch's curves, `None`
    /// for the rest.
    pub radii: Vec<Option<Slot>>,
    /// The variables kept within bounds, each with its least and most: a
    /// point's parameter on an open spline, from its start to its end.
    pub bounds: Vec<(usize, f64, f64)>,
    /// The splines equations read, by id, with the Bézier segments of
    /// those points are on, for finding where on them the points are.
    splines: HashMap<Id, (Rc<SplineSlots>, Option<Path>)>,
}

/// The parameter on `path` a point on it starts at: of the place nearest
/// the point, or with the point dragged among `targets`, nearest its
/// target, so it starts where it's going. `None` if the point is missing.
fn start_on(sketch: &Sketch, targets: &HashMap<Id, DVec2>, point: Id, path: &Path) -> Option<f64> {
    let place = targets
        .get(&point)
        .or(sketch.point(point).map(|point| &point.at))?;
    Some(path.closest(*place))
}

/// A point that's an end of both the curves `a` and `b` of `sketch`, if
/// there is one: a line's or an arc's start or end.
fn shared_end(sketch: &Sketch, a: Id, b: Id) -> Option<Id> {
    let ends = |id| sketch.curve(id)?.curve.ends();
    let (a, b) = (ends(a)?, ends(b)?);
    a.into_iter().find(|end| b.contains(end))
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
/// on it in their place (see [`System::implied`]).
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

/// A point at `at` that nothing moves.
fn constants(at: DVec2) -> PointSlots {
    [Slot::Const(at.x), Slot::Const(at.y)]
}

/// A set of variables the equations tie together, with those equations,
/// by index, both in increasing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Component {
    pub vars: Vec<usize>,
    pub equations: Vec<usize>,
}

impl System {
    /// The system of `sketch`, which is to have passed
    /// [`Sketch::check`]. A constraint naming something missing or of
    /// the wrong kind is left out rather than trusted.
    pub fn new(sketch: &Sketch, fixing: Fixing) -> Self {
        System::toward(sketch, fixing, &HashMap::new())
    }

    /// [`System::new`], a point on a spline that's among `targets`
    /// taking its parameter from the place nearest its target rather
    /// than the place nearest it: dragged along a spline, it starts where
    /// it's going, not across from where it was.
    pub fn toward(sketch: &Sketch, fixing: Fixing, targets: &HashMap<Id, DVec2>) -> Self {
        let mut fixed = HashSet::new();
        for entry in &sketch.constraints {
            if let Constraint::Fix(item) = entry.constraint {
                fixed.insert(item);
                if let Some(curve) = sketch.curve(item) {
                    fixed.extend(curve.curve.points());
                }
            }
        }
        let constant = fixing == Fixing::Constants;
        // A link's points and radii are constants however a fix is
        // modelled, as the origin's are: nothing in the sketch moves them.
        let linked = sketch.linked();
        let held = |id: &Id| (constant && fixed.contains(id)) || linked.contains(id);
        let mut values = Vec::new();
        let mut slot = |value: f64, held: bool| {
            if held {
                Slot::Const(value)
            } else {
                values.push(value);
                Slot::Var(values.len() - 1)
            }
        };
        let points: Vec<PointSlots> = sketch
            .points
            .iter()
            .map(|point| {
                let held = held(&point.id);
                [slot(point.at.x, held), slot(point.at.y, held)]
            })
            .collect();
        let radii: Vec<Option<Slot>> = sketch
            .curves
            .iter()
            .map(|entry| match entry.curve {
                Curve::Circle { radius, .. } => Some(slot(radius, held(&entry.id))),
                _ => None,
            })
            .collect();
        let mut system = System {
            values,
            equations: Vec::new(),
            points,
            radii,
            bounds: Vec::new(),
            splines: HashMap::new(),
        };
        system.read_splines(sketch);
        let images = images(sketch);
        let mirrored = mirrored_arcs(sketch, &images);
        for entry in &sketch.curves {
            // A link's arc is held as found: its radii are constants,
            // equal to rounding, so no equation of its own. A closed
            // arc's one point has the one radius.
            if let Curve::Arc { center, start, end } = entry.curve
                && start != end
                && !mirrored.contains(&entry.id)
                && !linked.contains(&entry.id)
                && let (Some(center), Some(start), Some(end)) = (
                    system.point(sketch, center),
                    system.point(sketch, start),
                    system.point(sketch, end),
                )
            {
                system.push(entry.id, Residual::ArcRadius { center, start, end });
            }
        }
        let mirrored = mirrored_corners(sketch, &images);
        for entry in &sketch.curves {
            let Some(corner) = entry.corner else {
                continue;
            };
            match mirrored.get(&entry.id) {
                Some((about, shared)) => system.implied(sketch, entry.id, *about, shared),
                None => {
                    for residual in system.corner(sketch, entry, corner).into_iter().flatten() {
                        system.push(entry.id, residual);
                    }
                }
            }
        }
        for entry in &sketch.constraints {
            system.add(sketch, entry.id, &entry.constraint, fixing, targets);
        }
        for entry in &sketch.dimensions {
            let dimension = &entry.dimension;
            if !dimension.driving {
                continue;
            }
            let residual =
                match dimension.measure {
                    Measure::Offset(a, b) => system
                        .pair_read(sketch, entry.id, [a, b], targets)
                        .map(|pair| Residual::PairOffset {
                            pair,
                            sign: dimension.side.sign(),
                            value: dimension.value.value,
                        }),
                    _ => system.dimension(sketch, dimension),
                };
            if let Some(residual) = residual {
                system.push(entry.id, residual);
            }
        }
        system
    }

    /// Maps the splines the constraints and dimensions read (a point on
    /// one, a tangent or a smooth join, a point's offset from one), each
    /// once, with their parameters found from
    /// where their points are (see [`SplineSlots`]), and the Bézier
    /// segments of those points are on.
    fn read_splines(&mut self, sketch: &Sketch) {
        // Each spline read, and whether a point is on it.
        let mut read: HashMap<Id, bool> = HashMap::new();
        let spline_pair =
            |[a, b]: [Id; 2]| (sketch.offset_pair([a, b]) == Some(OffsetPair::Spline)).then_some(a);
        for entry in &sketch.dimensions {
            if let Measure::Offset(a, b) = entry.dimension.measure
                && entry.dimension.driving
                && let Some(spline) = spline_pair([a, b])
            {
                read.insert(spline, true);
            }
        }
        for entry in &sketch.constraints {
            match entry.constraint {
                Constraint::PointOnCurve { curve, .. } => {
                    read.insert(curve, true);
                }
                Constraint::EqualOffset { a, b } => {
                    for spline in [a, b].into_iter().filter_map(spline_pair) {
                        read.insert(spline, true);
                    }
                }
                Constraint::Tangent {
                    a, b, at: Some(_), ..
                }
                | Constraint::Smooth { a, b, .. } => {
                    for id in [a, b] {
                        read.entry(id).or_insert(false);
                    }
                }
                _ => {}
            }
        }
        let at = |id| sketch.point(id).map(|point| point.at);
        for (id, on) in read {
            let Some(spline) = sketch.spline(id) else {
                continue;
            };
            let Some((map, ids)) = spline.map(at) else {
                continue;
            };
            let Some(inputs) = ids.iter().map(|&id| self.point(sketch, id)).collect() else {
                continue;
            };
            let places: Vec<DVec2> = spline.points.iter().filter_map(|&id| at(id)).collect();
            let mut length: f64 = places
                .windows(2)
                .map(|pair| pair[0].distance(pair[1]))
                .sum();
            if spline.closed
                && let (Some(first), Some(last)) = (places.first(), places.last())
            {
                length += first.distance(*last);
            }
            let path = on
                .then(|| {
                    let places: Option<Vec<DVec2>> = ids.iter().map(|&id| at(id)).collect();
                    Some(map.spline(&places?)?.path())
                })
                .flatten();
            let refit = match spline.kind {
                SplineKind::Through => spline.handle_indices().map(|handles| Refit {
                    fit: spline.points.len(),
                    closed: spline.closed,
                    handles,
                }),
                SplineKind::Control => None,
            };
            let slots = SplineSlots {
                map: RefCell::new(map),
                inputs,
                length: length.max(1e-3),
                refit,
            };
            self.splines.insert(id, (Rc::new(slots), path));
        }
    }

    /// Holds the point `point` on the spline read as `spline` by
    /// equations of `source`'s, at a parameter of its own, a new variable
    /// starting at `t` (from 0 to 1). Within the spline's ends, if it's
    /// open.
    fn on_spline(
        &mut self,
        sketch: &Sketch,
        source: Id,
        point: Id,
        spline: Rc<SplineSlots>,
        t: f64,
    ) {
        let Some(slots) = self.point(sketch, point) else {
            return;
        };
        let at = self.parameter(spline, t);
        for axis in 0..2 {
            let residual = Residual::OnSpline {
                point: slots,
                at: at.clone(),
                axis,
            };
            self.push(source, residual);
        }
    }

    /// Where the spline read as `spline` is read at a parameter of its
    /// own, a new variable starting at `t` (from 0 to 1, as a length along
    /// it), kept within its ends if it's open.
    fn parameter(&mut self, spline: Rc<SplineSlots>, t: f64) -> SplineAt {
        let t = if t.is_finite() { t } else { 0.0 };
        self.values.push(t * spline.length);
        let var = self.values.len() - 1;
        if !spline.map.borrow().closed() {
            self.bounds.push((var, 0.0, spline.length));
        }
        SplineAt {
            spline,
            t: Slot::Var(var),
        }
    }

    /// The equations of a tangent (or with `smooth`, a smooth join) of
    /// `a` and `b` with a spline, at its end `at` (see [`Joint`]): `at`
    /// on the other, unless it's the other's end too; the two running
    /// along each other there, the same way or opposite ways as `side`
    /// says; and smooth, curving alike.
    fn joined(
        &self,
        sketch: &Sketch,
        [a, b]: [Id; 2],
        at: Id,
        side: Side,
        smooth: bool,
    ) -> Option<Vec<Residual>> {
        let joint = Joint::of(sketch, a, b, at)?;
        let point = self.point(sketch, at)?;
        let heading = |id| {
            if let Some(line) = self.line(sketch, id) {
                return Some(Heading::Line(line));
            }
            if let Some(round) = self.round(sketch, id) {
                return Some(Heading::Round { round, at: point });
            }
            let spline = sketch.spline(id)?;
            let slots = self.splines.get(&id)?.0.clone();
            let t = Slot::Const(spline.end_param(at)? * slots.length);
            Some(Heading::Spline(SplineAt { spline: slots, t }))
        };
        let (a, b, sign) = (heading(a)?, heading(b)?, side.sign());
        let mut residuals = Vec::new();
        if !joint.shared {
            residuals.push(match self.line(sketch, joint.other) {
                Some(line) => Residual::OnLine { point, line },
                None => Residual::OnRound {
                    point,
                    round: self.round(sketch, joint.other)?,
                },
            });
        }
        if smooth {
            residuals.push(Residual::Curving {
                a: a.clone(),
                b: b.clone(),
                sign,
            });
        }
        residuals.push(Residual::Along { a, b, sign });
        Some(residuals)
    }

    fn push(&mut self, source: Id, residual: Residual) {
        self.equations.push(Equation {
            residual,
            source,
            implied: false,
        });
    }

    /// Holds the points `shared` of the mirrored fillet or chamfer
    /// `source`, their own images, on the line `about`, as the mirror
    /// relation its equations are left out for implies (see
    /// [`mirrored_corners`]). Each is [`Equation::implied`]: what else
    /// holds the point there, as a line on the axis drawn from the origin
    /// does, makes it no redundancy.
    fn implied(&mut self, sketch: &Sketch, source: Id, about: Id, shared: &[Id]) {
        let Some(line) = self.line(sketch, about) else {
            return;
        };
        for &point in shared {
            if let Some(point) = self.point(sketch, point) {
                self.equations.push(Equation {
                    residual: Residual::OnLine { point, line },
                    source,
                    implied: true,
                });
            }
        }
    }

    /// The slots of the point `id`: the origin's are constants.
    pub fn point(&self, sketch: &Sketch, id: Id) -> Option<PointSlots> {
        if id == Id::ORIGIN {
            return Some(constants(DVec2::ZERO));
        }
        sketch.point_index(id).map(|index| self.points[index])
    }

    /// The slots of the line `id`: an axis's are constants, from the
    /// origin a unit along it.
    fn line(&self, sketch: &Sketch, id: Id) -> Option<LineSlots> {
        if let Some((start, end)) = axis(id) {
            return Some(LineSlots {
                start: constants(start),
                end: constants(end),
            });
        }
        match sketch.curve(id)?.curve {
            Curve::Line { start, end } => Some(LineSlots {
                start: self.point(sketch, start)?,
                end: self.point(sketch, end)?,
            }),
            _ => None,
        }
    }

    /// The slots of what an angle is measured along: a line, or a handle
    /// named by its tip, from its fit point to its tip.
    fn direction(&self, sketch: &Sketch, id: Id) -> Option<LineSlots> {
        if let Some(line) = self.line(sketch, id) {
            return Some(line);
        }
        let (_, handle) = sketch.handle(id)?;
        Some(LineSlots {
            start: self.point(sketch, handle.at)?,
            end: self.point(sketch, handle.tip)?,
        })
    }

    fn round(&self, sketch: &Sketch, id: Id) -> Option<RoundSlots> {
        let index = sketch.curve_index(id)?;
        match sketch.curves[index].curve {
            Curve::Circle { center, .. } => Some(RoundSlots::Circle {
                center: self.point(sketch, center)?,
                radius: self.radii[index]?,
            }),
            Curve::Arc { center, start, .. } => Some(RoundSlots::Arc {
                center: self.point(sketch, center)?,
                start: self.point(sketch, start)?,
            }),
            Curve::Line { .. } | Curve::Spline(_) => None,
        }
    }

    /// The equations the fillet or chamfer `entry` on `corner` implies
    /// (see [`Corner`]), or none if it names what the sketch doesn't have.
    /// A fillet's radius at each end is at right angles to the line, on
    /// the inside of the corner: at a right angle clockwise from the way
    /// along its start's line from the corner, and anticlockwise from its
    /// end's, as an [`Residual::Angle`], which has no mirror image.
    fn corner(&self, sketch: &Sketch, entry: &CurveEntry, corner: Corner) -> Option<Vec<Residual>> {
        let point = |id| self.point(sketch, id);
        let at = point(corner.at)?;
        // The way along a line from the corner.
        let leg = |line: Id| match sketch.curve(line)?.curve {
            Curve::Line { start, end } => Some(LineSlots {
                start: at,
                end: point(if start == corner.at { end } else { start })?,
            }),
            _ => None,
        };
        let (a, b) = (self.line(sketch, corner.a)?, self.line(sketch, corner.b)?);
        Some(match entry.curve {
            Curve::Arc { center, start, end } => {
                let (center, start, end) = (point(center)?, point(start)?, point(end)?);
                let square = |leg: LineSlots, from: PointSlots, sin: f64| Residual::Angle {
                    a: leg,
                    b: LineSlots {
                        start: from,
                        end: center,
                    },
                    sign: 1.0,
                    cos: 0.0,
                    sin,
                };
                vec![
                    Residual::OnLine {
                        point: start,
                        line: a,
                    },
                    Residual::OnLine {
                        point: end,
                        line: b,
                    },
                    square(leg(corner.a)?, start, -1.0),
                    square(leg(corner.b)?, end, 1.0),
                ]
            }
            Curve::Line { start, end } => {
                let (start, end) = (point(start)?, point(end)?);
                let mut residuals = vec![
                    Residual::OnLine {
                        point: start,
                        line: a,
                    },
                    Residual::OnLine {
                        point: end,
                        line: b,
                    },
                ];
                if corner.equal {
                    residuals.push(Residual::EqualLength(
                        LineSlots {
                            start: at,
                            end: start,
                        },
                        LineSlots { start: at, end },
                    ));
                }
                residuals
            }
            Curve::Circle { .. } | Curve::Spline(_) => return None,
        })
    }

    /// Adds the equations of `constraint`, or none if it names what the
    /// sketch doesn't have.
    fn add(
        &mut self,
        sketch: &Sketch,
        source: Id,
        constraint: &Constraint,
        fixing: Fixing,
        targets: &HashMap<Id, DVec2>,
    ) {
        if let Constraint::EqualOffset { a, b } = *constraint {
            if let (Some(a), Some(b)) = (
                self.pair_read(sketch, source, a, targets),
                self.pair_read(sketch, source, b, targets),
            ) {
                self.push(source, Residual::EqualOffset(a, b));
            }
            return;
        }
        if let Constraint::PointOnCurve { point, curve } = *constraint
            && let Some((spline, Some(path))) = self.splines.get(&curve)
        {
            if let Some(t) = start_on(sketch, targets, point, path) {
                self.on_spline(sketch, source, point, spline.clone(), t);
            }
            return;
        }
        let residuals = self.residuals(sketch, constraint, fixing);
        for residual in residuals.into_iter().flatten() {
            self.push(source, residual);
        }
    }

    fn residuals(
        &self,
        sketch: &Sketch,
        constraint: &Constraint,
        fixing: Fixing,
    ) -> Option<Vec<Residual>> {
        let point = |id| self.point(sketch, id);
        let line = |id| self.line(sketch, id);
        let direction = |id| self.direction(sketch, id);
        let round = |id| self.round(sketch, id);
        // Both coordinates equal, or only x (0) or y (1).
        let equal = |a: PointSlots, b: PointSlots, axes: &[usize]| {
            axes.iter()
                .map(|&axis| Residual::Equal(a[axis], b[axis]))
                .collect()
        };
        Some(match *constraint {
            Constraint::Coincident(a, b) => equal(point(a)?, point(b)?, &[0, 1]),
            Constraint::PointOnCurve { point: p, curve } => {
                let p = point(p)?;
                vec![match line(curve) {
                    Some(line) => Residual::OnLine { point: p, line },
                    None => Residual::OnRound {
                        point: p,
                        round: round(curve)?,
                    },
                }]
            }
            Constraint::Horizontal(id) => {
                let line = direction(id)?;
                equal(line.start, line.end, &[1])
            }
            Constraint::Vertical(id) => {
                let line = direction(id)?;
                equal(line.start, line.end, &[0])
            }
            Constraint::HorizontalPoints(a, b) => equal(point(a)?, point(b)?, &[1]),
            Constraint::VerticalPoints(a, b) => equal(point(a)?, point(b)?, &[0]),
            Constraint::Parallel(a, b) => vec![Residual::Parallel(direction(a)?, direction(b)?)],
            Constraint::Perpendicular(a, b) => {
                vec![Residual::Perpendicular(direction(a)?, direction(b)?)]
            }
            // Tangent where they meet at an end of each, the distance and
            // radius are equal to first order about the tangency, which
            // would be no equation to the analysis: the radius to that
            // end is at right angles to the line there, or along the
            // other's radius, instead.
            Constraint::Tangent {
                a,
                b,
                side,
                at: Some(at),
            } => return self.joined(sketch, [a, b], at, side, false),
            Constraint::Smooth { a, b, at, side } => {
                return self.joined(sketch, [a, b], at, side, true);
            }
            Constraint::Tangent { a, b, .. } if let Some(end) = shared_end(sketch, a, b) => {
                let end = point(end)?;
                let radius = |id| {
                    Some(LineSlots {
                        start: round(id)?.center(),
                        end,
                    })
                };
                vec![match (line(a), line(b)) {
                    (Some(line), None) => Residual::Perpendicular(line, radius(b)?),
                    (None, Some(line)) => Residual::Perpendicular(line, radius(a)?),
                    (None, None) => Residual::Parallel(radius(a)?, radius(b)?),
                    (Some(_), Some(_)) => return None,
                }]
            }
            Constraint::Tangent { a, b, side, .. } => {
                let sign = side.sign();
                vec![match (line(a), line(b)) {
                    (Some(line), None) => Residual::TangentLine {
                        line,
                        round: round(b)?,
                        sign,
                    },
                    (None, Some(line)) => Residual::TangentLine {
                        line,
                        round: round(a)?,
                        sign,
                    },
                    (None, None) => Residual::TangentRounds {
                        a: round(a)?,
                        b: round(b)?,
                        sign,
                    },
                    (Some(_), Some(_)) => return None,
                }]
            }
            Constraint::Equal(a, b) => vec![match (line(a), line(b)) {
                (Some(a), Some(b)) => Residual::EqualLength(a, b),
                _ => Residual::EqualRadius(round(a)?, round(b)?),
            }],
            Constraint::Concentric(a, b) => {
                let center = |id| point(id).or_else(|| Some(round(id)?.center()));
                equal(center(a)?, center(b)?, &[0, 1])
            }
            Constraint::Midpoint { point: p, line: l } => {
                let (p, l) = (point(p)?, line(l)?);
                (0..2)
                    .map(|axis| Residual::Midpoint {
                        point: p[axis],
                        a: l.start[axis],
                        b: l.end[axis],
                    })
                    .collect()
            }
            Constraint::Symmetric { a, b, about } => {
                let (a, b, line) = (point(a)?, point(b)?, line(about)?);
                vec![
                    Residual::MidpointOn { a, b, line },
                    Residual::Across { a, b, line },
                ]
            }
            // Its pairs may add equations of their own: see `System::add`.
            Constraint::EqualOffset { .. } => return None,
            Constraint::Fix(item) => {
                if fixing == Fixing::Constants {
                    return Some(Vec::new());
                }
                let mut slots: Vec<Slot> = Vec::new();
                let mut residuals = Vec::new();
                if let Some(p) = point(item) {
                    slots.extend(p);
                } else {
                    let index = sketch.curve_index(item)?;
                    let entry = &sketch.curves[index];
                    match entry.curve {
                        // Its radius equation holds the end at the
                        // start's distance, so only the end's direction
                        // from the centre is left to fix: nothing for a
                        // closed arc's.
                        Curve::Arc { center, start, end } if start == end => {
                            slots.extend(point(center)?);
                            slots.extend(point(start)?);
                        }
                        Curve::Arc { center, start, end } => {
                            let (center, end) = (point(center)?, point(end)?);
                            slots.extend(center);
                            slots.extend(point(start)?);
                            let at = |p: PointSlots| p.map(|slot| self.value(slot));
                            let ([cx, cy], [ex, ey]) = (at(center), at(end));
                            let (dx, dy) = (ex - cx, ey - cy);
                            let length = angle::hypot(dx, dy);
                            if length > 0.0 {
                                residuals.push(Residual::FixDirection {
                                    point: end,
                                    center,
                                    direction: [dx / length, dy / length],
                                });
                            } else {
                                slots.extend(end);
                            }
                        }
                        _ => {
                            for id in entry.curve.points() {
                                slots.extend(point(id)?);
                            }
                            slots.extend(self.radii[index]);
                        }
                    }
                }
                residuals.extend(slots.into_iter().map(|slot| Residual::Fix {
                    slot,
                    at: self.value(slot),
                }));
                residuals
            }
        })
    }

    /// The equation of a driving dimension, or none if it names what the
    /// sketch doesn't have. Not an offset's, see [`System::pair_read`].
    fn dimension(&self, sketch: &Sketch, dimension: &Dimension) -> Option<Residual> {
        let point = |id| self.point(sketch, id);
        let line = |id| self.line(sketch, id);
        let (value, sign) = (dimension.value.value, dimension.side.sign());
        // The midpoint of `a` and `b` from the line.
        let from_line = |a, b, line| Residual::LineDistance {
            a,
            b,
            line,
            sign,
            value,
        };
        // Along x (0) or y (1).
        let offset = |a, b, axis: usize| {
            let (a, b): (PointSlots, PointSlots) = (point(a)?, point(b)?);
            Some(Residual::Offset {
                a: a[axis],
                b: b[axis],
                sign,
                value,
            })
        };
        Some(match dimension.measure {
            Measure::Distance(a, b) => match (point(a), point(b)) {
                (Some(a), Some(b)) => Residual::Distance { a, b, value },
                (Some(p), None) => from_line(p, p, line(b)?),
                (None, Some(p)) => from_line(p, p, line(a)?),
                (None, None) => {
                    let from = line(a)?;
                    from_line(from.start, from.end, line(b)?)
                }
            },
            Measure::HorizontalDistance(a, b) => offset(a, b, 0)?,
            Measure::VerticalDistance(a, b) => offset(a, b, 1)?,
            // A handle's whole line is twice its fit point's distance
            // from its tip.
            Measure::Length(id) => match line(id) {
                Some(line) => Residual::Distance {
                    a: line.start,
                    b: line.end,
                    value,
                },
                None => {
                    let half = self.direction(sketch, id)?;
                    Residual::Distance {
                        a: half.start,
                        b: half.end,
                        value: value / 2.0,
                    }
                }
            },
            Measure::Angle(a, b) => Residual::Angle {
                a: self.direction(sketch, a)?,
                b: self.direction(sketch, b)?,
                sign,
                cos: angle::cos(value),
                sin: angle::sin(value),
            },
            Measure::Radius(id) => Residual::Radius {
                round: self.round(sketch, id)?,
                value,
            },
            Measure::Diameter(id) => Residual::Radius {
                round: self.round(sketch, id)?,
                value: value / 2.0,
            },
            // Its pair may add equations of its own: see `System::toward`.
            Measure::Offset(..) => return None,
            Measure::EdgeDistance(round, other) => Residual::EdgeGap {
                round: self.round(sketch, round)?,
                to: match (point(other), line(other)) {
                    (Some(p), _) => EdgeTo::Point(p),
                    (None, Some(l)) => EdgeTo::Line(l),
                    (None, None) => EdgeTo::Round(self.round(sketch, other)?),
                },
                sign,
                value,
            },
        })
    }

    /// The slots of the offset pair `[a, b]`, see [`Sketch::offset_pair`].
    fn pair(&self, sketch: &Sketch, [a, b]: [Id; 2]) -> Option<PairSlots> {
        Some(match sketch.offset_pair([a, b])? {
            OffsetPair::Lines => PairSlots::Lines {
                line: self.line(sketch, a)?,
                copy: self.line(sketch, b)?,
            },
            OffsetPair::Rounds => PairSlots::Rounds {
                round: self.round(sketch, a)?,
                copy: self.round(sketch, b)?,
            },
            OffsetPair::Join => PairSlots::Join {
                copy: self.round(sketch, b)?,
            },
            // Read at a parameter of its own, see `System::spline_pair`.
            OffsetPair::Spline => return None,
        })
    }

    /// The offset pair `[spline, point]` of a spline and a point, read at
    /// a parameter of the point's own, a new variable starting at the
    /// place on the spline nearest the point, or with the point dragged
    /// among `targets`, nearest its target, held there by a
    /// [`Residual::Nearest`] of `source`'s. `None` for a spline not read
    /// or a point missing.
    fn spline_pair(
        &mut self,
        sketch: &Sketch,
        source: Id,
        [spline, point]: [Id; 2],
        targets: &HashMap<Id, DVec2>,
    ) -> Option<PairRead> {
        let (slots, Some(path)) = self.splines.get(&spline)? else {
            return None;
        };
        let slots = slots.clone();
        let t = start_on(sketch, targets, point, path)?;
        let point = self.point(sketch, point)?;
        let at = self.parameter(slots, t);
        self.push(
            source,
            Residual::Nearest {
                point,
                at: at.clone(),
            },
        );
        Some(PairRead::Spline { point, at })
    }

    /// The offset pair `pair` as an equation of `source`'s reads it: a
    /// spline and a point at a parameter of the point's own (see
    /// [`System::spline_pair`]), or else by its slots.
    fn pair_read(
        &mut self,
        sketch: &Sketch,
        source: Id,
        pair: [Id; 2],
        targets: &HashMap<Id, DVec2>,
    ) -> Option<PairRead> {
        match sketch.offset_pair(pair)? {
            OffsetPair::Spline => self.spline_pair(sketch, source, pair, targets),
            _ => Some(PairRead::Slots(self.pair(sketch, pair)?)),
        }
    }

    pub fn value(&self, slot: Slot) -> f64 {
        match slot {
            Slot::Var(index) => self.values[index],
            Slot::Const(value) => value,
        }
    }

    /// Splits the variables into the sets the equations tie together,
    /// each with its equations. Variables no equation reads are in none;
    /// equations reading no variable are returned apart, as `constant`.
    pub fn components(&self) -> (Vec<Component>, Vec<usize>) {
        let mut sets = Sets::new(self.values.len());
        let mut first = Vec::with_capacity(self.equations.len());
        for equation in &self.equations {
            let mut head = None;
            equation.residual.slots(|slot| {
                if let Slot::Var(var) = slot {
                    match head {
                        None => head = Some(var),
                        Some(head) => sets.join(head, var),
                    }
                }
            });
            first.push(head);
        }
        // Components numbered in the order their lowest variable comes.
        let mut number = vec![usize::MAX; self.values.len()];
        let mut components: Vec<Component> = Vec::new();
        let mut used = vec![false; self.values.len()];
        for head in first.iter().flatten() {
            used[sets.root(*head)] = true;
        }
        for var in 0..self.values.len() {
            let r = sets.root(var);
            if !used[r] {
                continue;
            }
            if number[r] == usize::MAX {
                number[r] = components.len();
                components.push(Component {
                    vars: Vec::new(),
                    equations: Vec::new(),
                });
            }
            components[number[r]].vars.push(var);
        }
        let mut constant = Vec::new();
        for (index, head) in first.into_iter().enumerate() {
            match head {
                Some(head) => {
                    let r = sets.root(head);
                    components[number[r]].equations.push(index);
                }
                None => constant.push(index),
            }
        }
        (components, constant)
    }

    /// Writes the variables' values back into `sketch`, the one the
    /// system was made from.
    pub fn write(&self, sketch: &mut Sketch) {
        for (point, slots) in sketch.points.iter_mut().zip(&self.points) {
            point.at.x = self.value(slots[0]);
            point.at.y = self.value(slots[1]);
        }
        for (entry, radius) in sketch.curves.iter_mut().zip(&self.radii) {
            if let (Curve::Circle { radius: r, .. }, Some(slot)) = (&mut entry.curve, radius) {
                *r = self.value(*slot);
            }
        }
    }
}
