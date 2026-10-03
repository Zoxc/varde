//! The 3D viewport: an iced shader widget backed by [`varde_render`], and
//! in a sketch the sketch being edited, drawn by the renderer too, and the
//! input on it.

mod extrude;
mod measure;
mod motion;
mod pivot;
mod regions;
mod revolve;
mod sketch;

use std::any::Any;
use std::sync::{Arc, Weak};

use glam::DVec2;
use iced::widget::shader::{self, Action};
use iced::widget::{container, stack};
use iced::{Element, Event, Length, Point, Rectangle, keyboard, mouse};
use varde_kernel::{RenderLines, RenderMesh};
use varde_render::{
    Camera, ClipRect, Colors, ErrorParts, Frame, GridPlane, Pivot, PrepareError, Renderer, Shading,
    SketchLayer, SketchScene, Slot, wgpu,
};

use crate::anchors::Anchors;
use crate::chrome::{Hint, chord_hint, mouse_hint};
use crate::errors::ShownErrors;
use crate::icons::MouseButton;
use crate::operation_panel::placed;
use crate::pick::{ModelHighlight, Pick, PickIndex, Picked, Picks, Snapped};
use crate::shortcut::Held;
use crate::theme::{Palette, SketchColors};
use crate::thumbnail::{THUMBNAIL_SCALE, ThumbnailRequest};
use crate::{Edges, Edit, Look, Message, PlanePick, ViewOptions, controls};

pub(crate) use extrude::Extruding;
pub(crate) use measure::Measuring;
pub(crate) use motion::Moving;
pub(crate) use revolve::Revolving;
pub(crate) use sketch::Sketching;

/// The operation being set up in the viewport, if one is: never with a
/// sketch.
#[derive(Debug, Clone)]
pub(crate) enum Operating<'a> {
    Extrude(Extruding<'a>),
    Revolve(Revolving<'a>),
    /// The measure tool: not an operation, but drawn over the model as
    /// one is, while the cursor picks the model as outside the sessions.
    Measure(Measuring<'a>),
    /// A move or mirror: its axis or plane drawn over the model, while
    /// the cursor picks the model as outside the sessions.
    Motion(Moving<'a>),
}

/// How far below the viewport's top the camera controls are, in pixels.
pub(crate) const CONTROLS_TOP: f32 = 10.0;

const ORBIT_SPEED: f32 = 0.008;
const ZOOM_PER_LINE: f32 = 0.9;
/// Natively pixels come from touchpads, a few per event.
#[cfg(not(target_arch = "wasm32"))]
const ZOOM_PER_PIXEL: f32 = 0.995;
/// Browsers report a wheel notch in pixels too, 40 to 130 or more by
/// browser and settings (Firefox: 44 a line, 3 lines a notch), where
/// natively it's a line. So on the web a pixel zooms less, and one event
/// at most a line, as a native notch does; touchpads, a few pixels an
/// event, zoom slower than natively.
#[cfg(target_arch = "wasm32")]
const ZOOM_PER_PIXEL: f32 = 0.998;
/// How far the cursor may move, in pixels, between pressing and letting go
/// of the middle button for it to be a click, which picks the point the
/// camera orbits, rather than an orbit.
const CLICK_SLOP: f32 = 3.0;

/// Picking the model shown with the cursor, outside sketches and
/// sessions: the viewport says what's under the cursor as it moves, or as
/// the camera or the model changes under it ([`Look::Hover`]), and what a
/// left click is on ([`Look::ClickModel`]).
#[derive(Debug, Clone, Copy)]
pub struct ModelPicking<'a> {
    /// The model shown, ready for picking.
    pub index: &'a PickIndex,
    /// What the app holds hovered, of `index`'s model: the viewport says
    /// only when that changes.
    pub hovered: Option<Picked>,
    /// The snap point the app holds hovered with it, if any.
    pub hovered_snap: Option<Snapped>,
    /// What the cursor picks.
    pub picks: Picks,
    /// Whether the cursor snaps to the points of what it's over (the
    /// measure tool's), see [`PickIndex::snap`], and of what's held
    /// hovered while it's within reach of one of them, so it can leave a
    /// round edge to reach its centre.
    pub snaps: bool,
    /// What a plane is being picked for, if one is: a click on a face
    /// that can take the sketch picks it ([`Edit::FacePicked`]) rather
    /// than selecting, and a click elsewhere does nothing.
    pub planes: Option<&'a PlanePick>,
}

impl ModelPicking<'_> {
    /// Whether `target`, hovered, is what a click acts on: anything
    /// picked, or while picking a plane only a face that can take the
    /// sketch ([`PlanePick::takes`]).
    pub fn takes(&self, target: Picked) -> bool {
        match (self.planes, target) {
            (None, _) => true,
            (Some(pick), Picked::Face(face)) => pick.takes(self.index, face),
            (Some(_), _) => false,
        }
    }
}

/// The 3D viewport showing `mesh` and the finished `sketches` from
/// `camera`, with the controls over its top-right corner, and in a sketch
/// the sketch being edited, with the layer of widgets anchored to it, or
/// setting up an operation, an extrude's regions and handle or a
/// revolve's regions and axis, and `panel`, floating
/// over the viewport's right under the controls, and the tool `rail`
/// over its left. `pivot`, the point the camera orbits if one was picked,
/// is marked, and `highlight` and the failures' `errors` drawn over the
/// model. With `picking`, the cursor picks the model, drawn as the view
/// `options` say: the edges the model hides dashed if asked for, outside
/// a sketch, every patch's edges and every triangle's faint if asked
/// for, and lit with their shading. Each of the mesh's parts is drawn as
/// opaque as `opacity` says, see [`Frame::opacity`].
#[expect(clippy::too_many_arguments)]
pub(crate) fn viewport<'a>(
    mesh: &Arc<RenderMesh>,
    opacity: Arc<[f32]>,
    sketches: &Arc<RenderLines>,
    camera: &'a Camera,
    pivot: Option<Pivot>,
    picking: Option<ModelPicking<'a>>,
    highlight: Option<&Arc<ModelHighlight>>,
    errors: &Arc<ShownErrors>,
    options: ViewOptions,
    palette: &Palette,
    sketching: Option<Sketching<'a>>,
    operating: Option<Operating<'a>>,
    panel: Option<Element<'a, Message>>,
    rail: Element<'a, Message>,
    thumbnail: Option<&Arc<ThumbnailRequest>>,
) -> Element<'a, Message> {
    // Constraint glyphs, nudged apart; dimensions' labels, where they're
    // put; the value field, in a layer of its own so its state stays its
    // own as labels come and go; the glyph of where a drawing tool snaps,
    // by the cursor; and the drawing tool's fields, beside it.
    let anchors = sketching.as_ref().map(|sketching| {
        let placement = sketching.placement();
        let glyphs = Anchors::new(*camera, placement, sketching.glyphs());
        let labels = Anchors::new(*camera, placement, sketching.labels());
        let field = Anchors::new(*camera, placement, sketching.field());
        let snap = Anchors::new(*camera, placement, sketching.snap_glyph());
        let fields = Anchors::new(*camera, placement, sketching.fields());
        [
            Element::from(glyphs.nudged(sketch::GLYPH_OFFSET)),
            labels.into(),
            field.into(),
            snap.nudged(sketch::SNAP_OFFSET).into(),
            fields.beside(sketch::FIELDS_OFFSET).into(),
        ]
    });
    // An extrude's handle's knobs, on its axis, those the model doesn't
    // hide. A layer even without them (and for a revolve, which has
    // none, and a move, whose handles the renderer draws), so the
    // panel's layer above keeps its place in the stack, and with it its
    // widgets' state (the field's focus, the body's scroll), as the last
    // region is unpicked or the first picked.
    let knobs = operating.as_ref().map(|operating| {
        let knobs = match operating {
            Operating::Extrude(extruding) => extruding.knobs(camera, mesh, &opacity),
            Operating::Revolve(_) | Operating::Motion(_) => None,
            // The distance's label, in the knobs' place.
            Operating::Measure(measuring) => measuring.label(camera),
        };
        knobs.unwrap_or_else(|| iced::widget::Space::new().into())
    });
    let mut program = Program {
        picking,
        highlight: highlight.cloned().unwrap_or_else(|| NO_HIGHLIGHT.clone()),
        ..program(mesh, sketches, camera, pivot, palette, sketching, operating)
    };
    program.scene.hidden_edges = options.hidden_edges;
    program.scene.wireframe = options.edges == Edges::Wireframe;
    program.scene.tessellation = options.edges == Edges::Tessellation;
    program.scene.shading = options.shading;
    program.scene.opacity = opacity;
    program.scene.thumbnail = thumbnail.cloned();
    program.scene.errors = errors.clone();
    let scene = iced::widget::shader(program)
        .width(Length::Fill)
        .height(Length::Fill);
    let controls = container(controls::view_controls(camera))
        .align_right(Length::Fill)
        .padding(iced::Padding::from([CONTROLS_TOP, 12.0]));
    // The layers over the scene take only what's over their widgets, and
    // let the rest through to it.
    // Clear of the viewport's bottom too: the panel's body scrolls rather
    // than run past it.
    let panel = panel.map(placed);
    stack![scene]
        .extend(anchors.into_iter().flatten())
        .extend(knobs)
        .push(rail)
        .push(controls)
        .extend(panel)
        .into()
}

/// The shader program drawing `mesh` and `sketches` from `camera` in
/// `palette`'s colors, in `sketching`'s sketch if there is one, or
/// setting up `operating`'s operation, with the edges the model hides,
/// every part opaque.
fn program<'a>(
    mesh: &Arc<RenderMesh>,
    sketches: &Arc<RenderLines>,
    camera: &Camera,
    pivot: Option<Pivot>,
    palette: &Palette,
    sketching: Option<Sketching<'a>>,
    operating: Option<Operating<'a>>,
) -> Program<'a> {
    Program {
        scene: Scene {
            mesh: mesh.clone(),
            opacity: Arc::new([]),
            sketches: sketches.clone(),
            camera: *camera,
            pivot,
            colors: palette.scene,
            sketch_plane: sketching.as_ref().map(Sketching::grid),
            hidden_edges: true,
            wireframe: false,
            tessellation: false,
            shading: Shading::Regular,
            thumbnail: None,
            errors: NO_ERRORS.clone(),
        },
        sketching,
        operating,
        picking: None,
        highlight: NO_HIGHLIGHT.clone(),
        sketch_colors: palette.sketching,
    }
}

/// What's drawn over the model while nothing is: one for all frames, so
/// the renderer uploads nothing again for it.
static NO_HIGHLIGHT: std::sync::LazyLock<Arc<ModelHighlight>> =
    std::sync::LazyLock::new(Arc::default);

/// The patches of a sketch's failing curves: none.
static NO_MESH: std::sync::LazyLock<RenderMesh> = std::sync::LazyLock::new(RenderMesh::default);

/// What's drawn of failures while none show: one for all frames.
static NO_ERRORS: std::sync::LazyLock<Arc<ShownErrors>> = std::sync::LazyLock::new(Arc::default);

/// The viewport's shader program: handles input and hands iced the scene it
/// was built with to draw.
struct Program<'a> {
    scene: Scene,
    /// The sketch being edited, if one is.
    sketching: Option<Sketching<'a>>,
    /// The operation being set up, if one is: never with a sketch.
    operating: Option<Operating<'a>>,
    /// Picking the model, if the cursor does.
    picking: Option<ModelPicking<'a>>,
    /// Drawn over the model.
    highlight: Arc<ModelHighlight>,
    sketch_colors: SketchColors,
}

/// What one frame of the viewport draws but the sketch being edited.
#[derive(Debug, Clone)]
struct Scene {
    mesh: Arc<RenderMesh>,
    /// How opaque each of the mesh's parts is: see [`Frame::opacity`].
    opacity: Arc<[f32]>,
    sketches: Arc<RenderLines>,
    camera: Camera,
    /// The point the camera orbits, marked, if one was picked.
    pivot: Option<Pivot>,
    colors: Colors,
    /// The plane of the sketch being edited, if one is.
    sketch_plane: Option<GridPlane>,
    /// Whether the edges the model hides are drawn, dashed (the renderer
    /// ignores it in a sketch).
    hidden_edges: bool,
    /// Whether the mesh's wires are drawn, every patch's edges.
    wireframe: bool,
    /// Whether the edges of the mesh's triangles are drawn.
    tessellation: bool,
    /// How the faces are lit.
    shading: Shading,
    /// A thumbnail to render offscreen beside the frame, if one is asked
    /// for: once, by the first frame prepared with it.
    thumbnail: Option<Arc<ThumbnailRequest>>,
    /// The failures' geometry drawn over the model.
    errors: Arc<ShownErrors>,
}

/// What dragging in the viewport does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Orbit,
    Pan,
}

impl DragKind {
    /// What dragging with `button` while `modifiers` are held does, if
    /// anything, in a sketch if `sketching`. The middle button, and the
    /// right one with [`Held::ORBIT`], orbit, the right one pans, and the
    /// left one orbits too outside a sketch; in a sketch it's for selecting
    /// and editing geometry instead. Keep [`hints`] and `README.md` in step.
    fn for_button(
        button: mouse::Button,
        modifiers: keyboard::Modifiers,
        sketching: bool,
    ) -> Option<Self> {
        match button {
            mouse::Button::Middle => Some(DragKind::Orbit),
            mouse::Button::Right if Held::ORBIT.is_held(modifiers) => Some(DragKind::Orbit),
            mouse::Button::Right => Some(DragKind::Pan),
            mouse::Button::Left if !sketching => Some(DragKind::Orbit),
            _ => None,
        }
    }
}

/// The status bar hints for the viewport's mouse bindings, in a sketch if
/// `sketching`, see [`DragKind::for_button`], and the middle click
/// picking the point to orbit. Orbiting with the middle button isn't
/// hinted: it's the wheel's icon, which zooms.
pub fn hints<'a>(sketching: bool) -> [Hint<'a>; 4] {
    let orbit = if sketching {
        chord_hint(Held::ORBIT, MouseButton::Right, "Orbit")
    } else {
        mouse_hint(MouseButton::Left, "Drag to orbit")
    };
    [
        orbit,
        mouse_hint(MouseButton::Right, "Pan"),
        mouse_hint(MouseButton::Wheel, "Zoom"),
        mouse_hint(MouseButton::Wheel, "Click to set pivot"),
    ]
}

#[derive(Default)]
struct Interaction {
    drag: Option<(DragKind, Point)>,
    /// Where the middle button, or the left one while the cursor picks the
    /// model, was pressed, while the cursor hasn't moved past
    /// [`CLICK_SLOP`] from there: letting go then is a click, which picks
    /// the point the camera orbits, or (the left one) selects, and until
    /// then it doesn't orbit.
    click: Option<Point>,
    /// The last left click on the model, when and where, to tell a
    /// double-click by.
    last_click: Option<(iced::time::Instant, DVec2)>,
    /// The camera, the model and the cursor position the cursor's pick
    /// was last worked out for: it's worked out again as any changes.
    hover_seen: Option<(Camera, u64, Point)>,
    /// The modifiers held, which change what a drag does, and `Ctrl`
    /// (`Cmd`) adds to a sketch's selection.
    modifiers: keyboard::Modifiers,
    /// What's kept of the sketch being edited.
    sketch: sketch::Input,
    /// What's kept of the extrude being set up.
    extrude: extrude::Input,
    /// What's kept of the revolve being set up.
    revolve: revolve::Input,
    /// What's kept of the move being set up: its handles.
    motion: motion::Input,
    /// Names this widget's [`Slot`] in the [`Pipeline`], for as long as the
    /// widget lives.
    slot: Arc<SlotKey>,
}

/// Only its address matters: see [`Interaction::slot`].
#[derive(Debug, Default)]
struct SlotKey;

impl shader::Program<Message> for Program<'_> {
    type State = Interaction;
    type Primitive = Primitive;

    fn update(
        &self,
        state: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            state.modifiers = *modifiers;
            let sketching = self.sketching.as_ref()?;
            return sketching.modifiers_changed(
                &mut state.sketch,
                bounds,
                cursor,
                &self.scene.camera,
                *modifiers,
            );
        }
        // The operation's picking and handle come first, unless the
        // camera is being dragged; the rest goes on as outside a sketch.
        if let Some(operating) = &self.operating
            && state.drag.is_none()
            && let Event::Mouse(event) = event
        {
            let camera = &self.scene.camera;
            let action = match operating {
                Operating::Extrude(extruding) => {
                    extruding.mouse(&mut state.extrude, *event, bounds, cursor, camera)
                }
                Operating::Revolve(revolving) => {
                    revolving.mouse(&mut state.revolve, *event, bounds, cursor, camera)
                }
                // A move's handles, ahead of picking the model.
                Operating::Motion(moving) => {
                    let hovered = self
                        .picking
                        .is_some_and(|picking| picking.hovered.is_some());
                    moving.mouse(&mut state.motion, *event, bounds, cursor, camera, hovered)
                }
                // The cursor picks the model as outside the sessions.
                Operating::Measure(_) => None,
            };
            if action.is_some() {
                return action;
            }
        }
        // Nothing's picked under a move's handles.
        let handled = matches!(self.operating, Some(Operating::Motion(_))) && state.motion.holds();
        if let Some(picking) = &self.picking
            && state.drag.is_none()
            && !handled
            && let Some(action) = self.hover(state, picking, event, bounds, cursor)
        {
            return Some(action);
        }
        // While the camera's dragged (past a click), nothing's hovered:
        // what was moves away from the cursor. It's worked out again once
        // the drag ends.
        if let Some(picking) = &self.picking
            && state.drag.is_some()
            && state.click.is_none()
            && let Event::Window(iced::window::Event::RedrawRequested(_)) = event
        {
            state.hover_seen = None;
            if picking.hovered.is_some() {
                return Some(Action::publish(Message::Look(Look::Hover(None))));
            }
        }
        let camera = match event {
            // The left button is the sketch's in a sketch, and moving the
            // cursor while the camera isn't dragged.
            Event::Mouse(
                mouse::Event::ButtonPressed(mouse::Button::Left)
                | mouse::Event::ButtonReleased(mouse::Button::Left),
            ) => self.sketching.is_none(),
            Event::Mouse(mouse::Event::CursorMoved { .. }) => state.drag.is_some(),
            Event::Mouse(_) => true,
            _ => false,
        };
        if camera {
            let Event::Mouse(event) = event else {
                return None;
            };
            return self.camera(state, *event, bounds, cursor);
        }
        let sketching = self.sketching.as_ref()?;
        let camera = &self.scene.camera;
        sketching.update(
            &mut state.sketch,
            event,
            bounds,
            cursor,
            camera,
            state.modifiers,
        )
    }

    fn draw(&self, state: &Interaction, _cursor: mouse::Cursor, bounds: Rectangle) -> Primitive {
        // The model isn't faded behind an operation, and hides what's
        // behind it of its regions, handle and axis.
        let operation = self.operating.as_ref().map(|operating| {
            let colors = self.sketch_colors;
            let (plane, (base, live)) = match operating {
                Operating::Extrude(extruding) => (
                    extruding.plane(),
                    extruding.layers(&state.extrude, colors, &self.scene.camera, bounds),
                ),
                Operating::Revolve(revolving) => (
                    revolving.plane(),
                    revolving.layers(&state.revolve, colors, &self.scene.camera, bounds),
                ),
                Operating::Measure(measuring) => {
                    (GridPlane::XY, measuring.layers(&self.scene.colors, colors))
                }
                Operating::Motion(moving) => (
                    moving.plane_of_layers(),
                    moving.layers(
                        &state.motion,
                        &self.scene.colors,
                        colors,
                        &self.scene.camera,
                        bounds,
                    ),
                ),
            };
            // What the measure tool draws is on top: a distance through
            // the model, or a point behind it, still shows; and a move's
            // axis or a mirror's plane through the bodies.
            let depth_tested = !matches!(operating, Operating::Measure(_) | Operating::Motion(_));
            SketchFrame {
                plane,
                depth_tested,
                base,
                live,
                failing: None,
            }
        });
        let sketch = self.sketching.as_ref().map(|sketching| {
            let camera = &self.scene.camera;
            let (base, live) = sketching.layers(
                &state.sketch,
                camera,
                bounds,
                self.sketch_colors,
                state.modifiers,
            );
            SketchFrame {
                plane: sketching.grid(),
                depth_tested: false,
                base,
                live,
                failing: sketching.failing(&state.sketch),
            }
        });
        Primitive {
            scene: self.scene.clone(),
            sketch: sketch.or(operation),
            highlight: self.highlight.clone(),
            slot: state.slot.clone(),
        }
    }

    fn mouse_interaction(
        &self,
        state: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match state.drag {
            Some((DragKind::Orbit, _)) => mouse::Interaction::Grabbing,
            Some((DragKind::Pan, _)) => mouse::Interaction::Move,
            None => self
                .sketching
                .as_ref()
                .and_then(|sketching| sketching.mouse_interaction(&state.sketch, bounds, cursor))
                .or_else(|| match self.operating.as_ref()? {
                    Operating::Extrude(extruding) => {
                        extruding.mouse_interaction(&state.extrude, bounds, cursor)
                    }
                    Operating::Revolve(revolving) => {
                        revolving.mouse_interaction(&state.revolve, bounds, cursor)
                    }
                    Operating::Motion(moving) => moving.mouse_interaction(&state.motion),
                    Operating::Measure(_) => None,
                })
                .or_else(|| {
                    // Over what a click would select.
                    let picking = self.picking.as_ref()?;
                    cursor.position_over(bounds)?;
                    (picking.hovered)
                        .filter(|&target| picking.takes(target))
                        .map(|_| mouse::Interaction::Pointer)
                })
                .unwrap_or_default(),
        }
    }
}

impl Program<'_> {
    /// Says what the cursor is over in the model as it moves, and as the
    /// camera or the model changes under it (looked at as a frame is
    /// drawn), if that's another face or edge than `picking`'s hovered,
    /// and nothing once it leaves.
    fn hover(
        &self,
        state: &mut Interaction,
        picking: &ModelPicking<'_>,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let pick = match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => self.pick_at(picking, bounds, cursor),
            Event::Window(iced::window::Event::RedrawRequested(_)) => {
                let at = cursor.position_over(bounds)?;
                let seen = (self.scene.camera, picking.index.model(), at);
                if state.hover_seen == Some(seen) {
                    return None;
                }
                state.hover_seen = Some(seen);
                self.pick_at(picking, bounds, cursor)
            }
            Event::Mouse(mouse::Event::CursorLeft) => None,
            _ => return None,
        };
        if let Some(at) = cursor.position_over(bounds) {
            state.hover_seen = Some((self.scene.camera, picking.index.model(), at));
        }
        let seen = |pick: Option<Pick>| pick.map(|pick| (pick.target, pick.snap));
        let held = picking.hovered.map(|target| (target, picking.hovered_snap));
        (seen(pick) != held).then(|| Action::publish(Message::Look(Look::Hover(pick))))
    }

    /// What of the model the cursor is over, if it's over the viewport.
    fn pick_at(
        &self,
        picking: &ModelPicking<'_>,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Pick> {
        let at = cursor.position_over(bounds)?;
        self.pick_point(picking, bounds, at)
    }

    /// What of the model shows at `at`, in the window's pixels.
    fn pick_point(&self, picking: &ModelPicking<'_>, bounds: Rectangle, at: Point) -> Option<Pick> {
        let at = DVec2::new((at.x - bounds.x).into(), (at.y - bounds.y).into());
        let size = [bounds.width, bounds.height];
        let camera = &self.scene.camera;
        let index = picking.index;
        let pick = index.pick(camera, size, at, picking.picks);
        if !picking.snaps {
            return pick;
        }
        let snap = |target| index.snap(camera, size, at, target);
        if let Some(mut pick) = pick
            && let Some((snapped, _)) = snap(pick.target)
        {
            pick.snap = Some(snapped);
            return Some(pick);
        }
        // The dots of what's held hovered stay to be taken, though the
        // cursor has left it to reach one: a round edge's centre is off
        // the edge, often over nothing.
        if let Some(held) = picking.hovered
            && let Some((snapped, point)) = snap(held)
        {
            return Some(Pick {
                model: index.model(),
                target: held,
                body: index.body(held)?,
                at: point,
                snap: Some(snapped),
            });
        }
        pick
    }

    /// Whether a sketch is being edited, where the left button is for its
    /// geometry.
    fn sketching(&self) -> bool {
        self.sketching.is_some()
    }

    /// Takes the mouse `event` as a move of the camera: orbiting, panning
    /// and zooming.
    fn camera(
        &self,
        state: &mut Interaction,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            mouse::Event::ButtonPressed(button) => {
                let position = cursor.position_over(bounds)?;
                let kind = DragKind::for_button(button, state.modifiers, self.sketching())?;
                state.drag = Some((kind, position));
                let clicks = match button {
                    mouse::Button::Middle => true,
                    mouse::Button::Left => self.picking.is_some(),
                    _ => false,
                };
                state.click = clicks.then_some(position);
                Some(Action::capture())
            }
            mouse::Event::ButtonReleased(button) => {
                state.drag.take()?;
                // A left click on the model selects.
                if button == mouse::Button::Left
                    && let Some(picking) = &self.picking
                {
                    let Some(at) = state.click.take() else {
                        return Some(Action::capture());
                    };
                    let pick = self.pick_point(picking, bounds, at);
                    if let Some(planes) = picking.planes {
                        // A face that can take the sketch is picked;
                        // anything else nothing.
                        let face = pick.and_then(|pick| match pick.target {
                            Picked::Face(face) if planes.takes(picking.index, face) => {
                                planes.face_ref(picking.index, face, pick.at)
                            }
                            _ => None,
                        });
                        return Some(match face {
                            Some(face) => {
                                Action::publish(Message::Edit(Edit::FacePicked(face))).and_capture()
                            }
                            None => Action::capture(),
                        });
                    }
                    let local = DVec2::new((at.x - bounds.x).into(), (at.y - bounds.y).into());
                    let double = sketch::double_click(&mut state.last_click, local);
                    let add = Held::TOGGLE.is_held(state.modifiers);
                    let message = Look::ClickModel { pick, add, double };
                    return Some(Action::publish(Message::Look(message)).and_capture());
                }
                let click = state
                    .click
                    .take()
                    .filter(|_| button == mouse::Button::Middle);
                let Some(at) = click else {
                    return Some(Action::capture());
                };
                let at = DVec2::new((at.x - bounds.x).into(), (at.y - bounds.y).into());
                let pivot = pivot::pick(
                    &self.scene.mesh,
                    &self.scene.camera,
                    [bounds.width, bounds.height],
                    at,
                    self.sketching.as_ref().map(Sketching::placement),
                );
                Some(Action::publish(Message::Look(Look::SetPivot(pivot))).and_capture())
            }
            mouse::Event::CursorMoved { position } => {
                // A middle click until the cursor moves far enough.
                if let Some(at) = state.click {
                    if at.distance(position) < CLICK_SLOP {
                        return Some(Action::capture());
                    }
                    state.click = None;
                }
                // Uses the raw position so drags continue over UI panels.
                let (kind, last) = state.drag.as_mut()?;
                let delta = position - *last;
                *last = position;

                let message = match kind {
                    DragKind::Orbit => Message::Look(Look::Orbit {
                        yaw: -delta.x * ORBIT_SPEED,
                        pitch: delta.y * ORBIT_SPEED,
                    }),
                    // A collapsed viewport has no height to scale the pan by.
                    DragKind::Pan if bounds.height < 1.0 => return None,
                    DragKind::Pan => Message::Look(Look::Pan {
                        dx: delta.x / bounds.height,
                        dy: delta.y / bounds.height,
                    }),
                };
                Some(Action::publish(message).and_capture())
            }
            mouse::Event::WheelScrolled { delta } => {
                let position = cursor.position_over(bounds)?;
                // A collapsed viewport has no height to place the cursor by.
                if bounds.height < 1.0 {
                    return None;
                }
                let factor = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => ZOOM_PER_LINE.powf(y),
                    mouse::ScrollDelta::Pixels { y, .. } => ZOOM_PER_PIXEL.powf(y),
                };
                #[cfg(target_arch = "wasm32")]
                let factor = factor.clamp(ZOOM_PER_LINE, ZOOM_PER_LINE.recip());
                let center = bounds.center();
                Some(
                    Action::publish(Message::Look(Look::Zoom {
                        factor,
                        x: (position.x - center.x) / bounds.height,
                        y: (position.y - center.y) / bounds.height,
                    }))
                    .and_capture(),
                )
            }
            _ => None,
        }
    }
}

/// A frame's scene and the widget that draws it.
#[derive(Debug, Clone)]
struct Primitive {
    scene: Scene,
    /// The sketch being edited, if one is.
    sketch: Option<SketchFrame>,
    /// Drawn over the model.
    highlight: Arc<ModelHighlight>,
    /// The widget's key to its slot in the [`Pipeline`].
    slot: Arc<SlotKey>,
}

/// What a frame draws of the sketch being edited: see [`SketchScene`].
#[derive(Debug, Clone)]
struct SketchFrame {
    plane: GridPlane,
    depth_tested: bool,
    base: Arc<SketchLayer>,
    live: SketchLayer,
    /// The sketch's curves a failing feature names, placed: drawn with
    /// the failures' geometry ([`Frame::errors`]) as their halo alone,
    /// around the sketch's own red curves.
    failing: Option<Arc<RenderLines>>,
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &shader::Viewport,
    ) {
        let scale = viewport.scale_factor();
        let target = viewport.physical_size();

        let scene = &self.scene;
        // Borrowed from the geometry, so made each frame (a few small
        // structs); the renderer uploads them again only when their
        // sources change. The sketch's failing curves only as their halo:
        // the sketch draws them, red, at its own width.
        let mut errors = scene.errors.parts();
        let failing = (self.sketch.as_ref()).and_then(|sketch| sketch.failing.as_ref());
        if let Some(lines) = failing {
            let erased: Arc<dyn Any + Send + Sync> = lines.clone();
            errors.push(ErrorParts {
                mesh: &NO_MESH,
                lines,
                points: &[],
                source: Arc::downgrade(&erased),
                halo_only: true,
            });
        }
        let prepared = pipeline.prepare(
            &self.slot,
            device,
            queue,
            &Frame {
                camera: &scene.camera,
                mesh: &scene.mesh,
                opacity: &scene.opacity,
                sketches: &scene.sketches,
                // A sketch being edited moves the grid onto its plane and
                // fades the model.
                grid: scene.sketch_plane.unwrap_or(GridPlane::XY),
                faded: scene.sketch_plane.is_some(),
                hidden_edges: scene.hidden_edges,
                wireframe: scene.wireframe,
                tessellation: scene.tessellation,
                shading: scene.shading,
                pivot: scene.pivot,
                hovered_faces: &self.highlight.hovered_faces,
                selected_faces: &self.highlight.selected_faces,
                second_faces: &self.highlight.second_faces,
                highlights: &self.highlight.highlights,
                errors: &errors,
                sketch: self.sketch.as_ref().map(|sketch| SketchScene {
                    plane: sketch.plane,
                    depth_tested: sketch.depth_tested,
                    base: &sketch.base,
                    live: &sketch.live,
                }),
                viewport: varde_render::Viewport {
                    x: bounds.x * scale,
                    y: bounds.y * scale,
                    width: bounds.width * scale,
                    height: bounds.height * scale,
                },
                target_size: [target.width, target.height],
                scale_factor: scale,
                colors: scene.colors,
            },
        );
        // `prepare` can't send a message, so this can't reach the UI.
        if let Err(error) = prepared {
            log::error!("Couldn't draw the design: {error}");
        }
        // Its own slot and target, the colours the model is drawn in; on
        // failure, what waits for it is dropped, so it saves without.
        if let Some(thumbnail) = &scene.thumbnail
            && let Some(done) = thumbnail.take()
            && let Err(error) = varde_render::render_preview(
                &pipeline.renderer,
                device,
                queue,
                &thumbnail.mesh,
                &thumbnail.opacity,
                &thumbnail.shot,
                scene.colors,
                THUMBNAIL_SCALE as f32,
                done,
            )
        {
            log::error!("Couldn't draw the thumbnail: {error}");
        }
    }

    fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        pipeline.render(
            &self.slot,
            encoder,
            target,
            ClipRect {
                x: clip_bounds.x,
                y: clip_bounds.y,
                width: clip_bounds.width,
                height: clip_bounds.height,
            },
        );
    }
}

/// iced keeps one pipeline for every viewport widget in every window, and
/// prepares all of a frame's primitives before rendering any, so each
/// widget draws from its own [`Slot`].
struct Pipeline {
    renderer: Renderer,
    /// Each widget's slot, by its [`Interaction::slot`].
    slots: Vec<Keyed>,
}

/// A widget's [`Slot`] and its key.
struct Keyed {
    /// Holding the key keeps its address from being reused for another
    /// widget's.
    key: Weak<SlotKey>,
    slot: Slot,
}

impl Keyed {
    fn is(&self, key: &Arc<SlotKey>) -> bool {
        self.key.as_ptr() == Arc::as_ptr(key)
    }
}

impl Pipeline {
    /// Prepares `frame` into the slot for `key`, creating it on first use.
    fn prepare(
        &mut self,
        key: &Arc<SlotKey>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) -> Result<(), PrepareError> {
        let Self { renderer, slots } = self;
        let i = match slots.iter().position(|keyed| keyed.is(key)) {
            Some(i) => i,
            None => {
                slots.push(Keyed {
                    key: Arc::downgrade(key),
                    slot: renderer.slot(device),
                });
                slots.len() - 1
            }
        };
        renderer.prepare(&mut slots[i].slot, device, queue, frame)
    }

    /// Renders the slot for `key`, if it was prepared.
    fn render(
        &self,
        key: &Arc<SlotKey>,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: ClipRect,
    ) {
        if let Some(keyed) = self.slots.iter().find(|keyed| keyed.is(key)) {
            self.renderer.render(&keyed.slot, encoder, target, clip);
        }
    }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Pipeline {
            renderer: Renderer::new(device, format),
            slots: Vec::new(),
        }
    }

    /// Drops the slots of widgets that are gone.
    fn trim(&mut self) {
        self.slots.retain(|keyed| keyed.key.strong_count() > 0);
    }
}

#[cfg(test)]
mod tests;
