//! The 3D viewport: an iced shader widget backed by [`varde_render`].

use std::sync::{Arc, Weak};

use iced::widget::shader::{self, Action};
use iced::widget::{container, stack};
use iced::{Element, Event, Length, Point, Rectangle, mouse};
use varde_kernel::RenderMesh;
use varde_render::{Camera, ClipRect, Colors, Frame, PrepareError, Renderer, Slot, wgpu};

use crate::chrome::mouse_hint;
use crate::icons::MouseButton;
use crate::theme::Palette;
use crate::{Look, Message, controls};

const ORBIT_SPEED: f32 = 0.008;
const ZOOM_PER_LINE: f32 = 0.9;
const ZOOM_PER_PIXEL: f32 = 0.995;

/// The 3D viewport showing `mesh` from `camera`, with the controls over its
/// top-right corner.
pub(crate) fn viewport<'a>(
    mesh: &Arc<RenderMesh>,
    camera: &'a Camera,
    palette: &Palette,
) -> Element<'a, Message> {
    let scene = iced::widget::shader(program(mesh, camera, palette))
        .width(Length::Fill)
        .height(Length::Fill);
    let controls = container(controls::view_controls(camera))
        .align_right(Length::Fill)
        .padding([10, 12]);
    stack![scene, controls].into()
}

/// The shader program drawing `mesh` from `camera` in `palette`'s colors.
fn program(mesh: &Arc<RenderMesh>, camera: &Camera, palette: &Palette) -> Program {
    Program(Scene {
        mesh: mesh.clone(),
        camera: *camera,
        colors: palette.scene,
    })
}

/// The viewport's shader program: handles input and hands iced the scene it
/// was built with to draw.
struct Program(Scene);

/// What one frame of the viewport draws.
#[derive(Debug, Clone)]
struct Scene {
    mesh: Arc<RenderMesh>,
    camera: Camera,
    colors: Colors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Orbit,
    Pan,
}

impl DragKind {
    /// What dragging with `button` does, if anything. Keep [`hints`] in
    /// step.
    fn for_button(button: mouse::Button) -> Option<Self> {
        match button {
            mouse::Button::Left => Some(DragKind::Orbit),
            mouse::Button::Right | mouse::Button::Middle => Some(DragKind::Pan),
            _ => None,
        }
    }
}

/// The status bar hints for the viewport's mouse bindings, see
/// [`DragKind::for_button`].
pub fn hints<'a>() -> [Element<'a, Message>; 3] {
    [
        mouse_hint(MouseButton::Left, "Drag to orbit"),
        mouse_hint(MouseButton::Right, "Pan"),
        mouse_hint(MouseButton::Wheel, "Zoom"),
    ]
}

#[derive(Debug, Default)]
struct Interaction {
    drag: Option<(DragKind, Point)>,
    /// Names this widget's [`Slot`] in the [`Pipeline`], for as long as the
    /// widget lives.
    slot: Arc<SlotKey>,
}

/// Only its address matters: see [`Interaction::slot`].
#[derive(Debug, Default)]
struct SlotKey;

impl shader::Program<Message> for Program {
    type State = Interaction;
    type Primitive = Primitive;

    fn update(
        &self,
        state: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(event) = event else {
            return None;
        };

        match *event {
            mouse::Event::ButtonPressed(button) => {
                let position = cursor.position_over(bounds)?;
                let kind = DragKind::for_button(button)?;
                state.drag = Some((kind, position));
                Some(Action::capture())
            }
            mouse::Event::ButtonReleased(_) => {
                state.drag.take()?;
                Some(Action::capture())
            }
            mouse::Event::CursorMoved { position } => {
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
                cursor.position_over(bounds)?;
                let factor = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => ZOOM_PER_LINE.powf(y),
                    mouse::ScrollDelta::Pixels { y, .. } => ZOOM_PER_PIXEL.powf(y),
                };
                Some(Action::publish(Message::Look(Look::Zoom(factor))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &Interaction, _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        Primitive {
            scene: self.0.clone(),
            slot: state.slot.clone(),
        }
    }

    fn mouse_interaction(
        &self,
        state: &Interaction,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match state.drag {
            Some((DragKind::Orbit, _)) => mouse::Interaction::Grabbing,
            Some((DragKind::Pan, _)) => mouse::Interaction::Move,
            None => mouse::Interaction::default(),
        }
    }
}

/// A frame's scene and the widget that draws it.
#[derive(Debug, Clone)]
struct Primitive {
    scene: Scene,
    /// The widget's key to its slot in the [`Pipeline`].
    slot: Arc<SlotKey>,
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
            log::error!("Couldn't draw the model: {error}");
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
