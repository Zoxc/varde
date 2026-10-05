# Sample designs

Designs to open in Varde. Every sketch in them is fully constrained and
dimensioned, so dimensions can be changed to see the part follow.

- `mounting-plate.vrdp`: an 80 x 50 x 8 mm plate with 8 mm rounded corners,
  a 6.5 mm bolt hole at each corner and a 30 x 10 mm slot cut through the
  middle. A sketch with arcs tangent to lines, an extrude, and a
  through-all cut.
- `knob.vrdp`: a knob turned about the Z axis from a half section drawn on
  XZ: a 36 mm disc with a rounded shoulder and a 12 mm stem, and a 6 mm
  blind shaft hole 20 mm up from the bottom. A revolve, and a blind cut.
- `pillow-block.vrdp`: a bearing pillow block. A 100 x 50 x 10 mm base with
  rounded corners and bolt holes; a housing ring revolved from a section
  on XZ as a body of its own, then combined (union) onto the base; two
  ribs drawn on YZ with spline backs, held mirror images by symmetry
  constraints, joined on with a symmetric extrude; and a 16 mm bore cut
  through all. Sketches on all three origin planes.

They're written by `crates/view/examples/samples.rs`, which also checks
them (sketches valid and fully constrained, regeneration without
failures, the files reading back), with the thumbnail a save in the app
would write, rendered offscreen; it needs a GPU adapter, and writes
nothing without one. To write them again:

```sh
cargo run -p varde-view --example samples
```
