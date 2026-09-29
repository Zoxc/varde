//! The solver benchmark's sketches and cases, and finding profiles, apart
//! from the clock, so the same cases can be timed natively and under wasm.

use glam::DVec2;
use varde_sketch::{
    Budget, Constraint, Curve, Goal, Handle, Id, Side, Sketch, Spline, analyse, solve,
};

/// Drag steps per drag case.
pub const DRAG_STEPS: usize = 30;

/// A small, fixed wobble, so a sketch starts off solved by a little.
fn wobble(i: usize, j: usize) -> f64 {
    0.3 * ((i * 7 + j * 3) as f64).sin()
}

/// A plate: a grid of `cells` by `cells` square cells, 10 apart, its
/// lines horizontal and vertical, spaced equally, each cell holding a
/// circle centred on its diagonal (a construction line) and tangent to its
/// bottom. With `fixed`, two corners of the first cell are fixed and it's
/// fully constrained; without, it has four degrees of freedom (where it
/// is, and its cells' width and height). `cells` 9 is 342 curves. Returns
/// its last corner, to drag.
pub fn plate(cells: usize, fixed: bool) -> (Sketch, Id) {
    let mut sketch = Sketch::default();
    let n = cells + 1;
    let mut points = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let at = DVec2::new(
                10.0 * j as f64 + wobble(i, j),
                10.0 * i as f64 + wobble(j, i),
            );
            points.push(sketch.add_point(at).unwrap());
        }
    }
    let p = |i: usize, j: usize| points[i * n + j];
    let line = |sketch: &mut Sketch, start, end, construction| {
        sketch
            .add_curve(Curve::Line { start, end }, construction)
            .unwrap()
    };
    let mut horizontal = Vec::new();
    let mut vertical = Vec::new();
    for i in 0..n {
        for j in 0..cells {
            horizontal.push(line(&mut sketch, p(i, j), p(i, j + 1), false));
            vertical.push(line(&mut sketch, p(j, i), p(j + 1, i), false));
        }
    }
    // `horizontal[i * cells + j]` runs along row i, `vertical[i * cells +
    // j]` up column i.
    let mut constraints = Vec::new();
    for (&h, &v) in horizontal.iter().zip(&vertical) {
        constraints.push(Constraint::Horizontal(h));
        constraints.push(Constraint::Vertical(v));
    }
    for k in 1..cells {
        constraints.push(Constraint::Equal(horizontal[k], horizontal[0]));
        constraints.push(Constraint::Equal(vertical[k], vertical[0]));
    }
    if fixed {
        constraints.push(Constraint::Fix(p(0, 0)));
        constraints.push(Constraint::Fix(p(1, 1)));
    }
    for i in 0..cells {
        for j in 0..cells {
            let diagonal = line(&mut sketch, p(i, j), p(i + 1, j + 1), true);
            let at = DVec2::new(10.0 * j as f64 + 5.2, 10.0 * i as f64 + 4.9);
            let center = sketch.add_point(at).unwrap();
            let radius = 3.5 + wobble(i, j);
            let circle = sketch
                .add_curve(Curve::Circle { center, radius }, false)
                .unwrap();
            constraints.push(Constraint::Midpoint {
                point: center,
                line: diagonal,
            });
            constraints.push(Constraint::Tangent {
                a: horizontal[i * cells + j],
                b: circle,
                side: Side::Positive,
                at: None,
            });
        }
    }
    for constraint in constraints {
        sketch.add_constraint(constraint).unwrap();
    }
    (sketch, p(cells, cells))
}

/// A chain of `arcs` half circles of radius 5 along the x axis, above and
/// below in turn, each tangent to the next where they meet, the first
/// fixed. Returns the chain's far end, to drag.
pub fn arc_chain(arcs: usize) -> (Sketch, Id) {
    let mut sketch = Sketch::default();
    // The first arc, fixed, is solved already.
    let wobble = |i, j| if i <= 1 { 0.0 } else { wobble(i, j) };
    let ends: Vec<Id> = (0..=arcs)
        .map(|i| {
            let at = DVec2::new(10.0 * i as f64 + wobble(i, 1), wobble(i, 3));
            sketch.add_point(at).unwrap()
        })
        .collect();
    let mut previous = None;
    let mut constraints = Vec::new();
    for i in 0..arcs {
        let at = DVec2::new(10.0 * i as f64 + 5.0, wobble(i, 2));
        let center = sketch.add_point(at).unwrap();
        // Counter-clockwise: over the top from the right end, under the
        // bottom from the left.
        let (start, end) = if i % 2 == 0 {
            (ends[i + 1], ends[i])
        } else {
            (ends[i], ends[i + 1])
        };
        let arc = sketch
            .add_curve(Curve::Arc { center, start, end }, false)
            .unwrap();
        match previous {
            None => constraints.push(Constraint::Fix(arc)),
            Some(previous) => constraints.push(Constraint::Tangent {
                a: previous,
                b: arc,
                side: Side::Positive,
                at: None,
            }),
        }
        previous = Some(arc);
    }
    for constraint in constraints {
        sketch.add_constraint(constraint).unwrap();
    }
    (sketch, ends[arcs])
}

/// A chain of `splines` splines through `fit` fit points each, along a
/// wave, each joined smoothly to the next where they share an end (a
/// handle at each end), the first's start and its handle there fixed,
/// and `on` points on each, a little off it. Returns the chain's far end,
/// to drag.
pub fn spline_chain(splines: usize, fit: usize, on: usize) -> (Sketch, Id) {
    let mut sketch = Sketch::default();
    let place = |k: usize| {
        let x = 2.0 * k as f64;
        DVec2::new(x + wobble(k, 1), 3.0 * (x / 7.0).sin() + wobble(k, 2))
    };
    let mut ids = Vec::new();
    let mut constraints = Vec::new();
    let mut previous: Option<(Id, Id)> = None;
    for i in 0..splines {
        let start = i * (fit - 1);
        let mut points: Vec<Id> = Vec::with_capacity(fit);
        for k in start..start + fit {
            match previous {
                Some((_, end)) if k == start => points.push(end),
                _ => points.push(sketch.add_point(place(k)).unwrap()),
            }
        }
        let mut spline = Spline::through(points.clone(), false);
        for (at, toward) in [(0, 1), (fit - 1, fit - 2)] {
            let from = place(start + at);
            let tip = from + (from - place(start + toward)) * -0.3;
            let tip = sketch.add_point(tip).unwrap();
            spline.handles.push(Handle {
                at: points[at],
                tip,
            });
            if i == 0 && at == 0 {
                constraints.extend([Constraint::Fix(points[0]), Constraint::Fix(tip)]);
            }
        }
        let id = sketch.add_curve(Curve::Spline(spline), false).unwrap();
        ids.push(id);
        if let Some((before, _)) = previous {
            constraints.push(sketch.smooth(before, id).expect("a smooth join"));
        }
        previous = Some((id, *points.last().unwrap()));
    }
    for (i, &id) in ids.iter().enumerate() {
        let shape = sketch.spline_shape(sketch.spline(id).unwrap()).unwrap();
        for j in 0..on {
            let at = shape.point((j as f64 + 0.5) / on as f64) + DVec2::splat(0.05 * wobble(i, j));
            let point = sketch.add_point(at).unwrap();
            constraints.push(Constraint::PointOnCurve { point, curve: id });
        }
    }
    for constraint in constraints {
        sketch.add_constraint(constraint).unwrap();
    }
    (sketch, previous.unwrap().1)
}

/// Runs every case, timing each with `now` (milliseconds) and passing
/// its name and the milliseconds one run of it took, on average, to
/// `report`. Panics if a case fails to solve.
pub fn run(now: &dyn Fn() -> f64, report: &mut dyn FnMut(&str, f64)) {
    let budget = Budget::default();
    let mut time = |name: &str, runs: usize, f: &mut dyn FnMut()| {
        let start = now();
        for _ in 0..runs {
            f();
        }
        report(name, (now() - start) / runs as f64);
    };
    let settled = |sketch: &Sketch| {
        solve(sketch, &Goal::Settle, &budget)
            .expect("the sketch solves")
            .sketch
    };
    let drags = |sketch: &Sketch, id: Id, by: DVec2| {
        let mut current = sketch.clone();
        let from = sketch.point(id).unwrap().at;
        for step in 1..=DRAG_STEPS {
            let goal = Goal::Drag {
                points: vec![(id, from + by * step as f64 / DRAG_STEPS as f64)],
                radii: Vec::new(),
            };
            current = solve(&current, &goal, &budget)
                .expect("the drag solves")
                .sketch;
        }
        current
    };

    for (name, (sketch, corner)) in [
        ("plate, fixed", plate(9, true)),
        ("plate, free", plate(9, false)),
    ] {
        let solved = settled(&sketch);
        let analysis = analyse(&solved);
        assert!(analysis.solved && analysis.redundant.is_empty());
        assert_eq!(analysis.freedom, if name == "plate, fixed" { 0 } else { 4 });
        time(&format!("{name}: settle"), 5, &mut || {
            settled(&sketch);
        });
        time(&format!("{name}: analyse"), 5, &mut || {
            analyse(&solved);
        });
        time(&format!("{name}: drag step"), 1, &mut || {
            drags(&solved, corner, DVec2::new(15.0, 8.0));
        });
    }
    for (name, (sketch, end)) in [
        ("spline chain", spline_chain(10, 12, 4)),
        ("long spline", spline_chain(1, 100, 20)),
    ] {
        let solved = settled(&sketch);
        let analysis = analyse(&solved);
        assert!(analysis.solved && analysis.redundant.is_empty(), "{name}");
        time(&format!("{name}: settle"), 5, &mut || {
            settled(&sketch);
        });
        time(&format!("{name}: analyse"), 5, &mut || {
            analyse(&solved);
        });
        time(&format!("{name}: drag step"), 1, &mut || {
            drags(&solved, end, DVec2::new(-6.0, 10.0));
        });
    }

    let (chain, end) = arc_chain(100);
    let solved = settled(&chain);
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    time("arc chain: settle", 5, &mut || {
        settled(&chain);
    });
    time("arc chain: analyse", 5, &mut || {
        analyse(&solved);
    });
    time("arc chain: drag step", 1, &mut || {
        drags(&solved, end, DVec2::new(-20.0, 30.0));
    });

    // Profiles: the plate solved, its circles touching their cells'
    // sides (five regions a cell); a plate of 40 by 40 cells, 6480 curves,
    // not solved, each circle a hole in its cell and a region inside; and a
    // grid of 100 lines across 100.
    let mut grid = Sketch::default();
    for i in 0..100 {
        let at = i as f64;
        for (start, end) in [
            (DVec2::new(at, -1.0), DVec2::new(at, 100.0)),
            (DVec2::new(-1.0, at), DVec2::new(100.0, at)),
        ] {
            let (start, end) = (grid.add_point(start).unwrap(), grid.add_point(end).unwrap());
            grid.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    }
    for (name, sketch, regions) in [
        ("plate: profiles", settled(&plate(9, true).0), 405),
        ("large plate: profiles", plate(40, false).0, 3200),
        ("grid: profiles", grid, 99 * 99),
    ] {
        let found = sketch.profiles().expect("not too complex");
        assert_eq!(found.regions.len(), regions, "{name}");
        time(name, 5, &mut || {
            sketch.profiles().unwrap();
        });
    }
}
