//! The Varde kernel's decision layer: numerics, Bernstein forms, records,
//! counting and the geometric order, generic over the patch type. So far
//! [`num::bounded`], values with certified error bounds, [`num::dd`],
//! double-double with them, [`num::bigf`], big-float with them,
//! [`num::expansion`], the exact floating-point expansions,
//! [`num::exact`], the exact flat predicates over them,
//! [`num::decide`], every record's sign with ties broken by symbolic
//! perturbation,
//! [`num::trig`], the deterministic trigonometry, [`bezier`], the
//! Bernstein forms over those numbers, in one and two parameters, and
//! [`perturb`], the perturbation rows `decide` reads for each kind of
//! record, and the directions the moving operand's elements move along,
//! [`ids`], the ids of a B-rep's entities, and [`records`], the records'
//! canonical keys, the table each kind is stored in, decided once, and the
//! flat shadow records (point against vertex, a point's ray against a
//! straight edge), and [`order`], the geometric order on vertices, points
//! on edges, edges, faces and snap clusters that choices read in place of
//! ids, and [`count`],
//! the counting the records feed: so far the dyadic boxes of a face, and
//! [`leaves`], the certified kinds of pairs of boxes of two faces.

pub mod bezier;
pub mod count;
pub mod ids;
pub mod leaves;
pub mod num;
pub mod order;
pub mod perturb;
pub mod records;
