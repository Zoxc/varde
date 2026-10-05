<p align="center">
  <img src="assets/logo.svg" alt="Varde logo" width="96" height="96">
</p>

<h1 align="center">Varde CAD</h1>

<p align="center">
  Experimental parametric CAD in Rust, for the desktop and the browser.
</p>

---

Varde is a parametric 3D CAD application: parts are built from constrained
sketches and editable features. It runs natively and in the browser.

> **Experimental.** Early work in progress; it may lose your work.
>
> **Unstable file format.** `.vrdp` files may not open in other versions.
>
> **Vibe coded.** Nearly all code is written by AI agents, steered by a
> human.

## Features

- Sketches with lines, rectangles, circles, arcs, polygons, splines and
  points, snapping and typed sizes.
- Constraints and dimensions, solved as you edit; dimensions take
  expressions with units (`1 in + 3 mm`).
- Trim, extend, offset, mirror, fillet and chamfer.
- Projecting model edges, corners and other sketches' geometry into a
  sketch, and cutting faces and edges with its plane, kept in step as
  the model changes.
- Extrudes (tapered or straight) and revolves that make bodies or join,
  cut or intersect them, editable in a timeline.
- Sketches on flat faces that follow the face as earlier features change.
- Combining bodies: one body united with, less, or intersected with others.
- Filleting and chamfering a body's edges, shelling a body to thin
  walls, moving a body's faces in or out (offset face, by a handle or a
  typed distance) and drafting faces by an angle from a neutral plane,
  set up and kept in the timeline (their geometry isn't built yet).
- Sweeping a sketch's regions along a path of other sketches' curves and
  model edges, or round a helix, set up and kept in the timeline (its
  geometry isn't built yet).
- Lofting through sketch regions and points in order, smooth or ruled,
  closed or along rails, set up and kept in the timeline (its geometry
  isn't built yet).
- Moving, mirroring and patterning bodies, in a row or round an axis,
  aligning one onto another by points and directions, and scaling them.
- Faces, edges, vertices and bodies selected by clicking them with no tool
  open (a double-click takes the body); `Shift`- or `Ctrl`-click (`Cmd` on
  macOS) adds or removes one, `Space` or a click on an empty part of the
  side panel or toolbar clears. Holding the button still where faces,
  edges or vertices overlap (or a sketch's points and curves) lists
  them to pick the right one.
- A body's opacity, set in its context menu in the side panel, to see
  what's behind it.
- Edges hidden behind bodies shown dashed; "Hidden edges" in the view
  options turns them off. Its Edges submenu draws every patch's edges
  too, faint ("Wireframe"), or every triangle's ("Tessellation").
- Shaded, flat shaded, metal or flat metal looks for the model, in the
  view options' Shading submenu.
- Solids of rational quadratic triangle patches: closed meshes to allow for
  robust booleans, but with conic edges, so planes, arcs, cylinders and
  cones are exact. Other shapes, such as splines, tori and curved-on-curved
  cuts, are fitted to a tolerance.
- Measuring faces, edges, points and bodies, and the distance and angle
  between two, in the design's units, ready to copy.
- Export of the visible bodies as a 3MF file for 3D printing.

## Getting started

Try it in the browser at
[zoxc.github.io/varde](https://zoxc.github.io/varde/).

To run it natively, with a Rust toolchain ([rustup](https://rustup.rs))
installed:

```sh
cargo run --release
```

For the browser build and tests, see [BUILDING.md](BUILDING.md).

## License

GNU Affero General Public License, version 3 or later. See
[LICENSE.txt](LICENSE.txt).
