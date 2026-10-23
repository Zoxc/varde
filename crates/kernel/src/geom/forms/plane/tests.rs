//! The plane's constructor: its one tile the exact parallelogram over
//! cell `(0, 0)` rounded once, on the plane, oriented by its normal.

use malachite::Rational as Q;
use varde_testkit::rng::Rng;

use super::super::testing::{dot, exact, frame, rounded_once};
use super::*;
use crate::geom::form::FormKey;
use crate::geom::round::nearest;

/// Planes through points up to `1e6` out, any normal.
fn keys(seed: u64) -> Vec<PlaneKey> {
    let mut rng = Rng::new(seed);
    let mut out = vec![PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap()];
    for k in 0..9 {
        let p = rng.point([1.0, 1e3, 1e6][k % 3]).to_array();
        out.push(PlaneKey::new(rng.direction().to_array(), p).unwrap());
    }
    out
}

#[test]
fn the_surface_is_one_exact_tile() {
    for key in keys(1) {
        let s = surface(key).unwrap();
        assert_eq!(s.tiles().len(), 1);
        let l = s.lattice();
        assert_eq!(s.tiles()[0], l.base_tile(0, 0).unwrap());
        // Control point (i, j) at `((i − 1)·u + (j − 1)·v)·CELL/2` from
        // the origin, rounded once, weight 1.
        let (o, [u, v, _]) = frame(l);
        let half = q(CELL) / Q::from(2u32);
        let want: [([Q; 3], f64); 9] = std::array::from_fn(|n| {
            let (s, t) = (Q::from(n as i64 / 3 - 1), Q::from(n as i64 % 3 - 1));
            let p = [0, 1, 2].map(|c| &o[c] + (&s * &u[c] + &t * &v[c]) * &half);
            (p, 1.0)
        });
        rounded_once(&s.tiles()[0], &want, &format!("{key:?}"));
        // Every point of the exact net on the plane to its rounding, and
        // the orientation the normal.
        let k: FormKey = key.into();
        let e = exact(&s.tiles()[0]);
        let n = key.normal().map(q);
        for (a, b) in [(0, 0), (1, 3), (2, 1), (4, 4)] {
            let p = e.eval(&Q::from_signeds(a, 4), &Q::from_signeds(b, 4));
            let f = nearest(&k.implicit(&p).unwrap()).abs();
            assert!(f <= 1e-15 * CELL, "{key:?}: {f:e}");
            let d = e.eval_partials(&Q::from_signeds(a, 4), &Q::from_signeds(b, 4));
            let c = [
                &d.du[1] * &d.dv[2] - &d.du[2] * &d.dv[1],
                &d.du[2] * &d.dv[0] - &d.du[0] * &d.dv[2],
                &d.du[0] * &d.dv[1] - &d.du[1] * &d.dv[0],
            ];
            assert!(dot(&c, &n) > 0u32, "{key:?}: oriented by the normal");
        }
        assert!(s.tiles()[0].is_regular());
    }
}
