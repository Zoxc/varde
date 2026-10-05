//! The sample designs of the workspace's `examples/`, which the web build
//! lists on its start page: each reads, and has a thumbnail for its card.
//! They're written by `cargo run -p varde-view --example samples`.

/// The samples' files, as `crates/app/src/samples.rs` lists them.
const SAMPLES: [&str; 3] = ["mounting-plate", "knob", "pillow-block"];

#[test]
fn every_sample_reads_with_a_thumbnail() {
    for name in SAMPLES {
        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/").to_owned() + name + ".vrdp";
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
        let (document, _) = varde_io::vrdp::from_bytes(&bytes).expect(name);
        assert!(!document.features().is_empty(), "{name}");
        let thumbnail = varde_io::thumbnail::of_file(&bytes)
            .unwrap_or_else(|| panic!("{name} has no thumbnail"));
        for image in [&thumbnail.light, &thumbnail.dark] {
            assert!(image.width() > 0 && image.height() > 0, "{name}");
        }
        assert_ne!(thumbnail.light, thumbnail.dark, "{name}");
    }
}
