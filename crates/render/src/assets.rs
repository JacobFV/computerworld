//! Embedded resources: deterministic image decode, no host access.
//!
//! Icons and symbol masks are PNG. Wallpapers — the only photographic assets —
//! are baseline JPEG, which stores them at about a fifth of the PNG size that
//! `gzip` could not compress further. Both decoders are integer-only on every
//! target: `png` always is, and `jpeg-decoder` is pinned to its
//! `platform_independent` feature, which compiles out the SSSE3 / NEON /
//! `simd128` IDCT and colour-convert paths that would otherwise make an x86
//! host disagree with Wasm. `wallpapers_decode_to_pinned_pixels` below is the
//! tripwire: it hashes the decoded pixels, so any decoder or target that
//! diverges fails loudly rather than silently shifting the rendered frame.
use super::Frame;
pub use crate::symbols::SYMBOLS;
use std::sync::{Arc, OnceLock};
pub const ASSET_IDS: &[&str] = &[
    "wallpaper/macos",
    "wallpaper/windows",
    "wallpaper/ubuntu",
    "wallpaper/ios",
    "wallpaper/android",
    "icon/macos/files",
    "icon/macos/browser",
    "icon/macos/terminal",
    "icon/macos/docs",
    "icon/macos/mail",
    "icon/macos/calendar",
    "icon/macos/chat",
    "icon/macos/settings",
    "icon/macos/camera",
    "icon/macos/photos",
    "icon/macos/phone",
    "icon/macos/store",
    "icon/macos/launcher",
    "icon/macos/trash",
    "icon/macos/notes",
    "icon/macos/contacts",
    "icon/macos/clock",
    "icon/macos/calculator",
    "icon/macos/music",
    "icon/macos/maps",
    "icon/macos/weather",
    "icon/macos/code",
    "icon/macos/preview",
    "icon/macos/pixelmator",
    "icon/macos/freecad",
    "icon/macos/spreadsheet",
    "icon/macos/excel",
    "icon/macos/database",
    "icon/macos/kicad",
    "icon/windows/files",
    "icon/windows/browser",
    "icon/windows/terminal",
    "icon/windows/docs",
    "icon/windows/mail",
    "icon/windows/calendar",
    "icon/windows/chat",
    "icon/windows/settings",
    "icon/windows/camera",
    "icon/windows/photos",
    "icon/windows/phone",
    "icon/windows/store",
    "icon/windows/launcher",
    "icon/windows/trash",
    "icon/windows/notes",
    "icon/windows/contacts",
    "icon/windows/clock",
    "icon/windows/calculator",
    "icon/windows/music",
    "icon/windows/maps",
    "icon/windows/weather",
    "icon/windows/code",
    "icon/windows/paint",
    "icon/windows/freecad",
    "icon/windows/spreadsheet",
    "icon/windows/database",
    "icon/windows/kicad",
    "icon/ubuntu/files",
    "icon/ubuntu/browser",
    "icon/ubuntu/terminal",
    "icon/ubuntu/docs",
    "icon/ubuntu/mail",
    "icon/ubuntu/calendar",
    "icon/ubuntu/chat",
    "icon/ubuntu/settings",
    "icon/ubuntu/camera",
    "icon/ubuntu/photos",
    "icon/ubuntu/phone",
    "icon/ubuntu/store",
    "icon/ubuntu/launcher",
    "icon/ubuntu/trash",
    "icon/ubuntu/notes",
    "icon/ubuntu/contacts",
    "icon/ubuntu/clock",
    "icon/ubuntu/calculator",
    "icon/ubuntu/music",
    "icon/ubuntu/maps",
    "icon/ubuntu/weather",
    "icon/ubuntu/code",
    "icon/ubuntu/gimp",
    "icon/ubuntu/pinta",
    "icon/ubuntu/freecad",
    "icon/ubuntu/spreadsheet",
    "icon/ubuntu/database",
    "icon/ubuntu/kicad",
    "icon/ios/files",
    "icon/ios/browser",
    "icon/ios/terminal",
    "icon/ios/docs",
    "icon/ios/mail",
    "icon/ios/calendar",
    "icon/ios/chat",
    "icon/ios/settings",
    "icon/ios/camera",
    "icon/ios/photos",
    "icon/ios/phone",
    "icon/ios/store",
    "icon/ios/launcher",
    "icon/ios/trash",
    "icon/ios/notes",
    "icon/ios/contacts",
    "icon/ios/clock",
    "icon/ios/calculator",
    "icon/ios/music",
    "icon/ios/maps",
    "icon/ios/weather",
    "icon/ios/code",
    "icon/ios/spreadsheet",
    "icon/android/files",
    "icon/android/browser",
    "icon/android/terminal",
    "icon/android/docs",
    "icon/android/mail",
    "icon/android/calendar",
    "icon/android/chat",
    "icon/android/settings",
    "icon/android/camera",
    "icon/android/photos",
    "icon/android/phone",
    "icon/android/store",
    "icon/android/launcher",
    "icon/android/trash",
    "icon/android/notes",
    "icon/android/contacts",
    "icon/android/clock",
    "icon/android/calculator",
    "icon/android/music",
    "icon/android/maps",
    "icon/android/weather",
    "icon/android/code",
    "icon/android/sketchbook",
    "icon/android/spreadsheet",
];
fn bytes(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "wallpaper/macos" => include_bytes!("../assets/wallpapers/macos.jpg"),
        "icon/macos/files" | "icon/files" => include_bytes!("../assets/icons/macos-files.png"),
        "icon/macos/browser" | "icon/browser" => {
            include_bytes!("../assets/icons/macos-browser.png")
        }
        "icon/macos/terminal" | "icon/terminal" => {
            include_bytes!("../assets/icons/macos-terminal.png")
        }
        "icon/macos/docs" | "icon/macos/editor" | "icon/docs" => {
            include_bytes!("../assets/icons/macos-docs.png")
        }
        "icon/macos/mail" | "icon/mail" => include_bytes!("../assets/icons/macos-mail.png"),
        "icon/macos/calendar" | "icon/calendar" => {
            include_bytes!("../assets/icons/macos-calendar.png")
        }
        "icon/macos/chat" | "icon/macos/messages" | "icon/chat" => {
            include_bytes!("../assets/icons/macos-chat.png")
        }
        "icon/macos/settings" | "icon/settings" => {
            include_bytes!("../assets/icons/macos-settings.png")
        }
        "icon/macos/camera" | "icon/camera" => include_bytes!("../assets/icons/macos-camera.png"),
        "icon/macos/photos" | "icon/photos" => include_bytes!("../assets/icons/macos-photos.png"),
        "icon/macos/phone" | "icon/phone" => include_bytes!("../assets/icons/macos-phone.png"),
        "icon/macos/store" | "icon/store" => include_bytes!("../assets/icons/macos-store.png"),
        "icon/macos/launcher" | "icon/launcher" => {
            include_bytes!("../assets/icons/macos-launcher.png")
        }
        "icon/macos/trash" | "icon/trash" => include_bytes!("../assets/icons/macos-trash.png"),
        "icon/macos/notes" | "icon/macos/notepad" | "icon/notes" => {
            include_bytes!("../assets/icons/macos-notes.png")
        }
        "icon/macos/contacts" | "icon/macos/addressbook" | "icon/contacts" => {
            include_bytes!("../assets/icons/macos-contacts.png")
        }
        "icon/macos/clock" | "icon/macos/clocks" | "icon/clock" => {
            include_bytes!("../assets/icons/macos-clock.png")
        }
        "icon/macos/calculator" | "icon/macos/calc" | "icon/calculator" => {
            include_bytes!("../assets/icons/macos-calculator.png")
        }
        "icon/macos/music" | "icon/music" => include_bytes!("../assets/icons/macos-music.png"),
        "icon/macos/maps" | "icon/maps" => include_bytes!("../assets/icons/macos-maps.png"),
        "icon/macos/weather" | "icon/weather" => {
            include_bytes!("../assets/icons/macos-weather.png")
        }
        "icon/macos/code" | "icon/code" => include_bytes!("../assets/icons/macos-code.png"),
        "icon/macos/preview" => include_bytes!("../assets/icons/macos-preview.png"),
        "icon/macos/pixelmator" => include_bytes!("../assets/icons/macos-pixelmator.png"),
        "icon/macos/freecad" => include_bytes!("../assets/icons/macos-freecad.png"),
        "icon/macos/spreadsheet" => include_bytes!("../assets/icons/macos-spreadsheet.png"),
        "icon/macos/excel" => include_bytes!("../assets/icons/macos-excel.png"),
        "icon/macos/database" => include_bytes!("../assets/icons/macos-database.png"),
        "icon/macos/kicad" => include_bytes!("../assets/icons/macos-kicad.png"),
        "wallpaper/windows" => include_bytes!("../assets/wallpapers/windows.jpg"),
        "icon/windows/files" => include_bytes!("../assets/icons/windows-files.png"),
        "icon/windows/browser" => include_bytes!("../assets/icons/windows-browser.png"),
        "icon/windows/terminal" => include_bytes!("../assets/icons/windows-terminal.png"),
        "icon/windows/docs" | "icon/windows/editor" => {
            include_bytes!("../assets/icons/windows-docs.png")
        }
        "icon/windows/mail" => include_bytes!("../assets/icons/windows-mail.png"),
        "icon/windows/calendar" => include_bytes!("../assets/icons/windows-calendar.png"),
        "icon/windows/chat" | "icon/windows/messages" => {
            include_bytes!("../assets/icons/windows-chat.png")
        }
        "icon/windows/settings" => include_bytes!("../assets/icons/windows-settings.png"),
        "icon/windows/camera" => include_bytes!("../assets/icons/windows-camera.png"),
        "icon/windows/photos" => include_bytes!("../assets/icons/windows-photos.png"),
        "icon/windows/phone" => include_bytes!("../assets/icons/windows-phone.png"),
        "icon/windows/store" => include_bytes!("../assets/icons/windows-store.png"),
        "icon/windows/launcher" => include_bytes!("../assets/icons/windows-launcher.png"),
        "icon/windows/trash" => include_bytes!("../assets/icons/windows-trash.png"),
        "icon/windows/notes" | "icon/windows/notepad" => {
            include_bytes!("../assets/icons/windows-notes.png")
        }
        "icon/windows/contacts" | "icon/windows/addressbook" => {
            include_bytes!("../assets/icons/windows-contacts.png")
        }
        "icon/windows/clock" | "icon/windows/clocks" => {
            include_bytes!("../assets/icons/windows-clock.png")
        }
        "icon/windows/calculator" | "icon/windows/calc" => {
            include_bytes!("../assets/icons/windows-calculator.png")
        }
        "icon/windows/music" => include_bytes!("../assets/icons/windows-music.png"),
        "icon/windows/maps" => include_bytes!("../assets/icons/windows-maps.png"),
        "icon/windows/weather" => include_bytes!("../assets/icons/windows-weather.png"),
        "icon/windows/code" => include_bytes!("../assets/icons/windows-code.png"),
        "icon/windows/paint" => include_bytes!("../assets/icons/windows-paint.png"),
        "icon/windows/freecad" => include_bytes!("../assets/icons/windows-freecad.png"),
        "icon/windows/spreadsheet" => include_bytes!("../assets/icons/windows-spreadsheet.png"),
        "icon/windows/database" => include_bytes!("../assets/icons/windows-database.png"),
        "icon/windows/kicad" => include_bytes!("../assets/icons/windows-kicad.png"),
        "wallpaper/ubuntu" => include_bytes!("../assets/wallpapers/ubuntu.jpg"),
        "icon/ubuntu/files" => include_bytes!("../assets/icons/ubuntu-files.png"),
        "icon/ubuntu/browser" => include_bytes!("../assets/icons/ubuntu-browser.png"),
        "icon/ubuntu/terminal" => include_bytes!("../assets/icons/ubuntu-terminal.png"),
        "icon/ubuntu/docs" | "icon/ubuntu/editor" => {
            include_bytes!("../assets/icons/ubuntu-docs.png")
        }
        "icon/ubuntu/mail" => include_bytes!("../assets/icons/ubuntu-mail.png"),
        "icon/ubuntu/calendar" => include_bytes!("../assets/icons/ubuntu-calendar.png"),
        "icon/ubuntu/chat" | "icon/ubuntu/messages" => {
            include_bytes!("../assets/icons/ubuntu-chat.png")
        }
        "icon/ubuntu/settings" => include_bytes!("../assets/icons/ubuntu-settings.png"),
        "icon/ubuntu/camera" => include_bytes!("../assets/icons/ubuntu-camera.png"),
        "icon/ubuntu/photos" => include_bytes!("../assets/icons/ubuntu-photos.png"),
        "icon/ubuntu/phone" => include_bytes!("../assets/icons/ubuntu-phone.png"),
        "icon/ubuntu/store" => include_bytes!("../assets/icons/ubuntu-store.png"),
        "icon/ubuntu/launcher" => include_bytes!("../assets/icons/ubuntu-launcher.png"),
        "icon/ubuntu/trash" => include_bytes!("../assets/icons/ubuntu-trash.png"),
        "icon/ubuntu/notes" | "icon/ubuntu/notepad" => {
            include_bytes!("../assets/icons/ubuntu-notes.png")
        }
        "icon/ubuntu/contacts" | "icon/ubuntu/addressbook" => {
            include_bytes!("../assets/icons/ubuntu-contacts.png")
        }
        "icon/ubuntu/clock" | "icon/ubuntu/clocks" => {
            include_bytes!("../assets/icons/ubuntu-clock.png")
        }
        "icon/ubuntu/calculator" | "icon/ubuntu/calc" => {
            include_bytes!("../assets/icons/ubuntu-calculator.png")
        }
        "icon/ubuntu/music" => include_bytes!("../assets/icons/ubuntu-music.png"),
        "icon/ubuntu/maps" => include_bytes!("../assets/icons/ubuntu-maps.png"),
        "icon/ubuntu/weather" => include_bytes!("../assets/icons/ubuntu-weather.png"),
        "icon/ubuntu/code" => include_bytes!("../assets/icons/ubuntu-code.png"),
        "icon/ubuntu/gimp" => include_bytes!("../assets/icons/ubuntu-gimp.png"),
        "icon/ubuntu/kicad" => include_bytes!("../assets/icons/ubuntu-kicad.png"),
        "icon/ubuntu/pinta" => include_bytes!("../assets/icons/ubuntu-pinta.png"),
        "icon/ubuntu/freecad" => include_bytes!("../assets/icons/ubuntu-freecad.png"),
        "icon/ubuntu/spreadsheet" => include_bytes!("../assets/icons/ubuntu-spreadsheet.png"),
        "icon/ubuntu/database" => include_bytes!("../assets/icons/ubuntu-database.png"),
        "wallpaper/ios" => include_bytes!("../assets/wallpapers/ios.jpg"),
        "icon/ios/files" => include_bytes!("../assets/icons/ios-files.png"),
        "icon/ios/browser" => include_bytes!("../assets/icons/ios-browser.png"),
        "icon/ios/terminal" => include_bytes!("../assets/icons/ios-terminal.png"),
        "icon/ios/docs" | "icon/ios/editor" => include_bytes!("../assets/icons/ios-docs.png"),
        "icon/ios/mail" => include_bytes!("../assets/icons/ios-mail.png"),
        "icon/ios/calendar" => include_bytes!("../assets/icons/ios-calendar.png"),
        "icon/ios/chat" | "icon/ios/messages" => include_bytes!("../assets/icons/ios-chat.png"),
        "icon/ios/settings" => include_bytes!("../assets/icons/ios-settings.png"),
        "icon/ios/camera" => include_bytes!("../assets/icons/ios-camera.png"),
        "icon/ios/photos" => include_bytes!("../assets/icons/ios-photos.png"),
        "icon/ios/phone" => include_bytes!("../assets/icons/ios-phone.png"),
        "icon/ios/store" => include_bytes!("../assets/icons/ios-store.png"),
        "icon/ios/launcher" => include_bytes!("../assets/icons/ios-launcher.png"),
        "icon/ios/trash" => include_bytes!("../assets/icons/ios-trash.png"),
        "icon/ios/notes" | "icon/ios/notepad" => {
            include_bytes!("../assets/icons/ios-notes.png")
        }
        "icon/ios/contacts" | "icon/ios/addressbook" => {
            include_bytes!("../assets/icons/ios-contacts.png")
        }
        "icon/ios/clock" | "icon/ios/clocks" => {
            include_bytes!("../assets/icons/ios-clock.png")
        }
        "icon/ios/calculator" | "icon/ios/calc" => {
            include_bytes!("../assets/icons/ios-calculator.png")
        }
        "icon/ios/music" => include_bytes!("../assets/icons/ios-music.png"),
        "icon/ios/maps" => include_bytes!("../assets/icons/ios-maps.png"),
        "icon/ios/weather" => include_bytes!("../assets/icons/ios-weather.png"),
        "icon/ios/code" => include_bytes!("../assets/icons/ios-code.png"),
        "icon/ios/spreadsheet" => include_bytes!("../assets/icons/ios-spreadsheet.png"),
        "wallpaper/android" => include_bytes!("../assets/wallpapers/android.jpg"),
        "icon/android/files" => include_bytes!("../assets/icons/android-files.png"),
        "icon/android/browser" => include_bytes!("../assets/icons/android-browser.png"),
        "icon/android/terminal" => include_bytes!("../assets/icons/android-terminal.png"),
        "icon/android/docs" | "icon/android/editor" => {
            include_bytes!("../assets/icons/android-docs.png")
        }
        "icon/android/mail" => include_bytes!("../assets/icons/android-mail.png"),
        "icon/android/calendar" => include_bytes!("../assets/icons/android-calendar.png"),
        "icon/android/chat" | "icon/android/messages" => {
            include_bytes!("../assets/icons/android-chat.png")
        }
        "icon/android/settings" => include_bytes!("../assets/icons/android-settings.png"),
        "icon/android/camera" => include_bytes!("../assets/icons/android-camera.png"),
        "icon/android/photos" => include_bytes!("../assets/icons/android-photos.png"),
        "icon/android/phone" => include_bytes!("../assets/icons/android-phone.png"),
        "icon/android/store" => include_bytes!("../assets/icons/android-store.png"),
        "icon/android/launcher" => include_bytes!("../assets/icons/android-launcher.png"),
        "icon/android/trash" => include_bytes!("../assets/icons/android-trash.png"),
        "icon/android/notes" | "icon/android/notepad" => {
            include_bytes!("../assets/icons/android-notes.png")
        }
        "icon/android/contacts" | "icon/android/addressbook" => {
            include_bytes!("../assets/icons/android-contacts.png")
        }
        "icon/android/clock" | "icon/android/clocks" => {
            include_bytes!("../assets/icons/android-clock.png")
        }
        "icon/android/calculator" | "icon/android/calc" => {
            include_bytes!("../assets/icons/android-calculator.png")
        }
        "icon/android/music" => include_bytes!("../assets/icons/android-music.png"),
        "icon/android/maps" => include_bytes!("../assets/icons/android-maps.png"),
        "icon/android/weather" => include_bytes!("../assets/icons/android-weather.png"),
        "icon/android/code" => include_bytes!("../assets/icons/android-code.png"),
        "icon/android/sketchbook" => include_bytes!("../assets/icons/android-sketchbook.png"),
        "icon/android/spreadsheet" => include_bytes!("../assets/icons/android-spreadsheet.png"),
        _ => {
            return SYMBOLS
                .iter()
                .find(|(name, _)| *name == id)
                .map(|(_, b)| *b)
        }
    })
}
pub fn decode(id: &str) -> Option<Arc<Frame>> {
    // Each immutable resource is decoded once per process/Wasm instance and shared
    // across all worlds; renderer reset/fork never duplicates wallpaper storage.
    static CACHE: OnceLock<Vec<OnceLock<Arc<Frame>>>> = OnceLock::new();
    let parts: Vec<&str> = id.split('/').collect();
    let canonical = if parts.first() == Some(&"icon") {
        let (platform, name) = match parts.as_slice() {
            [_, name] => ("macos", *name),
            // Platform-neutral content; shells substitute their own artwork.
            [_, "common", name] => ("macos", *name),
            [_, platform, name] => (*platform, *name),
            _ => return None,
        };
        let name = match name {
            "editor" => "docs",
            "messages" => "chat",
            "notepad" => "notes",
            "addressbook" => "contacts",
            "clocks" => "clock",
            "calc" => "calculator",
            other => other,
        };
        format!("icon/{platform}/{name}")
    } else {
        id.to_owned()
    };
    let index = ASSET_IDS
        .iter()
        .position(|candidate| *candidate == canonical)
        .or_else(|| {
            SYMBOLS
                .iter()
                .position(|(name, _)| *name == canonical)
                .map(|i| ASSET_IDS.len() + i)
        })?;
    let cache = CACHE.get_or_init(|| {
        (0..ASSET_IDS.len() + SYMBOLS.len())
            .map(|_| OnceLock::new())
            .collect()
    });
    Some(
        cache[index]
            .get_or_init(|| {
                Arc::new(decode_bytes(&canonical).expect("embedded image validated by asset test"))
            })
            .clone(),
    )
}
/// Dispatch on the container's magic number rather than on the identifier, so
/// re-encoding an asset never needs a matching edit here.
fn decode_bytes(id: &str) -> Option<Frame> {
    let bytes = bytes(id)?;
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => decode_jpeg(bytes),
        _ => decode_png(bytes),
    }
}
/// Baseline JPEG, integer-only. The wallpapers are encoded 4:4:4, so
/// `jpeg-decoder`'s chroma upsampler — the one part of that crate that uses
/// floating point — is never reached; `build-wallpapers.py` explains why.
fn decode_jpeg(bytes: &[u8]) -> Option<Frame> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            for p in pixels.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        jpeg_decoder::PixelFormat::L8 => {
            for p in &pixels {
                rgba.extend_from_slice(&[*p, *p, *p, 255]);
            }
        }
        _ => return None,
    }
    Some(Frame {
        width,
        height,
        rgba,
    })
}
fn decode_png(bytes: &[u8]) -> Option<Frame> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut raw = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut raw).ok()?;
    let raw = &raw[..info.buffer_size()];
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(raw),
        png::ColorType::Rgb => {
            for p in raw.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for p in raw {
                rgba.extend_from_slice(&[*p, *p, *p, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in raw.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        png::ColorType::Indexed => return None,
    }
    Some(Frame {
        width: info.width,
        height: info.height,
        rgba,
    })
}

#[cfg(test)]
mod icon_table_tests {
    use super::*;

    /// Original vector artwork rasterizes to 128 px; Yaru PNGs ship at 256 px.
    fn expected_side(id: &str) -> u32 {
        if id.starts_with("icon/ubuntu/") {
            256
        } else {
            128
        }
    }

    #[test]
    fn every_icon_decodes_at_its_expected_size_and_is_not_blank() {
        for id in ASSET_IDS.iter().filter(|id| id.starts_with("icon/")) {
            let frame = decode(id).unwrap_or_else(|| panic!("{id} resolves to no bytes"));
            let side = expected_side(id);
            assert_eq!((frame.width, frame.height), (side, side), "{id}");
            assert_eq!(frame.rgba.len(), (side * side * 4) as usize, "{id}");
            let opaque = frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] >= 128)
                .count();
            // An id that decodes but draws nothing is the failure this catches;
            // the thinnest artwork bundled (a Windows silhouette) covers a tenth.
            assert!(
                opaque * 20 >= frame.rgba.len() / 4,
                "{id} is {opaque} opaque pixels of {}",
                frame.rgba.len() / 4
            );
        }
    }

    #[test]
    fn icon_aliases_share_one_decoded_resource() {
        for platform in ["macos", "windows", "ubuntu", "ios", "android"] {
            for (alias, canonical) in [
                ("editor", "docs"),
                ("messages", "chat"),
                ("notepad", "notes"),
                ("addressbook", "contacts"),
                ("clocks", "clock"),
                ("calc", "calculator"),
            ] {
                let aliased = decode(&format!("icon/{platform}/{alias}"))
                    .unwrap_or_else(|| panic!("icon/{platform}/{alias}"));
                let named = decode(&format!("icon/{platform}/{canonical}")).unwrap();
                assert!(
                    Arc::ptr_eq(&aliased, &named),
                    "icon/{platform}/{alias} is decoded twice"
                );
            }
        }
    }

    #[test]
    fn every_advertised_id_has_embedded_bytes() {
        for id in ASSET_IDS {
            assert!(bytes(id).is_some(), "{id} has no embedded bytes");
        }
        for (name, _) in SYMBOLS {
            assert!(decode(name).is_some(), "{name} does not resolve");
        }
    }

    /// The wallpapers are the only assets that go through the JPEG decoder, and
    /// that decoder is the only place in the renderer where a target could
    /// plausibly disagree with another: `jpeg-decoder` ships SSSE3, NEON and
    /// `simd128` IDCT kernels that are *not* bit-identical to its scalar one.
    /// They are compiled out by the `platform_independent` feature, and these
    /// digests are what proves it — on x86, on aarch64 and under Wasm alike.
    /// A mismatch here means a rendered desktop has silently moved.
    #[test]
    fn wallpapers_decode_to_pinned_pixels() {
        use sha2::{Digest, Sha256};
        for (id, side, digest) in [
            (
                "wallpaper/macos",
                (1586u32, 992u32),
                "0c4ee56e008ffba63f90c471b3179c26fffda660e93e336133a45e3499bb71ba",
            ),
            (
                "wallpaper/windows",
                (1586, 992),
                "4aeea3cfd20992b1c54a824c044d30d81c5ed274df3fd1e709131a741623c2ef",
            ),
            (
                "wallpaper/ubuntu",
                (1600, 900),
                "a41c7d7f6ebaca2a1e347322ea15529acfd06d8fc32505c5a26eefcc765c3172",
            ),
            (
                "wallpaper/ios",
                (853, 1844),
                "ce600fdf77039d52b04338b0f1837e9c73a2394f4451cf30ce2da2d0e70e1bf2",
            ),
            (
                "wallpaper/android",
                (853, 1844),
                "b51aac4e98c71da36321e6a0567b33f2f1046b06ed23b2a1b239ad693d661ada",
            ),
        ] {
            let frame = decode(id).unwrap_or_else(|| panic!("{id} resolves to no bytes"));
            assert_eq!((frame.width, frame.height), side, "{id} dimensions");
            assert_eq!(
                frame.rgba.len(),
                (side.0 * side.1 * 4) as usize,
                "{id} buffer size"
            );
            // Wallpapers are opaque: JPEG carries no alpha and the renderer
            // composites them as a fully covering background.
            assert!(
                frame.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 255),
                "{id} decoded a non-opaque pixel"
            );
            assert_eq!(format!("{:x}", Sha256::digest(&frame.rgba)), digest, "{id}");
        }
    }
}
