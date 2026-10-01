use varde_document::Document;
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
    let placement = Plane::Origin(OriginPlane::XY).placement();
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
    let faces = index.picking().chains()[chain as usize].faces;
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
    let body = index.picking().faces()[0].body;
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let top = camera(View::Top, projection);
        let at = shown(&top, DVec3::new(20.0, 5.0, 10.0));
        let pick = index.pick(&top, SIZE, at, Picks::FacesAndEdges).unwrap();
        assert_eq!(plane(&index, pick.target), TOP, "{projection:?}");
        assert_eq!((pick.model, pick.body), (7, body));
        assert!(
            pick.at.distance(DVec3::new(20.0, 5.0, 10.0)) < 1e-3,
            "{pick:?}"
        );

        let bottom = camera(View::Bottom, projection);
        let at = shown(&bottom, DVec3::new(-20.0, 5.0, 0.0));
        let pick = index.pick(&bottom, SIZE, at, Picks::FacesAndEdges).unwrap();
        assert_eq!(plane(&index, pick.target), BOTTOM, "{projection:?}");

        let front = camera(View::Front, projection);
        let at = shown(&front, DVec3::new(10.0, -20.0, 5.0));
        let pick = index.pick(&front, SIZE, at, Picks::FacesAndEdges).unwrap();
        assert_eq!(plane(&index, pick.target), FRONT, "{projection:?}");

        // Off the plate.
        let off = shown(&top, DVec3::new(45.0, 0.0, 10.0));
        assert_eq!(index.pick(&top, SIZE, off, Picks::FacesAndEdges), None);
    }
    // Straight down the hole, far from its rims, nothing.
    let top = camera(View::Top, Projection::Orthographic);
    let middle = shown(&top, DVec3::new(0.0, 0.0, 10.0));
    assert_eq!(index.pick(&top, SIZE, middle, Picks::FacesAndEdges), None);
    // The hole's wall, from above and to the side.
    let mut above = camera(View::Front, Projection::Orthographic);
    above.orbit(0.0, 0.9);
    let wall = shown(&above, DVec3::new(0.0, 8.0, 8.0));
    let pick = index
        .pick(&above, SIZE, wall, Picks::FacesAndEdges)
        .unwrap();
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
            .pick(
                &top,
                SIZE,
                edge - DVec2::new(0.0, 4.0),
                Picks::FacesAndEdges,
            )
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
            .pick(
                &top,
                SIZE,
                edge - DVec2::new(0.0, 8.0),
                Picks::FacesAndEdges,
            )
            .unwrap();
        assert_eq!(plane(&index, pick.target), TOP, "{projection:?}");
        // And outside, off the plate, the edge.
        let pick = index
            .pick(
                &top,
                SIZE,
                edge + DVec2::new(0.0, 5.0),
                Picks::FacesAndEdges,
            )
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
        let pick = index
            .pick(&camera, SIZE, hidden, Picks::FacesAndEdges)
            .unwrap();
        assert_eq!(plane(&index, pick.target), TOP, "{projection:?}");
        let pick = index
            .pick(&camera, SIZE, back, Picks::FacesAndEdges)
            .unwrap();
        assert_eq!(
            sides(&index, pick.target),
            vec![TOP, BACK],
            "{projection:?}"
        );
        // The front's bottom edge shows.
        let front = shown(&camera, DVec3::new(10.0, -20.0, 0.0));
        let pick = index
            .pick(&camera, SIZE, front, Picks::FacesAndEdges)
            .unwrap();
        assert_eq!(
            sides(&index, pick.target),
            vec![FRONT, BOTTOM],
            "{projection:?}"
        );
    }
}

#[test]
fn highlights_hold_a_faces_triangles_and_an_edges_lines() {
    let index = plate();
    let top = camera(View::Top, Projection::Orthographic);
    let at = shown(&top, DVec3::new(20.0, 5.0, 10.0));
    let face = index
        .pick(&top, SIZE, at, Picks::FacesAndEdges)
        .unwrap()
        .target;
    let highlight = index.highlight([(face, Emphasis::Hovered)]);
    assert!(!highlight.is_empty());
    assert_eq!(index.highlight([]), Highlight::default());
    // Past the tables, nothing.
    let past = index.highlight([
        (Picked::Face(u32::MAX), Emphasis::Selected),
        (Picked::Edge(u32::MAX), Emphasis::Selected),
    ]);
    assert!(past.is_empty());
    assert_eq!(index.body(Picked::Edge(u32::MAX)), None);
}

#[test]
fn a_chains_segments_join_into_polylines() {
    let p = |x: f32, y: f32| Vec3::new(x, y, 0.0);
    // Out of order and either way round: an open run and a closed square.
    let lines = polylines(vec![
        [p(1.0, 0.0), p(2.0, 0.0)],
        [p(10.0, 0.0), p(11.0, 0.0)],
        [p(1.0, 0.0), p(0.0, 0.0)],
        [p(11.0, 1.0), p(11.0, 0.0)],
        [p(10.0, 1.0), p(11.0, 1.0)],
        [p(10.0, 0.0), p(10.0, 1.0)],
    ]);
    assert_eq!(
        lines,
        vec![
            vec![p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0)],
            vec![
                p(10.0, 0.0),
                p(11.0, 0.0),
                p(11.0, 1.0),
                p(10.0, 1.0),
                p(10.0, 0.0)
            ],
        ]
    );
}

#[test]
fn the_index_is_the_same_built_twice() {
    let [a, b] = [plate(), plate()];
    let camera = camera(View::Front, Projection::Perspective);
    for x in (0..40).map(|i| f64::from(i) * 10.0) {
        for y in (0..30).map(|i| f64::from(i) * 10.0) {
            let at = DVec2::new(x, y);
            assert_eq!(
                a.pick(&camera, SIZE, at, Picks::FacesAndEdges),
                b.pick(&camera, SIZE, at, Picks::FacesAndEdges)
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
    assert_eq!(other.pick(&top, SIZE, at, Picks::FacesAndEdges), None);
    let empty = PickIndex::new(Arc::default(), Arc::default(), 1);
    assert_eq!(empty.pick(&top, SIZE, at, Picks::FacesAndEdges), None);
    assert!(
        empty
            .highlight([(Picked::Face(0), Emphasis::Hovered)])
            .is_empty()
    );
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
