//! Random profiles revolved: right or refused, never wrong. About 20 s
//! in release (creases neither of `check`'s edge rules parts are
//! repaired into tens of thousands of patches, see `agents/kernel.md`;
//! 103 of the 120 right, 14 `TooComplex`); not run in debug builds.
#![cfg(not(debug_assertions))]

use super::*;

/// A random conic or line from `a` to `b`, of `curve`.
fn piece(rng: &mut Rng, a: DVec2, b: DVec2, curve: u64) -> Segment {
    if rng.next_u64().is_multiple_of(3) {
        return Segment::line(a, b, curve).unwrap();
    }
    let mid = (a + b) * 0.5 + (b - a) * rng.range(-0.2, 0.2);
    let across = (b - a).perp() * rng.range(-0.3, 0.3);
    Segment {
        conic: Conic2::new(a, mid + across, rng.range(0.4, 1.4), b).unwrap(),
        curve,
    }
}

/// A star-shaped loop round `centre`, off the axis or across it.
fn loop_off(rng: &mut Rng, centre: DVec2, scale: f64) -> Loop {
    let n = 3 + (rng.next_u64() % 7) as usize;
    let corners: Vec<DVec2> = (0..n)
        .map(|i| {
            let a = (i as f64 + rng.range(-0.3, 0.3)) / n as f64 * TAU;
            centre + DVec2::new(a.cos(), a.sin()) * scale * rng.range(0.4, 1.0)
        })
        .collect();
    Loop {
        segments: (0..n)
            .map(|i| piece(rng, corners[i], corners[(i + 1) % n], i as u64))
            .collect(),
    }
}

/// A loop fanned out from `(0, h)` on the axis: from the axis below
/// round to the axis above, and back down it.
fn loop_on(rng: &mut Rng, h: f64, scale: f64) -> Loop {
    let n = 2 + (rng.next_u64() % 6) as usize;
    let mut corners = vec![v(0.0, h - scale * rng.range(0.3, 1.0))];
    for i in 1..n {
        let a = -PI / 2.0 + PI * (i as f64 + rng.range(-0.3, 0.3)) / n as f64;
        corners.push(v(0.0, h) + DVec2::new(a.cos(), a.sin()) * scale * rng.range(0.4, 1.0));
    }
    corners.push(v(0.0, h + scale * rng.range(0.3, 1.0)));
    let mut segments: Vec<Segment> = corners
        .windows(2)
        .enumerate()
        .map(|(i, w)| piece(rng, w[0], w[1], i as u64))
        .collect();
    segments.push(Segment::line(corners[n], corners[0], 99).unwrap());
    Loop { segments }
}

#[test]
fn random_profiles_are_right_or_refused() {
    let mut rng = Rng::new(47);
    let (mut ok, mut refused) = (0, Vec::new());
    for i in 0..120 {
        let scale = rng.log_range(0.5, 20.0);
        let at = rng.range(0.0, 1.0);
        let lp = if i % 2 == 0 {
            loop_off(&mut rng, v(scale * (0.6 + 1.9 * at), 0.0), scale)
        } else {
            loop_on(&mut rng, 10.0 * at - 5.0, scale)
        };
        let p = profile(vec![lp]);
        let frame = if i % 3 == 0 {
            Z
        } else {
            random_frame(&mut rng, 1e3)
        };
        let tol = Tolerance::new([1e-2, 1e-3][i % 2]).unwrap();
        let sweep = if i % 4 < 2 {
            Sweep::Full
        } else {
            let from = rng.range(-PI, PI);
            Sweep::Part {
                from,
                to: from + rng.range(0.1, 6.0),
            }
        };
        match revolve(&p, &frame, sweep, 7, &tol, &Budget::new(1 << 20)) {
            Ok(solid) => {
                let (moment, walls, region) = moments(&p);
                let theta = angle(sweep);
                let volume = theta * moment;
                let mut area = theta * walls;
                if sweep != Sweep::Full {
                    area += 2.0 * region;
                }
                let slack = 0.5 * tol.fit() * area + 1e-10 * volume + 1e-13 * area * 1e3;
                let sv = solid.volume();
                assert!(
                    (sv - volume).abs() <= slack,
                    "case {i}: volume {sv} for {volume} ({:e} over {slack:e})",
                    (sv - volume).abs()
                );
                // Fitted faces within half the fit tolerance of curves
                // bent no tighter than about the shortest segment.
                let shortest = p.loops[0]
                    .segments
                    .iter()
                    .map(|s| (s.conic.p1 - s.conic.p0).length())
                    .fold(f64::INFINITY, f64::min);
                let sa = solid.area();
                let area_slack = 2.0 * tol.fit() / shortest * area + 1e-10 * area;
                assert!(
                    (sa - area).abs() <= area_slack,
                    "case {i}: area {sa} for {area} ({:e} over {area_slack:e})",
                    (sa - area).abs()
                );
                solid.mesh().check(&tol).unwrap();
                ok += 1;
            }
            Err(e) => refused.push((i, e)),
        }
    }
    assert!(ok >= 80, "{ok} right, refused: {refused:?}");
}
