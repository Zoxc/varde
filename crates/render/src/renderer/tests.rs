use super::*;

#[test]
fn alpha_steps_are_the_nearest_of_the_table() {
    assert_eq!(alpha_step(Some(0.0), 255), 0);
    assert_eq!(alpha_step(Some(0.3), 255), 77);
    assert_eq!(alpha_step(Some(0.999), 255), 255);
    assert_eq!(alpha_step(Some(1.0), 255), 255);
    for alpha in [None, Some(f32::NAN), Some(-0.5), Some(1.5)] {
        assert_eq!(alpha_step(alpha, 255), 255, "{alpha:?}");
    }
}

#[test]
fn alpha_steps_multiply_to_the_nearest_step() {
    assert_eq!(product_step(255, 77, 255), 77);
    assert_eq!(product_step(128, 128, 255), 64);
    assert_eq!(product_step(0, 255, 255), 0);
    assert_eq!(product_step(3, 3, 3), 3);
    assert_eq!(product_step(1, 2, 3), 1);
    // A table of a single step has none to multiply.
    assert_eq!(product_step(0, 0, 0), 0);
}

#[test]
fn a_part_less_than_opaque_is_never_drawn_invisible_on_a_short_table() {
    // Two steps, 0 and 1: a faint body is drawn opaque, not at all only
    // at 0.
    for alpha in [0.1, 0.3, 0.49] {
        assert_eq!(alpha_step(Some(alpha), 1), 1, "{alpha}");
    }
    assert_eq!(alpha_step(Some(0.0), 1), 0);
    // A single step is opaque.
    assert_eq!(alpha_step(Some(0.1), 0), 0);
    // A finer table rounds the faintest up to its first step.
    assert_eq!(alpha_step(Some(0.01), 3), 1);
}

#[test]
fn errors_are_built_together_without_points_past_the_bound() {
    let mut lines = RenderLines::default();
    lines
        .push([Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0)])
        .unwrap();
    lines.push([Vec3::Z, Vec3::new(0.0, 2.0, 1.0)]).unwrap();
    let far = RenderLines::MAX_POSITION * 2.0;
    let points = [
        [5.0, -1.0, 0.0],
        [f32::NAN, 0.0, 0.0],
        [f32::INFINITY, 0.0, 0.0],
        [0.0, far, 0.0],
    ];
    let (mesh, source) = (RenderMesh::default(), Arc::new(()));
    let parts = |points| ErrorParts {
        mesh: &mesh,
        lines: &lines,
        points,
        source: Arc::downgrade(&source) as Weak<dyn Any + Send + Sync>,
        halo_only: false,
    };
    let built = BuiltErrors::new(&[parts(&points), parts(&[])]);
    // Only the first point is drawn, and bounded.
    assert_eq!(built.points.len(), 1);
    let bounds = built.bounds.unwrap();
    assert_eq!(bounds.min, Vec3::new(0.0, -1.0, 0.0));
    assert_eq!(bounds.max, Vec3::new(5.0, 2.0, 1.0));
    // Four polylines, each its own edge, between the stream's two ends.
    let edges: Vec<u32> = built.edges.iter().map(|point| point.edge).collect();
    assert_eq!(
        edges,
        [NO_EDGE, 0, 0, 0, 1, 1, 2, 2, 2, 3, 3, NO_EDGE].to_vec()
    );
}

#[test]
fn errors_drawn_whole_come_before_those_drawn_as_their_halo() {
    let mut lines = RenderLines::default();
    lines.push([Vec3::ZERO, Vec3::X]).unwrap();
    let mut other = RenderLines::default();
    other.push([Vec3::Y, Vec3::Z, Vec3::ONE]).unwrap();
    let (mesh, source) = (RenderMesh::default(), Arc::new(()));
    let parts = |lines, points, halo_only| ErrorParts {
        mesh: &mesh,
        lines,
        points,
        source: Arc::downgrade(&source) as Weak<dyn Any + Send + Sync>,
        halo_only,
    };
    let point = [[1.0, 2.0, 3.0]];
    let built = BuiltErrors::new(&[parts(&other, &[], true), parts(&lines, &point, false)]);
    // The whole one's two points first, then the halo's three.
    let edges: Vec<u32> = built.edges.iter().map(|point| point.edge).collect();
    assert_eq!(edges, [NO_EDGE, 0, 0, 1, 1, 1, NO_EDGE].to_vec());
    assert_eq!(
        built.cores,
        Cores {
            corners: 0,
            edges: 3,
            points: 1,
        }
    );
    // All whole: the cores are all of them, the stream's end aside.
    let built = BuiltErrors::new(&[parts(&lines, &point, false)]);
    assert_eq!(built.cores.edges, 3);
    assert_eq!(built.edges.len(), 4);
    // All halo: none.
    let built = BuiltErrors::new(&[parts(&lines, &point, true)]);
    assert_eq!(
        built.cores,
        Cores {
            corners: 0,
            edges: 1,
            points: 0
        }
    );
}

#[test]
fn errors_with_nothing_to_draw_draw_nothing() {
    // No errors, or errors of no triangles, curves or points: the stream
    // of curves is empty rather than its two ends, which would keep the
    // errors' passes (and their target) going while none are shown.
    let (mesh, lines, source) = (RenderMesh::default(), RenderLines::default(), Arc::new(()));
    let empty = ErrorParts {
        mesh: &mesh,
        lines: &lines,
        points: &[],
        source: Arc::downgrade(&source) as Weak<dyn Any + Send + Sync>,
        halo_only: false,
    };
    for errors in [&[][..], &[empty.clone(), empty]] {
        let built = BuiltErrors::new(errors);
        assert!(built.edges.is_empty(), "{} points", built.edges.len());
        assert!(built.positions.is_empty() && built.points.is_empty());
    }

    let instance = wgpu::Instance::default();
    let options = wgpu::RequestAdapterOptions::default();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&options)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        // As iced asks for, which allows two bind groups only.
        required_limits: wgpu::Limits {
            max_bind_groups: 2,
            ..wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
        },
        ..Default::default()
    }))
    .unwrap();
    let mut lines = RenderLines::default();
    lines.push([Vec3::ZERO, Vec3::X]).unwrap();
    let shown = ErrorParts {
        mesh: &mesh,
        lines: &lines,
        points: &[],
        source: Arc::downgrade(&source) as Weak<dyn Any + Send + Sync>,
        halo_only: false,
    };
    let mut buffers = ErrorBuffers::default();
    buffers.write(&device, &queue, &[shown]).unwrap();
    assert!(buffers.any());
    // Shown, then hidden again.
    buffers.write(&device, &queue, &[]).unwrap();
    assert!(!buffers.any());
}
