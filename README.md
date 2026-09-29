# Varde CAD

A CAD application written in Rust, on [iced](https://iced.rs) and
[wgpu](https://wgpu.rs). It runs as a desktop app and in the browser.

Varde is early work in progress. Today it opens, edits and saves designs made
of simple solids and shows them in a 3D viewport. Sketches, constraints,
extrude and booleans are still to come, and the file format may change
between versions without a way to read older files.

To build and run it yourself, see [BUILDING.md](BUILDING.md).

## Using it

The app opens on a welcome screen: start a new design (`N`), open one (`O`),
reopen a recent file, or recover unsaved changes left by a crash.

| action                      | input                                      |
|-----------------------------|--------------------------------------------|
| orbit                       | left drag                                  |
| pan                         | right or middle drag                       |
| zoom                        | mouse wheel                                |
| peek at the other side tab  | hold `Alt` (`Option` on macOS)             |
| save / save as              | `Ctrl S` / `Ctrl Shift S` (`Cmd` on macOS) |
| close the file menu         | `Esc`                                      |

Designs are saved as `.vrdp` files. Unsaved changes are auto-saved a few
seconds after you stop editing, to a hidden `.<name>.vrdp.autosave` file next
to the design, and offered back if the app crashes. A design that is already
open in another window opens read-only.

In the browser, Chromium-based browsers open and save your files in place.
Firefox and Safari open a copy and save by downloading. Auto-saves are kept in
the browser's site storage, and there is no recent files list yet.

## License

Varde is licensed under the GNU Affero General Public License, version 3 or
(at your option) any later version. See [LICENSE.txt](LICENSE.txt).
