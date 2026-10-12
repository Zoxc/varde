# Settings page

A design for an app settings page, built (see "The settings page" in
`agents/viewport.md`). The mock shows it:
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
- Moving to another view closes it, so what's moved to shows: New
  design, Open (a file, a recent design, a recovered one, a sample) and
  Close document.

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

A setting changed from its default says so in its row: an accent bar
along the row's left, and a reset arrow before its control, "Reset to
<default>" as its tooltip, resetting just it.

## Contents

**Appearance**

- Theme: System, Light, Dark (`ThemeChoice`).
- Interface scale: a slider, 75 % to 200 % in 5 % steps, the value beside
  it, set when let go; 100 % default. For everything but the model
  (iced's `scale_factor`, over the system's; the web takes the same
  `iced_winit` path).

**View**, how the viewport draws the model. These are the view options
menu's (the status bar's `⋯`): the menu stays, a quick way to the same
settings, and a change in either shows in both.

- Projection: Orthographic, Perspective.
- Shading: Regular, Flat, Metal, Flat metal (`varde_render::Shading`).
- Edges: Features, Patches, Triangles (`Edges::Default`, `Wireframe`,
  `Tessellation`).
- The view options menu names the edges as the page does: Features,
  Patches, Triangles (it said Feature edges, Wireframe, Tessellation).
- Shading and edges show the menu's icons (`Icon::Shade*`, `Icon::Edges*`)
  beside their names.
- Hidden edges, Names, Mouse hints: switches.

All are stored (`varde_io::settings`), each key read on its own, so old
files keep working.

**Units.** A live sample under the heading shows a length and an angle
as they're shown.

- Length: mm (default), cm, m, in, ft (`varde_expr::LengthUnit`), the
  units a new design starts in, stored in it: for sizes typed without a
  unit, and how sizes are shown. A design opened keeps its own.
- Angle: Degrees (default), Radians. App-wide, not stored in designs:
  how angles are shown. Angles typed without a unit stay degrees, as a
  design's texts must evaluate as they did.
- Decimal places: Auto (default), or 0 to 6 with a stepper, at most,
  trailing zeros left out. App-wide. Auto is each unit's own count, as
  the app has always shown (`LengthUnit::decimals`: 3 for mm, 4 for cm
  and in, 6 for m, 5 for ft; 3 for degrees, 5 for radians), so the
  default changes nothing; the stepper starts from the length unit's.
  Only what's shown (labels, readouts, the status bar, notes): fields
  filled in to be taken, and dimensions made where the geometry is, keep
  each unit's count and degrees, so the setting never changes the model
  (`varde_expr::Shown`).

**Mouse.** Each defaults to what the app did before it was a setting.

- A drag action per button, each Orbit, Pan, Zoom or Nothing
  (`DragKind::for_button`, `agents/viewport.md`):
  - Left: Orbit (default). Outside a sketch only; a click still selects.
  - Middle: Orbit (default). Set to Nothing, its click still sets the
    rotation point.
  - Right: Pan (default). Shift held still orbits.
  - A zoom drag zooms in going up, about where it started.
- **Rotation,** how a drag turns the view: Turntable (default: yaw about
  Z and pitch clamped to `Camera::PITCH_LIMIT`, so Z stays up), Turntable
  over the top (no pitch limit, the view can go upside down; the yaw then
  turns the other way so the model follows the pointer; back to
  Turntable, the view is set right side up, the eye kept), Trackball
  (free, about the view's own axes: a sideways drag turns about up on
  screen, an upward one about right, so a drag round a loop rolls the
  view; back to Turntable the view is set upright, to Over the top
  unrolled the nearer way, upright or upside down, the eye kept either
  way; the view cube's faces and Home turn to upright views).
- **Rotation point,** what the view orbits about: a clicked point
  (default, `Look::SetPivot`, else the view's target), the point under
  the pointer where each drag starts (no marker shown; off the model and
  grid, the middle), or the view's centre always. Leaving Clicked forgets
  the clicked point.
- **Set rotation point with,** with Clicked: Middle click (default, any
  middle click, with Shift or not), Middle double-click, Shift + middle
  click. On the model or grid it sets the point, elsewhere back to the
  centre. The left button is left out: its modifiers are taken (Ctrl
  adds to the selection, Alt places a reference, a double-click selects
  the body).
- **Zoom direction** (wheel up zooms in, default) and **zoom to pointer**
  (on, default, `Camera::zoom_at`; off zooms about the middle).
- **Snapping** in sketch tools, on (default), Shift to not snap; off,
  tools and dragged points snap only while Shift is held, the status
  bar's hint saying "Snap". There is no grid snapping in the app to turn
  off: this was "Snap to grid" in the first draft, but what Shift frees
  is the snapping to points, curves and directions.

The status bar's mouse hints follow the buttons as set.

**Files (desktop only;** the tab isn't on the web, which always
auto-saves every minute and has no recent files list).

- Auto-save: Off, every minute (default), 5 min, 10 min: changes not
  saved go to the recovery file that long after the first of them, so a
  crash loses at most that much. This replaces the 3 s pause and the
  2 min cap. Off: edits are never auto-saved, so a crash loses what
  wasn't saved. The sidecar lock, recovery offers, emptying it on
  undoing back to the saved state, and writing restored changes back all
  stay (`agents/files.md`).
- Recent designs kept: 10, 25, 50, 100 (default,
  `varde_io::recent::MAX`). Lowered, the oldest are dropped and the list
  stored so.

**MCP (desktop only;** a page can't listen). Serve over MCP (off,
default) and the port (7311, default; 1024–65535), both with row resets
and in Reset to defaults' list; the client connected with Disconnect;
the token (`mcp-token.toml`, hidden, with Show, Copy and Regenerate);
and the Claude Code, Codex and opencode lines to copy
(`agents/mcp.md`).

## Suggested

Not built; others to consider:

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
- Stored through `Settings::chose`, each setting with its own "chosen
  before loading" flag (`settings::Key`). The page's own settings come
  as `SettingsPage::Set(Setting)`.
- The page is a pure view in `crates/view/src/settings.rs`.
- Bounds: the decimal places stepper stops at 0 and 6; a value the page
  doesn't offer (a scale of 112 % or past 75 % to 200 %, 7 places, auto-save every 2 min) is ignored
  as set and read as the default as stored.
- Tests, headless: toggling and Escape; `SetTheme` stores; viewport input
  is off while open; an open operation is still there after closing;
  light and dark screenshots of the page beside the existing shots.

## Open questions

1. Should units be per design (a `.vrdp` change) or app-wide? The
   length unit is per design (a new one's from the settings); the angle
   unit and decimal places are app-wide, display only.
2. Should Mouse hints stay in the view options menu as well as here?
3. Should the side panel stay beside the page, or should the page fill
   everything under the toolbar?
