//! The 3D viewport: an iced shader widget backed by [`varde_render`], and
//! in a sketch the sketch being edited, drawn by the renderer too, and the
//! input on it.

mod extrude;
mod pivot;
mod sketch;

use std::sync::{Arc, Weak};

use glam::DVec2;
use iced::widget::shader::{self, Action};
use iced::widget::{container, stack};
use iced::{Element, Event, Length, Point, Rectangle, keyboard, mouse};
use varde_kernel::{RenderLines, RenderMesh};
use varde_render::{
    Camera, ClipRect, Colors, Frame, GridPlane, Pivot, PrepareError, Renderer, SketchLayer,
    SketchScene, Slot, wgpu,
};

use crate::anchors::Anchors;
use crate::chrome::{Hint, chord_hint, mouse_hint};
use crate::icons::MouseButton;
use crate::operation_panel::placed;
use crate::shortcut::Held;
use crate::theme::{Palette, SketchColors};
use crate::{Look, Message, controls};

pub(crate) use extrude::Extruding;
pub(crate) use sketch::Sketching;

/// How far below the viewport's top the camera controls are, in pixels.
pub(crate) const CONTROLS_TOP: f32 = 10.0;

const ORBIT_SPEED: f32 = 0.008;
const ZOOM_PER_LINE: f32 = 0.9;
const ZOOM_PER_PIXEL: f32 = 0.995;
/// How far the cursor may move, in pixels, between pressing and letting go
/// of the middle button for it to be a click, which picks the point the
/// camera orbits, rather than an orbit.
const CLICK_SLOP: f32 = 3.0;

/// The 3D viewport showing `mesh` and the finished `sketches` from
/// `camera`, with the controls over its top-right corner, and in a sketch
/// the sketch being edited, with the layer of widgets anchored to it, or
/// setting up an extrude, its regions and handle, and `panel`, floating
/// over the viewport's right under the controls, and the tool `rail`
/// over its left. `pivot`, the point the camera orbits if one was picked,
/// is marked.
#[expect(clippy::too_many_arguments)]
pub(crate) fn viewport<'a>(
    mesh: &Arc<RenderMesh>,
    sketches: &Arc<RenderLines>,
    camera: &'a Camera,
    pivot: Option<Pivot>,
    palette: &Palette,
    sketching: Option<Sketching<'a>>,
    extruding: Option<Extruding<'a>>,
    panel: Option<Element<'a, Message>>,
    rail: Element<'a, Message>,
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
    // The handle's knobs, on its axis, those the model doesn't hide. A
    // layer even without them, so the panel's layer above keeps its place
    // in the stack, and with it its widgets' state (the field's focus, the
    // body's scroll), as the last region is unpicked or the first picked.
    let knobs = extruding.as_ref().map(|extruding| {
        extruding
            .knobs(camera, mesh)
            .unwrap_or_else(|| iced::widget::Space::new().into())
    });
    let program = program(mesh, sketches, camera, pivot, palette, sketching, extruding);
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
/// setting up `extruding`'s extrude.
fn program<'a>(
    mesh: &Arc<RenderMesh>,
    sketches: &Arc<RenderLines>,
    camera: &Camera,
    pivot: Option<Pivot>,
    palette: &Palette,
    sketching: Option<Sketching<'a>>,
    extruding: Option<Extruding<'a>>,
) -> Program<'a> {
    Program {
        scene: Scene {
            mesh: mesh.clone(),
            sketches: sketches.clone(),
            camera: *camera,
            pivot,
            colors: palette.scene,
            sketch_plane: sketching.as_ref().map(Sketching::grid),
        },
        sketching,
        extruding,
        sketch_colors: palette.sketching,
    }
}

/// The viewport's shader program: handles input and hands iced the scene it
/// was built with to draw.
struct Program<'a> {
    scene: Scene,
    /// The sketch being edited, if one is.
    sketching: Option<Sketching<'a>>,
    /// The extrude being set up, if one is: never with a sketch.
    extruding: Option<Extruding<'a>>,
    sketch_colors: SketchColors,
}

/// What one frame of the viewport draws but the sketch being edited.
#[derive(Debug, Clone)]
struct Scene {
    mesh: Arc<RenderMesh>,
    sketches: Arc<RenderLines>,
    camera: Camera,
    /// The point the camera orbits, marked, if one was picked.
    pivot: Option<Pivot>,
    colors: Colors,
    /// The plane of the sketch being edited, if one is.
    sketch_plane: Option<GridPlane>,
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
    /// Where the middle button was pressed, while the cursor hasn't moved
    /// past [`CLICK_SLOP`] from there: letting go then is a click, which
    /// picks the point the camera orbits, and until then it doesn't orbit.
    click: Option<Point>,
    /// The modifiers held, which change what a drag does, and `Ctrl`
    /// (`Cmd`) adds to a sketch's selection.
    modifiers: keyboard::Modifiers,
    /// What's kept of the sketch being edited.
    sketch: sketch::Input,
    /// What's kept of the extrude being set up.
    extrude: extrude::Input,
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
        // The extrude's picking and handle come first, unless the camera
        // is being dragged; the rest goes on as outside a sketch.
        if let Some(extruding) = &self.extruding
            && state.drag.is_none()
            && let Event::Mouse(event) = event
            && let Some(action) = extruding.mouse(
                &mut state.extrude,
                *event,
                bounds,
                cursor,
                &self.scene.camera,
            )
        {
            return Some(action);
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
        // The model isn't faded behind an extrude, and hides what's
        // behind it of the extrude's regions and handle.
        let extrude = self.extruding.as_ref().map(|extruding| {
            let (base, live) = extruding.layers(&state.extrude, self.sketch_colors);
            SketchFrame {
                plane: extruding.plane(),
                depth_tested: true,
                base,
                live,
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
            }
        });
        Primitive {
            scene: self.scene.clone(),
            sketch: sketch.or(extrude),
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
                .or_else(|| {
                    let extruding = self.extruding.as_ref()?;
                    extruding.mouse_interaction(&state.extrude, bounds, cursor)
                })
                .unwrap_or_default(),
        }
    }
}

impl Program<'_> {
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
                state.click = (button == mouse::Button::Middle).then_some(position);
                Some(Action::capture())
            }
            mouse::Event::ButtonReleased(button) => {
                state.drag.take()?;
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
        let prepared = pipeline.prepare(
            &self.slot,
            device,
            queue,
            &Frame {
                camera: &scene.camera,
                mesh: &scene.mesh,
                sketches: &scene.sketches,
                // A sketch being edited moves the grid onto its plane and
                // fades the model.
                grid: scene.sketch_plane.unwrap_or(GridPlane::XY),
                faded: scene.sketch_plane.is_some(),
                pivot: scene.pivot,
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
