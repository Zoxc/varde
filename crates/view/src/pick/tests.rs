use varde_document::{Document, Plane};
use varde_regen::{Cache, Summary, evaluate, tessellate_picking};
use varde_render::{Projection, View};

use super::*;

/// The viewport the tests look through, in logical pixels.
pub(crate) const SIZE: [f32; 2] = [400.0, 300.0];

/// The example's plate, 60 × 40 × 10 mm from z 0 up, with a hole of
/// radius 8 through its middle, made ready for picking as model 7.
pub(crate) fn plate() -> PickIndex {
    let document = Document::example();
    let mut cache = Cache::default();
    let evaluation = evaluate(&document, &mut cache);
    let (mesh, picking) = tessellate_picking(&document, &evaluation, &mut cache).unwrap();
    PickIndex::new(mesh, picking, 7)
}

/// Looking from `view` at the plate's middle, 60 mm across the view's
/// height: 5 pixels a millimetre at the target, in `projection`.
pub(crate) fn camera(view: View, projection: Projection) -> Camera {
    let mut camera = Camera::default();
    camera.set_projection(projection);
    camera.look_from(view);
    camera.set_target(Vec3::new(0.0, 0.0, 5.0));
    camera.zoom(60.0 / camera.view_height());
    camera
}

/// Where `camera` shows the world point `at` in [`SIZE`].
pub(crate) fn shown(camera: &Camera, at: DVec3) -> DVec2 {
    let placement = OriginPlane::XY.placement();
    Projector::new(camera, placement, SIZE[0], SIZE[1])
        .unwrap()
        .show(at)
}

/// The plane face `target` is on, as its outward normal and offset.
pub(crate) fn plane(index: &PickIndex, target: Picked) -> ([f64; 3], f64) {
    let Picked::Face(face) = target else {
        panic!("{target:?} isn't a face");
    };
    match index.picking().faces()[face as usize].summary {
        // Without -0.
        Summary::Plane { n, d } => (n.map(|x| x.round() + 0.0), d.round() + 0.0),
        summary => panic!("{summary:?} isn't a plane"),
    }
}

/// The faces either side of the edge `target`, as their planes, in
/// order.
fn sides(index: &PickIndex, target: Picked) -> Vec<([f64; 3], f64)> {
    let Picked::Edge(chain) = target else {
        panic!("{target:?} isn't an edge");
    };
    let faces = index.edge_faces(chain).unwrap();
    let mut sides: Vec<_> = (faces.iter())
        .map(|&face| plane(index, Picked::Face(face)))
        .collect();
    sides.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sides
}

pub(crate) const TOP: ([f64; 3], f64) = ([0.0, 0.0, 1.0], 10.0);
pub(crate) const BOTTOM: ([f64; 3], f64) = ([0.0, 0.0, -1.0], 0.0);
pub(crate) const FRONT: ([f64; 3], f64) = ([0.0, -1.0, 0.0], 20.0);
const BACK: ([f64; 3], f64) = ([0.0, 1.0, 0.0], 20.0);

#[test]
fn rays_at_known_pixels_hit_the_plates_faces() {
    let index = plate();
    let body = index.face_body(0).unwrap();
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let top = camera(View::Top, projection);
        let at = shown(&top, DVec3::new(20.0, 5.0, 10.0));
        let pick = index.pick(&top, SIZE, at, Picks::All).unwrap();
        assert_eq!(plane(&index, pick.target), TOP, "{projection:?}");
        assert_eq!((pick.model, pick.body), (7, body));
        assert!(
            pick.at.distance(DVec3::new(20.0, 5.0, 10.0)) < 1e-3,
            "{pick:?}"
        );

        let bottom = camera(View::Bottom, projection);
        let at = shown(&bottom, DVec3::new(-20.0, 5.0, 0.0));
        let pick = index.pick(&bottom, SIZE, at, Picks::All).unwrap();
        assert_eq!(plane(&index, pick.target), BOTTOM, "{projection:?}");

        let front = camera(View::Front, projection);
        let at = shown(&front, DVec3::new(10.0, -20.0, 5.0));
        let pick = index.pick(&front, SIZE, at, Picks::All).unwrap();
        assert_eq!(plane(&index, pick.target), FRONT, "{projection:?}");

        // Off the plate.
        let off = shown(&top, DVec3::new(45.0, 0.0, 10.0));
        assert_eq!(index.pick(&top, SIZE, off, Picks::All), None);
    }
    // Straight down the hole, far from its rims, nothing.
    let top = camera(View::Top, Projection::Orthographic);
    let middle = shown(&top, DVec3::new(0.0, 0.0, 10.0));
    assert_eq!(index.pick(&top, SIZE, middle, Picks::All), None);
    // The hole's wall, from above and to the side.
    let mut above = camera(View::Front, Projection::Orthographic);
    above.orbit(0.0, 0.9);
    let wall = shown(&above, DVec3::new(0.0, 8.0, 8.0));
    let pick = index.pick(&above, SIZE, wall, Picks::All).unwrap();
    let Picked::Face(face) = pick.target else {
        panic!("{pick:?}");
    };
    let summary = index.picking().faces()[face as usize].summary;
    assert!(
        matches!(summary, Summary::Cylinder { radius, .. } if (radius - 8.0).abs() < 1e-9),
        "{summary:?}"
    );
}

#[test]
fn an_edge_within_reach_wins_over_the_face() {
    let index = plate();
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let top = camera(View::Top, projection);
        // 4 pixels in from the top's front edge.
        let edge = shown(&top, DVec3::new(10.0, -20.0, 10.0));
        let pick = index
            .pick(&top, SIZE, edge - DVec2::new(0.0, 4.0), Picks::All)
            .unwrap();
        assert_eq!(
            sides(&index, pick.target),
            vec![FRONT, TOP],
            "{projection:?}"
        );
        assert!(
            pick.at.distance(DVec3::new(10.0, -20.0, 10.0)) < 0.01,
            "{pick:?}"
        );
        // 8 pixels in, the top.
        let pick = index
            .pick(&top, SIZE, edge - DVec2::new(0.0, 8.0), Picks::All)
            .unwrap();
        assert_eq!(plane(&index, pick.target), TOP, "{projection:?}");
        // And outside, off the plate, the edge.
        let pick = index
            .pick(&top, SIZE, edge + DVec2::new(0.0, 5.0), Picks::All)
            .unwrap();
        assert_eq!(
            sides(&index, pick.target),
            vec![FRONT, TOP],
            "{projection:?}"
        );
    }
}

#[test]
fn an_edge_behind_the_plate_isnt_picked() {
    let index = plate();
    for projection in [Projection::Orthographic, Projection::Perspective] {
        // From the front and above: the back's bottom edge shows over the
        // top, behind it, well away from the back's top edge and the
        // hole's rims.
        let mut camera = camera(View::Front, projection);
        camera.orbit(0.0, 0.6);
        let hidden = shown(&camera, DVec3::new(24.0, 20.0, 0.0));
        let back = shown(&camera, DVec3::new(24.0, 20.0, 10.0));
        assert!(hidden.distance(back) > 4.0 * EDGE_REACH, "{hidden} {back}");
        let pick = index.pick(&camera, SIZE, hidden, Picks::All).unwrap();
        assert_eq!(plane(&index, pick.target), TOP, "{projection:?}");
        let pick = index.pick(&camera, SIZE, back, Picks::All).unwrap();
        assert_eq!(
            sides(&index, pick.target),
            vec![TOP, BACK],
            "{projection:?}"
        );
        // The front's bottom edge shows.
        let front = shown(&camera, DVec3::new(10.0, -20.0, 0.0));
        let pick = index.pick(&camera, SIZE, front, Picks::All).unwrap();
        assert_eq!(
            sides(&index, pick.target),
            vec![FRONT, BOTTOM],
            "{projection:?}"
        );
    }
}

#[test]
fn a_hovered_face_is_drawn_with_the_edges_around_it_outlined() {
    let index = plate();
    let top = camera(View::Top, Projection::Orthographic);
    let at = shown(&top, DVec3::new(20.0, 5.0, 10.0));
    let target = index.pick(&top, SIZE, at, Picks::All).unwrap().target;
    let Picked::Face(face) = target else {
        panic!("{target:?}");
    };
    let drawn = index.highlight(&[target], &[]);
    assert_eq!(drawn.hovered_faces, [face]);
    // The top's four sides and the hole's rim.
    let outlined = &drawn.highlights.outlined;
    assert_eq!(outlined.len(), 5, "{outlined:?}");
    for &edge in outlined {
        assert!(index.edge_faces(edge).unwrap().contains(&face));
    }
    assert!(drawn.selected_faces.is_empty());
    assert!(drawn.highlights.vertices.is_empty());
    assert!(index.highlight(&[], &[]).is_empty());
    // Past the tables, nothing.
    let past = [
        Picked::Face(u32::MAX),
        Picked::Edge(u32::MAX),
        Picked::Vertex(u32::MAX),
    ];
    assert!(index.highlight(&past, &past).is_empty());
    assert_eq!(index.body(Picked::Edge(u32::MAX)), None);
    assert_eq!(index.body(Picked::Vertex(u32::MAX)), None);
}

#[test]
fn a_hovered_edge_or_vertex_is_drawn_alone() {
    let index = plate();
    let edge = index.highlight(&[Picked::Edge(3)], &[]);
    assert!(edge.hovered_faces.is_empty());
    assert_eq!(edge.highlights.outlined, [3]);
    let vertex = index.highlight(&[Picked::Vertex(3)], &[]);
    assert!(vertex.highlights.outlined.is_empty());
    let hovered = Vertex {
        corner: 3,
        hovered: true,
        selected: false,
    };
    assert_eq!(vertex.highlights.vertices, [hovered]);
}

#[test]
fn the_selection_is_drawn_by_kind_and_a_hovered_vertex_once() {
    let index = plate();
    let selected = [
        Picked::Face(2),
        Picked::Edge(5),
        Picked::Vertex(3),
        Picked::Face(6),
        Picked::Vertex(6),
    ];
    let drawn = index.highlight(&[Picked::Vertex(6)], &selected);
    assert!(drawn.hovered_faces.is_empty());
    assert_eq!(drawn.selected_faces, [2, 6]);
    assert_eq!(drawn.highlights.selected_edges, [5]);
    let vertex = |corner, hovered| Vertex {
        corner,
        hovered,
        selected: true,
    };
    assert_eq!(
        drawn.highlights.vertices,
        [vertex(3, false), vertex(6, true)]
    );
}

/// The plate's top front left corner, where its top, front and left side
/// meet.
const CORNER: DVec3 = DVec3::new(-30.0, -20.0, 10.0);

#[test]
fn a_vertex_within_reach_wins_over_its_edges_and_faces() {
    let index = plate();
    for projection in [Projection::Orthographic, Projection::Perspective] {
        for view in [View::Top, View::Front] {
            let camera = camera(view, projection);
            // About 4 pixels off the corner, on the plate, square to the
            // view.
            let inward = match view {
                View::Top => DVec3::new(0.6, 0.6, 0.0),
                _ => DVec3::new(0.6, 0.0, -0.6),
            };
            let at = shown(&camera, CORNER + inward);
            let pick = index.pick(&camera, SIZE, at, Picks::All).unwrap();
            let Picked::Vertex(corner) = pick.target else {
                panic!("{view:?} {projection:?}: {pick:?}");
            };
            // The front one, not the one behind it.
            assert!(pick.at.distance(CORNER) < 1e-4, "{pick:?}");
            assert_eq!(index.corner_point(corner), Some(pick.at));
            assert_eq!(Some(pick.body), index.face_body(0));
            // Faces or edges only, no vertex.
            for picks in [Picks::Faces, Picks::Edges] {
                let pick = index.pick(&camera, SIZE, at, picks).unwrap();
                assert!(!matches!(pick.target, Picked::Vertex(_)), "{pick:?}");
            }
            // Out of reach, an edge or the face.
            let far = shown(&camera, CORNER + inward * 4.0);
            let pick = index.pick(&camera, SIZE, far, Picks::All).unwrap();
            assert!(!matches!(pick.target, Picked::Vertex(_)), "{pick:?}");
        }
    }
}

#[test]
fn only_corners_where_three_faces_meet_are_vertices() {
    // The plate's eight corners; the holes' rims, closing on themselves,
    // have none.
    let index = plate();
    let corners = index.mesh().corners().len() as u32;
    let vertices: Vec<u32> = (0..corners)
        .filter(|&c| index.vertex_keys(c).is_some())
        .collect();
    assert_eq!(vertices.len(), 8, "{vertices:?}");
    assert!(corners > 8);
    // Each is found again by its keys and where it is.
    let body = index.face_body(0).unwrap();
    for &corner in &vertices {
        let keys = index.vertex_keys(corner).unwrap();
        assert!(keys.is_sorted());
        let at = index.corner_point(corner).unwrap();
        assert_eq!(index.find_vertex(body, keys, at), Some(corner));
        assert_eq!(index.body(Picked::Vertex(corner)), Some(body));
    }
}

#[test]
fn the_index_is_the_same_built_twice() {
    let [a, b] = [plate(), plate()];
    let camera = camera(View::Front, Projection::Perspective);
    for x in (0..40).map(|i| f64::from(i) * 10.0) {
        for y in (0..30).map(|i| f64::from(i) * 10.0) {
            let at = DVec2::new(x, y);
            assert_eq!(
                a.pick(&camera, SIZE, at, Picks::All),
                b.pick(&camera, SIZE, at, Picks::All)
            );
        }
    }
    assert_eq!(a.triangles.items, b.triangles.items);
}

#[test]
fn tables_not_of_the_mesh_pick_nothing() {
    let index = plate();
    let other = PickIndex::new(Arc::default(), index.picking().clone(), 1);
    let top = camera(View::Top, Projection::Orthographic);
    let at = shown(&top, DVec3::new(20.0, 5.0, 10.0));
    assert_eq!(other.pick(&top, SIZE, at, Picks::All), None);
    let empty = PickIndex::new(Arc::default(), Arc::default(), 1);
    assert_eq!(empty.pick(&top, SIZE, at, Picks::All), None);
    assert!(empty.highlight(&[Picked::Face(0)], &[]).is_empty());
}

#[test]
fn picking_only_faces_or_only_edges_skips_the_other() {
    let index = plate();
    let top = camera(View::Top, Projection::Orthographic);
    // 4 pixels in from the top's front edge, which wins over the top.
    let near_edge = shown(&top, DVec3::new(10.0, -20.0, 10.0)) - DVec2::new(0.0, 4.0);
    let pick = index.pick(&top, SIZE, near_edge, Picks::Faces).unwrap();
    assert_eq!(plane(&index, pick.target), TOP);
    let pick = index.pick(&top, SIZE, near_edge, Picks::Edges).unwrap();
    assert_eq!(sides(&index, pick.target), vec![FRONT, TOP]);
    // Over the top's middle no edge is within reach.
    let middle = shown(&top, DVec3::new(20.0, 5.0, 10.0));
    assert_eq!(index.pick(&top, SIZE, middle, Picks::Edges), None);
    assert!(index.pick(&top, SIZE, middle, Picks::Faces).is_some());
}

/// A plate `60 × 40 × 10` times `scale` from z 0 up, its middle at
/// `(offset, 0)`, with `holes` × `holes` holes through it on a grid,
/// made ready for picking as model 7.
pub(crate) fn plate_of(scale: f64, offset: f64, holes: u32) -> PickIndex {
    use varde_document::{Command, Editor, Extent, Extrude, Operation};
    use varde_expr::Value;
    use varde_sketch::{Curve, Sketch};

    let mut editor = Editor::new(Document::default());
    let plane = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let at = |x: f64, y: f64| DVec2::new(offset + x * scale, y * scale);
    let corners = [(-30.0, -20.0), (30.0, -20.0), (30.0, 20.0), (-30.0, 20.0)]
        .map(|(x, y)| sketch.add_point(at(x, y)).unwrap());
    for (k, &start) in corners.iter().enumerate() {
        let end = corners[(k + 1) % 4];
        sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    let pitch = 56.0 / f64::from(holes.max(1));
    let radius = pitch * 0.3;
    for i in 0..holes {
        for j in 0..holes {
            let x = -28.0 + pitch * (f64::from(i) + 0.5);
            let y = (-28.0 + pitch * (f64::from(j) + 0.5)) * 36.0 / 56.0;
            let center = sketch.add_point(at(x, y)).unwrap();
            let circle = Curve::Circle {
                center,
                radius: radius * 36.0 / 56.0 * scale,
            };
            sketch.add_curve(circle, false).unwrap();
        }
    }
    let profiles = sketch.profiles().unwrap();
    let region = (profiles.regions.iter())
        .position(|region| region.holes.len() as u32 == holes * holes)
        .and_then(|index| profiles.reference(index))
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let design = editor.document().design();
    let thickness = Value::new(&(10.0 * scale).to_string(), &Extent::ask(&design)).unwrap();
    let extrude = Extrude {
        sketch: feature,
        regions: vec![region],
        extent: Extent::OneSide(thickness),
        flip: false,
        operation: Operation::NewBody(varde_document::BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let document = editor.document().clone();
    let mut cache = Cache::default();
    let evaluation = evaluate(&document, &mut cache);
    let (mesh, picking) = tessellate_picking(&document, &evaluation, &mut cache).unwrap();
    PickIndex::new(mesh, picking, 7)
}

#[test]
#[ignore]
fn measure_the_index_of_a_plate_with_400_holes() {
    let start = std::time::Instant::now();
    let document_and_mesh = plate_of(1.0, 0.0, 20);
    let built = start.elapsed();
    let mesh = document_and_mesh.mesh().clone();
    let picking = document_and_mesh.picking().clone();
    let start = std::time::Instant::now();
    let index = PickIndex::new(mesh.clone(), picking.clone(), 1);
    let indexed = start.elapsed();
    let top = camera(View::Top, Projection::Perspective);
    let start = std::time::Instant::now();
    let mut n = 0;
    for x in 0..40 {
        for y in 0..30 {
            let at = DVec2::new(f64::from(x) * 10.0, f64::from(y) * 10.0);
            n += usize::from(index.pick(&top, SIZE, at, Picks::All).is_some());
        }
    }
    let picked = start.elapsed();
    eprintln!(
        "triangles {} edge points {} faces {} edges {}: model {built:?}, index {indexed:?}, 1200 picks {picked:?} ({n} hits)",
        mesh.triangle_count(),
        mesh.edge_vertices().len(),
        picking.faces().len(),
        mesh.edge_count()
    );
}

/// Looking from `view` at `target`, `height` across the view's height, in
/// `projection`.
fn camera_at(view: View, projection: Projection, target: DVec3, height: f64) -> Camera {
    let mut camera = Camera::default();
    camera.set_projection(projection);
    camera.look_from(view);
    camera.set_target(target.as_vec3());
    camera.zoom(height as f32 / camera.view_height());
    camera
}

/// The plate's top, the edge between its front and top, and that its
/// back's bottom edge seen over the top isn't picked, for a plate of
/// [`plate_of`] `scale` and `offset` seen `height` across from the top
/// and from the front and above, in both projections.
fn picks_the_plate(index: &PickIndex, scale: f64, offset: f64, height: f64) {
    let p = |x: f64, y: f64, z: f64| DVec3::new(offset + x * scale, y * scale, z * scale);
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let case = format!("{projection:?} at {scale} {offset} {height}");
        // From the top, looking at a point on the top.
        let target = p(20.0, 5.0, 10.0);
        let top = camera_at(View::Top, projection, target, height);
        let pick = index
            .pick(&top, SIZE, shown(&top, target), Picks::All)
            .unwrap_or_else(|| panic!("{case}"));
        assert_eq!(plane(index, pick.target).0, TOP.0, "{case}");
        assert!(
            pick.at.distance(target) <= 1e-5 * height.max(scale),
            "{case}: {pick:?}"
        );
        // 3 pixels in from the top's front edge, with the edge in view.
        let edge = p(20.0, -20.0, 10.0);
        let top = camera_at(View::Top, projection, edge, height);
        let at = shown(&top, edge) - DVec2::new(0.0, 3.0);
        let pick = index
            .pick(&top, SIZE, at, Picks::All)
            .unwrap_or_else(|| panic!("{case}"));
        let sides: Vec<_> = sides(index, pick.target).iter().map(|s| s.0).collect();
        assert_eq!(sides, [FRONT.0, TOP.0], "{case}");
        // From the front and above, the back's bottom edge, hidden.
        let mut above = camera_at(View::Front, projection, p(0.0, 0.0, 5.0), height);
        above.orbit(0.0, 0.6);
        let hidden = p(24.0, 20.0, 0.0);
        let at = shown(&above, hidden);
        if (0.0..f64::from(SIZE[0])).contains(&at.x) && (0.0..f64::from(SIZE[1])).contains(&at.y) {
            let back = shown(&above, p(24.0, 20.0, 10.0));
            if at.distance(back) > 2.0 * EDGE_REACH {
                let pick = index
                    .pick(&above, SIZE, at, Picks::All)
                    .unwrap_or_else(|| panic!("{case}"));
                assert_eq!(plane(index, pick.target).0, TOP.0, "{case}");
            }
        }
    }
}

#[test]
fn tiny_huge_and_far_off_models_pick_alike() {
    for (scale, offset) in [
        (1.0, 0.0),
        (1e-3, 0.0),
        (1e4, 0.0),
        (1.0, 1e5),
        (1e-2, -2e4),
    ] {
        let index = plate_of(scale, offset, 0);
        picks_the_plate(&index, scale, offset, 60.0 * scale);
    }
}

#[test]
fn extreme_zooms_pick_what_shows() {
    let index = plate_of(1.0, 0.0, 0);
    // Zoomed in as far as the camera goes, and nearly.
    for height in [1e-3, 1e-2, 0.1] {
        picks_the_plate(&index, 1.0, 0.0, height);
    }
    // Zoomed out as far as it goes: the plate is a dot, and the cursor
    // over it picks it.
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = camera_at(View::Top, projection, DVec3::new(0.0, 0.0, 5.0), 60.0);
        camera.zoom(f32::MAX);
        let at = shown(&camera, DVec3::new(20.0, 5.0, 10.0));
        let pick = index.pick(&camera, SIZE, at, Picks::All);
        assert!(pick.is_some(), "{projection:?}");
        // And well away from it, nothing.
        let off = at + DVec2::new(50.0, 0.0);
        assert_eq!(index.pick(&camera, SIZE, off, Picks::All), None);
    }
}

#[test]
fn edges_seen_end_on_and_faces_seen_edge_on_pick_what_shows() {
    let index = plate();
    for projection in [Projection::Orthographic, Projection::Perspective] {
        for tilt in [0.0, 1e-4, 1e-2] {
            let case = format!("{projection:?} tilted {tilt}");
            // The plate's corner from straight above, or nearly: its
            // upright edge shows end on, the top's two edges meet there,
            // and the corner on top wins over them.
            let mut above = camera(View::Top, projection);
            above.orbit(0.0, -tilt);
            let corner = DVec3::new(30.0, -20.0, 10.0);
            let at = shown(&above, corner) + DVec2::new(2.0, 2.0);
            let pick = index
                .pick(&above, SIZE, at, Picks::All)
                .unwrap_or_else(|| panic!("{case}"));
            // The top one, or tilted, maybe the bottom one, which shows
            // too, on the outline.
            assert!(matches!(pick.target, Picked::Vertex(_)), "{case}: {pick:?}");
            let ends = [corner, corner - DVec3::Z * 10.0];
            let end = |at: DVec3| ends.iter().position(|end| end.distance(at) < 1e-4);
            assert!(end(pick.at).is_some(), "{case}: {pick:?}");
            assert!(tilt != 0.0 || end(pick.at) == Some(0), "{case}: {pick:?}");
            assert!(
                shown(&above, pick.at).distance(at) <= VERTEX_REACH,
                "{case}: {pick:?}"
            );
            let pick = index
                .pick(&above, SIZE, at, Picks::Edges)
                .unwrap_or_else(|| panic!("{case}"));
            assert!(matches!(pick.target, Picked::Edge(_)), "{case}: {pick:?}");
            assert!(
                shown(&above, pick.at).distance(at) <= EDGE_REACH,
                "{case}: {pick:?}"
            );
            // The top seen edge on from the front, or nearly: on its line,
            // the front's top edge, not the back's behind it.
            let mut front = camera(View::Front, projection);
            front.orbit(0.0, tilt);
            let on = shown(&front, DVec3::new(10.0, -20.0, 10.0));
            let pick = index
                .pick(&front, SIZE, on, Picks::All)
                .unwrap_or_else(|| panic!("{case}"));
            assert_eq!(sides(&index, pick.target), vec![FRONT, TOP], "{case}");
            // Below it, the front.
            let pick = index
                .pick(&front, SIZE, on + DVec2::new(0.0, 8.0), Picks::Faces)
                .unwrap_or_else(|| panic!("{case}"));
            assert_eq!(plane(&index, pick.target), FRONT, "{case}");
        }
    }
}
