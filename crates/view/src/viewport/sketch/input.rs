//! The left button's work on the sketch being edited: selecting, box
//! selecting, dragging geometry and labels, drawing with the tools, `Esc`
//! while it's held, and the cursor.

use std::sync::Arc;

use glam::DVec2;
use iced::keyboard::key::Named;
use iced::keyboard::{self, Modifiers};
use iced::widget::shader::Action;
use iced::{Event, Rectangle, mouse};
use varde_render::Camera;
use varde_sketch::{Id, Kind, NearMiss, Selectable};

use iced::time::{Duration, Instant};

use super::{DRAG_DISTANCE, Input, LabelPress, NEAR_MISS_GAP, Near, Press, Sketching};
use crate::hit::{self, BoxMode};
use crate::overlaps::{self, OverlapItems, Overlaps};
use crate::projection::{Cursor, Projector};
use crate::shortcut::Held;
use crate::snap::{self, Snap};
use crate::viewport::sketch_pick::HIT_PIXELS;
use crate::viewport::{local, local_cursor};
use crate::{Edit, Look, Message, Tool, ToolClick};

/// How near the cursor a corner has to be for Fillet and Chamfer to take
/// it, in pixels: a point, but all they look for.
const CORNER_TOLERANCE: f64 = 12.0;
/// How soon a second press of a tool has to follow the first, near where
/// it was, to make a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl<'a> Sketching<'a> {
    /// Takes `event`, with the `cursor` over the viewport's `bounds` seen
    /// by `camera` and `modifiers` held. `None` for what isn't the
    /// sketch's: buttons other than the left, the wheel, and keys but
    /// `Esc` while the left button is held.
    pub(crate) fn update(
        &self,
        input: &mut Input,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        match event {
            // Lets go of the button held down, and the key does nothing
            // more. A drag of geometry is the app's to put back, so it's
            // told to, whether or not the drag has moved anything yet: left
            // to the app, the key would back out of the sketch then.
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(Named::Escape),
                ..
            }) => {
                // An Offset dragged goes nowhere.
                if std::mem::take(&mut input.releasing) {
                    return Some(Action::request_redraw().and_capture());
                }
                let press = input.press.take()?;
                Some(if press.grab && press.moved {
                    Action::publish(Message::Look(Look::CancelDrag)).and_capture()
                } else {
                    Action::request_redraw().and_capture()
                })
            }
            Event::Mouse(event) => {
                let projector = self.projector(camera, bounds)?;
                self.mouse(input, *event, bounds, cursor, &projector, modifiers)
            }
            Event::Window(iced::window::Event::RedrawRequested(now)) => {
                self.hold(input, *now, bounds, camera)
            }
            _ => None,
        }
    }

    /// Takes a frame drawn at `now` while the button may be held: held
    /// still for [`overlaps::HOLD_DELAY`] over more than one item, it lets
    /// go and lists them ([`Look::OpenOverlaps`]); over one or none it goes
    /// on as it was.
    fn hold(
        &self,
        input: &mut Input,
        now: Instant,
        bounds: Rectangle,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let press = input.press.as_mut()?;
        let due = press.lists_at()?;
        if now < due {
            // Asked again: a redraw asked for sooner lets go of it.
            return Some(Action::request_redraw_at(due));
        }
        press.held = true;
        let projector = self.projector(camera, bounds)?;
        let cursor = projector.cursor(press.from)?;
        let tolerance = overlaps::OVERLAP_REACH * cursor.pixel;
        let mut ids = hit::overlaps(self.sketch, cursor.at, tolerance);
        if ids.len() < 2 {
            return None;
        }
        ids.truncate(overlaps::MAX_OVERLAPS);
        let size = [bounds.width, bounds.height];
        let list = Overlaps::new(press.from, size, OverlapItems::Sketch(ids));
        input.press = None;
        Some(Action::publish(Message::Look(Look::OpenOverlaps(list))))
    }

    /// Takes the mouse `event`, with the `cursor` over `bounds`.
    fn mouse(
        &self,
        input: &mut Input,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        projector: &Projector,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        match event {
            mouse::Event::CursorMoved { position } => {
                let over = local_cursor(bounds, cursor);
                input.cursor = over;
                let previous = input.pointer.replace(local(bounds, position));
                if let Some((id, _)) = self.label_drag {
                    return self.drag_label(
                        input,
                        id,
                        previous,
                        local(bounds, position),
                        projector,
                    );
                }
                if let Some(press) = &mut input.press {
                    // The raw position, so a drag goes on over the rest of
                    // the window.
                    return self.drag(press, local(bounds, position), projector, modifiers);
                }
                let under = over.and_then(|at| projector.cursor(at));
                let hover = under.and_then(|cursor| self.hit(cursor));
                let changed = std::mem::replace(&mut input.hover, hover) != hover;
                if let Some(pointed) = self.point_to(input, under, modifiers) {
                    return Some(pointed);
                }
                // The place a point would go follows the cursor along a
                // spline.
                let on_spline = self.spline_hovered(hover).is_some();
                (changed || on_spline || self.tool.is_some()).then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => {
                input.cursor = None;
                let hovered = input.hover.take().is_some();
                if let Some(snapped) = self.point_to(input, None, modifiers) {
                    return Some(snapped);
                }
                (hovered || self.tool.is_some()).then(Action::request_redraw)
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let position = cursor.position_over(bounds)?;
                let from = local(bounds, position);
                let under = projector.cursor(from);
                if self.releases() {
                    input.releasing = true;
                    return Some(Action::capture());
                }
                // The model's picking's: see [`Sketching::picks_outside`].
                if self.picks_outside() {
                    return None;
                }
                if self.tool.is_some() {
                    let double = double_click(&mut input.last_click, from);
                    // The app lets go of the snap it shows as it takes the
                    // click, so the next is sent afresh.
                    input.snap = None;
                    let message = under.map(|cursor| {
                        let click = ToolClick {
                            double,
                            ..self.click(cursor, modifiers)
                        };
                        Message::Edit(Edit::ToolClick(click))
                    });
                    return Some(capture(message));
                }
                let hit = under.and_then(|cursor| self.hit(cursor));
                input.press = Some(Press {
                    from,
                    to: from,
                    at: under.map(|cursor| cursor.at),
                    hit,
                    point: (hit.and_then(Selectable::item))
                        .and_then(|id| self.sketch.point(id))
                        .map(|point| point.at),
                    moved: false,
                    // The origin and axes stay where they are, and so does
                    // what a link made.
                    grab: self.editable
                        && hit.is_some_and(|hit| {
                            let id = hit.id();
                            !id.is_builtin() && !self.sketch.is_linked(id)
                        }),
                    when: Instant::now(),
                    held: false,
                });
                let due = input.press.as_ref().and_then(Press::lists_at)?;
                Some(Action::request_redraw_at(due).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if std::mem::take(&mut input.releasing) {
                    let under = cursor
                        .position_over(bounds)
                        .and_then(|at| projector.cursor(local(bounds, at)));
                    let message = under.map(|cursor| {
                        Message::Edit(Edit::ToolClick(self.click(cursor, modifiers)))
                    });
                    return Some(capture(message));
                }
                // A label grabbed is let go of, moved or not.
                if input.label.take().is_some() || self.label_drag.is_some() {
                    return Some(capture(Some(Message::Edit(Edit::DropLabel))));
                }
                let press = input.press.take()?;
                let add = modifiers.command();
                // A second click on a spline, soon after the first and
                // near it, adds a point to it, as one click does while
                // adding points.
                let double = !press.moved
                    && (double_click(&mut input.last_select, press.from) || self.inserting);
                let spline = self.spline_hovered(press.hit);
                let message = match press.area() {
                    _ if double && let (Some(spline), Some(at)) = (spline, press.at) => {
                        Message::Edit(Edit::InsertSplinePoint { spline, at })
                    }
                    _ if !press.moved => Message::Look(Look::ClickGeometry {
                        hit: press.hit,
                        add,
                    }),
                    None => Message::Edit(Edit::DropGeometry),
                    Some(area) => {
                        let mode = BoxMode::dragged(press.from, press.to);
                        let ids = hit::in_box(self.sketch, projector, area, mode);
                        Message::Look(Look::SelectBox { ids, add })
                    }
                };
                Some(capture(Some(message)))
            }
            _ => None,
        }
    }

    /// Moves the button held down, `press`, to `to`: past
    /// [`DRAG_DISTANCE`] it drags what it went down on, or a box. A point
    /// dragged snaps as [`Sketching::snaps`] says with `modifiers`.
    fn drag(
        &self,
        press: &mut Press,
        to: DVec2,
        projector: &Projector,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        press.to = to;
        if !press.moved && press.from.distance(to) < DRAG_DISTANCE {
            return Some(Action::capture());
        }
        press.moved = true;
        if !press.grab {
            return Some(Action::request_redraw().and_capture());
        }
        // Off the sketch, the geometry stays where it was dragged last.
        let message =
            press
                .hit
                .zip(press.at)
                .zip(projector.cursor(to))
                .map(|((id, from), cursor)| {
                    let snapped = self
                        .drag_snap(press, cursor, modifiers)
                        .filter(|(_, snap)| snap.snapped());
                    // Snapped, the point goes where it snapped rather than
                    // keep the offset it was grabbed at (a rim's radius
                    // takes only where the cursor is).
                    let (from, to, target) = match snapped {
                        Some((grabbed, snap)) => (grabbed, snap.at, snap.target),
                        None => (from, cursor.at, None),
                    };
                    Message::Look(Look::DragGeometry {
                        id,
                        from,
                        to,
                        target,
                    })
                });
        Some(capture(message))
    }

    /// Where what `press` drags snaps with the cursor at `cursor`, with
    /// where it was grabbed: a point as [`snap::snap_drag`] has it, a
    /// circle's rim as [`snap::snap_rim`]. `None` for anything else, or
    /// where [`Sketching::snaps`] says not with `modifiers`.
    pub(super) fn drag_snap(
        &self,
        press: &Press,
        cursor: Cursor,
        modifiers: Modifiers,
    ) -> Option<(DVec2, Snap)> {
        if !self.snaps(modifiers) {
            return None;
        }
        let id = press.hit?.item()?;
        if let Some(point) = press.point {
            return Some((
                point,
                snap::snap_drag(self.sketch, id, cursor.at, cursor.pixel),
            ));
        }
        let rim = self
            .sketch
            .curve(id)
            .filter(|entry| entry.curve.kind() == Kind::Circle)?;
        Some((
            press.at?,
            snap::snap_rim(self.sketch, rim.id, cursor.at, cursor.pixel),
        ))
    }

    /// Follows the cursor moved to `to` from `previous` with the label of
    /// the dimension `id` grabbed: past [`DRAG_DISTANCE`] from where it
    /// was grabbed, it drags the label there.
    fn drag_label(
        &self,
        input: &mut Input,
        id: Id,
        previous: Option<DVec2>,
        to: DVec2,
        projector: &Projector,
    ) -> Option<Action<Message>> {
        let grabbed = match input.label {
            Some(label) if label.id == id => label,
            // The label took the press, where the cursor was last.
            _ => LabelPress {
                id,
                from: previous.unwrap_or(to),
                moved: false,
            },
        };
        let moved = grabbed.moved || grabbed.from.distance(to) >= DRAG_DISTANCE;
        input.label = Some(LabelPress { moved, ..grabbed });
        if !moved {
            return Some(Action::capture());
        }
        let message = projector
            .cursor(grabbed.from)
            .zip(projector.cursor(to))
            .map(|(from, to)| {
                Message::Look(Look::DragLabel {
                    id,
                    from: from.at,
                    to: to.at,
                })
            });
        Some(capture(message))
    }

    /// Whether its tool picks outside the sketch, in the model and other
    /// sketches ([`Tool::picks_outside`]): then the left button is the
    /// model's picking's, and nothing of the sketch is hit.
    pub(crate) fn picks_outside(&self) -> bool {
        self.tool.is_some_and(|tool| tool.tool.picks_outside())
    }

    /// The item under `cursor`, see [`hit::hit`]: with Trim or Extend,
    /// or Offset picking its chain, the curve ([`hit::hit_curve`]), with
    /// Mirror choosing its line the line ([`hit::hit_line`]), with Fillet
    /// or Chamfer picking their corner the point where lines make one, a
    /// little farther off ([`hit::hit_corner`]), and nothing with Offset
    /// placing its copy, or Fillet or Chamfer theirs, which go where the
    /// cursor is, nor with Project or Intersect, which pick outside the
    /// sketch.
    fn hit(&self, cursor: Cursor) -> Option<Selectable> {
        let tolerance = HIT_PIXELS * cursor.pixel;
        let item = match self.tool {
            _ if self.releases() || self.picks_outside() => None,
            Some(tool) if tool.tool.corners() => {
                hit::hit_corner(self.sketch, cursor.at, CORNER_TOLERANCE * cursor.pixel)
            }
            Some(tool) if matches!(tool.tool, Tool::Trim | Tool::Extend | Tool::Offset) => {
                hit::hit_curve(self.sketch, cursor.at, tolerance)
            }
            Some(tool) if tool.tool == Tool::Mirror && tool.about => {
                hit::hit_line(self.sketch, cursor.at, tolerance)
            }
            _ => return hit::hit(self.sketch, cursor.at, tolerance),
        };
        item.map(Selectable::Item)
    }

    /// The spline `hit` is, if it is one and points can be added to it.
    pub(super) fn spline_hovered(&self, hit: Option<Selectable>) -> Option<Id> {
        (hit.and_then(Selectable::item))
            .filter(|&id| self.editable && self.sketch.kind(id) == Some(Kind::Spline))
    }

    /// The near misses of the sketch's profiles seen through `projector`:
    /// open ends within [`NEAR_MISS_GAP`] pixels at the target, paired
    /// again only when the profiles or the zoom have changed since
    /// `input` last kept them.
    pub(super) fn near_misses(&self, input: &Input, projector: &Projector) -> Vec<NearMiss> {
        let Some(profiles) = self.profiles else {
            return Vec::new();
        };
        let gap = NEAR_MISS_GAP * projector.pixel();
        let mut near = input.near.borrow_mut();
        match &*near {
            Some(near) if Arc::ptr_eq(&near.profiles, profiles) && near.gap == gap => {
                near.misses.clone()
            }
            _ => {
                let misses = profiles.near_misses(gap);
                *near = Some(Near {
                    profiles: profiles.clone(),
                    gap,
                    misses: misses.clone(),
                });
                misses
            }
        }
    }

    /// Where the drawing tool's next click snaps to with the cursor at
    /// `cursor`, see [`snap::snap`], if it snaps to anything: not without
    /// a drawing tool, nor where [`Sketching::snaps`] says not with
    /// `modifiers`.
    pub(super) fn snap(&self, cursor: Cursor, modifiers: Modifiers) -> Option<Snap> {
        let tool = self.tool.filter(|tool| tool.tool.draws())?;
        if !self.snaps(modifiers) {
            return None;
        }
        Some(snap::snap(self.sketch, &tool, cursor.at, cursor.pixel)).filter(Snap::snapped)
    }

    /// The drawing tool's click with the cursor at `cursor` and
    /// `modifiers` held, where it snaps, if it does: not a double-click.
    fn click(&self, cursor: Cursor, modifiers: Modifiers) -> ToolClick {
        let click = ToolClick {
            at: cursor.at,
            target: None,
            inference: None,
            hit: self.hit(cursor),
            pixel: cursor.pixel,
            double: false,
            reference: Held::REFERENCE.is_held(modifiers),
        };
        match self.snap(cursor, modifiers) {
            Some(snap) => click.snapped(snap),
            None => click,
        }
    }

    /// Whether the tool clicks where the left button is let go rather
    /// than pressed, so the click can be dragged there: Offset placing its
    /// copy, or Fillet or Chamfer theirs, which the preview shows through
    /// the cursor meanwhile.
    fn releases(&self) -> bool {
        self.tool
            .is_some_and(|tool| tool.tool.places() && !tool.picked.is_empty())
    }

    /// Whether the drawing tool's shape has fields, so the app hears where
    /// its click would go as the cursor moves ([`Look::Aim`]).
    pub(super) fn aiming(&self) -> bool {
        self.tool.is_some_and(|tool| !tool.fields().is_empty())
    }

    /// Tells the app where the drawing tool's click would go with the
    /// cursor at `under`, if it's over the sketch, and `modifiers` held:
    /// while its shape has fields, the click itself, else where it snaps,
    /// if that's changed. With the cursor off the sketch, a shape with
    /// fields stays aimed where it was, snapped as it was, as `Enter`
    /// places it there.
    fn point_to(
        &self,
        input: &mut Input,
        under: Option<Cursor>,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        if self.aiming() {
            let cursor = under?;
            let click = self.click(cursor, modifiers);
            // The app takes where it snaps from it.
            input.snap = Some(click.snap()).filter(Snap::snapped);
            return Some(Action::publish(Message::Look(Look::Aim(click))));
        }
        let snap = under.and_then(|cursor| self.snap(cursor, modifiers));
        self.send_snap(input, snap)
    }

    /// Tells the app `snap` is where the next click snaps to, if that's
    /// not what it was told last: its glyph shows by the cursor.
    fn send_snap(&self, input: &mut Input, snap: Option<Snap>) -> Option<Action<Message>> {
        if input.snap == snap {
            return None;
        }
        input.snap = snap;
        Some(Action::publish(Message::Look(Look::Snap(snap))))
    }

    /// Takes `modifiers` changing to what they are, with the `cursor` over
    /// the viewport's `bounds` seen by `camera`: [`Held::FREE`] lets go of
    /// the snap, or takes it again.
    pub(crate) fn modifiers_changed(
        &self,
        input: &mut Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        self.tool?;
        let projector = self.projector(camera, bounds)?;
        let at = local_cursor(bounds, cursor)?;
        Some(
            self.point_to(input, projector.cursor(at), modifiers)
                .unwrap_or_else(Action::request_redraw),
        )
    }

    /// The cursor over the viewport's `bounds`, if the sketch sets it:
    /// grabbing while geometry is dragged, else over the viewport a
    /// crosshair with a tool, or a pointer over an item to select.
    pub(crate) fn mouse_interaction(
        &self,
        input: &Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<mouse::Interaction> {
        let label = input.label.is_some_and(|label| label.moved) && self.label_drag.is_some();
        if label || input.press.is_some_and(|press| press.drags_geometry()) {
            return Some(mouse::Interaction::Grabbing);
        }
        cursor.position_over(bounds)?;
        if self.tool.is_some() {
            Some(mouse::Interaction::Crosshair)
        } else if input.press.is_none() && input.hover.is_some() {
            Some(mouse::Interaction::Pointer)
        } else {
            None
        }
    }
}

/// Whether a click at `at`, in the viewport's pixels, ends a double-click:
/// soon after the click `last` holds and near it. `last` then holds this
/// click, unless it ended one.
pub(in crate::viewport) fn double_click(last: &mut Option<(Instant, DVec2)>, at: DVec2) -> bool {
    let now = Instant::now();
    let double = last.take().is_some_and(|(time, from)| {
        now.saturating_duration_since(time) < DOUBLE_CLICK && from.distance(at) < DRAG_DISTANCE
    });
    if !double {
        *last = Some((now, at));
    }
    double
}

/// `message`, if there is one, with the event captured.
fn capture(message: Option<Message>) -> Action<Message> {
    match message {
        Some(message) => Action::publish(message).and_capture(),
        None => Action::capture(),
    }
}
