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

- Constrained sketches with snapping, typed sizes and dimensions that take
  expressions with units (`1 in + 3 mm`), solved as you edit.
- Sketch tools: trim, extend, offset, mirror, fillet and chamfer, and
  projecting model geometry into a sketch, kept in step as the model
  changes.
- Extrudes and revolves that make, join, cut or intersect bodies, in an
  editable timeline, on planes or flat faces.
- Combining, moving, mirroring, patterning, aligning and scaling bodies.
- Exact solids: rational quadratic patches with conic edges, so planes,
  arcs, cylinders and cones are exact and booleans are robust.
- Measuring faces, edges, points and bodies, and the distance and angle
  between two.
- 3MF export for 3D printing.

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
