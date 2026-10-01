use super::*;

fn basis(view: View) -> Basis {
    let mut camera = Camera::default();
    camera.look_from(view);
    Basis::new(&camera)
}

#[test]
fn head_on_shows_one_face_filling_the_middle() {
    for view in View::ALL {
        let basis = basis(view);
        let faces: Vec<_> = visible_faces(&basis).map(|f| f.view).collect();
        assert_eq!(faces, [view]);
        let center = Vec2::splat(SIZE / 2.0);
        assert_eq!(face_at(&basis, center), Some(view));
        assert_eq!(face_at(&basis, center + Vec2::splat(HALF + 1.0)), None);
    }
}

#[test]
fn head_on_letters_are_upright() {
    for view in View::ALL {
        let face = visible_faces(&basis(view)).next().unwrap();
        assert!(face.u.abs_diff_eq(Vec2::X, 1e-5), "{view:?}: {}", face.u);
        assert!(face.v.abs_diff_eq(Vec2::Y, 1e-5), "{view:?}: {}", face.v);
    }
}

#[test]
fn default_camera_shows_top_front_right() {
    let basis = Basis::new(&Camera::default());
    let mut faces: Vec<_> = visible_faces(&basis)
        .map(|f| format!("{:?}", f.view))
        .collect();
    faces.sort();
    assert_eq!(faces, ["Front", "Right", "Top"]);
}

#[test]
fn decompose_reproduces_the_matrix() {
    let rot = |a: f32, p: Vec2| Vec2::from_angle(a).rotate(p);
    for (x, y) in [
        (Vec2::X, Vec2::Y),
        (Vec2::new(0.8, 0.3), Vec2::new(-0.2, 0.5)),
        (Vec2::new(0.1, -0.9), Vec2::new(0.7, 0.4)),
        (Vec2::new(0.5, 0.0), Vec2::new(0.5, 0.0)),
    ] {
        let (phi, scale, theta) = decompose(x, y);
        let apply = |p: Vec2| rot(phi, rot(theta, p) * scale);
        assert!(apply(Vec2::X).abs_diff_eq(x, 1e-5), "{x} {y}");
        assert!(apply(Vec2::Y).abs_diff_eq(y, 1e-5), "{x} {y}");
    }
}

#[test]
fn hover_follows_the_cube_under_a_still_cursor() {
    use canvas::Program;

    let bounds = Rectangle::new(Point::new(10.0, 20.0), iced::Size::new(SIZE, SIZE));
    let before = ViewCube {
        basis: Basis::new(&Camera::default()),
    };
    // A spot on the right face near the widget's right edge.
    let spot = (0..SIZE as u32)
        .rev()
        .map(|x| Vec2::new(x as f32, SIZE / 2.0))
        .find(|&p| face_at(&before.basis, p) == Some(View::Right))
        .unwrap();
    let cursor = mouse::Cursor::Available(Point::new(bounds.x + spot.x, bounds.y + spot.y));
    let moved = Event::Mouse(mouse::Event::CursorMoved {
        position: cursor.position().unwrap(),
    });
    let mut state = None;
    assert!(before.update(&mut state, &moved, bounds, cursor).is_some());
    assert_eq!(state, Some(View::Right));

    // Clicking R turns the camera to look at it head on, leaving the still
    // cursor outside every face.
    let after = ViewCube {
        basis: basis(View::Right),
    };
    assert_eq!(after.face_under(bounds, cursor), None);
    assert_eq!(
        after.mouse_interaction(&state, bounds, cursor),
        mouse::Interaction::default()
    );
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    assert!(after.update(&mut state, &press, bounds, cursor).is_some());
    assert_eq!(state, None);
}

#[test]
fn axes_run_along_edges_in_sight() {
    let mut camera = Camera::default();
    for _ in 0..80 {
        // Around and over the cube, through head-on views and the poles.
        camera.orbit(0.37, 0.29);
        let basis = Basis::new(&camera);
        let faces: Vec<_> = visible_faces(&basis).collect();
        let on_a_face = |p: Vec2| faces.iter().any(|f| contains(&f.corners, p));
        let axes = axes(&basis);
        for (index, axis) in axes.iter().enumerate() {
            let along = WORLD_AXES[index];
            let Some(axis) = axis else {
                // Only an axis seen end on is left out.
                assert!(basis.project(along).length() < 0.05, "{camera:?} {index}");
                continue;
            };
            assert_eq!(axis.index, index);
            assert!(
                (axis.end - axis.start)
                    .normalize()
                    .abs_diff_eq(axis.direction, 1e-3)
            );
            // The edge's middle is on a face in sight, or its outline.
            let middle = (axis.start + axis.end) / 2.0;
            let near = [Vec2::ZERO, Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y]
                .iter()
                .any(|&d| on_a_face(middle + d * 0.5));
            assert!(near, "{camera:?} {index}: edge off the faces");
        }
    }
}

#[test]
fn hidden_is_behind_the_cube() {
    let basis = basis(View::Front);
    // From the front, a point behind the back face is hidden, one off to
    // the side or in front isn't, and nor is a point on the front face.
    assert!(hidden(&basis, Vec3::new(0.0, 2.0 * HALF, 0.0)));
    assert!(!hidden(&basis, Vec3::new(2.0 * HALF, 2.0 * HALF, 0.0)));
    assert!(!hidden(&basis, Vec3::new(0.0, -2.0 * HALF, 0.0)));
    assert!(!hidden(&basis, Vec3::new(0.0, -HALF, 0.0)));
}

#[test]
fn axes_and_their_letters_stay_inside_the_widget() {
    let mut camera = Camera::default();
    for _ in 0..60 {
        camera.orbit(0.41, 0.23);
        for axis in axes(&Basis::new(&camera)).iter().flatten() {
            for p in [axis.start, axis.end, axis.tip] {
                assert!(p.cmpge(Vec2::ZERO).all() && p.cmple(Vec2::splat(SIZE)).all());
            }
            let margin = Vec2::splat(AXIS_LETTER_MARGIN);
            assert!(axis.letter.cmpge(margin).all());
            assert!(axis.letter.cmple(Vec2::splat(SIZE) - margin).all());
        }
    }
}

#[test]
fn home_shows_every_axis_clear_of_the_cube() {
    let axes = axes(&Basis::new(&Camera::default()));
    for axis in &axes {
        let axis = axis.expect("every axis in sight");
        assert!(axis.tip_shown, "{axis:?}");
    }
    // X runs right and Z up on screen, Y away, to the right and up.
    let direction = axes.map(|a| a.unwrap().direction);
    assert!(direction[0].x > 0.5);
    assert!(direction[2].y < -0.5);
    assert!(direction[1].x > 0.0 && direction[1].y < 0.0);
}

#[test]
fn head_on_leaves_out_the_axis_seen_end_on() {
    for view in View::ALL {
        let axes = axes(&basis(view));
        let shown: Vec<_> = axes.iter().map(Option::is_some).collect();
        let end_on = view.normal().abs().max_position();
        for (index, shown) in shown.into_iter().enumerate() {
            assert_eq!(shown, index != end_on, "{view:?} {index}");
        }
    }
}
