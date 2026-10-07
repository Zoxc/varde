//! Writes the sample designs in the workspace's `examples/` directory:
//! `cargo run -p varde-view --example samples [-- <directory>]`.
//!
//! Each design is built as the commands a user would give build it,
//! every sketch fully constrained and dimensioned. Before writing, each
//! is checked: its sketches pass [`Sketch::check`], solve as they stand
//! and have no freedom or redundancy left, the file reads back as the
//! same document, and its history regenerates without a failed feature.
//!
//! Each is written with its thumbnail, as the app's save writes it (see
//! `crates/app/src/doc/thumbnail.rs`): the regenerated model's bodies,
//! framed from the home camera, rendered offscreen in both themes'
//! colours. That needs a GPU adapter: without one nothing is written.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use glam::DVec2;
use varde_document::{
    AxisLine, BodyId, BodyOp, Combine, Command, Document, Editor, Extent, Extrude, FeatureId,
    FeatureKind, Operation, OriginPlane, Plane, Revolve, Targets, Turn,
};
use varde_expr::Value;
use varde_io::thumbnail::{Image, Thumbnail};
use varde_render::{Camera, Renderer, wgpu};
use varde_sketch::{
    Constraint, Curve, Design, Dimension, Id, Measure, Region, RegionRef, Sketch, Spline, analyse,
};
use varde_view::{THUMBNAIL_SCALE, ThumbnailRequest, thumbnail_shot};

/// A sketch being drawn, with the design its dimensions are read in
/// (without parameters, which dimensions don't use yet).
struct Draw {
    sketch: Sketch,
    design: Design<'static>,
}

impl Draw {
    fn new(editor: &Editor) -> Draw {
        Draw {
            sketch: Sketch::default(),
            design: Design {
                params: varde_document::Params::EMPTY,
                ..editor.document().design()
            },
        }
    }

    fn point(&mut self, x: f64, y: f64) -> Id {
        self.sketch
            .add_point(DVec2::new(x, y))
            .expect("ids to spare")
    }

    fn curve(&mut self, curve: Curve) -> Id {
        self.sketch.add_curve(curve, false).expect("ids to spare")
    }

    fn line(&mut self, start: Id, end: Id) -> Id {
        self.curve(Curve::Line { start, end })
    }

    fn arc(&mut self, center: Id, start: Id, end: Id) -> Id {
        self.curve(Curve::Arc { center, start, end })
    }

    fn circle(&mut self, center: Id, radius: f64) -> Id {
        self.curve(Curve::Circle { center, radius })
    }

    fn constrain(&mut self, constraint: Constraint) {
        self.sketch
            .add_constraint(constraint)
            .expect("ids to spare");
    }

    /// Makes the curves `a` and `b` tangent, on the side they're drawn.
    fn tangent(&mut self, a: Id, b: Id) {
        let constraint = self.sketch.tangent(a, b).expect("curves that can touch");
        self.constrain(constraint);
    }

    /// A driving dimension of `measure`, `text` as typed, its label
    /// `label` from where the measure is.
    fn dimension(&mut self, measure: Measure, text: &str, label: DVec2) {
        let value = Value::new(text, &measure.ask(&self.design)).expect("a valid dimension");
        let side = self.sketch.side(&measure);
        self.sketch
            .add_dimension(Dimension {
                measure,
                value,
                driving: true,
                label,
                side,
            })
            .expect("ids to spare");
    }

    /// Checks the sketch through and through and commits it to `feature`.
    fn commit(self, editor: &mut Editor, feature: FeatureId, name: &str) -> Sketch {
        self.sketch
            .check(&self.design)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let analysis = analyse(&self.sketch);
        assert!(analysis.solved, "{name} doesn't solve as drawn");
        assert_eq!(analysis.freedom, 0, "{name} isn't fully constrained");
        assert!(
            analysis.redundant.is_empty(),
            "{name} has redundant constraints: {:?}",
            analysis.redundant
        );
        editor
            .apply(Command::SetSketch {
                feature,
                sketch: Box::new(self.sketch.clone()),
            })
            .expect("a valid sketch");
        self.sketch
    }
}

fn add_sketch(editor: &mut Editor, name: &str, plane: OriginPlane) -> FeatureId {
    editor
        .apply(Command::AddSketch {
            name: name.to_owned(),
            plane: Plane::Origin(plane),
        })
        .expect("ids to spare");
    editor.document().features().last().expect("just added").id
}

fn add_feature(editor: &mut Editor, name: &str, kind: FeatureKind) {
    editor
        .apply(Command::AddFeature {
            name: name.to_owned(),
            kind: Box::new(kind),
        })
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
}

/// The one region of `sketch` that `pick` takes.
fn region(sketch: &Sketch, pick: impl Fn(&Region) -> bool) -> RegionRef {
    let profiles = sketch.profiles().expect("a simple sketch");
    let found: Vec<usize> = (0..profiles.regions.len())
        .filter(|&index| pick(&profiles.regions[index]))
        .collect();
    assert_eq!(found.len(), 1, "one region to pick");
    profiles.reference(found[0]).expect("a region to name")
}

fn length(text: &str) -> Value {
    Value::new(text, &Extent::ask(&Document::default().design())).expect("a length")
}

/// An 80 × 50 × 8 mm plate with rounded corners, a bolt hole at each
/// corner's centre and a slot cut through its middle.
fn mounting_plate() -> Document {
    let mut editor = Editor::new(Document::default());
    let outline = add_sketch(&mut editor, "Plate outline", OriginPlane::XY);
    let mut draw = Draw::new(&editor);
    let (w, h, r) = (40.0, 25.0, 8.0);
    // The lines' ends, counter-clockwise from the bottom line's start,
    // and the corners' centres.
    let ends = [
        (-w + r, -h),
        (w - r, -h),
        (w, -h + r),
        (w, h - r),
        (w - r, h),
        (-w + r, h),
        (-w, h - r),
        (-w, -h + r),
    ]
    .map(|(x, y)| draw.point(x, y));
    let centers = [
        (w - r, -h + r),
        (w - r, h - r),
        (-w + r, h - r),
        (-w + r, -h + r),
    ]
    .map(|(x, y)| draw.point(x, y));
    let mut lines = [Id::ORIGIN; 4];
    let mut arcs = [Id::ORIGIN; 4];
    for k in 0..4 {
        lines[k] = draw.line(ends[2 * k], ends[2 * k + 1]);
        arcs[k] = draw.arc(centers[k], ends[2 * k + 1], ends[(2 * k + 2) % 8]);
    }
    for k in 0..4 {
        draw.tangent(lines[k], arcs[k]);
        draw.tangent(arcs[k], lines[(k + 1) % 4]);
    }
    draw.constrain(Constraint::Horizontal(lines[0]));
    draw.constrain(Constraint::Vertical(lines[1]));
    draw.constrain(Constraint::Horizontal(lines[2]));
    draw.constrain(Constraint::Vertical(lines[3]));
    for &arc in &arcs[1..] {
        draw.constrain(Constraint::Equal(arcs[0], arc));
    }
    // Bolt holes about the corners' centres.
    let holes = centers.map(|center| draw.circle(center, 3.25));
    for &hole in &holes[1..] {
        draw.constrain(Constraint::Equal(holes[0], hole));
    }
    draw.dimension(Measure::Radius(arcs[0]), "8", DVec2::new(6.0, -6.0));
    draw.dimension(Measure::Diameter(holes[1]), "6.5", DVec2::new(6.0, 6.0));
    draw.dimension(
        Measure::Length(lines[0]),
        "80 - 2 * 8",
        DVec2::new(0.0, -8.0),
    );
    draw.dimension(
        Measure::Length(lines[1]),
        "50 - 2 * 8",
        DVec2::new(8.0, 0.0),
    );
    // Centred on the origin.
    draw.dimension(
        Measure::HorizontalDistance(Id::ORIGIN, ends[2]),
        "40",
        DVec2::new(0.0, -12.0),
    );
    draw.dimension(
        Measure::VerticalDistance(Id::ORIGIN, ends[4]),
        "25",
        DVec2::new(-12.0, 0.0),
    );
    let sketch = draw.commit(&mut editor, outline, "Plate outline");
    let plate = region(&sketch, |region| region.holes.len() == 4);
    add_feature(
        &mut editor,
        "Plate",
        Extrude {
            taper: None,
            sketch: outline,
            regions: vec![plate],
            extent: Extent::OneSide(length("8")),
            flip: false,
            operation: Operation::NewBody(BodyId::NEW),
        }
        .into(),
    );

    let slot_sketch = add_sketch(&mut editor, "Slot", OriginPlane::XY);
    let mut draw = Draw::new(&editor);
    let (half, r) = (10.0, 5.0);
    let right = draw.point(half, 0.0);
    let left = draw.point(-half, 0.0);
    let ends = [(-half, -r), (half, -r), (half, r), (-half, r)].map(|(x, y)| draw.point(x, y));
    let bottom = draw.line(ends[0], ends[1]);
    let right_arc = draw.arc(right, ends[1], ends[2]);
    let top = draw.line(ends[2], ends[3]);
    let left_arc = draw.arc(left, ends[3], ends[0]);
    draw.tangent(bottom, right_arc);
    draw.tangent(right_arc, top);
    draw.tangent(top, left_arc);
    draw.tangent(left_arc, bottom);
    draw.constrain(Constraint::Horizontal(bottom));
    draw.constrain(Constraint::Equal(right_arc, left_arc));
    draw.constrain(Constraint::PointOnCurve {
        point: right,
        curve: Id::X_AXIS,
    });
    draw.dimension(Measure::Radius(right_arc), "5", DVec2::new(5.0, 5.0));
    draw.dimension(Measure::Distance(left, right), "20", DVec2::new(0.0, 8.0));
    draw.dimension(
        Measure::HorizontalDistance(Id::ORIGIN, right),
        "10",
        DVec2::new(0.0, -8.0),
    );
    let sketch = draw.commit(&mut editor, slot_sketch, "Slot");
    let slot = region(&sketch, |_| true);
    add_feature(
        &mut editor,
        "Slot cut",
        Extrude {
            taper: None,
            sketch: slot_sketch,
            regions: vec![slot],
            extent: Extent::ThroughAll,
            flip: false,
            operation: Operation::Cut(Targets::default()),
        }
        .into(),
    );
    editor.document().clone()
}

/// A knob turned about the Z axis: a disc with a rounded shoulder and a
/// stem, and a blind hole for its shaft up from the bottom.
fn knob() -> Document {
    let mut editor = Editor::new(Document::default());
    let profile_sketch = add_sketch(&mut editor, "Profile", OriginPlane::XZ);
    let mut draw = Draw::new(&editor);
    // Counter-clockwise from the axis at the bottom: the half section.
    let base = draw.point(0.0, 0.0);
    let rim_bottom = draw.point(18.0, 0.0);
    let rim_top = draw.point(18.0, 10.0);
    let center = draw.point(10.0, 10.0);
    let shoulder = draw.point(10.0, 18.0);
    let stem_bottom = draw.point(6.0, 18.0);
    let stem_top = draw.point(6.0, 30.0);
    let top = draw.point(0.0, 30.0);
    let floor = draw.line(base, rim_bottom);
    let rim = draw.line(rim_bottom, rim_top);
    let round = draw.arc(center, rim_top, shoulder);
    let ledge = draw.line(shoulder, stem_bottom);
    let stem = draw.line(stem_bottom, stem_top);
    let cap = draw.line(stem_top, top);
    let axis = draw.line(top, base);
    draw.constrain(Constraint::Coincident(base, Id::ORIGIN));
    draw.constrain(Constraint::PointOnCurve {
        point: top,
        curve: Id::Y_AXIS,
    });
    draw.constrain(Constraint::Horizontal(floor));
    draw.constrain(Constraint::Vertical(rim));
    draw.constrain(Constraint::Horizontal(ledge));
    draw.constrain(Constraint::Vertical(stem));
    draw.constrain(Constraint::Horizontal(cap));
    draw.tangent(rim, round);
    draw.tangent(round, ledge);
    draw.dimension(Measure::Length(floor), "36 / 2", DVec2::new(0.0, -6.0));
    draw.dimension(Measure::Length(rim), "10", DVec2::new(6.0, 0.0));
    draw.dimension(Measure::Radius(round), "8", DVec2::new(6.0, 6.0));
    draw.dimension(Measure::Length(cap), "6", DVec2::new(0.0, 6.0));
    draw.dimension(Measure::Length(axis), "30", DVec2::new(-6.0, 0.0));
    let sketch = draw.commit(&mut editor, profile_sketch, "Profile");
    let section = region(&sketch, |_| true);
    add_feature(
        &mut editor,
        "Turn",
        Revolve {
            sketch: profile_sketch,
            regions: vec![section],
            axis: AxisLine::SketchY,
            extent: Turn::Full,
            flip: false,
            operation: Operation::NewBody(BodyId::NEW),
        }
        .into(),
    );

    let bore_sketch = add_sketch(&mut editor, "Shaft hole", OriginPlane::XY);
    let mut draw = Draw::new(&editor);
    let center = draw.point(0.0, 0.0);
    let bore = draw.circle(center, 3.0);
    draw.constrain(Constraint::Coincident(center, Id::ORIGIN));
    draw.dimension(Measure::Diameter(bore), "6", DVec2::new(6.0, 6.0));
    let sketch = draw.commit(&mut editor, bore_sketch, "Shaft hole");
    let hole = region(&sketch, |_| true);
    add_feature(
        &mut editor,
        "Shaft bore",
        Extrude {
            taper: None,
            sketch: bore_sketch,
            regions: vec![hole],
            extent: Extent::OneSide(length("20")),
            flip: false,
            operation: Operation::Cut(Targets::default()),
        }
        .into(),
    );
    editor.document().clone()
}

/// A bearing pillow block: a base plate with rounded corners and bolt
/// holes, a revolved housing ring combined onto it, two ribs with
/// curved (spline) backs drawn on YZ and joined on, and a bore cut
/// through all.
fn pillow_block() -> Document {
    let mut editor = Editor::new(Document::default());

    // The base: as the mounting plate's outline, 100 x 50 mm.
    let base_sketch = add_sketch(&mut editor, "Base outline", OriginPlane::XY);
    let mut draw = Draw::new(&editor);
    let (w, h, r) = (50.0, 25.0, 8.0);
    let ends = [
        (-w + r, -h),
        (w - r, -h),
        (w, -h + r),
        (w, h - r),
        (w - r, h),
        (-w + r, h),
        (-w, h - r),
        (-w, -h + r),
    ]
    .map(|(x, y)| draw.point(x, y));
    let centers = [
        (w - r, -h + r),
        (w - r, h - r),
        (-w + r, h - r),
        (-w + r, -h + r),
    ]
    .map(|(x, y)| draw.point(x, y));
    let mut lines = [Id::ORIGIN; 4];
    let mut arcs = [Id::ORIGIN; 4];
    for k in 0..4 {
        lines[k] = draw.line(ends[2 * k], ends[2 * k + 1]);
        arcs[k] = draw.arc(centers[k], ends[2 * k + 1], ends[(2 * k + 2) % 8]);
    }
    for k in 0..4 {
        draw.tangent(lines[k], arcs[k]);
        draw.tangent(arcs[k], lines[(k + 1) % 4]);
    }
    draw.constrain(Constraint::Horizontal(lines[0]));
    draw.constrain(Constraint::Vertical(lines[1]));
    draw.constrain(Constraint::Horizontal(lines[2]));
    draw.constrain(Constraint::Vertical(lines[3]));
    for &arc in &arcs[1..] {
        draw.constrain(Constraint::Equal(arcs[0], arc));
    }
    let holes = centers.map(|center| draw.circle(center, 3.25));
    for &hole in &holes[1..] {
        draw.constrain(Constraint::Equal(holes[0], hole));
    }
    draw.dimension(Measure::Radius(arcs[0]), "8", DVec2::new(6.0, -6.0));
    draw.dimension(Measure::Diameter(holes[1]), "6.5", DVec2::new(6.0, 6.0));
    draw.dimension(
        Measure::Length(lines[0]),
        "100 - 2 * 8",
        DVec2::new(0.0, -8.0),
    );
    draw.dimension(
        Measure::Length(lines[1]),
        "50 - 2 * 8",
        DVec2::new(8.0, 0.0),
    );
    draw.dimension(
        Measure::HorizontalDistance(Id::ORIGIN, ends[2]),
        "50",
        DVec2::new(0.0, -12.0),
    );
    draw.dimension(
        Measure::VerticalDistance(Id::ORIGIN, ends[4]),
        "25",
        DVec2::new(-12.0, 0.0),
    );
    let sketch = draw.commit(&mut editor, base_sketch, "Base outline");
    let base = region(&sketch, |region| region.holes.len() == 4);
    add_feature(
        &mut editor,
        "Base",
        Extrude {
            taper: None,
            sketch: base_sketch,
            regions: vec![base],
            extent: Extent::OneSide(length("10")),
            flip: false,
            operation: Operation::NewBody(BodyId::NEW),
        }
        .into(),
    );

    // The housing: a ring's section on XZ, turned about Z.
    let ring_sketch = add_sketch(&mut editor, "Housing section", OriginPlane::XZ);
    let mut draw = Draw::new(&editor);
    let corners =
        [(10.0, 5.0), (20.0, 5.0), (20.0, 30.0), (10.0, 30.0)].map(|(x, y)| draw.point(x, y));
    let sides: Vec<Id> = (0..4)
        .map(|k| draw.line(corners[k], corners[(k + 1) % 4]))
        .collect();
    draw.constrain(Constraint::Horizontal(sides[0]));
    draw.constrain(Constraint::Vertical(sides[1]));
    draw.constrain(Constraint::Horizontal(sides[2]));
    draw.constrain(Constraint::Vertical(sides[3]));
    draw.dimension(
        Measure::HorizontalDistance(Id::ORIGIN, corners[0]),
        "10",
        DVec2::new(0.0, -6.0),
    );
    draw.dimension(
        Measure::VerticalDistance(Id::ORIGIN, corners[0]),
        "5",
        DVec2::new(-6.0, 0.0),
    );
    draw.dimension(Measure::Length(sides[0]), "10", DVec2::new(0.0, -6.0));
    draw.dimension(Measure::Length(sides[1]), "25", DVec2::new(6.0, 0.0));
    let sketch = draw.commit(&mut editor, ring_sketch, "Housing section");
    let section = region(&sketch, |_| true);
    add_feature(
        &mut editor,
        "Housing",
        Revolve {
            sketch: ring_sketch,
            regions: vec![section],
            axis: AxisLine::SketchY,
            extent: Turn::Full,
            flip: false,
            operation: Operation::NewBody(BodyId::NEW),
        }
        .into(),
    );
    let bodies = editor.document().bodies();
    let (base_body, housing_body) = (bodies[0].id, bodies[1].id);
    add_feature(
        &mut editor,
        "Housing onto base",
        Combine {
            target: base_body,
            tools: vec![housing_body],
            op: BodyOp::Union,
            keep_tools: false,
        }
        .into(),
    );

    // Two ribs on YZ, mirror images in the sketch's y axis (world Z),
    // each with a spline back from the base up to the housing.
    let rib_sketch = add_sketch(&mut editor, "Ribs", OriginPlane::YZ);
    let mut draw = Draw::new(&editor);
    let rib = [(19.0, 8.0), (45.0, 8.0), (28.0, 14.0), (19.0, 28.0)];
    let [foot, toe, bend, top] = rib.map(|(x, y)| draw.point(x, y));
    let mirrored = rib.map(|(x, y)| draw.point(-x, y));
    let floor = draw.line(foot, toe);
    draw.curve(Curve::Spline(Spline::through(vec![toe, bend, top], false)));
    let wall = draw.line(top, foot);
    let [m_foot, m_toe, m_bend, m_top] = mirrored;
    draw.line(m_toe, m_foot);
    draw.curve(Curve::Spline(Spline::through(
        vec![m_top, m_bend, m_toe],
        false,
    )));
    draw.line(m_foot, m_top);
    draw.constrain(Constraint::Horizontal(floor));
    draw.constrain(Constraint::Vertical(wall));
    for (a, b) in [foot, toe, bend, top].into_iter().zip(mirrored) {
        draw.constrain(Constraint::Symmetric {
            a,
            b,
            about: Id::Y_AXIS,
        });
    }
    draw.dimension(
        Measure::HorizontalDistance(Id::ORIGIN, foot),
        "19",
        DVec2::new(0.0, -6.0),
    );
    draw.dimension(
        Measure::VerticalDistance(Id::ORIGIN, foot),
        "8",
        DVec2::new(-6.0, 0.0),
    );
    draw.dimension(Measure::Length(floor), "26", DVec2::new(0.0, -6.0));
    draw.dimension(Measure::Length(wall), "20", DVec2::new(-6.0, 0.0));
    draw.dimension(
        Measure::HorizontalDistance(foot, bend),
        "9",
        DVec2::new(0.0, 4.0),
    );
    draw.dimension(
        Measure::VerticalDistance(foot, bend),
        "6",
        DVec2::new(4.0, 0.0),
    );
    let sketch = draw.commit(&mut editor, rib_sketch, "Ribs");
    let profiles = sketch.profiles().expect("a simple sketch");
    assert_eq!(profiles.regions.len(), 2, "two ribs");
    let ribs = (0..2)
        .map(|index| profiles.reference(index).expect("a region to name"))
        .collect();
    add_feature(
        &mut editor,
        "Ribs",
        Extrude {
            taper: None,
            sketch: rib_sketch,
            regions: ribs,
            extent: Extent::Symmetric(length("6")),
            flip: false,
            operation: Operation::Join(Targets::default()),
        }
        .into(),
    );

    // The bearing's bore, through the housing and the base.
    let bore_sketch = add_sketch(&mut editor, "Bore", OriginPlane::XY);
    let mut draw = Draw::new(&editor);
    let center = draw.point(0.0, 0.0);
    let bore = draw.circle(center, 8.0);
    draw.constrain(Constraint::Coincident(center, Id::ORIGIN));
    draw.dimension(Measure::Diameter(bore), "16", DVec2::new(8.0, 8.0));
    let sketch = draw.commit(&mut editor, bore_sketch, "Bore");
    let hole = region(&sketch, |_| true);
    add_feature(
        &mut editor,
        "Bore cut",
        Extrude {
            taper: None,
            sketch: bore_sketch,
            regions: vec![hole],
            extent: Extent::ThroughAll,
            flip: false,
            operation: Operation::Cut(Targets::default()),
        }
        .into(),
    );
    editor.document().clone()
}

/// A sample's file name, without the extension, and how it's made.
type Sample = (&'static str, fn() -> Document);

/// The GPU the thumbnails are rendered on, as the app's viewport has it.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
}

impl Gpu {
    /// A device with the limits iced asks for, or `None` without an
    /// adapter.
    fn new() -> Option<Gpu> {
        let instance = wgpu::Instance::default();
        let options = wgpu::RequestAdapterOptions::default();
        let adapter = pollster::block_on(instance.request_adapter(&options)).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_bind_groups: 2,
                ..wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
            },
            ..Default::default()
        }))
        .ok()?;
        let renderer = Renderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        Some(Gpu {
            device,
            queue,
            renderer,
        })
    }

    /// `document`'s thumbnail as the app renders it as it saves: the
    /// bodies of the model it regenerates to, each as opaque as it is.
    fn thumbnail(&self, document: &Document) -> Thumbnail {
        let editor = Editor::new(document.clone());
        let response = varde_regen::handle(varde_regen::Request::Regenerate {
            sight: None,
            generation: editor.generation(),
            document: editor.snapshot(),
            exclude: None,
            until: None,
            draft: None,
            inspect: None,
        });
        let varde_regen::Response::Regenerated {
            mesh,
            picking,
            sketches,
            ..
        } = response
        else {
            panic!("the design doesn't regenerate");
        };
        let opacity: Arc<[f32]> = (picking.bodies().iter())
            .map(|&body| document.body(body).map_or(1.0, |body| body.opacity.alpha()))
            .collect();
        let tints: Arc<[_]> = (picking.bodies().iter())
            .map(|&body| (document.body(body)?.color).map(varde_view::body_tint))
            .collect();
        let shot = thumbnail_shot(&mesh, &sketches, &Camera::default()).expect("a model to frame");
        let (send, read) = std::sync::mpsc::channel();
        varde_render::render_preview(
            &self.renderer,
            &self.device,
            &self.queue,
            &mesh,
            &sketches,
            &opacity,
            &tints,
            &shot,
            &ThumbnailRequest::COLORS,
            THUMBNAIL_SCALE as f32,
            move |images| {
                let _ = send.send(images);
            },
        )
        .expect("the thumbnail to render");
        let images = read
            .recv_timeout(Duration::from_secs(30))
            .expect("the thumbnail to be read back")
            .expect("the thumbnail's pixels");
        let [light, dark] = <[_; 2]>::try_from(images).expect("an image in each theme");
        let image = |image: varde_render::PreviewImage| {
            Image::new(image.width, image.height, image.rgba).expect("an image of its size")
        };
        Thumbnail {
            light: image(light),
            dark: image(dark),
        }
    }
}

fn main() {
    let directory = std::env::args_os().nth(1).map_or_else(
        || PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples")),
        PathBuf::from,
    );
    let Some(gpu) = Gpu::new() else {
        eprintln!("No GPU adapter: the thumbnails can't be rendered, so nothing was written.");
        std::process::exit(1);
    };
    let samples: [Sample; 3] = [
        ("mounting-plate", mounting_plate),
        ("knob", knob),
        ("pillow-block", pillow_block),
    ];
    for (name, make) in samples {
        let document = make();
        let mut cache = varde_regen::Cache::default();
        let evaluation = varde_regen::evaluate(&document, &mut cache);
        assert!(
            evaluation.failed.is_empty(),
            "{name} fails to regenerate: {:?}",
            evaluation.failed
        );
        assert!(!evaluation.bodies.is_empty(), "{name} makes no body");
        assert!(
            evaluation.uncut.iter().all(|(_, bodies)| bodies.is_empty()),
            "{name} has a cut that takes nothing"
        );
        let thumbnail = gpu.thumbnail(&document);
        let previews = varde_io::thumbnail::previews(Some(&thumbnail));
        assert_eq!(previews.len(), 2, "{name}'s thumbnail doesn't encode");
        let (bytes, _) = varde_io::vrdp::to_bytes(&document, &previews).expect("a design to write");
        let (read, _) = varde_io::vrdp::from_bytes(&bytes).expect("the design written");
        assert_eq!(read, document, "{name} reads back changed");
        assert_eq!(
            varde_io::thumbnail::of_file(&bytes).as_ref(),
            Some(&thumbnail),
            "{name}'s thumbnail reads back changed"
        );
        let path = directory.join(format!("{name}.vrdp"));
        std::fs::write(&path, &bytes).unwrap_or_else(|error| panic!("{path:?}: {error}"));
        println!(
            "{}: {} features, {} bodies, a {}×{} thumbnail, {} bytes",
            path.display(),
            document.features().len(),
            evaluation.bodies.len(),
            thumbnail.light.width(),
            thumbnail.light.height(),
            bytes.len()
        );
    }
}
