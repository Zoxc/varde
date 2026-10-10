# Parameters

`crates/expr/src/params.rs`, `crates/document/src/param.rs`; the UI in
`crates/view/src/params.rs` and `crates/app/src/doc/params.rs`; names
offered as they're typed in `crates/view/src/suggest.rs`. Sketch
dimensions take them too (`agents/sketch.md`, Dimensions and
parameters).

- **Expressions.** A word (`[A-Za-z_][A-Za-z0-9_]*`) right after a
  number or a `)` is a unit, or an unknown one (`2 yd`); anywhere else a
  word that isn't a unit is a parameter's name (`Node::Name`). Unit
  words (`mm cm m in ft deg rad`, any case) are never names
  (`check_name`, `NameError`). `names(text)` gives the names' spans,
  lexing on past what doesn't lex (skipping it), so a text in error
  still shows every name it uses to `uses(text, name)`, whether it names
  one, and `rename(text, old, new)`, which rewrites them, refused past
  `MAX_LEN`. A unit after a name (`count mm`) is `UnitOnParam(name)`
  ("a parameter takes no unit after it: multiply by one, as 'count * 1
  mm'"), not `UnitOnUnit`.
- **Resolving.** `Params::evaluate(list, units)` resolves a design's
  `(name, text)` list in any order, depth first without recursion
  (`MAX_PARAMS`, 1000, bounds it; names found through a map built
  there, `Params::index`), each to a `Resolved { value,
  quantity }` or an `Error`: its text's own, `Cycle` for one using
  itself through others (each in the cycle, the span the use leading
  round), `BrokenParam(name)` for one using a parameter in error,
  `BadName`, `Duplicate` (the first of a name is the one used),
  `TooManyParams`, `ParamKind` for an area or a mix of units. A
  parameter is evaluated without an asked quantity: it's a length, an
  angle or a number, whichever it comes to, and bare numbers alone are a
  number (`height = 50` used as a length is refused, "that's a number,
  not a length"; `50 mm` is a length). Bare numbers beside a length
  inside one take the design's units, as in any value.
- **Use.** An `Ask` carries the parameters names are looked up in
  (`Ask::with_params`; `Params::EMPTY` unless given, so it stays
  `Copy`). A use is never bare, so it takes no units and gets no pins; an
  unknown name is `UnknownName` with a near name suggested (the same but
  for case, or two edits; `Params::nearest` skips names whose length
  differs by more and fills only the band of the edit table within the
  edits allowed, stopping once past them), one in error `BrokenParam`.
  While `Params::evaluate` runs, unknown names suggest nothing; after,
  the parameters' own errors get theirs within `SUGGESTION_BUDGET`
  (20 000 names compared, all told), so a design full of unknown names
  still evaluates quickly; a value read later compares each name once. `Design` carries
  the document's resolved parameters (`Document::design`), and every
  kind's `*_ask(design)` passes them on, so `Value::check` in
  `Document::check` reads names: a value using a parameter is checked to
  come to its stored value with the parameters as they are, and one
  using a parameter in error fails. Sketch dimensions use them as any
  value does: `Measure::ask(design)` passes the design's on, so
  `Sketch::check` (in `Document::check`, the solve lane and its worker)
  reads them.
- **Document.** `Document::params` (`Param { name, text }`, a field
  stored as `file-format.md` says) and `Document::params_resolved`, a
  cached `Arc<Params>` kept in step with the list and the units (resolved
  again as a document is read, on every parameter command and `SetUnits`;
  `Document::check` asserts it in debug builds). `params_shared` hands
  the `Arc` to what reads values with it while the document changes: the
  app's feature sessions keep a `ReadIn` (units and parameters) for their
  fields; a session's `ReadIn::follow` hands back what it read in
  before when the units or the parameters change, and
  `TypedText::follow` pins the units as before and reads a text naming
  parameters again, so `height / 2` follows `height` (or is refused with
  why) while the session is open. Queries for the panel:
  `param_uses(name)` (features with a value naming it, a sketch for a
  dimension naming it), `param_users(name)` (other parameters naming
  it), both for every parameter at once in `all_param_uses()`
  (`ParamUses`), `param_used`, `new_param_name(base)`.
- **Commands**, one undo step each, nothing changing for an index that
  isn't there or a text or name as it is:
  - `AddParam { name, text }` adds one last (text trimmed). The name is
    checked (`CheckError::Param(index, ParamError::Name | Duplicate |
    TextLength)`); the text may be in error, nothing using it.
  - `SetParam { index, text }` sets its text, then resolves the list
    again and evaluates every feature value naming a parameter again
    (live link: values keep their text, their value follows), the
    features changed set through `SetFeature`'s path (`Document::
    with_kind`), so a pattern whose count follows a parameter gets or
    loses its copy bodies. A value that would be in error, directly or
    through other parameters, refuses it: `EditError::Value(ValueOf::
    Feature(id), error)` (a sketch's id for a dimension's). Sketches
    whose dimensions name parameters are evaluated again and solved
    there and then (`varde_sketch::revalue`, below), in the same
    command, so one undo step puts back the parameter and the geometry;
    one that won't solve refuses it, `EditError::Unsolved(id, why)`
    ("Sketch 2 wouldn't solve: ..." in the popup).
  - `RenameParam { index, name }` renames it and rewrites every use, in
    feature values, sketch dimensions and other parameters' texts (`EditError::Value(
    ValueOf::Param(i), TooLong)` where that runs past `MAX_LEN`).
  - `RemoveParam(index)` is refused (`EditError::ParamUsed(name)`) while
    a feature value, a sketch dimension or another parameter uses it.
  - `SetUnits` pins parameters' texts too (`Params::pin_units`, with
    the units before; one the units would take too long becomes its
    value, `varde_expr::exact`); a parameter in error is left as typed.
- **UI.** A popup as the model mock's (`model.html#bracket/params`),
  opened and closed only by the Parameters tool, last in the model
  rail's Modify set (`Entry::Params`, no key, `Look::ToggleParams`),
  the model bar's Parameters button (last, after Measure, with nothing
  selected; lit while the popup is open; at 1280 px the bar cuts it
  off), and its Close; `Esc` leaves it, as the mock's does. It's styled as an operation's
  panel (`operation_panel::Sections` without the well; head with the
  icon, "Parameters", the count with an alert while any is in error,
  Close) and shows in sketches too. The table: header row, a banded row
  each (name field, boxed expression field, value muted in the design's
  units or a dash, "Used by" the features then the parameters naming
  it or "Not used", delete shown on hover), a note under a row in
  error (its expression boxed red, a danger mark down its left edge),
  an accent mark on rows the feature selected in the Timeline uses,
  and "+ Parameter" under them; it scrolls when long. The app keeps
  per-field drafts (`ParamDraft`) of the list as it was (`basis`; an
  undo or redo that changes the list drops them): `Enter`, typing in
  another field, or anything else done but hovering, scrolling and
  moving the camera (`rename::passive`) commits the field typed in,
  `Esc` in a field puts it back. A refused commit keeps the draft with
  why (`refusal`: "Extrude 1 would be in error: ...", "Another parameter
  has this name", the name's error), not in the status bar. Add makes
  `new_param_name("param")` = `10 mm` and focuses its name (selected).
  Delete of one in use is refused in the app with what uses it, under
  its row. Each commit is one command, so one undo step; regeneration
  follows. While edits wait on the solver it waits behind them
  (`Change::Param`) and its draft stays as typed: once it's made
  (`Doc::make_param_now`), `Doc::param_answered` drops the draft, or
  keeps it with why (only if its text is still the command's, as one
  typed meanwhile is newer); a removal shifts the drafts after it only
  once made, and a refused one says why under its row. The "Used by"
  column comes from `all_param_uses`, which the app keeps per editor
  generation while the popup is open (`Doc::refresh_param_uses`, from
  `sync`), not found every frame (`ParamsState::uses`).
- **Suggestions** (`crates/view/src/suggest.rs`), as the mock's
  (`suggestBox`, `.psug`). An operation panel's typed fields
  (`operation_panel::value_field`, `TypedField::params`) and the sketch's
  value field (`panels::value_field`, `ValueField::params`) carry the
  design's parameters and units (`ParamsIn`, `Doc::params_in`).
  `suggest(text, params)` offers the names starting with the word the
  text ends with (a word starting with a letter or `_`; "10mm" is a
  number and a unit; none where a unit goes, right after a number or a
  `)`), not the word itself, at most `MAX_SUGGESTIONS` (6),
  each with its value as the popup shows it ("error" for one in error).
  The view sees the text, not the caret, so the word is the text's last;
  the `Suggesting` widget wraps every field with an input whether or not
  names are offered (the field its first child), so its focus and caret
  outlive names coming and going, and shows the list (an overlay
  under it, at least its width, the first lit, "Tab puts it in") and
  takes Tab only while the field has the focus with its caret at the
  end (iced's `text_input::State`). Tab or a click on a name sends the
  field's input with the name in place of the word and moves the caret
  to the end; the overlay takes the press, so the field keeps the
  focus. Otherwise Tab is the app's as before (next field, or backing
  out).
