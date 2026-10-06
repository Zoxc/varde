# iced

## Wanted

### The system's light or dark mode before the first frame

**Version:** iced 0.14 (iced_winit 0.14.1).

**What's wanted.** The mode the system prefers, readable while the app boots, or
handed to the `theme` function, so that a custom theme which follows the system
draws its first frame in the right mode.

**What iced does.** On Linux (`linux-theme-detection`), iced_winit asks the
system for its colour scheme with `mundy::Preferences::once_blocking`, waiting at
most 200 ms, before it creates any window (`iced_winit/src/lib.rs`, `run_instance`). So iced has the answer in time. But the app only hears it through
`iced::system::theme()`, a task whose answer arrives after the window's first
frame. The only thing iced does with the mode itself is `Base::default(mode)`, for
a `theme` function that returns `None`, which gives iced's built-in Light or
Dark rather than the app's palette.

**What the app does instead.** With the theme set to `auto` and a dark system,
the window opens light for a frame or so and then turns dark. The app reads its
stored theme before the window draws (`varde_io::settings::read_now`, used in
`Varde::boot` in `crates/app/src/lib.rs`), so `light` and `dark` open right; only
`auto` flashes. Asking `mundy` from the app as iced does would fix it, at the
cost of a dependency, and wasn't done.
