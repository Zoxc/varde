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
    let mut out = Chain {
        conics: Vec::new(),
        room: usize::MAX,
    };
    fit(&s, 1e-4, 0, &mut out).unwrap();
    assert!(out.conics.len() >= 2);
    assert_eq!(out.conics[0].p0, s[0]);
    assert_eq!(out.conics.last().unwrap().p1, s[3]);
    for pair in out.conics.windows(2) {
        assert_eq!(pair[0].p1, pair[1].p0);
    }
}

#[test]
fn a_straight_cubic_is_a_line() {
    let straight = elevated(DVec2::ZERO, DVec2::new(1.0, 1.0), DVec2::new(2.0, 2.0));
    let mut out = Chain {
        conics: Vec::new(),
        room: usize::MAX,
    };
    fit(&straight, 1e-3, 0, &mut out).unwrap();
    let [conic] = &out.conics[..] else {
        panic!("one segment");
    };
    assert_eq!(*conic, Conic2::line(straight[0], straight[3]).unwrap());
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
    let mut out = Chain {
        conics: Vec::new(),
        room: usize::MAX,
    };
    assert_eq!(fit(&back, 1e-3, 0, &mut out), Err(ProfileError::Fit));
}

/// The parabola `a, c, b` cut into `n` cubic segments at even parameters.
fn parabola_pieces(a: DVec2, c: DVec2, b: DVec2, n: usize) -> Vec<Bezier> {
    let at = |t: f64| a * ((1.0 - t) * (1.0 - t)) + c * (2.0 * t * (1.0 - t)) + b * (t * t);
    let control = |t0: f64, t1: f64| {
        // The quadratic over [t0, t1]: its middle control point where the
        // end tangents meet, the polar form at (t0, t1).
        let mix = |p: DVec2, q: DVec2, t: f64| p * (1.0 - t) + q * t;
        mix(mix(a, c, t0), mix(c, b, t0), t1)
    };
    (0..n)
        .map(|i| {
            let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
            elevated(at(t0), control(t0, t1), at(t1))
        })
        .collect()
}

#[test]
fn a_run_of_segments_on_one_parabola_is_one_conic() {
    let (a, c, b) = (DVec2::ZERO, DVec2::new(1.5, 1.0), DVec2::new(3.0, 0.0));
    let pieces = parabola_pieces(a, c, b, 8);
    let mut out = Chain {
        conics: Vec::new(),
        room: usize::MAX,
    };
    runs(&pieces, 1e-6, &mut out).unwrap();
    let [conic] = &out.conics[..] else {
        panic!("{} conics", out.conics.len());
    };
    assert_eq!((conic.p0, conic.p1), (pieces[0][0], pieces[7][3]));
    assert!(conic.c.distance(c) < 1e-9 && (conic.w - 1.0).abs() < 1e-6);
}

#[test]
fn a_run_straight_within_a_quarter_of_the_tolerance_is_a_line() {
    // A zigzag 0.02 high: straight at 0.1, not at 0.01.
    let pieces: Vec<Bezier> = (0..20)
        .map(|i| {
            let x = f64::from(i);
            let up = if i % 2 == 0 { 0.02 } else { -0.02 };
            [
                DVec2::new(x, 0.0),
                DVec2::new(x + 0.3, up),
                DVec2::new(x + 0.7, up),
                DVec2::new(x + 1.0, 0.0),
            ]
        })
        .collect();
    let mut out = Chain {
        conics: Vec::new(),
        room: usize::MAX,
    };
    runs(&pieces, 0.1, &mut out).unwrap();
    let line = Conic2::line(DVec2::ZERO, DVec2::new(20.0, 0.0)).unwrap();
    assert_eq!(out.conics, [line]);
    let mut out = Chain {
        conics: Vec::new(),
        room: usize::MAX,
    };
    runs(&pieces, 0.01, &mut out).unwrap();
    assert!(out.conics.len() >= 20, "{}", out.conics.len());
}

#[test]
fn a_chain_past_the_room_left_is_refused() {
    let s: Bezier = [
        DVec2::ZERO,
        DVec2::new(1.0, 1.0),
        DVec2::new(2.0, -1.0),
        DVec2::new(3.0, 0.0),
    ];
    let mut out = Chain {
        conics: Vec::new(),
        room: 1,
    };
    assert_eq!(
        runs(&[s], 1e-4, &mut out),
        Err(ProfileError::TooManySegments)
    );
}
