# Settings page

A design for an app settings page, not built yet. The mock shows it:
`notes/ui-mock/` on every page, opened with the gear or `/settings` in the
address (`model.html#bracket/settings`, `welcome.html#/settings/web`).

## Entry

- The theme button becomes a **settings toggle**, a gear, where the theme
  button is now: at the right end of the toolbar after Undo and Redo, and
  in the welcome screen's foot (as "Settings" with its key). On while the
  page is open; a second click closes it.
- `Ctrl ,` toggles it too, and Escape closes it.
- The theme is chosen on the page; there is no more cycling.

## Where it shows

- **Document:** it takes the viewport's place. The toolbar and the side
  panel stay; the status bar, the tool rail and the Alt bar, which float
  over the view, go with it. Whatever is open, such as a sketch being
  edited or an operation being set up, stays open underneath and is
  back as it was when the page closes; closing it cancels nothing. The
  viewport keeps its camera and the regeneration lanes go on: only the
  widget is swapped out.
- While it's open the viewport's keys (tools, rail sets, selection) are
  off. Undo, redo and save stay.
- **Welcome, desktop:** in the recent designs' place, beside the column.
  **Welcome, web:** under the header bar, in place of the stored designs.
- Open or not is the app's, not a document's: it stays open across
  opening and closing designs.

## Layout

A head with "Settings", Reset to defaults and a close button, over a tab
per section (as the side panel's tabs), the open section a card of rows
in one centred column (at most 620 px): name and a line saying what it
does on the left, the control on the right. Controls: a segmented bar for
a choice among a few, a switch for on or off, a stepper for a small
count. A foot says where the settings are kept.

Reset to defaults asks first, in a popup listing each setting it
changes, from what it is to the default (Enter resets, Escape cancels).
With every setting already the default, the button is off.

## Contents

**Appearance**

- Theme: System, Light, Dark (`ThemeChoice`). Stored now.

**View**, how the viewport draws the model. These are the view options
menu's (the status bar's `⋯`): the menu stays, a quick way to the same
settings, and a change in either shows in both.

- Projection: Orthographic, Perspective.
- Shading: Regular, Flat, Metal, Flat metal (`varde_render::Shading`).
- Edges: Features, Patches, Triangles (`Edges::Default`, `Wireframe`,
  `Tessellation`).
- Shading and edges show the menu's icons (`Icon::Shade*`, `Icon::Edges*`)
  beside their names.
- Hidden edges, Names, Mouse hints: switches.

Only the theme and mouse hints are stored now (`varde_io::settings`); the
rest are `ViewOptions` reset each run. Storing them adds keys to
`settings.toml`, read each on its own as the module describes, so old
files keep working.

**Units**, the defaults for a new design. A live sample under the
heading shows a length and an angle as they would be shown.

- Length: mm (default), cm, m, in, ft (`varde_expr::LengthUnit`). For
  sizes typed without a unit, and how sizes are shown.
- Angle: Degrees (default), Radians.
- Decimal places: 0 to 6, at most, trailing zeros left out, default 3,
  mm's now (`LengthUnit::decimals`; the app has a count per unit, 3 for
  mm and degrees). Only what's shown; typed values keep their precision.

Units per design mean the units are stored in the
document, which is a `.vrdp` change and needs the user's permission
first. Until then they could be app-wide only, with every document read
and shown in them; the stored values stay in mm either way.

## Suggested

Tagged "Proposed" in the mock. Each defaults to what the app does now,
so storing none of them changes nothing.

- **Interface scale:** 90 %, 100 %, 125 %, 150 %, for everything but the
  model (iced's `scale_factor`).
- **Mouse,** a drag action per button, each Orbit, Pan, Zoom or Nothing
  (`DragKind::for_button`, `agents/viewport.md`):
  - Left: Orbit (default). Outside a sketch only; a click still selects.
  - Middle: Orbit (default).
  - Right: Pan (default). Shift held still orbits.
- **Rotation,** how a drag turns the view: Turntable (default, today's:
  yaw about Z and pitch clamped to `Camera::PITCH_LIMIT`, so Z stays up),
  Turntable over the top (no pitch limit, the view can go upside down),
  Trackball (free, about the view's axes).
- **Rotation point,** what the view orbits about: a clicked point
  (default, `Look::SetPivot`, else the view's target), the point under
  the pointer where each drag starts, or the view's centre always.
- **Set rotation point with,** with Clicked: Middle click (default, today's), Middle
  double-click, Shift + middle click. On the model or grid it sets the
  point, elsewhere back to the centre. The left button is left out: its
  modifiers are taken (Ctrl adds to the selection, Alt places a
  reference, a double-click selects the body).
- **Zoom direction** (wheel up zooms in, default) and **zoom to pointer**
  (on, default, `Camera::zoom_at`; off zooms about the middle).
- **Snap to grid** in sketch tools, on (default), Shift to not snap.
- **Files (desktop):** auto-save after a pause in editing: Off, 3 s
  (default, today's pause; the 2 min cap while edits keep coming stays),
  10 s, 30 s. Recent designs kept: 10, 25, 50, 100 (default,
  `varde_io::recent::MAX`).

Others to consider:

- Grid spacing: automatic with the zoom, or fixed.
- Selection colour or accent colour.
- Default material or colour for new bodies.
- Tessellation quality for display (coarse, normal, fine), apart from
  export.
- Export defaults: 3MF units, which bodies.
- Language and number format (decimal comma).
- Show the welcome screen at start, or reopen the last designs.
- Anti-aliasing (MSAA samples), for slow GPUs.
- Confirm before deleting a feature.

## Plumbing

- `Message::CycleTheme` becomes `SetTheme(ThemeChoice)`; a new
  `ToggleSettings` opens and closes the page. The settings set by the page
  and by the menu are the same messages.
- Stored through `Settings::chose_*` as theme and mouse hints are now,
  each setting with its own "chosen before loading" flag, or a set of
  them.
- The page is a pure view in `crates/view/src/settings.rs`.
- Bounds: steppers clamp (decimal places 0 to 6, recent designs 0 to 30);
  a stored value out of range reads as the default.
- Tests, headless: toggling and Escape; `SetTheme` stores; viewport input
  is off while open; an open operation is still there after closing;
  light and dark screenshots of the page beside the existing shots.

## Open questions

1. Should units be per design (a `.vrdp` change) or app-wide?
2. Should Mouse hints stay in the view options menu as well as here?
3. Should the side panel stay beside the page, or should the page fill
   everything under the toolbar?
