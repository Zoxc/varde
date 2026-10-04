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
- Extrudes and revolves that make bodies or join, cut or intersect them,
  editable in a timeline.
- Sketches on flat faces that follow the face as earlier features change.
- Combining bodies: one body united with, less, or intersected with others.
- Moving, mirroring and patterning bodies, in a row or round an axis,
  aligning one onto another by points and directions, and scaling them.
- Faces, edges, vertices and bodies selected by clicking them with no tool
  open (a double-click takes the body); `Shift`- or `Ctrl`-click (`Cmd` on
  macOS) adds or removes one, `Space` or a click on an empty part of the
  side panel or toolbar clears.
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
