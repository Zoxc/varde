//! The precision schedule and the unit table: the levels every curved
//! sign and certificate is computed at, in a fixed order, and the units
//! each kind of computation is charged at each level.
//!
//! **Levels.** A computation runs at [`Level::L0`] first (`f64` with a
//! certified error bound, [`super::bounded`]), then, while it can't
//! settle, at the next level up ([`Level::next`]): double-double
//! ([`super::dd`]), then big-float at 212, 424 and 848 bits
//! ([`super::bigf`]). The sequence is the same for every item, on every
//! thread count and platform. It stops at [`P_MAX_BITS`]: an item still
//! open at [`Level::L4`] has reached the top of the schedule. A
//! computation is written once over the levels' common numeric trait and
//! stepped through them by one runner ([`super::level`]). Wider
//! widths would bound nothing tighter, as the error parts are `f64`
//! (a width's own rounding shows only above about `2^(bits − 1074)`).
//!
//! **Promotion is exact.** A value moves up a level with its value and
//! both error parts unchanged (`Bounded<f64>::promote` to double-double,
//! `Bounded<Dd>::promote` to the first width, `widen` between widths), so
//! a level never starts from a worse value than the one below ended
//! with.
//!
//! **Space or precision.** An item whose bound doesn't settle escalates
//! in precision while its rounding error dominates, `err_round >
//! err_trunc`, and in space (a subdivision, a smaller box, a local frame)
//! otherwise ([`escalation`]): a comparison of certified values, so the
//! choice is deterministic.
//!
//! **Units.** Every charged computation costs a constant number of
//! units by its kind and level ([`UNITS`]), one unit about half a
//! microsecond on the reference machine. Exact flat predicates are
//! priced per predicate kind ([`PredKind`]): each [`Pred`] names its
//! kind, and its filter and exact runs are charged at that kind's row; a
//! predicate of variable length (a polygon's signed area) is charged per
//! term at its term's kind.
//!
//! [`Pred`]: super::exact::Pred

use super::bigf::Width;
use super::bounded::Bounded;

/// A precision level of the schedule (module docs), lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// `f64` with a certified error bound.
    L0,
    /// Double-double, without fused multiply-add.
    L1,
    /// Big-float at 212 bits.
    L2,
    /// Big-float at 424 bits.
    L3,
    /// Big-float at 848 bits, [`P_MAX_BITS`]: the top of the schedule.
    L4,
}

/// The number of precision levels: `f64`, double-double, then the
/// big-float widths ([`Width::ALL`]).
pub const LEVELS: usize = 2 + Width::ALL.len();

/// The widest precision of the schedule, in bits: [`Level::L4`]'s width.
/// An item still open there ends as running out of precision.
pub const P_MAX_BITS: u32 = 848;

const _: () = assert!(Width::MAX.bits() == P_MAX_BITS);
const _: () = assert!(Level::ALL.len() == LEVELS);

impl Level {
    /// Every level, in the order an item escalates through them.
    pub const ALL: [Level; 5] = [Level::L0, Level::L1, Level::L2, Level::L3, Level::L4];

    /// The top of the schedule.
    pub const TOP: Level = Level::L4;

    /// The level's position in the schedule, from 0 (`f64`): the index of
    /// its row in the unit table's per-level arrays.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The level at position `i`, if the schedule has one.
    pub const fn from_index(i: usize) -> Option<Level> {
        if i < LEVELS {
            Some(Level::ALL[i])
        } else {
            None
        }
    }

    /// The level an item escalates to from this one, `None` at the top.
    pub const fn next(self) -> Option<Level> {
        Level::from_index(self.index() + 1)
    }

    /// The level that computes in big-float at width `w`.
    pub fn of_width(w: Width) -> Level {
        match Width::ALL.iter().position(|x| *x == w) {
            Some(0) => Level::L2,
            Some(1) => Level::L3,
            _ => Level::L4,
        }
    }

    /// The big-float width the level computes at; `None` for `f64` and
    /// double-double.
    pub const fn width(self) -> Option<Width> {
        match self {
            Level::L0 | Level::L1 => None,
            Level::L2 => Some(Width::ALL[0]),
            Level::L3 => Some(Width::ALL[1]),
            Level::L4 => Some(Width::ALL[2]),
        }
    }
}

/// How an item that doesn't settle at its level escalates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Escalation {
    /// Refine in space: its truncation error is at least its rounding
    /// error.
    Space,
    /// Recompute at the next level: its rounding error dominates.
    Precision,
}

/// How `b` escalates if it doesn't settle: in precision when its rounding
/// error dominates, `err_round > err_trunc`, else in space (module docs).
/// An uncertified part (infinite) dominates a finite one; a NaN part
/// compares false either way, so a NaN rounding error refines in space.
pub fn escalation<T>(b: &Bounded<T>) -> Escalation {
    if b.rounding_dominates() {
        Escalation::Precision
    } else {
        Escalation::Space
    }
}

/// The most halvings of one item, counted from the item's own start (a
/// seam span, a profile pair, a box from its tile): refinement in space
/// stops there. 48 halvings take the 4·10⁶ mm base tile down to about
/// 10⁻⁸ mm, the kernel's resolution.
pub const MAX_SUBDIV_DEPTH: u32 = 48;

/// The parts of a unit a cost is counted in: the kinds a computation runs
/// many of ([`RecordUnits`], [`ExactUnits`]) take less than a unit each,
/// so they are priced in parts, summed per record and rounded up to
/// whole units once.
pub const UNIT_PARTS: u64 = 64;

/// The kind of an exact flat predicate, for its price: predicates of one
/// kind have one degree and about one size, so their filter and exact
/// runs cost about the same, within the calibration's band. Each
/// [`Pred`](super::exact::Pred) names its own; the calibration benchmark
/// times each kind's stages on its predicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PredKind {
    /// A difference of two points along a fixed direction, `a·(w − q)`:
    /// a point's side of a shadow line or its place along it, its height
    /// above another point; also a point's side of a stored plane, `n·p −
    /// c`, of the same degree and fewer terms.
    Side,
    /// A 2D orientation `orient(a, b, c)`, or another sum of a few
    /// products of two differences: a dot product of two segments, a
    /// distance squared against a radius squared; also each term of a
    /// polygon's signed area, a form of any length charged per term.
    Orient2,
    /// A 3×3 determinant of differences: a 3D orientation, a segment's
    /// side of a line, a point's side of a triangle's plane, a point
    /// against a line's shadow (`det(UP, b − a, q − a)`), a plane's height
    /// above a point, a shared curve's crossing row at a stored point
    /// (`UP·(T × Δ)`, `T` a chord, `Δ` a rate); also another form of
    /// degree three, a place along a line at a parameter against a point
    /// (`dir·(e₀ + t·(e₁ − e₀) − p)`).
    Orient3,
    /// Two edges' crossing along `UP`, `(UP·m)·(m·(g₀ − e₀))` with `m`
    /// the cross product of their directions: degree four; also the
    /// height gap of two lines on one vertical plane at a parameter.
    Crossing,
    /// A shadow row of homogeneous points, `X/W` with `X`, `W` exact
    /// sums of weighted control points.
    Hom,
}

impl PredKind {
    /// The number of kinds.
    pub const COUNT: usize = 5;

    /// Every kind, in the unit table's order.
    pub const ALL: [PredKind; PredKind::COUNT] = [
        PredKind::Side,
        PredKind::Orient2,
        PredKind::Orient3,
        PredKind::Crossing,
        PredKind::Hom,
    ];

    /// The kind's row in [`RecordUnits::exact`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// The units of one computation of each kind the kernel charges: the one
/// table of unit costs. A unit is about half a microsecond on one thread
/// of the reference machine (an AMD Ryzen 7 5700X3D), each kind's value
/// measured by the kernel's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which fails when a kind's time
/// per unit drifts more than 4× from it; the kernel's `MAX_WORK` is set
/// from the same measurement. The values are constants, so charges are
/// the same on every run, thread count and platform; changing one
/// changes where operations run out, a versioned change like one to the
/// precision schedule. A kind gets its row with the loop that charges it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Units {
    /// A seam-bound cell (the kernel's `geom::certify`) at each precision
    /// level ([`LEVELS`]): its spans restricted and halved down to it, the
    /// tile composed with the p-curve (degree 8) and the bound formed
    /// against the curve (degree 10).
    pub seam_cell: [u64; LEVELS],
    /// A cell of a curve's distance to an implicit surface (the kernel's
    /// `geom::certify::implicit`) at each level: its span halved down to
    /// it, put into the implicit, and the Bernstein net of the
    /// implicit's squared gradient formed over its hull box, twice.
    pub implicit_cell: ImplicitUnits,
    /// A piece of a trimming span a trim membership query tests (the
    /// kernel's `geom::query`) at each level: the span formed at the
    /// level, halved down to the piece and put into the query point's two
    /// lines and its near box's four.
    pub trim_piece: [u64; LEVELS],
    /// A cell of an extremum query (the kernel's `geom::query`) at each
    /// level, by what it bounds.
    pub extremum_cell: ExtremumUnits,
    /// A piece of a face's count tested
    /// ([`count_pieces`](crate::count::s02::count_pieces)) at each level:
    /// a box classified against the trims, split, or tested off the
    /// query point's lines, a side's or a run's degree attempted (settled
    /// or not), a trim span scanned against a loop's side.
    pub count_piece: [u64; LEVELS],
    /// A piece of a face's counting setup
    /// ([`Corners::new`](crate::count::boxes::Corners::new), built once
    /// per operation at the first level): a piece of a trimming curve
    /// whose hull in space a corner's run search forms.
    pub face_setup: u64,
    /// A piece an edge pair's cluster search tests
    /// ([`clusters`](crate::count::xrec::clusters)) at each level: a hull
    /// of a box's two pieces, a tangent cone, a side's turn, a band test.
    pub xrec_piece: [u64; LEVELS],
    /// The work a record's decision reports
    /// ([`Cost`](crate::records::shadow::Cost)), in parts.
    pub record: RecordUnits,
    /// The near/far distance certificate of a square of the kernel's
    /// label audit, in parts.
    pub audit: AuditUnits,
    /// An operation's scan of its operands' stored values at its start
    /// ([`OperandScan`](super::storage::OperandScan)), in parts.
    pub operand_scan: ScanUnits,
    /// A form key's parameter correctly rounded (the kernel's
    /// `geom::formkey`), in parts.
    pub form_key: KeyUnits,
    /// The broad phase's bounding volume hierarchy (the kernel's
    /// `infra::bvh`): its leaves built, its box tests and hits, in parts.
    pub bvh: BvhUnits,
    /// A tile's fold check and normal cone search (the kernel's
    /// `geom::tile`): its exact part once, then the search at each
    /// level it runs at.
    pub fold: FoldUnits,
    /// A p-curve fitted to a curve on a surface with the same parameter
    /// (the kernel's `geom::pcurve_fit`): its points and its spans.
    pub fit: FitUnits,
    /// A perturbation direction's exact solves
    /// ([`DirWork`](crate::perturb::dir::DirWork)), in parts.
    pub direction: DirectionUnits,
    /// The local certificates of faces sharing an edge or a vertex (the
    /// kernel's `topo::adjacent`), per cell or box at each level.
    pub adjacent: AdjacentUnits,
}

/// The units of the adjacent pairs' local certificates
/// ([`Units::adjacent`]) at each level ([`LEVELS`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdjacentUnits {
    /// A cell of an edge's field where a face's p-curve span is
    /// quadratic in a coordinate: the spans restricted and halved down to
    /// it, both faces' normal numerators formed at the level and put along
    /// their p-curves, the field and its two products with them formed,
    /// the strips' tiles chosen on the cell's control values.
    pub field: [u64; LEVELS],
    /// The same where both faces' spans are linear or iso.
    pub field_low: [u64; LEVELS],
    /// A box of an edge face's strip against the ruled surface where a
    /// face's span is quadratic in a coordinate: the spans restricted to
    /// it, the tile over the strip's map, the field halved down to it and
    /// the sign function (of degrees (28, 4) for quadratic spans and a
    /// conic) formed and judged.
    pub sign: [u64; LEVELS],
    /// The same where both faces' spans are linear or iso.
    pub sign_low: [u64; LEVELS],
    /// A box of an edge face's strip against the curve's plane: the tile
    /// over the strip's map in the plane's equation.
    pub plane: [u64; LEVELS],
    /// A box of a vertex sector: the tile over the sector's triangle in
    /// the plane's equation.
    pub sector: [u64; LEVELS],
}

/// The parts of a unit ([`UNIT_PARTS`]) of each step of a perturbation
/// direction's exact work ([`DirWork`](crate::perturb::dir::DirWork)):
/// rationals of a few normals' size, so a direction charges the sum of
/// its parts, rounded up to whole units once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionUnits {
    /// A small system solved: a Gram system of at most three unknowns
    /// formed and eliminated, its point formed.
    pub solve: u64,
    /// A constraint row checked against a point: one exact dot product
    /// and comparison.
    pub check: u64,
    /// A tableau row a simplex pivot updates.
    pub pivot: u64,
}

/// The units of a p-curve fit ([`Units::fit`]); its spans' seam bounds
/// are charged as seam cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FitUnits {
    /// A point of the curve, certified, its UV by the surface's analytic
    /// inverse and, at a knot, the surface's point there and its lower
    /// bound on the deviation.
    pub point: u64,
    /// A span: its least-squares middle point and the span formed.
    pub span: u64,
    /// A tile line inside a span's control values: its crossings found
    /// and the span cut there, exactly.
    pub line: u64,
}

/// The units of a tile's fold check and normal cone search
/// ([`Units::fold`]): the exact numerator's lines (or the whole
/// numerator) formed on the stored net once, then at each level the
/// search runs at the numerator formed in the tile's frame and divided by
/// its singular sides, and each piece of it whose cone is tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldUnits {
    /// A line of the exact numerator on a side of the tile, from the
    /// net's two rows there, in rationals.
    pub line: u64,
    /// The whole exact numerator and its quotient, in rationals (only a
    /// corner between two singular sides needs it).
    pub full: u64,
    /// The numerator formed at each precision level ([`LEVELS`]) in the
    /// tile's frame, divided by its singular sides.
    pub level: [u64; LEVELS],
    /// A piece of the numerator whose cone is tested at each level, its
    /// split from its parent included.
    pub piece: [u64; LEVELS],
}

/// The units of a cell of a curve's distance to an implicit surface
/// ([`Units::implicit_cell`]) at each level ([`LEVELS`]), by the
/// implicit's degree: its squared gradient's net grows with the cube of
/// the degree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImplicitUnits {
    /// A plane or a quadric, degree 1 or 2.
    pub quadric: [u64; LEVELS],
    /// A cubic or a quartic (a torus), degree 3 or 4.
    pub quartic: [u64; LEVELS],
}

/// The units of a cell of an extremum query ([`Units::extremum_cell`])
/// at each level ([`LEVELS`]): the form put into the direction at the
/// level, halved down to the cell, and its coefficients' ratios and
/// corner values bounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtremumUnits {
    /// A span of a curve, in space or in UV.
    pub span: [u64; LEVELS],
    /// A tile, biquadratic.
    pub tile: [u64; LEVELS],
}

/// The parts of a unit ([`UNIT_PARTS`]) of the kernel's bounding volume
/// hierarchy over boxes: a tree of many leaves, queried by many boxes,
/// each step far below a unit, so a build or a query charges the sum of
/// its parts, rounded up to whole units as it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BvhUnits {
    /// A leaf of a tree built: its box's centre and Morton code, its
    /// place in the sort, its internal node's range and split, its box in
    /// the bottom-up union.
    pub build: u64,
    /// A box a query tests against a node, the walk to it included.
    pub test: u64,
    /// A hit a query collects, its place in the sorted list included.
    pub hit: u64,
}

/// The parts of a unit ([`UNIT_PARTS`]) of rounding one parameter of a
/// form's key to its nearest `f64`: a key is a few such parameters,
/// each far below a unit at the first levels, so a key's construction
/// charges the sum of its parts, rounded up to whole units as it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyUnits {
    /// The parameter's exact value formed in rationals and its route
    /// read off: rounded there if it is rational (no radicand, or an
    /// exact square), else the irrational route's inputs.
    pub rational: u64,
    /// A step of the irrational route's loop at each precision level
    /// ([`LEVELS`]): the parameter's formula evaluated there and its two
    /// midpoint tests.
    pub level: [u64; LEVELS],
    /// A round of settling a parameter left open at the top level
    /// exactly: its root enclosed at a doubled precision and both ends
    /// rounded.
    pub settle: u64,
}

/// The parts of a unit ([`UNIT_PARTS`]) of each value an operation's
/// start scan of its operands visits ([`ScanSize`](super::storage::ScanSize)):
/// a few comparisons each, so a body's scan charges the sum of its
/// parts, rounded up to whole units once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanUnits {
    /// A stored position or an entity's few values: a vertex (its
    /// position and tolerance), an edge (its range and tolerance), a
    /// control point of a curve or p-curve (with its weight and knot), a
    /// surface's form (its lattice's origin and axes, its key's points,
    /// directions and radius).
    pub position: u64,
    /// A tile's net of control points and weights.
    pub net: u64,
}

/// The parts of a unit ([`UNIT_PARTS`]) of each piece of the near/far
/// distance certificate of one square of a planar face's label audit
/// (the kernel's `boolean::classify`). Its exact predicates (the square
/// against the region's loops, and the queries at far squares' centres)
/// are charged at their kinds' rows ([`RecordUnits::exact`]), not here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditUnits {
    /// The square itself: its centre evaluated on the face's tile and
    /// certified, and the reach of the square from it.
    pub square: u64,
    /// A segment of the region's loops or a leaf's box tested against
    /// the square's extent.
    pub test: u64,
    /// The distance from the square's centre to a triangle of the other
    /// operand's boundary bounded below and above.
    pub triangle: u64,
    /// The distance from the square's centre to a coedge's ribbon piece
    /// bounded below.
    pub ribbon: u64,
}

/// The parts of a unit ([`UNIT_PARTS`]) of one exact predicate's runs of
/// one kind ([`PredKind`]): an expansion run's length is bounded by the
/// predicate's degree, so each stage has one price per kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExactUnits {
    /// A filter run ([`Runs::filters`](super::exact::Runs::filters)).
    pub filter: u64,
    /// An exact run in expansions
    /// ([`Runs::expansions`](super::exact::Runs::expansions)).
    pub expansion: u64,
    /// A run in rationals
    /// ([`Runs::rationals`](super::exact::Runs::rationals)).
    pub rational: u64,
}

/// The parts of a unit ([`UNIT_PARTS`]) of each kind of work a record's
/// decision reports in its [`Cost`](crate::records::shadow::Cost).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordUnits {
    /// An exact predicate's runs, by its kind ([`PredKind::index`]).
    pub exact: [ExactUnits; PredKind::COUNT],
    /// A run in rationals outside a flat predicate (an exact polynomial
    /// formed, made square-free and its roots isolated;
    /// [`Cost::rationals`](crate::records::shadow::Cost::rationals)).
    pub rational: u64,
    /// A row or polynomial piece evaluated
    /// ([`LevelWork::evals`](crate::records::shadow::LevelWork::evals)),
    /// at each level.
    pub eval: [u64; LEVELS],
    /// A root isolation's split, interval test or Krawczyk step
    /// ([`LevelWork::roots`](crate::records::shadow::LevelWork::roots)),
    /// at each level.
    pub root: [u64; LEVELS],
    /// A foot polynomial formed and its roots' first test
    /// ([`LevelWork::feet`](crate::records::shadow::LevelWork::feet)), at
    /// each level: a span's distance to the query point squared less the
    /// radius squared, degree 4.
    pub foot_poly: [u64; LEVELS],
    /// A curved height key's value or row formed at a parameter
    /// ([`LevelWork::keys`](crate::records::shadow::LevelWork::keys)), at
    /// each level.
    pub key_at: [u64; LEVELS],
    /// An end test of a ray record's crossing beside an end
    /// ([`LevelWork::ends`](crate::records::shadow::LevelWork::ends)), at
    /// each level: the span's three polynomials bounded over the piece
    /// from the end.
    pub at_end: [u64; LEVELS],
    /// A sub-box of a curved z-order's crossing localization (a halving
    /// tested apart, a Krawczyk step;
    /// [`LevelWork::boxes`](crate::records::shadow::LevelWork::boxes)), at
    /// each level.
    pub zorder_box: [u64; LEVELS],
    /// A tile or ribbon span whose column hits are found exactly
    /// ([`Cost::hits`](crate::records::shadow::Cost::hits)): the column's
    /// equations eliminated to one polynomial in rationals and its roots
    /// isolated.
    pub hit: u64,
}

impl RecordUnits {
    /// The prices of `kind`'s exact runs.
    pub const fn of(&self, kind: PredKind) -> &ExactUnits {
        &self.exact[kind.index()]
    }
}

/// The unit costs ([`Units`]), as measured on the reference machine.
/// The record-evaluation kinds are measured on real record computations
/// at the first level (and a curved key's value at every level); where
/// no fixture reaches a level they are set from a measured row: a foot
/// polynomial at six evaluations, an end test at four, a z-order
/// sub-box at an edge pair's search piece. The exact predicates are
/// measured per kind on the kind's own predicates: general positions for
/// the filter, exactly degenerate ones for the expansions, degenerate
/// ones scaled far below unit size for the rationals. The label audit's
/// pieces are measured on squares of a plane face against cuboids'
/// triangles; the operand scan's on bodies of many vertices and line
/// edges, and of many planes' tiles. The distance-to-implicit cells are
/// measured on arcs near a sphere and near a torus, under the load of
/// other builds, scaled by the seam cells' time per unit in the same run.
/// The non-decision queries' trim pieces and extremum cells are measured
/// 16 halvings deep, on a circle in UV, a weighted quarter arc and a
/// sphere's tiles.
/// The tree's are measured on random
/// boxes of sizes on a log scale: built over a thousand and over sixty
/// thousand, queried by boxes meeting few of them (the tests) and by
/// boxes meeting most (the hits, past the tests). A direction's solves,
/// checks and pivots are measured on vertex stars of random `f64` normals
/// (three faces, and pyramids of four and six) and on a group's chain of
/// edges. The adjacent pairs' cells and boxes are measured on a quarter
/// arc's edge and a straight one's on a plane, both faces on it, and on a
/// corner's sector there.
pub const UNITS: Units = Units {
    seam_cell: [190, 310, 840, 890, 1060],
    implicit_cell: ImplicitUnits {
        quadric: [66, 150, 310, 330, 410],
        quartic: [1200, 1900, 4700, 5250, 6600],
    },
    trim_piece: [31, 39, 150, 160, 165],
    extremum_cell: ExtremumUnits {
        span: [15, 19, 82, 86, 94],
        tile: [55, 71, 370, 370, 400],
    },
    count_piece: [9, 10, 40, 40, 48],
    face_setup: 32,
    xrec_piece: [9, 10, 28, 30, 34],
    record: RecordUnits {
        exact: [
            // Side; its rationals measured on a point's side of a stored
            // plane, the one form of the kind stored floats take there.
            ExactUnits {
                filter: 23,
                expansion: 89,
                rational: 296,
            },
            // Orient2
            ExactUnits {
                filter: 20,
                expansion: 105,
                rational: 296,
            },
            // Orient3
            ExactUnits {
                filter: 48,
                expansion: 223,
                rational: 774,
            },
            // Crossing
            ExactUnits {
                filter: 65,
                expansion: 323,
                rational: 1276,
            },
            // Hom
            ExactUnits {
                filter: 180,
                expansion: 689,
                rational: 2041,
            },
        ],
        rational: 550,
        eval: [11, 180, 520, 550, 660],
        root: [130, 410, 1400, 1400, 1700],
        foot_poly: [64, 1080, 3120, 3300, 3960],
        key_at: [190, 460, 1240, 1360, 1600],
        at_end: [44, 720, 2080, 2200, 2640],
        zorder_box: [576, 640, 1792, 1920, 2176],
        hit: 5504,
    },
    audit: AuditUnits {
        square: 320,
        test: 1,
        triangle: 840,
        ribbon: 192,
    },
    operand_scan: ScanUnits {
        position: 1,
        net: 8,
    },
    form_key: KeyUnits {
        rational: 160,
        level: [40, 90, 250, 270, 330],
        settle: 460,
    },
    bvh: BvhUnits {
        build: 24,
        test: 2,
        hit: 2,
    },
    fold: FoldUnits {
        line: 160,
        full: 600,
        level: [570, 870, 2550, 2710, 3300],
        piece: [28, 33, 120, 125, 130],
    },
    fit: FitUnits {
        point: 8,
        span: 2,
        line: 220,
    },
    direction: DirectionUnits {
        solve: 5120,
        check: 128,
        pivot: 224,
    },
    adjacent: AdjacentUnits {
        field: [5900, 9600, 25800, 27700, 34300],
        field_low: [2800, 4200, 11000, 11800, 13600],
        sign: [2400, 3800, 9700, 10200, 12100],
        sign_low: [1000, 1350, 4000, 4300, 4900],
        plane: [1100, 1550, 4000, 4400, 5400],
        sector: [300, 410, 1550, 1700, 2000],
    },
};

#[cfg(test)]
mod tests;
