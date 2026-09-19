//! Photos over the machine's own filesystem. The grid is the pictures folder's real
//! listing — the screenshots the world writes there and whatever else the machine holds.
//! An empty folder shows empty and an unreadable one says so; no album is invented.
//!
//! A tile shows the file's real pixels once the shell has decoded them, and the file's
//! name until then. `AppEffect::ReadImage` asks the shell to decode a file, because plain
//! `ReadFile` delivers lossy UTF-8 and would destroy image bytes; a build with no
//! rasterizer, or a file that will not decode, says so rather than showing a gap.
use super::imaging::{self, Product, Studio};
use super::look::{action, look, notice, screen, FAINT, INK, LINE, MUTED};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Retained listing bound, the way the terminal bounds its transcript. A folder with more
/// files than this shows the first `ENTRY_LIMIT` in name order and says how many it holds.
pub const ENTRY_LIMIT: usize = 512;
/// Long edge of a retained thumbnail, and how many are kept at once.
pub const THUMBNAIL: u32 = 192;
pub const DECODED_LIMIT: usize = 64;
/// Extensions the grid treats as pictures. A folder may hold anything; only these are shown,
/// so a stray text file never becomes a photo.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp"];

/// Product name of an image editor kind, for "Edit with …".
fn editor_name(kind: &str) -> &str {
    match kind {
        "paint" => "Paint",
        "preview" => "Preview",
        "pixelmator" => "Pixelmator Pro",
        "gimp" => "GIMP",
        "pinta" => "Pinta",
        "sketchbook" => "Sketchbook",
        other => other,
    }
}

fn is_image(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        }
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Photos {
    /// Folder the pictures live in, resolved by the shell against the machine's filesystem.
    pub folder: String,
    /// Image file names the folder really holds, in name order, capped at `ENTRY_LIMIT`.
    pub entries: Vec<String>,
    /// How many image files the last listing carried, before the cap.
    pub total: usize,
    /// The photo shown full screen, when one is open.
    pub open: Option<String>,
    /// Set when the folder could not be listed; shown instead of an empty grid.
    pub problem: Option<String>,
    /// Pixels the shell decoded for us, by file name. A photo library is the one place
    /// an application really does need to draw a picture it did not draw itself.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pixels: BTreeMap<String, Picture>,
    /// Names that could not be decoded, so a broken file reads as broken rather than
    /// as one that is still loading.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub undecodable: BTreeSet<String>,
    /// The photo being edited on a phone: iOS Photos' edit mode and Google Photos'
    /// editor live inside the library, as they do on the devices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing: Option<Box<Studio>>,
}
/// Decoded image, downsampled to at most `THUMBNAIL` on its long edge. A photo library
/// must not put a full-resolution bitmap per file into every snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Photos {
    pub const KIND: &'static str = "photos";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let folder = if argument.is_empty() {
            "Pictures".into()
        } else {
            argument.trim_end_matches('/').to_owned()
        };
        let app = Self {
            folder: folder.clone(),
            entries: vec![],
            total: 0,
            open: None,
            problem: None,
            pixels: BTreeMap::new(),
            undecodable: BTreeSet::new(),
            editing: None,
        };
        (
            app,
            vec![AppEffect::ListDirectory {
                window,
                tab: 0,
                path: folder,
            }],
        )
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        match theme {
            DesktopTheme::Ubuntu => "Image Viewer",
            DesktopTheme::Android => "Gallery",
            _ => "Photos",
        }
        .into()
    }
    pub fn document(&self) -> String {
        self.open
            .as_ref()
            .map(|name| format!("{}/{name}", self.folder))
            .unwrap_or_default()
    }
    pub fn caption(&self) -> String {
        match &self.open {
            Some(name) => name.clone(),
            None if self.problem.is_some() => String::new(),
            None => match self.total {
                0 => "No photos".into(),
                1 => "1 photo".into(),
                n => format!("{n} photos"),
            },
        }
    }
    pub fn modified(&self) -> bool {
        self.editing.as_ref().is_some_and(|e| e.modified)
    }
    /// Full path of a photo in the library.
    fn path_of(&self, name: &str) -> String {
        format!("{}/{name}", self.folder.trim_end_matches('/'))
    }
    /// The edited photo was written: leave the editor and show the result.
    pub fn image_saved(&mut self, window: u64, path: &str) -> Result<Vec<AppEffect>, String> {
        let editing = self.editing.take().ok_or("no photo is being edited")?;
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        // The file changed on disk; its old thumbnail is stale.
        self.pixels.remove(&name);
        self.open = Some(name);
        drop(editing);
        Ok(vec![AppEffect::ListDirectory {
            window,
            tab: 0,
            path: self.folder.clone(),
        }])
    }
    /// The shell delivers a listing of the pictures folder here.
    /// Accept decoded pixels, downsampled so a snapshot stays a reasonable size. The
    /// reduction is integer point sampling: reproducible on every host, unlike a filter.
    pub fn image(
        &mut self,
        path: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        // The photo being edited arrives at full resolution for the editor.
        if let Some(editor) = self
            .editing
            .as_mut()
            .filter(|e| e.loading.as_deref() == Some(path))
        {
            return editor.image(path, width, height, rgba);
        }
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        if width == 0 || height == 0 || rgba.len() < (width as usize * height as usize * 4) {
            self.undecodable.insert(name);
            return Err("image data is malformed".into());
        }
        let scale = (width.max(height).div_ceil(THUMBNAIL)).max(1);
        let (w, h) = ((width / scale).max(1), (height / scale).max(1));
        let mut small = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let src = ((y * scale) as usize * width as usize + (x * scale) as usize) * 4;
                small.extend_from_slice(rgba.get(src..src + 4).unwrap_or(&[0, 0, 0, 255]));
            }
        }
        self.undecodable.remove(&name);
        if self.pixels.len() >= DECODED_LIMIT && !self.pixels.contains_key(&name) {
            // Keep the ones nearest what is on screen: drop the alphabetically furthest.
            if let Some(evict) = self.pixels.keys().next_back().cloned() {
                if evict > name {
                    self.pixels.remove(&evict);
                } else {
                    return Ok(());
                }
            }
        }
        self.pixels.insert(
            name,
            Picture {
                width: w,
                height: h,
                rgba: small,
            },
        );
        Ok(())
    }
    /// This file will not decode. The rest of the library is unaffected.
    pub fn image_failed(&mut self, path: &str, reason: &str) {
        if let Some(editor) = self
            .editing
            .as_mut()
            .filter(|e| e.loading.as_deref() == Some(path))
        {
            editor.image_failed(path, reason);
            return;
        }
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        self.pixels.remove(&name);
        self.undecodable.insert(name);
    }
    /// Files on screen whose pixels we do not have yet.
    pub fn undecoded(&self, window: u64) -> Vec<AppEffect> {
        self.entries
            .iter()
            .filter(|name| !self.pixels.contains_key(*name) && !self.undecodable.contains(*name))
            .take(DECODED_LIMIT)
            .map(|name| AppEffect::ReadImage {
                window,
                path: format!("{}/{name}", self.folder.trim_end_matches('/')),
            })
            .collect()
    }
    pub fn listed(&mut self, entries: Vec<String>) {
        let images: Vec<String> = entries.into_iter().filter(|e| is_image(e)).collect();
        self.total = images.len();
        self.entries = images.into_iter().take(ENTRY_LIMIT).collect();
        // A photo deleted between two listings must not stay open over an empty frame.
        if self
            .open
            .as_ref()
            .is_some_and(|n| !self.entries.contains(n))
        {
            self.open = None;
        }
        // Pixels for files that are gone are dead weight in every future snapshot.
        self.pixels.retain(|name, _| self.entries.contains(name));
        self.undecodable.retain(|name| self.entries.contains(name));
        self.problem = None;
    }
    /// Photos never reads a file's bytes, so this exists only to satisfy a delivery the
    /// shell may make; the listing is the whole of what the app knows.
    pub fn loaded(&mut self, _content: String) {}
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        self.problem = Some(reason.to_owned());
    }
    pub fn http(
        &mut self,
        _window: u64,
        _tag: &str,
        _status: u16,
        _body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        Err("photos reads the machine's filesystem and makes no requests".into())
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        match &mut self.editing {
            Some(editor) => editor.text(text),
            None => Err("photos has no text field".into()),
        }
    }
    /// Commands of the phone editors, and the Edit buttons that start them.
    fn edit_command(&mut self, window: u64, command: &str) -> Result<Vec<AppEffect>, String> {
        if let Some(product) = command.strip_prefix("begin-edit:") {
            let product = match product {
                "ios" => Product::IosPhotos,
                "android" => Product::GooglePhotos,
                _ => return Err("unknown photo editor".into()),
            };
            let name = self.open.clone().ok_or("open a photo to edit it")?;
            let path = self.path_of(&name);
            let (mut editor, effects) = Studio::launch(product, &path, window);
            editor.tab = "adjust".into();
            editor.focus = if product == Product::IosPhotos {
                "exposure".into()
            } else {
                "brightness".into()
            };
            self.editing = Some(Box::new(editor));
            return Ok(effects);
        }
        if let Some(kind) = command.strip_prefix("edit-with:") {
            let name = self.open.clone().ok_or("open a photo to edit it")?;
            return Ok(vec![AppEffect::Launch {
                window,
                kind: kind.to_owned(),
                argument: self.path_of(&name),
            }]);
        }
        let rest = command
            .strip_prefix("edit:")
            .ok_or_else(|| format!("unknown photos command {command}"))?;
        let entries = self.entries.clone();
        let editor = self.editing.as_mut().ok_or("no photo is being edited")?;
        match rest {
            "discard" => {
                self.editing = None;
                Ok(vec![])
            }
            "look:done" => imaging::look::done(editor, window, &entries),
            other => editor.command(window, other),
        }
    }
    fn step(&mut self, forward: bool) -> Result<(), String> {
        let open = self.open.clone().ok_or("no photo is open")?;
        let at = self
            .entries
            .iter()
            .position(|e| *e == open)
            .ok_or("open photo is no longer in the folder")?;
        let next = if forward {
            at + 1
        } else {
            at.checked_sub(1).ok_or("already at the first photo")?
        };
        self.open = Some(
            self.entries
                .get(next)
                .ok_or("already at the last photo")?
                .clone(),
        );
        Ok(())
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        if let Some(editor) = &mut self.editing {
            return editor.key(window, key);
        }
        match key {
            "Escape" => self.click(window, "photos:close", clock_us),
            "ArrowLeft" => self.click(window, "photos:prev", clock_us),
            "ArrowRight" => self.click(window, "photos:next", clock_us),
            other => Err(format!("unsupported photos key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("photos:")
            .ok_or("interaction does not belong to photos")?;
        if command.starts_with("edit") || command.starts_with("begin-edit:") {
            return self.edit_command(window, command);
        }
        match command {
            "reload" => {
                self.pixels.clear();
                self.undecodable.clear();
                Ok(vec![AppEffect::ListDirectory {
                    window,
                    tab: 0,
                    path: self.folder.clone(),
                }])
            }
            "close" => {
                self.open = None;
                Ok(vec![])
            }
            "prev" => self.step(false).map(|()| vec![]),
            "next" => self.step(true).map(|()| vec![]),
            // The folder itself is a real place on the machine, so Photos hands it to the
            // file manager rather than growing a second, worse one.
            "folder" => Ok(vec![AppEffect::Launch {
                window,
                kind: "files".into(),
                argument: self.folder.clone(),
            }]),
            rest => {
                let name = rest
                    .strip_prefix("open:")
                    .ok_or_else(|| format!("unknown photos command {command}"))?;
                if !self.entries.iter().any(|e| e == name) {
                    return Err("photo not found".into());
                }
                self.open = Some(name.to_owned());
                Ok(vec![])
            }
        }
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        if let Some(editor) = &self.editing {
            editor.page(page);
            return;
        }
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        page.elements.push(E::Heading {
            id: "photos-folder".into(),
            text: self.folder.clone(),
            level: 2,
        });
        page.elements.push(E::Text {
            id: "photos-count".into(),
            text: match &self.problem {
                Some(problem) => problem.clone(),
                None => self.caption(),
            },
        });
        for (id, label) in [
            ("photos:reload", "Reload"),
            ("photos:folder", "Show in Files"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
        for name in &self.entries {
            page.elements.push(E::Button {
                id: format!("photos:open:{name}"),
                text: name.clone(),
                action: act(&format!("photos:open:{name}")),
            });
        }
        if let Some(open) = &self.open {
            page.elements.push(E::Heading {
                id: "photos-open".into(),
                text: open.clone(),
                level: 3,
            });
            for (id, label) in [
                ("photos:prev", "Previous"),
                ("photos:next", "Next"),
                ("photos:close", "Close"),
            ] {
                page.elements.push(E::Button {
                    id: id.into(),
                    text: label.into(),
                    action: act(id),
                });
            }
        }
    }
    /// Tiles per row. A phone shows a dense square grid; a desktop sizes to the window.
    fn columns(&self, theme: DesktopTheme, width: u32) -> u32 {
        match theme {
            DesktopTheme::Ios => 3,
            DesktopTheme::Android => 3,
            DesktopTheme::Windows => (width / 150).clamp(2, 8),
            DesktopTheme::Ubuntu => (width / 172).clamp(2, 7),
            DesktopTheme::Macos => (width / 160).clamp(2, 7),
        }
    }
    /// Draw a photo into `r`: its own pixels when we have them, a glyph and its name
    /// while we wait, and a plainly different mark when the file will not decode.
    fn tile(&self, p: &mut Painter, r: Rect, name: &str) {
        if let Some(picture) = self.pixels.get(name) {
            // Fit inside the tile, preserving the picture's own proportions.
            let (pw, ph) = (picture.width.max(1), picture.height.max(1));
            let scale = (r.width * 1024 / pw).min(r.height * 1024 / ph).max(1);
            let (w, h) = (
                (pw * scale / 1024).clamp(1, r.width),
                (ph * scale / 1024).clamp(1, r.height),
            );
            p.node(
                Rect::new(
                    r.x + (r.width as i32 - w as i32) / 2,
                    r.y + (r.height as i32 - h as i32) / 2,
                    w,
                    h,
                ),
                cw_scene::Primitive::Image {
                    width: picture.width,
                    height: picture.height,
                    rgba: picture.rgba.clone(),
                },
                None,
            );
            return;
        }
        let broken = self.undecodable.contains(name);
        let size = 28.min(r.width / 2).max(8);
        p.symbol(
            if broken { "close" } else { "image" },
            r.x + (r.width as i32 - size as i32) / 2,
            r.y + (r.height as i32 - size as i32) / 2,
            size,
            FAINT,
        );
        if broken && r.height > 56 {
            p.label(
                r.x,
                r.y + r.height as i32 - 18,
                r.width,
                "Cannot be shown",
                10,
                FAINT,
                false,
                Align::Center,
            );
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        if let Some(editor) = &self.editing {
            editor.render(p, env);
            return;
        }
        if let Some(open) = &self.open {
            self.viewer(p, theme, &l, width, height, open);
            self.edit_button(p, env, open);
            return;
        }
        let screen = screen(p, theme, &l, width, height as i32, &self.title(theme));
        let mut top = screen.top;
        // Windows puts its verbs in a labelled command bar; the other desktops keep a
        // compact toolbar; the phones put the two controls under the large title.
        let mut left = 0;
        match theme {
            DesktopTheme::Windows => {
                p.box_(Rect::new(0, top, width, 40), l.chrome, 0);
                p.hline(0, top + 40, width, LINE);
                action(
                    p,
                    &l,
                    Rect::new(8, top + 7, 78, 26),
                    "Refresh",
                    "photos:reload",
                    false,
                );
                action(
                    p,
                    &l,
                    Rect::new(92, top + 7, 108, 26),
                    "Open folder",
                    "photos:folder",
                    false,
                );
                p.right(
                    width as i32 - 210,
                    top + 13,
                    200,
                    &self.caption(),
                    12,
                    MUTED,
                );
                top += 41;
            }
            DesktopTheme::Macos | DesktopTheme::Ubuntu => {
                // A sidebar naming the real folder, so the window says where it is looking.
                left = if width < 520 { 0 } else { 168 };
                if left > 0 {
                    p.box_(Rect::new(0, top, left as u32, height), l.chrome, 0);
                    p.vline(left, top, height, LINE);
                    p.strong(12, top + 12, left as u32 - 24, "Library", 12, MUTED);
                    p.button(
                        Rect::new(6, top + 32, left as u32 - 12, l.row),
                        l.selection,
                        l.radius,
                        "photos:folder",
                        &self.folder,
                    );
                    p.symbol(
                        "folder",
                        14,
                        top + 32 + (l.row as i32 - 16) / 2,
                        16,
                        l.accent,
                    );
                    p.left(
                        36,
                        top + 32 + (l.row as i32 - 15) / 2,
                        left as u32 - 46,
                        &self.folder,
                        12,
                        INK,
                    );
                    p.left(
                        12,
                        top + 40 + l.row as i32,
                        left as u32 - 24,
                        &self.caption(),
                        11,
                        MUTED,
                    );
                }
                action(
                    p,
                    &l,
                    Rect::new(width as i32 - 82, top + 8, 72, 26),
                    "Reload",
                    "photos:reload",
                    false,
                );
                top += 42;
            }
            DesktopTheme::Ios | DesktopTheme::Android => {
                let mut x = 16;
                for (target, label) in [("photos:reload", "Reload"), ("photos:folder", "Files")] {
                    let w = p.measure(label, 13, false) + 26;
                    action(p, &l, Rect::new(x, top + 8, w, 32), label, target, false);
                    x += w as i32 + 8;
                }
                p.right(
                    width as i32 - 176,
                    top + 16,
                    160,
                    &self.caption(),
                    12,
                    MUTED,
                );
                top += 48;
            }
        }
        let body = width.saturating_sub(left as u32);
        if let Some(problem) = &self.problem {
            notice(p, body, top + 24, problem);
        } else if self.entries.is_empty() {
            notice(p, body, top + 24, "No photos");
            notice(p, body, top + 46, &self.folder);
        } else {
            let grid = screen.column(
                p,
                "grid",
                Rect::new(left, top, body, (height as i32 - top).max(1) as u32),
            );
            self.grid(p, theme, &l, left, grid.top, body);
            grid.end(p);
        }
        screen.end(p);
    }
    #[allow(clippy::too_many_arguments)]
    fn grid(
        &self,
        p: &mut Painter,
        theme: DesktopTheme,
        l: &super::look::Look,
        left: i32,
        top: i32,
        body: u32,
    ) {
        let columns = self.columns(theme, body).max(1);
        // iOS packs its grid edge to edge with hairline gutters; everything else breathes.
        let (gap, pad, caption) = match theme {
            DesktopTheme::Ios => (2, 0, false),
            DesktopTheme::Android => (6, 10, true),
            DesktopTheme::Windows => (6, 8, true),
            _ => (10, 12, true),
        };
        let usable = body.saturating_sub(pad * 2 + gap * (columns - 1));
        let cell = (usable / columns).max(32);
        let label_h = if caption { 18 } else { 0 };
        for (index, name) in self.entries.iter().enumerate() {
            let column = index as u32 % columns;
            let row = index as u32 / columns;
            let x = left + pad as i32 + (column * (cell + gap)) as i32;
            let y = top + pad as i32 + (row * (cell + label_h + gap)) as i32;
            let r = Rect::new(x, y, cell, cell);
            p.button(
                r,
                Color(0, 0, 0, 16),
                l.radius,
                &format!("photos:open:{name}"),
                name,
            );
            self.tile(p, r, name);
            if caption {
                p.label(
                    r.x,
                    r.y + cell as i32 + 2,
                    cell,
                    name,
                    11,
                    MUTED,
                    false,
                    Align::Center,
                );
            }
        }
    }
    /// The viewer's Edit button. Phones edit in place; desktops hand the file to the
    /// platform's installed image editor, and say so when there is none.
    fn edit_button(&self, p: &mut Painter, env: &crate::AppEnv<'_>, open: &str) {
        let theme = env.theme;
        let bar: u32 = if theme.mobile() { 52 } else { 40 };
        let (label, target) = match theme {
            DesktopTheme::Ios => ("Edit".to_owned(), Some("photos:begin-edit:ios".to_owned())),
            DesktopTheme::Android => (
                "Edit".to_owned(),
                Some("photos:begin-edit:android".to_owned()),
            ),
            _ => match env.editor {
                Some(kind) => (
                    format!("Edit with {}", editor_name(kind)),
                    Some(format!("photos:edit-with:{kind}")),
                ),
                None => ("Edit".to_owned(), None),
            },
        };
        // Only files the editors can decode are editable.
        let editable = imaging::is_image(open);
        let w = p.measure(&label, 12, true) + 24;
        let r = Rect::new(env.width as i32 - 156 - w as i32, 6, w, bar - 12);
        match target.filter(|_| editable && !self.undecodable.contains(open)) {
            Some(target) => p.button(r, Color(255, 214, 10, 230), 6, &target, &label),
            None => {
                p.box_(r, Color(255, 255, 255, 10), 6);
                p.disabled(if editable {
                    "No image editor is installed"
                } else {
                    "This file cannot be edited"
                });
            }
        }
        p.label(
            r.x,
            r.y + (r.height as i32 - 17) / 2,
            r.width,
            &label,
            12,
            Color::rgb(20, 20, 20),
            true,
            Align::Center,
        );
    }
    fn viewer(
        &self,
        p: &mut Painter,
        theme: DesktopTheme,
        l: &super::look::Look,
        width: u32,
        height: u32,
        open: &str,
    ) {
        // A viewer is dark on every platform, because the picture is the whole screen.
        p.scene.background = Color::rgb(18, 18, 20);
        p.box_(Rect::new(0, 0, width, height), Color::rgb(18, 18, 20), 0);
        let bar: u32 = if theme.mobile() { 52 } else { 40 };
        p.box_(Rect::new(0, 0, width, bar), Color(0, 0, 0, 120), 0);
        let close = Rect::new(8, 6, if theme.mobile() { 76 } else { 64 }, bar - 12);
        p.button(
            close,
            Color(255, 255, 255, 28),
            l.radius,
            "photos:close",
            "Close",
        );
        p.label(
            close.x,
            close.y + (close.height as i32 - 17) / 2,
            close.width,
            "Close",
            13,
            Color::WHITE,
            true,
            Align::Center,
        );
        // Room is left for Edit beside Previous and Next; a phone too narrow for the
        // name leaves it to the caption below the photo.
        let title = width.saturating_sub(close.width + 340);
        if title >= 48 {
            p.label(
                close.x + close.width as i32 + 12,
                (bar as i32 - 16) / 2,
                title,
                open,
                13,
                Color::WHITE,
                false,
                Align::Left,
            );
        }
        let at = self.entries.iter().position(|e| e == open);
        let mut x = width as i32 - 150;
        for (target, label, enabled) in [
            ("photos:prev", "Previous", at.is_some_and(|i| i > 0)),
            (
                "photos:next",
                "Next",
                at.is_some_and(|i| i + 1 < self.entries.len()),
            ),
        ] {
            let r = Rect::new(x, 6, 68, bar - 12);
            if enabled {
                p.button(r, Color(255, 255, 255, 28), l.radius, target, label);
            } else {
                // Announced disabled rather than painted live: there is no photo that way.
                p.box_(r, Color(255, 255, 255, 10), l.radius);
                p.disabled(label);
            }
            p.label(
                r.x,
                r.y + (r.height as i32 - 17) / 2,
                r.width,
                label,
                12,
                if enabled {
                    Color::WHITE
                } else {
                    Color(255, 255, 255, 90)
                },
                false,
                Align::Center,
            );
            x += 72;
        }
        // The picture fills the viewport, at its own proportions.
        let side = (width.saturating_sub(64))
            .min(height.saturating_sub(bar + 96))
            .max(48);
        let frame = Rect::new(
            (width as i32 - side as i32) / 2,
            bar as i32 + (height as i32 - bar as i32 - side as i32) / 2,
            side,
            side,
        );
        p.box_(frame, Color(255, 255, 255, 14), l.radius);
        self.tile(p, frame, open);
        if !self.pixels.contains_key(open) {
            // Still waiting, or it will not decode: say which file, either way.
            p.label(
                frame.x,
                frame.y + (frame.height as i32) / 2 + 40,
                frame.width,
                &format!("{}/{open}", self.folder),
                12,
                Color(255, 255, 255, 150),
                false,
                Align::Center,
            );
        }
        if let Some(index) = at {
            p.label(
                0,
                height as i32 - 26,
                width,
                &format!("{} of {}", index + 1, self.entries.len()),
                11,
                Color(255, 255, 255, 130),
                false,
                Align::Center,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> Photos {
        let (mut app, effects) = Photos::launch("/home/alice/Pictures", 1, 0);
        assert!(
            matches!(&effects[0], AppEffect::ListDirectory { path, .. } if path == "/home/alice/Pictures")
        );
        app.listed(vec![
            "notes.txt".into(),
            "albums/".into(),
            "screen-120.png".into(),
            "screen-40.png".into(),
        ]);
        app
    }
    #[test]
    fn the_grid_is_the_folders_real_image_files_and_nothing_else() {
        let app = app();
        assert_eq!(app.entries, vec!["screen-120.png", "screen-40.png"]);
        assert_eq!(app.caption(), "2 photos");
        assert!(app.problem.is_none());
    }
    #[test]
    fn an_empty_folder_shows_empty_and_an_unreadable_one_says_why() {
        let (mut app, _) = Photos::launch("", 1, 0);
        assert_eq!(app.folder, "Pictures");
        app.listed(vec!["readme.md".into()]);
        assert!(app.entries.is_empty());
        assert_eq!(app.caption(), "No photos");
        app.offline("listing", "folder access denied");
        assert_eq!(app.problem.as_deref(), Some("folder access denied"));
    }
    #[test]
    fn opening_walks_the_folder_and_stops_at_both_ends() {
        let mut app = app();
        assert!(app.click(1, "photos:open:nope.png", 0).is_err());
        app.click(1, "photos:open:screen-120.png", 0).unwrap();
        assert_eq!(app.open.as_deref(), Some("screen-120.png"));
        assert!(app.click(1, "photos:prev", 0).is_err());
        app.click(1, "photos:next", 0).unwrap();
        assert_eq!(app.open.as_deref(), Some("screen-40.png"));
        assert!(app.click(1, "photos:next", 0).is_err());
        app.key(1, "Escape", 0).unwrap();
        assert!(app.open.is_none());
        // A refresh that loses the open photo closes the viewer instead of framing nothing.
        app.click(1, "photos:open:screen-40.png", 0).unwrap();
        app.listed(vec!["screen-120.png".into()]);
        assert!(app.open.is_none());
    }
    #[test]
    fn the_folder_button_hands_the_real_path_to_the_file_manager() {
        let mut app = app();
        let effects = app.click(1, "photos:folder", 0).unwrap();
        assert!(
            matches!(&effects[0], AppEffect::Launch { kind, argument, .. } if kind == "files" && argument == "/home/alice/Pictures")
        );
        assert!(app.http(1, "any", 200, "{}").is_err());
        assert!(app.text("x").is_err());
    }
    #[test]
    fn a_listing_larger_than_the_bound_is_capped_but_counted() {
        let mut app = app();
        app.listed(
            (0..ENTRY_LIMIT + 10)
                .map(|n| format!("p{n:04}.png"))
                .collect(),
        );
        assert_eq!(app.entries.len(), ENTRY_LIMIT);
        assert_eq!(app.total, ENTRY_LIMIT + 10);
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        for theme in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
            DesktopTheme::Ios,
            DesktopTheme::Android,
        ] {
            for open in [None, Some("screen-120.png")] {
                let mut base = app();
                if let Some(name) = open {
                    base.click(1, &format!("photos:open:{name}"), 0).unwrap();
                }
                let mut scene = Painter::themed(theme, 900, 620, 0);
                base.render(
                    &mut scene,
                    &crate::AppEnv {
                        theme,
                        width: 900,
                        height: 620,
                        clock_us: 0,
                        settings: &crate::SystemSettings::DEFAULT,
                        clipboard: None,
                        share_to: None,
                        editor: None,
                        pointer: None,
                        files: Default::default(),
                    },
                );
                let targets: Vec<_> = scene
                    .scene
                    .nodes
                    .iter()
                    .filter_map(|n| n.interaction.clone())
                    .collect();
                assert!(!targets.is_empty(), "{theme:?} paints no controls");
                for target in targets {
                    let mut app = base.clone();
                    assert!(
                        app.click(1, &target, 0).is_ok(),
                        "unhandled control {target} on {theme:?}"
                    );
                }
            }
        }
    }
}
