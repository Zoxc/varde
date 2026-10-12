# The extrude UI (`crates/view`, `crates/app`)

**The session** (`app/src/doc/extrude.rs`, `Doc::mode` holding an
`ExtrudeSession`) is started by the Extrude tool (`X`, `Look::StartExtrude`,
outside sketches, where `X` turns geometry into construction; with no
sketch yet too, the panel waiting for one; a revolve or combine set up
is dropped for it; again, or `Esc`, cancels it) or by
editing an extrude (`Look::EditFeature` on one: a double-click in the
Timeline, or `Enter` with it selected). It never runs with a sketch
session, nor in a read-only document. It holds: the extrude edited, if
any; the source sketch (the one selected in the Timeline, the edited
extrude's, or else the one the first region picked is in, which un-picking
every region lets go of again); the profiles of the source, or before
there is one of every visible sketch with regions (each found once, and
again when its sketch changes; those with none to pick, too complex
within the whole of `MAX_WORK` or empty, are kept too, by value, so a
change elsewhere doesn't work them out again; the visible sketches worked
out in one refresh share `refresh_work()`, twice `MAX_WORK`, through
`Sketch::profiles_spending`, and those past it have none to pick and
aren't worked out at all, not even their splines' shapes, so a file of
many hostile sketches costs the UI thread a bounded time; skipped so, or
too complex for the less than `MAX_WORK` that was left, they aren't kept
but tried again on the next change, when those found are kept and spend
nothing, so each sketch gets its whole `MAX_WORK` within a few changes);
the regions picked, as indices and as
`Profiles::reference`s made as they're picked (a region too thin for a
reference can't be picked); the extent kind, the two distance fields
(text, last good `Value`, error, read with `Extent::ask`), flip, the
taper's field (the same, read with `Extrude::taper_ask`; the edited
extrude's taper shown as typed, units followed as the distances'), the
operation and the excluded bodies (kept from the edited extrude). When
the source sketch changes under it (undo), the picked regions are found
again by their references (`Profiles::resolve`); while its regions can't
be found none are picked, and the references wait for them. An edited extrude's
references that aren't found are counted (`missing`, shown in the panel)
and dropped: `SetFeature` gets fresh references of what's picked. Editing
never changes the extrude's sketch, so `SetFeature` doesn't hide one.
A replacement of the whole document (restoring recovered changes, or
undoing or redoing that: `Editor::lineage` changes from the one `Doc`
saw at its last sync) ends the session and its draft, as its ids
(the extrude edited, the source, the excluded bodies) may name other
things: OK would otherwise write the values read before over whatever
extrude the id names now. The Timeline's selection, which an extrude
being edited sets to it, is let go of across a replacement as well
(`Doc::prune`). A `Replace` equal to the document is no change
and keeps the session.

**The preview** is the session's extrude sent as the request's draft
(`Doc::request_model` after every edit and look): whole once a region is
picked and the extent's distances have read at least once, with
`NewBody(BodyId::NEW)` for a new body. See `agents/viewport.md` for how
`MeshFeed` numbers drafts and keeps the newest answer; the draft's error
(`Drafted.error`) shows in the panel while it's the latest's.

**Committing** (`Edit::CommitExtrude`: OK, `Enter` in a distance field or,
outside one, the screen's `Enter`) needs `Doc::extrude_ready`: the
document editable, **no sketch edits waiting on the solver**
(`Doc::proposing`), and the session ready (`ExtrudeSession::ready`): an
extrude whole, no distance refused, and passing the extrude's own check
against the document's design (`Extrude::check_own`, public for this:
two sides together over `MAX_COORD`, say). The own check's refusal shows
in the panel at once (`ExtrudeState.refused`, in place of the draft's
error). The solver wait is because edits made in a sketch just left are
committed only once the solver accepts them: an extrude committed before
would be of the sketch without them (its regions found again afterwards,
maybe another one than the preview showed) and would come before them in
the undo history. Once they're answered, `prune_extrude` finds the picked
regions again in the new sketch and the draft is asked for again, so
what OK commits is what was previewed; undo in the window drops them and
frees OK. So do the other ways the wait ends (tested): a rejected edit
(the picks stay), the sketch deleted (its edits dropped once answered,
the session gone with it), restoring recovered changes (drops them; one
that changes the document ends the session, above), a units change in
the window (set after the answers, then followed by the session); read-only in the window ends the session. A solver lane not
started yet keeps OK waiting, as saving waits. After `feed::SLOW` (250 ms) the panel says "Checking the
sketch…" (`ExtrudeState.checking`, from `Proposals::slow`) when
there's neither a refusal nor a draft error to show. OK, the screen's
`Enter` binding and the hint take `ExtrudeState.ready`, which is
`extrude_ready`; `commit_extrude` checks `extrude_ready` itself, since a
field's `Enter` sends `CommitExtrude` whatever the state. A draft whose
feature fails can still be committed; it shows red in the Timeline. It
applies `AddFeature` (the document's name "Extrude N", adding the body
and hiding the sketch) or `SetFeature`, one undo step, selects the new
extrude and ends the session; refused by the document (left to the
cross-references, which the session keeps valid, so no test reaches
it), the session stays and the edit error shows. `Esc` or Cancel drops
the session and its draft whatever its state, and the model is asked
for again without it.

**The panel** (`view/src/extrude.rs`) floats at the viewport's right in
the operation panel (`view/src/operation_panel.rs`, meant to hold every
operation's panel, with the parts the extrude and the revolve share:
`Candidate`, `TypedField`, `OperationKind`, `BodyTarget`, the two-sided
extent's `Side` and `Sides` (its knobs and value fields), the region
rows, the choices, ticks, typed fields, Bodies list and footer message; the app's session
parts they share are in `app/src/doc/regions.rs`, see "UI" under
Revolve in `agents/features/revolve.md`): a card as the tool rail's (the UI
mock's `.opp`), a fixed head (the operation's icon, the title on one
line, clipped, and Cancel and OK as small square icon buttons), under
it a recessed well holding a body that scrolls (`PANEL_BODY`) when the
panel would run past the viewport's bottom margin and a fixed foot (the
message), so OK and Cancel show and take clicks however many bodies are
listed or however short the window; nothing is drawn past the panel
but its shadow, and a body's name, the field errors and the message
break inside words that don't fit (`Wrapping::WordOrGlyph`), the
message scrolling on its own past about five lines. Its body holds the
Profile, a field of the regions picked as rows (`Icon::SeRegion`,
"Region 1", a cross taking it out, `ExtrudeLook::PickRegion`) over
"Click regions" (or "Click regions to extrude", "No closed regions to
extrude"), always the one picking; the extents (Through all only while Cut is
chosen, else disabled with "Only a cut goes through all"; choosing
another operation while through all goes back to one side), the distance fields (the first is `VALUE_FIELD`, which
takes the focus as the session opens, all selected; `Esc` in any field
cancels, once: every field's `OnEscape` sends its cancel, but only where
nothing before it took the key, so the first takes it and the rest stay
quiet, in every panel of value fields), Flip for one side and two sides, Taper (a typed
angle, `ExtrudeLook::Taper`, read with `Extrude::taper_ask`, 0° to begin
with and stored as none, refused at 90° or more either way; the mock has
no taper, so it's a field in the style of the distances, after Flip;
the handle still drags only the distance), the operations, for Join, Cut and
Intersect a "Bodies" list with a checkbox per body (`BodyTarget`: the
draft's touched bodies as the newest answer of the current run of drafts
that ran the touch test gave them, `MeshFeed::draft_touched`: kept while a
changed draft is on its way, and while one fails before its tool exists
or makes a new body; a run starts when a draft is asked for after none,
or for another feature, and lists nothing until its first such answer,
so a new session never shows another's bodies; then the excluded ones,
and those ticked again (the session's `reticked`, each with the newest
draft revision given out when it was, `MeshFeed::revision`) until a
touch test of a later draft answers (`MeshFeed::draft_touched_revision`),
so a body taken out and put back doesn't drop out of the list while its
answer is on its way; a body that a join before the extrude merged
into another is never touched, so it's listed only while taken out or
just ticked again, with "in Body 1" after it (`BodyTarget::holder`,
faint): taking it out does nothing then (its material is in its
holder), and the row shows that and lets it be put back, after which
it drops out once the touch test answers. Which bodies are merged there
is `MeshFeed::merged_before`, replaying the joins before it that the
model shown has working and touching two or more, from
`touched_features` and `failed_features`, as `feed::Merges` by regen's
`note_merge`, so the two can't disagree on a prefix; the final `merged`
won't do, as a join after the extrude may consume a body it rightly
lists; all in the order they were made; ticked unless excluded;
`ExtrudeLook::Target` toggles, keeping the session's `excluded` sorted and
only taking bodies made before the extrude edited; bodies undone away
drop out, and aren't taken out again when redone: undo gives the ids
back, so a new edit may give theirs to other bodies), and under it, for a
join ticked for two or more (not counting one merged away before),
"Joined into Body 1", the first ticked (`extrude::joined_into`), which holds them all once it's committed; its foot the
refusal, the draft's error or "Checking the sketch…" (`Verdict::footer`), else the uncut note as its warning.
While the draft's error shows, OK and `Enter` wait, and the error shows
in a box (`Footer::Fails`): "Extrude fails" with a red alert on a red
wash, the error under it in muted words, and an "Add anyway" button in
the strong danger colour that commits anyway (`Edit::AcceptError`,
`Doc::commit_by`; every operation's panel, never a key), Show or Go
back left of it where the error has geometry (see `agents/viewport.md`): the feature is
kept with its error, marked failed in the Timeline, to fix later. The error is the one shown
(`MeshFeed::shown_draft_error`): the newest draft's, none while it's
unanswered, so OK doesn't wait on the preview. While a changed draft is
on its way, the failure of the run's draft before it stays in the box
until the answer or `feed::SLOW`, its Show (`shown_draft_geometry`) and
Add anyway (`Doc::offers_accept`; pressed before the answer it does
nothing, as `commit_by` takes the newest draft's error) with it, so
ticking a body or changing a value that fails again doesn't blink the
box. The viewport doesn't hold the failure's geometry.
Errors that stand alone, the field errors, the refusal and the draft's
error here and a failed feature's tooltip, are shown as sentences,
capitalised by the view (`chrome::sentence`): the messages themselves
(`varde_expr`'s, `ExtrudeError`'s, `regen::why`'s) start in lower
case to follow a colon, as in the status bar's "Couldn't regenerate: …".
The Bodies list has no scrollable of its own: the body scrolls as a
whole, keeping its offset while rows come and go (clamped to what's left,
so no empty stretch shows) and while the handle's knobs come and go (the
labels' layer stays, empty, so the panel's keeps its place in the
viewport's stack and its state), and the wheel over it scrolls it rather
than the camera. Giving the first field the focus (a session started, or
another extrude edited while a panel is open) also scrolls the body back
to its top if the field shows from there, else down to the field and its
label (`RevealField` in `app/src/tasks.rs`). The panel starts 150 px down
(`PANEL_TOP`, clear of the camera controls) unless the viewport leaves
it less than 200 px (`PANEL_ROOM`) below that; then it rises, at most to
12 px from the top, over the controls (`operation_panel::placed`). Where
even the head and foot don't fit, the head keeps its height and the
foot gives way. Below about 580 px of window width (the
side panel's 256, the panel's 288 and its margins) the panel narrows
with the viewport and its text wraps; in a much narrower window its
buttons squeeze away, as the window has no minimum size. The handle and region picking are in `agents/viewport.md`.
Dragging a knob types its distance (one side past the plane flips; a knob
on the plane changes nothing) as the design's units format it. A knob
stops where its field would refuse the distance, or where the extrude's
own check would refuse the extrude and didn't before (two sides together
over `MAX_COORD`; one side and symmetric only stop where the field
would). Two sides typed over the limit already, a knob only moves back
towards it (`drag_side` in `app/src/doc/regions.rs`, the revolve's
too). If the design's units change while the session is open, each
distance's value is pinned as the document pins its own
(`Value::pin_units` with the units it was read in,
`ExtrudeSession::follow_design`, from `prune_extrude` as the change is
applied), so a bare "20" typed in millimetres becomes "20 mm" rather
than disagreeing with the document, and OK, checked against the
document's design, isn't blocked by the change; a refused text stays as
typed.

**The Timeline** (`view/src/panels.rs`) shows an extrude with the extrude
icon and its distances as the note, in the design's units
(`extrude::extent_note`: "10 mm", "10 mm symmetric", "10 mm + 5 mm", "Through
all"), a taper after it (`extrude_note`: "10 mm · 2°"; the status bar
"Distance 10 mm · Taper 2° · New body"). A tapered extrude's preview
fails until the kernel's taper is built ("a tapered extrude isn't
supported yet"): OK waits and Add anyway keeps it,
failing in the Timeline. A feature in the answer's `failed` (which `MeshFeed` keeps with
the model shown, `failed_features`) has its name in the danger colour
and tells why in a tooltip; a sketch that doesn't solve is marked as
before. Neither is given out from a model of before a replacement of
the whole document (`MeshFeed::replaced`), whose ids may name other
features. Double-clicking an extrude opens its session. Right-clicking
a row selects its feature and opens its context menu (`Look::OpenMenu`
with `RowMenu::Feature`, `Doc::row_menu`; the widget is
`view/src/context_menu.rs`, an overlay at the click, the scrolled list's
offset allowed for): Edit sketch or Edit extrude, for a sketch Hide or
Show and Change plane, and Delete, all but the edit only if the document
is editable. The rows of Objects have one too (`RowMenu::Body`,
`RowMenu::Sketch`), selecting the row alone as a click would unless
it's already part of the selection, which is then kept: Edit sketch for a sketch,
Hide or Show where the row has an eye, a body's Opacity (not a merged
one's, drawn as its holder is), and Delete. The context menu key or `Shift F10`
(`DocumentKeys::menu`, `Doc::menu_target`) opens the menu of what's
selected (`Look::KeyMenu`): the Timeline's feature, else the last
sketch selected in Objects, else the first body of the model
selection, in a sketch the first point or curve selected, showing the
row's tab; a menu not opened by a right-click shows below its row's
left end. Opacity is a slider from 10
to 100 % in steps of 5 with the percentage beside it, wrapped in
`MouseOnly` (`view/src/mouse_only.rs`) so it's only dragged: iced's
slider also steps on arrow keys and `Ctrl`-wheel, with no release to
commit after. Dragging sends `Look::PreviewOpacity` (the value through
`Opacity::clamped`), kept in `Doc::body_look.opacity` while the body's
menu is open and the document editable, and drawn and shown in the row
in place of the body's own (`DocumentState::opacity_preview`,
`shown_opacity`) without touching the document. Until it's let go the
shortcuts are off (`Doc::keys` is `None`) and the peek key doesn't swap
the tab (`Doc::peeks`), which would take the slider and its release with
it. Letting go sends `Edit::CommitOpacity`, one `Command::SetOpacity`
(none if unchanged), and leaves the menu open. While the body shown
isn't opaque, a reset icon at the right of the Opacity heading sends
`Edit::ResetOpacity`: one `Command::SetOpacity` to 100 %, dropping any
preview and leaving the menu open too. Below it, likewise for an own
body only, Colour: a heading with a wide swatch of the colour as drawn,
"Default" beside it (and dashes for the values) while the body has no
colour of its own (`theme::body_swatch`, the theme's
model colour tinted), a Hue slider (0 to 359°) and a Saturation one (0
to `Tint::MAX_SATURATION`, 30 %, so bodies stay duller than the
selection and hover), `MouseOnly` too, previewing with
`Look::PreviewColor` into `Doc::body_look.color` (`DocumentState::color_preview`,
`shown_color`) and committing on release with `Edit::CommitColor`, one
`Command::SetColor`; dragging the hue of a body with no saturation
starts it at `Tint::DEFAULT_SATURATION`, 17 %, so it shows; with no
colour of its own the Saturation slider shows that too, so its handle
doesn't jump. While the body has a colour, the
Colour heading's reset icon sends `Edit::ResetColor`, one
`Command::SetColor` to none. The Hue and Saturation sliders lie over
rails of their colours (`color_line`, `theme::color_rail`, a gradient
inset by the handle's radius, so a colour sits under the handle that
picks it): the hue's round the wheel at the most saturation, whatever
the saturation, the saturation's from grey to the most at the hue;
their own rails are left out and their handles filled with the colour
(`theme::color_slider`). The sliders run the menu's width (a body's
menu is `BODY_MENU_WIDTH`, wider than other rows' menus), each with its
value beside it. Opacity and Colour (`look_part`) sit in one area
(`look_area`) that sends `Look::HoverBodyLook` on entering and leaving
it, kept in `Doc::body_look.hovered` (dropped with the menu) and washed
as one hovered row; while it's hovered or a slider dragged, `Doc::highlight` is none, so
the body shows as drawn without the selection over it. Either
preview keeps the shortcuts and the peek off as the opacity's does
(`Doc::sliding`). The previews go whenever
the menu does (`Doc::prune_preview`), so `Esc` mid-drag drops them. `Esc` or a
press off a menu closes it alone; any other message closes it too, and
it goes with what its row lists. Outside a
sketch or a session the floating status bar (`view/src/status.rs`, as
the mock's; see `agents/viewport.md`) shows the feature selected in a
box of its own, its icon, name and `feature_info` (`view/src/feature_text.rs`, which
hands off to `extrude::extrude_info` and the other kinds' own:
"Distance 10 mm · New body", "Symmetric 4 mm · Cut", a sketch's
"4 lines · 1 circle · 5 points · on XY"), and nothing of the model with
none; then regenerating, failures and saving, cut short before the
hints, so a long failure never pushes them off the screen. The bodies
are counted as the joins leave them, a merged one with its holder, in
the Objects group's count (`panels::bodies_after_joins`).

**Deleting** (`app/src/doc/delete.rs`): `Edit::RemoveFeature` (`Delete`
on the Timeline's selection) and `Edit::RemoveBody` (Objects' bin) ask
`Document::removal` what goes. Joins, cuts and intersects don't depend on
the bodies they touch (they're found again when regenerating), so a
body's removal takes only its maker (and what uses that); a later join
left touching nothing fails in the Timeline. So the prompt warns of
them: every join, cut or intersect that stays and touched only bodies
that go (one that also touched a body that stays goes on working on
that), as the model shown found (`Response::Regenerated::touched`, the
history's `Evaluation::touched`, kept by `MeshFeed::touched_features`
like the failed features, and not given out across a replacement until
a newer model; one the model shown doesn't know, such as a cut committed
before its model comes back, is taken to touch every body made before
it that it doesn't take out, so it's warned of when they all go, as it
then surely has nothing to work on; a body that holds bodies joins
before the feature merged into it counts as those too, replayed as for
the Bodies list, since one of them staying takes its place: deleting
Body 1 that Body 2 was merged into leaves a later cut of Body 1 working
on Body 2), is listed under the
prompt in the warning colour (the mock's for panel warnings, its construction orange): "Extrude 2 works on Body 1 and stays, so
it may fail with nothing to work on." (`Doc::worked`, `delete_warning`).
One that took the body out isn't (excluded bodies aren't asked
about, so it touched none). A removal with
such a feature asks even if only its own feature goes, so a body's
prompt shows. For a body the prompt asks
"Delete *Body N* with the M features and K bodies that go with it?",
counting its maker and the other bodies that makes, or, when only its
maker goes with it, "Delete *Body N* and *Extrude M*, which makes it?". If that's one
feature (a feature and its own bodies, or a body and the feature making
it) and nothing's warned of, the command applies at once. Otherwise the app keeps a `Deleting` (the target, the `Removal`,
the editor's generation) and the view shows `DeletePrompt` over the
whole screen, on the unsaved-changes prompt's scrim: "Delete *name* with
the N features and K bodies that depend on it?" (`delete_question`,
leaving out a count of none), the features in timeline order with
their icons, then the bodies, scrolling past about ten rows, Cancel
(`Look::CancelDelete`, also `Esc`: `Doc::dialog` tells the escape key
which prompt is up, the unsaved one first) and Delete
(`Edit::ConfirmDelete`, danger style, no `Enter`). While it's up
`Doc::keys` is `None`, so no shortcut acts behind it, and the status
bar's hints are only "Esc Cancel". Delete applies the
same command, so exactly the listed set goes, one undo step. The prompt
is dropped in `sync` once the generation moves (undo, recovery), and a
stale one is neither shown nor applied. A delete asked for while edits
wait on the solver waits behind them and asks when made, in its turn:
nothing behind it moves until it's answered, and Delete makes it at
once (`agents/sketch/editing.md`, Proposals). Confirmed while edits wait
otherwise, the delete waits behind them too, and is made then if the
same set goes, else asks again (`Doc::remove_now`). A read-only
document asks nothing and deletes nothing.

When the removal takes features beyond the target's own
(`DeletePrompt::keeping`: `Document::breaking_removal` takes fewer
features than `removal_of`), the prompt offers Delete only beside
Delete all (the danger button, `Edit::ConfirmDelete`), with a note
under the list: "Delete only keeps the features that depend on
*Sketch 1*; they fail until they're given another."
(`delete_only_note`, naming a body's maker for a body).
`Edit::ConfirmDeleteOnly` applies `Command::RemoveOnly`: the target (a
body's maker for a body) and the bodies it makes, one undo step,
waiting behind solver edits as Delete does and asking again if that
set changed by then. The features kept name a sketch or body that isn't
there, which `Document::check` takes (as a sketch's face: the id below
`next_id`), and regenerating fails them ("its sketch isn't there", "a
body it names isn't there", from `own_solids`). Edited, a combine lets
go of the bodies that aren't there (`CombineSession::prune`), a move
and the like shows them gone, and a region pick whose sketch is gone
(`RegionPick::editing`) has no source, so another sketch's regions may
be picked, which drops the old references listed as missing.

**Tolerance** (`view/src/toolbar.rs`): the file menu has a "Tolerance"
heading under Units with 0.1 µm, 1 µm and 10 µm, each sending
`Edit::SetTolerance` (`Command::SetTolerance`, one undo step, which
regenerates everything as the cache keys hold the tolerance). A value
from a file that isn't one of those shows as a fourth, unticked row,
named in µm, or exactly in mm where its rounded name would be one of the
offered ones' (`tolerance_choices`).
