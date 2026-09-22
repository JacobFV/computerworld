//! The sample clips seeded into users' movie folders are made by this engine, not by
//! hand: every file under `worlds/company-2026/samples` named by `cw_video::samples::files`
//! must be exactly what the generators write. Regenerate them with
//! `CW_UPDATE_SAMPLES=1 cargo test -p cw-video --test samples`, then run
//! `node scripts/build-live-world.mjs` to seed them.
use cw_video::{samples, Media, MediaKind};
use std::path::PathBuf;

fn files() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../worlds/company-2026/samples")
}

#[test]
fn the_seeded_sample_media_is_what_the_engine_writes() {
    let all = samples::files().unwrap();
    for (name, bytes) in &all {
        let path = files().join(name);
        if std::env::var_os("CW_UPDATE_SAMPLES").is_some() {
            std::fs::write(&path, bytes).unwrap();
            continue;
        }
        let current = std::fs::read(&path).unwrap_or_default();
        assert!(
            current == *bytes,
            "{name} is not what the engine writes; regenerate with CW_UPDATE_SAMPLES=1"
        );
    }
    // And each one imports as what it claims to be.
    for (name, bytes) in all {
        let media = Media::import(name, &bytes, 22_050).unwrap();
        let expected = if name.ends_with(".wav") {
            MediaKind::Audio
        } else {
            MediaKind::Video
        };
        assert_eq!(media.kind, expected, "{name}");
        assert!(media.duration_us >= 2_000_000, "{name}");
    }
}

#[test]
fn generation_is_a_pure_function() {
    assert_eq!(samples::files().unwrap(), samples::files().unwrap());
}
