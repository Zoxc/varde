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
