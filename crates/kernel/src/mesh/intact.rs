//! Which triangles of a boolean's result its operands' checks already
//! cover: those it kept as they were.
//!
//! A boolean carries most of its operands' triangles through untouched,
//! and its operands passed the check, so testing those again for the
//! fold and hull rules finds nothing. A triangle of the result is
//! **intact** when it is a triangle of an operand that passed the check
//! at the same resolution, bit for bit: the same patch (corners, control
//! points and weights, corner for corner, so not turned over), on
//! corners that map one to one onto that operand's (one map for each
//! operand, and no vertex of the result the image of two). Then:
//!
//! - an intact triangle passes the fold check, which looks at its patch
//!   alone;
//! - two intact triangles of one operand share as many corners as they
//!   did there, the same ones, so the hull rules (which look at their
//!   patches and which corners they share) decide them as the operand's
//!   check did: they pass.
//!
//! Only pairs of intact triangles of different operands are left: they
//! are tested where one comes within the resolution of the box of the
//! other operand's intact triangles, which holds every one of them (so
//! no such pair whose boxes come that near is missed). Everything else is
//! tested as before. Which triangle of which operand a triangle of the
//! result may be is only a hint: it is verified here, so a wrong one
//! costs a test, never a pass.

use super::Mesh;
use super::bvh::near;
use crate::patch::{Bounds3, Patch};

/// Which triangle of which operand (`0` for `A`, `1` for `B`, and the
/// triangle's index) a triangle of a boolean's result may be, if any:
/// a hint, which [`tested`] verifies.
pub(crate) type Hint = Option<(u8, u32)>;

/// No vertex yet, in the maps of [`tested`].
const NONE: u32 = u32::MAX;

/// Which triangles of `mesh` repair and the check must test for the
/// fold and hull rules with the resolution `margin`, given that
/// `operands` (each `None` unless it passed the check with `margin`)
/// passed it, and that triangle `t` may be triangle `source[t]` of an
/// operand (`0` or `1`, and its index): every triangle but the intact
/// ones (see the [module](self) docs), and the intact ones within
/// `margin` of the box of the other operand's intact ones. The maps are
/// built in triangle order, so a triangle whose corners don't fit those
/// before it is tested. Everything is tested where `source` doesn't
/// have a hint for every triangle.
pub(crate) fn tested(
    mesh: &Mesh,
    operands: [Option<&Mesh>; 2],
    source: &[Hint],
    margin: f64,
) -> Vec<bool> {
    let n = mesh.tris.len();
    if source.len() != n {
        return vec![true; n];
    }
    // Each operand's vertices' images, and each vertex's preimage (its
    // operand above the low 32 bits).
    let mut image: [Vec<u32>; 2] = operands.map(|m| vec![NONE; m.map_or(0, |m| m.verts.len())]);
    let mut preimage = vec![u64::MAX; mesh.verts.len()];
    let mut kept: Vec<Option<(usize, Bounds3)>> = vec![None; n];
    let mut boxes: [Option<Bounds3>; 2] = [None, None];
    for (t, hint) in source.iter().enumerate() {
        let Some((k, s)) = *hint else { continue };
        let k = usize::from(k);
        let Some(operand) = operands.get(k).copied().flatten() else {
            continue;
        };
        if s as usize >= operand.tris.len() {
            continue;
        }
        let patch = mesh.patch(t);
        if !same_bits(&patch, &operand.patch(s as usize)) {
            continue;
        }
        let ours = mesh.corners(t as u32);
        let theirs = operand.corners(s);
        let tag = |w: u32| u64::from(w) | (k as u64) << 32;
        let fits = (0..3).all(|i| {
            let (v, w) = (ours[i] as usize, theirs[i] as usize);
            [NONE, ours[i]].contains(&image[k][w])
                && [u64::MAX, tag(theirs[i])].contains(&preimage[v])
        });
        if !fits {
            continue;
        }
        for i in 0..3 {
            image[k][theirs[i] as usize] = ours[i];
            preimage[ours[i] as usize] = tag(theirs[i]);
        }
        let bounds = patch.bounds();
        boxes[k] = Some(boxes[k].map_or(bounds, |b| b.union(bounds)));
        kept[t] = Some((k, bounds));
    }
    kept.iter()
        .map(|kept| match kept {
            None => true,
            Some((k, bounds)) => boxes[1 - k].is_some_and(|other| near(bounds, &other, margin)),
        })
        .collect()
}

/// Whether the patches are the same, bit for bit (so `0.0` and `-0.0`
/// differ, and a NaN is itself).
fn same_bits(a: &Patch, b: &Patch) -> bool {
    let bits = |p: &Patch| {
        let mut out = [0u64; 21];
        for (i, x) in p.p.iter().chain(&p.c).enumerate() {
            out[3 * i..3 * i + 3].copy_from_slice(&x.to_array().map(f64::to_bits));
        }
        out[18..].copy_from_slice(&p.w.map(f64::to_bits));
        out
    };
    bits(a) == bits(b)
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;
    use crate::mesh::MeshBuilder;
    use crate::mesh::tests::{free, joined, tetrahedron};

    const RES: f64 = 1e-6;

    /// Hints naming triangle `t mod n` of operand `k` for each of `count`
    /// triangles.
    fn hints(count: usize, n: u32, k: u8) -> Vec<Hint> {
        (0..count as u32).map(|t| Some((k, t % n))).collect()
    }

    #[test]
    fn an_operand_kept_as_it_was_isnt_tested() {
        let m = tetrahedron(DVec3::ZERO);
        assert_eq!(
            tested(&m, [Some(&m), None], &hints(4, 4, 0), RES),
            [false; 4]
        );
        assert_eq!(
            tested(&m, [None, Some(&m)], &hints(4, 4, 1), RES),
            [false; 4]
        );
        // Without the operand (checked at another resolution), with a
        // hint short, wrong or out of range, everything is tested.
        assert_eq!(tested(&m, [None, None], &hints(4, 4, 0), RES), [true; 4]);
        assert_eq!(
            tested(&m, [Some(&m), None], &hints(4, 4, 1), RES),
            [true; 4]
        );
        assert_eq!(
            tested(&m, [Some(&m), None], &hints(3, 4, 0), RES),
            [true; 4]
        );
        let shifted: Vec<_> = (0..4).map(|t| Some((0, (t + 1) % 4))).collect();
        assert_eq!(tested(&m, [Some(&m), None], &shifted, RES), [true; 4]);
        let beyond: Vec<_> = (0..4).map(|t| Some((0, t + 4))).collect();
        assert_eq!(tested(&m, [Some(&m), None], &beyond, RES), [true; 4]);
        let mut some = hints(4, 4, 0);
        some[2] = None;
        assert_eq!(
            tested(&m, [Some(&m), None], &some, RES),
            [false, false, true, false]
        );
    }

    #[test]
    fn a_corner_moved_by_a_bit_has_its_fan_tested() {
        let m = tetrahedron(DVec3::ZERO);
        // The origin as `-0.0` along `x`: the same point, other bits.
        for moved in [DVec3::new(-0.0, 0.0, 0.0), DVec3::new(1e-300, 0.0, 0.0)] {
            let mut builder = MeshBuilder::new();
            let f = free(&mut builder);
            let v = [moved, DVec3::X, DVec3::Y, DVec3::Z].map(|p| builder.vert(p));
            for [a, b, c] in [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] {
                builder.tri([v[a], v[b], v[c]], f);
            }
            let result = builder.build().unwrap();
            let got = tested(&result, [Some(&m), None], &hints(4, 4, 0), RES);
            assert_eq!(got, [true, true, true, false], "{moved}");
        }
    }

    #[test]
    fn turned_triangles_are_tested() {
        let m = tetrahedron(DVec3::ZERO);
        let turned = joined(&[(&m, true)]);
        assert_eq!(
            tested(&turned, [Some(&m), None], &hints(4, 4, 0), RES),
            [true; 4]
        );
    }

    #[test]
    fn corners_map_one_to_one() {
        // The operand twice over, on vertices of its own each: the second
        // copy's corners would map the operand's vertices twice.
        let m = tetrahedron(DVec3::ZERO);
        let twice = joined(&[(&m, false), (&m, false)]);
        let mut want = [false; 8];
        want[4..].fill(true);
        assert_eq!(tested(&twice, [Some(&m), None], &hints(8, 4, 0), RES), want);
    }

    #[test]
    fn kept_triangles_near_the_other_operands_are_tested() {
        let a = tetrahedron(DVec3::ZERO);
        // `B` kept whole beside `A`: tested where their boxes come within
        // the resolution, else not.
        for (gap, near) in [(2.0, false), (0.5, true), (0.0, true), (-0.5, true)] {
            let b = tetrahedron(DVec3::new(1.0 + gap * RES, 0.0, 0.0));
            let both = joined(&[(&a, false), (&b, false)]);
            let mut hints = hints(8, 4, 0);
            for (t, hint) in hints.iter_mut().enumerate().skip(4) {
                *hint = Some((1, t as u32 - 4));
            }
            let got = tested(&both, [Some(&a), Some(&b)], &hints, RES);
            // Only the triangles whose own boxes come near: those with a
            // corner at `x = 1` and `x = 1 + gap`.
            let reach = |t: usize| {
                let x = if t < 4 {
                    both.patch(t).bounds().max.x
                } else {
                    both.patch(t).bounds().min.x
                };
                if t < 4 {
                    x >= 1.0
                } else {
                    x <= 1.0 + gap * RES
                }
            };
            let want: Vec<bool> = (0..8).map(|t| near && reach(t)).collect();
            assert_eq!(got, want, "gap {gap}");
        }
    }
}
