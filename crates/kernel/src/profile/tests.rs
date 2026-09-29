use std::f64::consts::PI;

use glam::DVec2;

use super::*;

/// The exact arc from `a` to `b` round `center`, the shorter way (under
/// 180°), built without trigonometry ([`Conic2::arc_between`]).
pub(crate) fn arc(center: DVec2, a: DVec2, b: DVec2, curve: u64) -> Segment {
    let r = (a - center).length();
    Segment {
        conic: Conic2::arc_between(center, r, a, b).unwrap(),
        curve,
    }
}

/// The loop through `points` with straight sides, curves `curve..`.
pub(crate) fn polygon(points: &[DVec2], curve: u64) -> Loop {
    let n = points.len();
    Loop {
        segments: (0..n)
            .map(|i| Segment::line(points[i], points[(i + 1) % n], curve + i as u64).unwrap())
            .collect(),
    }
}

/// The rectangle from `min` to `max`, counter-clockwise from its bottom
/// side, curves `curve..curve + 4`.
pub(crate) fn rect(min: DVec2, max: DVec2, curve: u64) -> Loop {
    polygon(
        &[min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)],
        curve,
    )
}

/// The circle round `center` as four quarter arcs of `curve`, from `+x`,
/// counter-clockwise, or clockwise for a hole.
pub(crate) fn circle(center: DVec2, r: f64, curve: u64, clockwise: bool) -> Loop {
    let mut points = [DVec2::X, DVec2::Y, DVec2::NEG_X, DVec2::NEG_Y].map(|d| center + d * r);
    if clockwise {
        points.reverse();
    }
    Loop {
        segments: (0..4)
            .map(|i| arc(center, points[i], points[(i + 1) % 4], curve))
            .collect(),
    }
}

/// The loop reversed: same curves, run the other way.
pub(crate) fn reversed(lp: &Loop) -> Loop {
    Loop {
        segments: lp
            .segments
            .iter()
            .rev()
            .map(|s| Segment {
                conic: s.conic.reversed(),
                curve: s.curve,
            })
            .collect(),
    }
}

#[test]
fn areas_are_signed_and_exact() {
    let square = rect(DVec2::ZERO, DVec2::new(3.0, 2.0), 0);
    assert_eq!(square.area(), 6.0);
    assert_eq!(reversed(&square).area(), -6.0);
    for r in [1e-3, 1.0, 7.5, 1e5] {
        let center = DVec2::new(3.0 * r, -2.0 * r);
        let disc = circle(center, r, 0, false);
        let exact = PI * r * r;
        assert!((disc.area() - exact).abs() <= 1e-14 * exact, "{r}");
        assert!((circle(center, r, 0, true).area() + exact).abs() <= 1e-14 * exact);
    }
    // An arc of 60° and its chord: the circle's segment.
    let (a, b) = (DVec2::new(1.0, 0.0), DVec2::new(0.5, 0.75f64.sqrt()));
    let lens = Loop {
        segments: vec![arc(DVec2::ZERO, a, b, 0), Segment::line(b, a, 1).unwrap()],
    };
    let exact = PI / 6.0 - 0.75f64.sqrt() / 2.0;
    assert!(
        (lens.area() - exact).abs() < 1e-15,
        "{} {exact}",
        lens.area()
    );

    let plate = Profile {
        loops: vec![
            rect(DVec2::ZERO, DVec2::new(10.0, 10.0), 0),
            circle(DVec2::splat(5.0), 2.0, 4, true),
        ],
    };
    assert!((plate.area() - (100.0 - 4.0 * PI)).abs() < 1e-13);
    assert_eq!(plate.check(), Ok(()));
}

#[test]
fn check_refuses_bad_profiles() {
    let good = rect(DVec2::ZERO, DVec2::ONE, 0);
    let one = |lp: Loop| Profile { loops: vec![lp] };
    assert_eq!(one(good.clone()).check(), Ok(()));
    assert_eq!(Profile::default().check(), Err(ProfileError::Empty));

    let short = Loop {
        segments: good.segments[..1].to_vec(),
    };
    assert_eq!(one(short).check(), Err(ProfileError::Short(0)));

    let mut open = good.clone();
    open.segments[2].conic.p1.x += 1e-12;
    assert_eq!(one(open).check(), Err(ProfileError::Open(0, 2)));

    let mut far = good.clone();
    far.segments[1].conic.p1.y = 2e6;
    far.segments[2].conic.p0.y = 2e6;
    far.segments[1].conic.c.y = 1e6;
    assert_eq!(
        one(far).check(),
        Err(ProfileError::Segment(0, 1, PatchError::Coordinate(2e6)))
    );

    let mut heavy = good.clone();
    heavy.segments[3].conic.w = 100.0;
    assert_eq!(
        one(heavy).check(),
        Err(ProfileError::Segment(0, 3, PatchError::Weight(100.0)))
    );

    let mut nan = good.clone();
    nan.segments[0].conic.c.x = f64::NAN;
    assert!(matches!(
        one(nan).check(),
        Err(ProfileError::Segment(0, 0, PatchError::Coordinate(_)))
    ));

    let point = DVec2::new(1.0, 1.0);
    let degenerate = Loop {
        segments: vec![
            Segment::line(DVec2::ZERO, point, 0).unwrap(),
            Segment {
                conic: Conic2::new(point, DVec2::new(2.0, 0.0), 1.0, point).unwrap(),
                curve: 1,
            },
            Segment::line(point, DVec2::ZERO, 2).unwrap(),
        ],
    };
    assert_eq!(one(degenerate).check(), Err(ProfileError::Degenerate(0, 1)));

    // There and back again.
    let flat = Loop {
        segments: vec![
            Segment::line(DVec2::ZERO, point, 0).unwrap(),
            Segment::line(point, DVec2::ZERO, 1).unwrap(),
        ],
    };
    assert_eq!(one(flat).check(), Err(ProfileError::Area(0)));

    let many = Loop {
        segments: (0..=MAX_PROFILE_SEGMENTS)
            .map(|i| {
                let angle = |i: usize| i as f64;
                Segment::line(DVec2::splat(angle(i)), DVec2::splat(angle(i + 1)), 0).unwrap()
            })
            .collect(),
    };
    assert_eq!(
        one(many).check(),
        Err(ProfileError::TooManySegments(MAX_PROFILE_SEGMENTS + 1))
    );
}
