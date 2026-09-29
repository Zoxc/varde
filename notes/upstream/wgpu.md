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
