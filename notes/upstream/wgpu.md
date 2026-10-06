# wgpu

## GL on X11 panics with "Invalid surface" when a Wayland socket exists

**Version:** wgpu 27.0.1 / wgpu-hal 27.0.4, pulled in by iced 0.14.

**What was hit.** Running the app on X11 while a Wayland compositor is also
running (e.g. `env -u WAYLAND_DISPLAY DISPLAY=:0 cargo run` in a Plasma
Wayland session) panics at startup:

```
wgpu error: Validation Error
Caused by:
  In Surface::configure
    Invalid surface
```

It only happens on the GLES backend, which wgpu picks on this VM because the
only Vulkan device is llvmpipe (VMware SVGA GPU).

**Cause.** In wgpu-hal 27, `gles::egl::Instance::init` chooses the EGL
platform before it sees any window. `test_wayland_display()` calls
`wl_display_connect(NULL)`, which falls back to the `wayland-0` socket when
`WAYLAND_DISPLAY` is unset. That socket exists, so the EGL display is created
for Wayland, and the X11 window's surface can't be configured on it.

**Why it's not ours.**
- It reproduces at `8d405bb`, before any of the UI work.
- It happens inside iced's surface setup (`Surface::configure`), with no varde
  code involved.
- With `WGPU_BACKEND=vulkan` it runs.
- With the Wayland socket unreachable (`XDG_RUNTIME_DIR` pointed at an empty
  directory) the X11 path runs too.

**Cost here.** None: no workaround in code. A real X11 session has no Wayland
socket and works. To force X11 from a Wayland session, also set
`WGPU_BACKEND=vulkan` or point `XDG_RUNTIME_DIR` somewhere without a Wayland
socket.

**Upstream status.** Fixed in wgpu-hal 29: the EGL platform is chosen from the
display handle the instance is given (`Rdh::Wayland` → Wayland platform), not by
probing for a socket. It will arrive with an iced release on wgpu ≥ 29. Not
reported, since it's fixed upstream.

## Parallel instances crash in the Vulkan loader

**Version:** wgpu 27.0.1 / wgpu-hal 27.0.4, on vulkan-loader 1.4.341 (Fedora
44) with Mesa 26.1.8's llvmpipe as the only Vulkan device.

**What was hit.** `cargo test -p varde-render --test viewport` died with
SIGSEGV in about one run in four (5 of 20), with every test passing
otherwise. Each GPU test made and dropped its own `wgpu::Instance` and
device, and the test harness runs them on parallel threads.

**Cause.** Under gdb, one test thread crashed in the loader's
`loader_get_icd_and_device`, reached from `vkSetDebugUtilsObjectNameEXT`
(wgpu-hal naming objects while `Renderer::new` built its pipelines), at
the same moment another thread was creating its own instance. The loader's
lookup from a device to its driver isn't safe against another instance
being created or destroyed concurrently.

**Why it's not ours.**
- The crash is inside the loader, called from wgpu-hal; varde only asked
  wgpu for pipelines, on its own device, as any user of wgpu does.
- With `--test-threads=1` it didn't happen in 20 runs.
- The app itself never has more than one instance, so it's only the tests
  that create them in parallel.

**Cost here.** Each GPU test binary makes one instance and device, once, in
a `static OnceLock`, and its tests share it: `crates/render/tests/viewport.rs`,
`crates/render/tests/limits.rs` and `crates/view/src/viewport/tests.rs`
(`recursion_limit` raised in the render tests for the `Sync` check on the
static). 30 runs of each with no crash. If a later loader is fixed, tests
could go back to their own devices, but sharing one is cheaper anyway.

**Upstream status.** Not reported. Whether it's the loader's or wgpu-hal
calling a device-level function while another instance is torn down wasn't
settled; a reduced reproduction (two threads creating instances and naming
objects) would tell, and is what a report needs.

## GL caches programs without their pipeline constants

**Version:** wgpu 27.0.1 / wgpu-hal 27.0.4, pulled in by iced 0.14.

**What was hit.** In the browser (WebGL2), the extrude's regions and handle
showed through the preview body, though they're drawn depth tested there:
the depth tested pipelines drew exactly as the on-top ones.

**Cause.** wgpu-hal's GLES device keeps a program cache keyed by each
stage's shader module, entry point and workgroup-memory flag, and the bind
group slots (`gles::ProgramCacheKey`), but not the pipeline constants
(overrides), which it bakes into the GLSL. The sketch's on-top and depth
tested pipelines use the same module and entry points and differ only in
the `SKETCH_DEPTH` override, so the second got the first's program.

**Why it's not ours.**
- Natively on Vulkan the same pipelines are depth tested
  (`crates/render/tests/viewport.rs`).
- On wgpu's native GL backend they aren't, the same as in the browser
  (`crates/render/tests/gl.rs` fails without the workaround).
- `ProgramCacheKey` in `wgpu-hal/src/gles/mod.rs` has no field for them.

**Cost here.** The renderer makes a second shader module from the same
source for the depth tested pipelines (`Renderer::new`), whose own module
id keys them apart. Any later pair of pipelines that differ only by an
override needs the same, until upstream keys the cache by the constants.

**Upstream status.** Not reported, not checked against a newer wgpu.

## GL resolves MSAA only inside the pass's last scissor rect

**Version:** wgpu 27.0.1 / wgpu-hal 27.0.4, pulled in by iced 0.14. The code is
unchanged in wgpu-hal 29.0.4.

**What was hit.** In the browser (WebGL2), a trail of old view cubes, each at
an earlier camera angle, drawn in a row along the top of the viewport over
the model and the rail's open list. It showed after leaving a sketch, while
the camera turned back and the viewport's width changed, moving the cube.

Hit again natively on GL (this VM's backend): the Timeline's rollback
marker, then drawn by two small canvases (its diamond and its line) in the
scrolled list, left a copy of its line under every row it was dragged
past, and its diamond never showed, not being the last mesh drawn. The
headless shots, on Vulkan, drew it right.

**Cause.** iced draws canvas meshes into a shared 4× MSAA target with a
resolve texture, setting a scissor rect per mesh, then composites the whole
resolve texture onto the frame. wgpu-hal's GLES backend resolves at the end
of the pass with `glBlitFramebuffer` over the full render size
(`C::ResolveAttachment` in `wgpu-hal/src/gles/queue.rs`), but `C::SetScissor`
left `GL_SCISSOR_TEST` enabled, and the blit honours it. Only the last
mesh's rect (the cube's) is resolved; the rest of the resolve texture keeps
earlier frames' cubes, which iced then composites over everything under the
cube's layer. Vulkan resolves the whole attachment.

**Why it's not ours.**
- Reproduced headlessly with the shot harness (`crates/app/src/doc/extrude/tests/shots.rs`):
  example document, a sketch entered and left, the Create list open, then 12
  frames orbiting while the window widens from 840 to 1280. With
  `WGPU_BACKEND=gl` the trail shows; on Vulkan the same frames are clean.
- No varde code is in the path: the cube is a plain iced `canvas`.
- The blit in `C::ResolveAttachment` has no `disable(SCISSOR_TEST)` before
  it.

**Cost here.** The rollback marker (`crates/view/src/rollback.rs`) is drawn
without canvases: its line a container, its diamond an SVG. Its drag
handling is still a canvas over the list, but one that draws nothing. The
view cube has no workaround: it shows on the web.
The fix is one line in wgpu-hal, `gl.disable(glow::SCISSOR_TEST)` before the
blit (every pass sets its own scissor again in `begin_render_pass`), which
would need a patched wgpu-hal until upstream has it. The alternative,
`antialiasing: false` on wasm (`crates/app/src/lib.rs`), would make the
cube's edges and letters jagged in the browser.

**Upstream status.** Not reported. A report needs a reduced reproduction:
a 4× MSAA pass with a resolve target, drawn under a scissor rect that moves
between frames, then the resolve target read back on `WGPU_BACKEND=gl`.
