use super::*;

/// The quadratic Bézier `a, c, b` as a cubic.
fn elevated(a: DVec2, c: DVec2, b: DVec2) -> Bezier {
    [a, a + (c - a) * (2.0 / 3.0), b + (c - b) * (2.0 / 3.0), b]
}

#[test]
fn a_parabola_is_one_exact_conic() {
    let (a, c, b) = (DVec2::ZERO, DVec2::new(1.5, 1.0), DVec2::new(3.0, 0.0));
    let conic = conic(&elevated(a, c, b), 1e-9).unwrap();
    assert_eq!((conic.p0, conic.p1), (a, b));
    assert!(conic.c.distance(c) < 1e-12);
    assert!((conic.w - 1.0).abs() < 1e-12);
}

#[test]
fn a_conic_s_own_points_are_on_it_and_others_off_it() {
    let conic = Conic2::new(DVec2::ZERO, DVec2::new(1.0, 1.0), 0.6, DVec2::new(2.0, 0.0)).unwrap();
    for i in 0..=10 {
        let at = conic.eval(f64::from(i) / 10.0);
        assert!(distance(&conic, at).unwrap() < 1e-12);
        // Moved off it square to it, about that far.
        let (_, d) = conic.eval_deriv(f64::from(i) / 10.0);
        let off = at + d.perp().normalize() * 1e-4;
        let measured = distance(&conic, off);
        if let Some(measured) = measured {
            assert!((measured - 1e-4).abs() < 1e-6, "{i}: {measured}");
        }
    }
    // Outside the control triangle.
    assert_eq!(distance(&conic, DVec2::new(1.0, -1.0)), None);
}

#[test]
fn an_inflection_is_split() {
    let s: Bezier = [
        DVec2::ZERO,
        DVec2::new(1.0, 1.0),
        DVec2::new(2.0, -1.0),
        DVec2::new(3.0, 0.0),
    ];
    assert_eq!(conic(&s, 1.0), None);
    let mut out = Segments {
        count: 0,
        fit: 1e-4,
        segments: Vec::new(),
    };
    fit(&s, 1e-4, 0, 1, &mut out).unwrap();
    assert!(out.segments.len() >= 2);
    assert_eq!(out.segments[0].conic.p0, s[0]);
    assert_eq!(out.segments.last().unwrap().conic.p1, s[3]);
    for pair in out.segments.windows(2) {
        assert_eq!(pair[0].conic.p1, pair[1].conic.p0);
    }
}

#[test]
fn a_straight_cubic_is_a_line() {
    let straight = elevated(DVec2::ZERO, DVec2::new(1.0, 1.0), DVec2::new(2.0, 2.0));
    let mut out = Segments {
        count: 0,
        fit: 1e-3,
        segments: Vec::new(),
    };
    fit(&straight, 1e-3, 0, 1, &mut out).unwrap();
    let [segment] = &out.segments[..] else {
        panic!("one segment");
    };
    assert_eq!(
        segment.conic,
        Conic2::line(straight[0], straight[3]).unwrap()
    );
}

#[test]
fn a_cubic_that_stops_is_refused() {
    // Its middle runs back over itself: no chain of conics follows it.
    let back: Bezier = [
        DVec2::ZERO,
        DVec2::new(3.0, 0.0),
        DVec2::new(-1.0, 0.0),
        DVec2::new(2.0, 0.0),
    ];
    let mut out = Segments {
        count: 0,
        fit: 1e-3,
        segments: Vec::new(),
    };
    assert_eq!(fit(&back, 1e-3, 0, 1, &mut out), Err(ProfileError::Fit));
}
