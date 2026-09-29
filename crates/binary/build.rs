//! Embeds `assets/logo.ico` as the executable's icon on Windows.
//!
//! The `.ico` is rendered from `assets/logo.svg` (rsvg via ImageMagick, then
//! packed with PNG entries by Pillow) and checked in:
//!
//! ```sh
//! for s in 16 24 32 48 64 128 256; do
//!     magick -background none -density $((s*4)) logo.svg -resize ${s}x$s PNG32:logo-$s.png
//! done
//! python3 -c 'from PIL import Image; i = [Image.open(f"logo-{s}.png") for s in (16, 24, 32, 48, 64, 128, 256)]; i[-1].save("logo.ico", sizes=[x.size for x in i], append_images=i[:-1])'
//! ```

fn main() {
    println!("cargo::rerun-if-changed=../../assets/logo.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/logo.ico")
            .compile()
            .expect("failed to embed the Windows icon");
    }
}
