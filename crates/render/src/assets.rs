//! Embedded resources: deterministic PNG decode, no host access.
use super::Frame;
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
];
fn bytes(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "wallpaper/macos" => include_bytes!("../assets/wallpapers/macos.png"),
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
        "wallpaper/windows" => include_bytes!("../assets/wallpapers/windows.png"),
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
        "wallpaper/ubuntu" => include_bytes!("../assets/wallpapers/ubuntu.png"),
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
        "wallpaper/ios" => include_bytes!("../assets/wallpapers/ios.png"),
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
        "wallpaper/android" => include_bytes!("../assets/wallpapers/android.png"),
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
        _ => return None,
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
            [_, platform, name] => (*platform, *name),
            _ => return None,
        };
        let name = match name {
            "editor" => "docs",
            "messages" => "chat",
            other => other,
        };
        format!("icon/{platform}/{name}")
    } else {
        id.to_owned()
    };
    let index = ASSET_IDS
        .iter()
        .position(|candidate| *candidate == canonical)?;
    let cache = CACHE.get_or_init(|| ASSET_IDS.iter().map(|_| OnceLock::new()).collect());
    Some(
        cache[index]
            .get_or_init(|| {
                Arc::new(decode_png(&canonical).expect("embedded PNG validated by asset test"))
            })
            .clone(),
    )
}
fn decode_png(id: &str) -> Option<Frame> {
    let bytes = bytes(id)?;
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
            for p in raw.chunks_exact(3) {
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for p in raw {
                rgba.extend_from_slice(&[*p, *p, *p, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in raw.chunks_exact(2) {
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
