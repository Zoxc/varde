//! The sample designs of the workspace's `examples/`, built into the app
//! with the `samples` feature, which the GitHub Pages build turns on: the
//! web page lists them below browser storage, and each opens as a new
//! design (see `agents/web-files.md`). Without the feature there are
//! none.

/// A sample design: its name, a line on what it shows, and its `.vrdp`
/// file.
pub(crate) struct Sample {
    pub(crate) name: &'static str,
    pub(crate) about: &'static str,
    pub(crate) file: &'static [u8],
}

/// A sample in `examples/`.
#[cfg(feature = "samples")]
macro_rules! sample {
    ($name:literal, $about:literal, $file:literal) => {
        Sample {
            name: $name,
            about: $about,
            file: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/",
                $file
            )),
        }
    };
}

/// The samples, in the order they're listed.
#[cfg(feature = "samples")]
pub(crate) const SAMPLES: &[Sample] = &[
    sample!(
        "Mounting plate",
        "A plate with bolt holes and a slot: extrude, cut",
        "mounting-plate.vrdp"
    ),
    sample!(
        "Knob",
        "A turned knob with a shaft hole: revolve, cut",
        "knob.vrdp"
    ),
    sample!(
        "Pillow block",
        "Revolve, combine, spline ribs, a bore",
        "pillow-block.vrdp"
    ),
];

#[cfg(not(feature = "samples"))]
pub(crate) const SAMPLES: &[Sample] = &[];
