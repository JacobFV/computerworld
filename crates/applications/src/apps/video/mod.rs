//! Video editors. One engine ([`cw_video`]) and one editing state ([`Editor`]) sit under
//! every product; each product is only its interface, drawn the way that product draws
//! itself and offering only what that product offers:
//!
//! | Platform | Application |
//! |---|---|
//! | Windows 11 | Clipchamp |
//! | macOS | iMovie |
//! | Ubuntu | Kdenlive |
//! | iOS | iMovie (the phone layout of the same app) |
//! | Android | Video Editor (a phone editor with keyframes, reverse and overlays) |
//!
//! Media is read from the machine: Animated PNG movies, PNG and JPEG stills, WAV sound.
//! The program monitor shows [`cw_video::Compositor`]'s frame at the playhead — the
//! frame an export writes — and playback advances with the world clock. Exports are
//! encoded a few frames per simulation step and written as APNG plus a WAV mixdown.
//! A command a product does not have is refused, not quietly honoured.
use crate::desktop_scene::DesktopTheme;
use crate::{AppEffect, PointerPhase};
use cw_video::{
    audio, media, Compositor, Ease, Edge, Export, Library, Masks, Media, MediaKind, Param, Project,
    Source, TextMask, Title, TitlePosition, TrackKind, TransitionKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod desktop;
mod mobile;
mod widgets;

#[cfg(test)]
mod tests;

/// Target namespace of every video editor's controls.
pub const PREFIX: &str = "video";
/// Extension of a saved project.
pub const PROJECT_EXT: &str = "cwvideo";
/// Undo depth.
const HISTORY: usize = 64;
/// Retained sheet listing.
const LISTING_LIMIT: usize = 256;
const NAME_LIMIT: usize = 64;
/// Timeline zoom bounds, in pixels per second.
pub const ZOOM_MIN: u32 = 4;
pub const ZOOM_MAX: u32 = 480;
/// Seconds a Clipchamp skip button jumps.
const SKIP_SECONDS: i64 = 5;
/// Pixels within which a dragged edge snaps.
const SNAP_PX: i64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Product {
    Clipchamp,
    Imovie,
    Kdenlive,
    VideoEditor,
}

/// What a product's interface offers. The engine can do everything; an interface only
/// shows (and only accepts) what the real application has.
#[derive(Clone, Copy, Debug)]
pub struct Features {
    pub keyframes: bool,
    pub ken_burns: bool,
    pub reverse: bool,
    pub shuttle: bool,
    pub track_mute: bool,
    pub track_hide: bool,
    pub track_lock: bool,
    pub crop: bool,
    pub transform: bool,
    pub opacity: bool,
    pub color: bool,
    /// Delete closes the gap (a magnetic timeline).
    pub magnetic: bool,
}

impl Product {
    pub fn name(self) -> &'static str {
        match self {
            Self::Clipchamp => "Clipchamp",
            Self::Imovie => "iMovie",
            Self::Kdenlive => "Kdenlive",
            Self::VideoEditor => "Video Editor",
        }
    }
    /// The folder a product looks in first, relative to home.
    pub fn folder(self) -> &'static str {
        match self {
            Self::Clipchamp | Self::Kdenlive => "Videos",
            Self::Imovie | Self::VideoEditor => "Movies",
        }
    }
    pub fn features(self, mobile: bool) -> Features {
        match self {
            Self::Clipchamp => Features {
                keyframes: false,
                ken_burns: false,
                reverse: false,
                shuttle: false,
                track_mute: true,
                track_hide: true,
                track_lock: false,
                crop: true,
                transform: true,
                opacity: true,
                color: true,
                magnetic: false,
            },
            Self::Imovie => Features {
                keyframes: false,
                ken_burns: true,
                reverse: !mobile,
                shuttle: !mobile,
                track_mute: false,
                track_hide: false,
                track_lock: false,
                crop: !mobile,
                transform: !mobile,
                opacity: !mobile,
                color: !mobile,
                magnetic: true,
            },
            Self::Kdenlive => Features {
                keyframes: true,
                ken_burns: false,
                reverse: true,
                shuttle: true,
                track_mute: true,
                track_hide: true,
                track_lock: true,
                crop: true,
                transform: true,
                opacity: true,
                color: true,
                magnetic: false,
            },
            Self::VideoEditor => Features {
                keyframes: true,
                ken_burns: false,
                reverse: true,
                shuttle: false,
                track_mute: true,
                track_hide: false,
                track_lock: false,
                crop: true,
                transform: true,
                opacity: true,
                color: true,
                magnetic: true,
            },
        }
    }
    /// What the product calls each transition.
    pub fn transition_name(self, kind: TransitionKind) -> &'static str {
        use TransitionKind::*;
        match (self, kind) {
            (Self::Clipchamp, CrossDissolve) => "Cross fade",
            (Self::Clipchamp, DipToBlack) => "Fade through black",
            (Self::Clipchamp, DipToWhite) => "Fade through white",
            (Self::Clipchamp, Wipe) => "Wipe right",
            (Self::Clipchamp, Slide) => "Slide left",
            (Self::Imovie, DipToBlack) => "Fade to Black",
            (Self::Imovie, DipToWhite) => "Fade to White",
            (Self::Imovie, Wipe) => "Wipe Right",
            (Self::Imovie, Slide) => "Slide Left",
            (Self::Kdenlive, CrossDissolve) => "Dissolve",
            (Self::Kdenlive, DipToBlack) => "Dip to Black",
            (Self::Kdenlive, DipToWhite) => "Dip to White",
            (Self::Kdenlive, Wipe) => "Wipe",
            (Self::Kdenlive, Slide) => "Slide",
            (_, CrossDissolve) => "Cross Dissolve",
            (_, DipToBlack) => "Fade",
            (_, DipToWhite) => "Flash",
            (_, Wipe) => "Wipe",
            (_, Slide) => "Slide",
        }
    }
}

/// A property of the selected clip an inspector control reads and sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Prop {
    Opacity,
    X,
    Y,
    Scale,
    Rotation,
    Volume,
    Brightness,
    Contrast,
    Saturation,
    Temperature,
    Speed,
    FadeIn,
    FadeOut,
    CropLeft,
    CropTop,
    CropRight,
    CropBottom,
    TitleSize,
    /// The selected transition's length.
    Transition,
}
impl Prop {
    pub const ALL: [Prop; 19] = [
        Self::Opacity,
        Self::X,
        Self::Y,
        Self::Scale,
        Self::Rotation,
        Self::Volume,
        Self::Brightness,
        Self::Contrast,
        Self::Saturation,
        Self::Temperature,
        Self::Speed,
        Self::FadeIn,
        Self::FadeOut,
        Self::CropLeft,
        Self::CropTop,
        Self::CropRight,
        Self::CropBottom,
        Self::TitleSize,
        Self::Transition,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Opacity => "opacity",
            Self::X => "x",
            Self::Y => "y",
            Self::Scale => "scale",
            Self::Rotation => "rotation",
            Self::Volume => "volume",
            Self::Brightness => "brightness",
            Self::Contrast => "contrast",
            Self::Saturation => "saturation",
            Self::Temperature => "temperature",
            Self::Speed => "speed",
            Self::FadeIn => "fade-in",
            Self::FadeOut => "fade-out",
            Self::CropLeft => "crop-left",
            Self::CropTop => "crop-top",
            Self::CropRight => "crop-right",
            Self::CropBottom => "crop-bottom",
            Self::TitleSize => "title-size",
            Self::Transition => "transition",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }
    /// Whether the property can carry keyframes.
    pub fn animatable(self) -> bool {
        matches!(
            self,
            Self::Opacity | Self::X | Self::Y | Self::Scale | Self::Rotation | Self::Volume
        )
    }
}

/// A pointer drag in progress, kept so the frame can draw it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Drag {
    /// A bin item on its way to the timeline. `(ox, oy)` is the lanes' origin relative to
    /// the item, `scroll` the first frame the lanes showed.
    Media {
        id: u32,
        ox: i32,
        oy: i32,
        lane: u32,
        scroll: i64,
        x: i32,
        y: i32,
    },
    /// A timeline clip being moved.
    Clip {
        id: u32,
        lane: u32,
        press_x: i32,
        press_y: i32,
        x: i32,
        y: i32,
    },
    /// A clip edge being trimmed.
    Trim {
        id: u32,
        out: bool,
        press: i32,
        dx: i32,
    },
    /// The playhead being scrubbed along the ruler.
    Scrub { scroll: i64 },
    /// A slider held down.
    Slider { prop: Prop, width: u32 },
    /// The timeline zoom slider.
    Zoom { width: u32 },
}

/// The playhead moving on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Play {
    /// World time playback (re)started at, and the frame it started from.
    pub since_us: u64,
    pub from: i64,
    /// Percent of real time; negative plays backwards (J).
    pub rate: i32,
}

/// What a floating sheet over the editor is doing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Sheet {
    /// Choosing media (or a project, when `project`) from a folder.
    Browse {
        folder: String,
        entries: Vec<String>,
        loading: bool,
        error: Option<String>,
        project: bool,
    },
    /// Naming the project file to save.
    Save { name: String },
    /// Export settings: name, size and rate.
    Export {
        name: String,
        width: u32,
        height: u32,
        fps: u32,
    },
}

/// Which text field keystrokes go to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    #[default]
    None,
    /// The selected title's text.
    Title,
    /// The open sheet's name field.
    Name,
    /// The project's name (Clipchamp's title box).
    Project,
}

/// Why a file was asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Import,
    /// Imported and laid on the timeline at the playhead, as a phone editor does.
    ImportAdd,
    Project,
    /// Media a loaded project refers to.
    Relink(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Editor {
    pub product: Product,
    /// The folder browsed first and exported into.
    pub folder: String,
    pub project: Project,
    /// Where the project was saved, empty until it has been.
    pub path: String,
    pub modified: bool,
    /// Imported media, by the id the project knows it by.
    pub library: Library,
    /// Media the project names that could not be read, and why.
    #[serde(default)]
    pub offline: BTreeMap<u32, String>,
    /// Rasterised title lines, and the ones still being rasterised, in request order.
    #[serde(default)]
    pub masks: Masks,
    #[serde(default)]
    pub pending_masks: Vec<String>,
    /// The renderer could not rasterise title text, and why.
    #[serde(default)]
    pub text_problem: Option<String>,
    #[serde(default)]
    pub undo: Vec<Project>,
    #[serde(default)]
    pub redo: Vec<Project>,
    pub selected: Option<u32>,
    pub selected_media: Option<u32>,
    pub selected_transition: Option<u32>,
    /// Where the playhead rests while not playing.
    pub playhead: i64,
    pub play: Option<Play>,
    /// Timeline pixels per second, and the first frame at its left edge.
    pub zoom: u32,
    pub scroll: i64,
    pub snapping: bool,
    /// The browser or inspector tab showing (product-specific names).
    pub tab: String,
    /// The inspector page showing for the selected clip.
    pub inspector: String,
    pub sheet: Option<Sheet>,
    pub export: Option<Export>,
    pub drag: Option<Drag>,
    pub field: Field,
    pub status: String,
    pub reading: Vec<(String, Purpose)>,
    /// Last export settings, so the sheet reopens on them.
    pub export_size: (u32, u32),
    pub export_fps: u32,
}

fn parse<T: std::str::FromStr>(s: &str, what: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("invalid {what}"))
}
fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{}/{name}", folder.trim_end_matches('/'))
    }
}
fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':') && !c.is_control())
        .take(NAME_LIMIT)
        .collect::<String>()
        .trim()
        .to_owned()
}

impl Editor {
    pub fn launch(product: Product, argument: &str, window: u64) -> (Self, Vec<AppEffect>) {
        let mut editor = Self {
            product,
            folder: product.folder().into(),
            project: Project::new("My Movie", 320, 180, 24, 22_050),
            path: String::new(),
            modified: false,
            library: Library::new(),
            offline: BTreeMap::new(),
            masks: Masks::new(),
            pending_masks: vec![],
            text_problem: None,
            undo: vec![],
            redo: vec![],
            selected: None,
            selected_media: None,
            selected_transition: None,
            playhead: 0,
            play: None,
            zoom: 48,
            scroll: 0,
            snapping: true,
            tab: match product {
                Product::Clipchamp => "media",
                Product::Imovie => "media",
                Product::Kdenlive => "bin",
                Product::VideoEditor => "edit",
            }
            .into(),
            inspector: String::new(),
            sheet: None,
            export: None,
            drag: None,
            field: Field::None,
            status: String::new(),
            reading: vec![],
            export_size: (320, 180),
            export_fps: 24,
        };
        let argument = argument.trim();
        let mut effects = vec![];
        if argument.ends_with(&format!(".{PROJECT_EXT}")) {
            effects.push(editor.read(window, argument, Purpose::Project));
        } else if media::kind_of(argument).is_some() {
            effects.push(editor.read(window, argument, Purpose::Import));
        } else if !argument.is_empty() {
            editor.folder = argument.trim_end_matches('/').to_owned();
        }
        (editor, effects)
    }
    pub fn features(&self, mobile: bool) -> Features {
        self.product.features(mobile)
    }
    pub fn title(&self) -> String {
        self.product.name().into()
    }
    pub fn caption(&self) -> String {
        if let Some(job) = &self.export {
            return format!("Exporting {}%", job.percent());
        }
        format!(
            "{} — {}",
            self.project.name,
            self.project.timecode(self.project.duration())
        )
    }

    // ---- time ------------------------------------------------------------------------

    /// The playhead as the world clock has it now.
    pub fn now(&self, clock_us: u64) -> i64 {
        let Some(play) = self.play else {
            return self.playhead;
        };
        let elapsed = clock_us.saturating_sub(play.since_us) as i128;
        let moved =
            elapsed * i128::from(self.project.fps) * i128::from(play.rate) / (100 * 1_000_000);
        let end = self.project.duration();
        (i128::from(play.from) + moved).clamp(0, i128::from(end.max(0))) as i64
    }
    /// Fold elapsed playback into the resting playhead; playback that ran off either end
    /// has stopped there.
    fn settle(&mut self, clock_us: u64) {
        if let Some(play) = self.play {
            let now = self.now(clock_us);
            self.playhead = now;
            let end = self.project.duration();
            if (play.rate > 0 && now >= end) || (play.rate < 0 && now <= 0) {
                self.play = None;
            } else {
                self.play = Some(Play {
                    since_us: clock_us,
                    from: now,
                    rate: play.rate,
                });
            }
        }
    }
    fn start_playing(&mut self, rate: i32, clock_us: u64) {
        if rate > 0 && self.playhead >= self.project.duration() {
            self.playhead = 0;
        }
        self.play = Some(Play {
            since_us: clock_us,
            from: self.playhead,
            rate,
        });
    }
    fn seek(&mut self, frame: i64) {
        self.play = None;
        self.playhead = frame.clamp(0, self.project.duration().max(0));
    }

    // ---- edits -----------------------------------------------------------------------

    /// Apply an edit to the project, recording it for undo if it succeeds.
    fn edit(
        &mut self,
        change: impl FnOnce(&mut Project, &Library) -> Result<(), String>,
    ) -> Result<(), String> {
        let before = self.project.clone();
        change(&mut self.project, &self.library)?;
        if self.project != before {
            self.undo.push(before);
            if self.undo.len() > HISTORY {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.modified = true;
        }
        self.tidy();
        Ok(())
    }
    /// Drop selections that no longer point at anything.
    fn tidy(&mut self) {
        if self
            .selected
            .is_some_and(|id| self.project.clip(id).is_none())
        {
            self.selected = None;
            self.field = Field::None;
        }
        if self
            .selected_transition
            .is_some_and(|id| !self.project.transitions.iter().any(|t| t.id == id))
        {
            self.selected_transition = None;
        }
        if self
            .selected_media
            .is_some_and(|id| !self.project.media.iter().any(|m| m.id == id))
        {
            self.selected_media = None;
        }
        self.playhead = self.playhead.clamp(0, self.project.duration().max(0));
    }
    fn undo(&mut self) -> Result<(), String> {
        let previous = self.undo.pop().ok_or("there is nothing to undo")?;
        self.redo
            .push(std::mem::replace(&mut self.project, previous));
        self.modified = true;
        self.tidy();
        Ok(())
    }
    fn redo(&mut self) -> Result<(), String> {
        let next = self.redo.pop().ok_or("there is nothing to redo")?;
        self.undo.push(std::mem::replace(&mut self.project, next));
        self.modified = true;
        self.tidy();
        Ok(())
    }
    pub fn selected_clip(&self) -> Option<&cw_video::Clip> {
        self.project.clip(self.selected?)
    }
    /// The selected clip's local frame at the playhead, clamped into the clip.
    fn local(&self, clock_us: u64) -> Option<i64> {
        let clip = self.selected_clip()?;
        Some((self.now(clock_us) - clip.start).clamp(0, clip.length - 1))
    }
    /// Track ids top to bottom as a timeline stacks them: video from the highest down,
    /// then audio.
    pub fn lanes(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .project
            .tracks_of(TrackKind::Video)
            .iter()
            .rev()
            .map(|t| t.id)
            .collect();
        v.extend(
            self.project
                .tracks_of(TrackKind::Audio)
                .iter()
                .map(|t| t.id),
        );
        v
    }
    /// Frames per `px` pixels at the current zoom.
    pub fn frames(&self, px: i64) -> i64 {
        let (fps, zoom) = (i64::from(self.project.fps), i64::from(self.zoom.max(1)));
        if px >= 0 {
            (px * fps + zoom / 2) / zoom
        } else {
            -((-px * fps + zoom / 2) / zoom)
        }
    }
    pub fn pixels(&self, frames: i64) -> i64 {
        frames * i64::from(self.zoom) / i64::from(self.project.fps.max(1))
    }
    /// Where a dragged position settles: nearest edge or the playhead within reach.
    fn snapped(&self, frame: i64, exclude: Option<u32>) -> i64 {
        if !self.snapping {
            return frame.max(0);
        }
        let reach = self.frames(SNAP_PX).max(1);
        self.project
            .snap(frame, exclude, self.playhead, reach)
            .max(0)
    }
    /// The track a lane index names, creating a track when the drop is just above the top
    /// video lane or just below the bottom audio lane, the way dropping there adds a
    /// track in every one of these editors.
    fn lane_track(&mut self, lane: i64, kind: TrackKind) -> Result<u32, String> {
        let lanes = self.lanes();
        if lane == -1 && kind == TrackKind::Video {
            return self.project.add_track(TrackKind::Video);
        }
        if lane == lanes.len() as i64 && kind == TrackKind::Audio {
            return self.project.add_track(TrackKind::Audio);
        }
        let id = *lanes
            .get(usize::try_from(lane).map_err(|_| "that is not a track")?)
            .ok_or("that is not a track")?;
        let track = self.project.track(id).ok_or("no such track")?;
        if track.kind != kind {
            // Dropped on the wrong kind of lane: the nearest lane of the right kind.
            return self
                .project
                .tracks_of(kind)
                .first()
                .map(|t| t.id)
                .ok_or_else(|| "no track of that kind".into());
        }
        Ok(id)
    }
    fn media_kind(&self, id: u32) -> Option<MediaKind> {
        self.project
            .media
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.kind)
    }

    // ---- files -----------------------------------------------------------------------

    fn read(&mut self, window: u64, path: &str, purpose: Purpose) -> AppEffect {
        self.reading.retain(|(p, _)| p != path);
        self.reading.push((path.to_owned(), purpose));
        AppEffect::ReadBytes {
            window,
            path: path.to_owned(),
        }
    }
    fn list(&mut self, window: u64, folder: &str, project: bool) -> Vec<AppEffect> {
        self.sheet = Some(Sheet::Browse {
            folder: folder.to_owned(),
            entries: vec![],
            loading: true,
            error: None,
            project,
        });
        vec![AppEffect::ListDirectory {
            window,
            tab: 0,
            path: folder.to_owned(),
        }]
    }
    pub fn listed(&mut self, mut entries: Vec<String>) {
        if let Some(Sheet::Browse {
            entries: e,
            loading,
            error,
            project,
            ..
        }) = &mut self.sheet
        {
            let wanted = |name: &str| {
                name.ends_with('/')
                    || if *project {
                        name.ends_with(&format!(".{PROJECT_EXT}"))
                    } else {
                        media::kind_of(name).is_some()
                    }
            };
            entries.retain(|n| wanted(n) && !n.starts_with('.'));
            entries.sort_by(|a, b| {
                (!a.ends_with('/'))
                    .cmp(&!b.ends_with('/'))
                    .then(a.to_lowercase().cmp(&b.to_lowercase()))
            });
            entries.truncate(LISTING_LIMIT);
            *e = entries;
            *loading = false;
            *error = None;
        }
    }
    pub fn listing_failed(&mut self, reason: &str) {
        if let Some(Sheet::Browse {
            loading,
            error,
            entries,
            ..
        }) = &mut self.sheet
        {
            *loading = false;
            entries.clear();
            *error = Some(reason.to_owned());
        } else {
            self.status = reason.to_owned();
        }
    }
    /// Bytes a read asked for.
    pub fn bytes(
        &mut self,
        window: u64,
        path: &str,
        result: Result<Vec<u8>, String>,
    ) -> Result<Vec<AppEffect>, String> {
        let at = self
            .reading
            .iter()
            .position(|(p, _)| p == path)
            .ok_or("nothing was waiting for that file")?;
        let (_, purpose) = self.reading.remove(at);
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        match purpose {
            Purpose::Import | Purpose::ImportAdd => {
                let bytes = match result {
                    Ok(b) => b,
                    Err(e) => {
                        self.status = format!("Cannot import {name}: {e}");
                        return Ok(vec![]);
                    }
                };
                match Media::import(path, &bytes, self.project.sample_rate) {
                    Ok(m) => {
                        let kind = m.kind;
                        let before = self.project.clone();
                        let id = self.project.add_media(path, kind);
                        if self.project != before {
                            self.undo.push(before);
                            self.redo.clear();
                            self.modified = true;
                        }
                        self.library.insert(id, m);
                        self.offline.remove(&id);
                        self.selected_media = Some(id);
                        self.status = format!("Imported {name}");
                        if purpose == Purpose::ImportAdd {
                            self.sheet = None;
                            return self.command(window, &format!("append:{id}"), 0);
                        }
                    }
                    Err(e) => self.status = format!("Cannot import {name}: {e}"),
                }
                Ok(vec![])
            }
            Purpose::Relink(id) => {
                match result.and_then(|b| Media::import(path, &b, self.project.sample_rate)) {
                    Ok(m) => {
                        self.library.insert(id, m);
                        self.offline.remove(&id);
                    }
                    Err(e) => {
                        self.offline.insert(id, e);
                    }
                }
                Ok(vec![])
            }
            Purpose::Project => {
                let text = match result {
                    Ok(b) => String::from_utf8(b).map_err(|_| "not a project file".to_owned()),
                    Err(e) => Err(e),
                };
                let project = match text.and_then(|t| Project::from_json(&t)) {
                    Ok(p) => p,
                    Err(e) => {
                        self.status = format!("Cannot open {name}: {e}");
                        return Ok(vec![]);
                    }
                };
                self.project = project;
                self.path = path.to_owned();
                self.modified = false;
                self.undo.clear();
                self.redo.clear();
                self.library.clear();
                self.offline.clear();
                self.selected = None;
                self.selected_media = None;
                self.selected_transition = None;
                self.play = None;
                self.playhead = 0;
                self.scroll = 0;
                self.status = format!("Opened {name}");
                let refs = self.project.media.clone();
                let mut effects: Vec<AppEffect> = refs
                    .iter()
                    .map(|m| self.read(window, &m.path, Purpose::Relink(m.id)))
                    .collect();
                effects.extend(self.request_masks(window));
                Ok(effects)
            }
        }
    }
    /// A write finished (or failed).
    pub fn saved(&mut self, path: &str, result: Result<(), String>) {
        let name = path.rsplit('/').next().unwrap_or(path);
        match result {
            Ok(()) if path == self.path => {
                self.modified = false;
                self.status = format!("Saved {name}");
            }
            Ok(()) => self.status = format!("Exported {name}"),
            Err(e) => self.status = format!("Could not write {name}: {e}"),
        }
    }
    fn save_to(&mut self, window: u64, path: String) -> Vec<AppEffect> {
        self.path = path.clone();
        self.sheet = None;
        self.field = Field::None;
        let mut effects = vec![];
        if let Some((folder, _)) = path.rsplit_once('/') {
            effects.push(AppEffect::CreateDirectory {
                window,
                path: folder.to_owned(),
            });
        }
        effects.push(AppEffect::WriteBytes {
            window,
            path,
            bytes: self.project.to_json().into_bytes(),
        });
        effects
    }

    // ---- titles ----------------------------------------------------------------------

    /// Ask the renderer for every title line not yet rasterised.
    fn request_masks(&mut self, window: u64) -> Vec<AppEffect> {
        let mut effects = vec![];
        let titles: Vec<Title> = self
            .project
            .clips
            .iter()
            .filter_map(|c| match &c.source {
                Source::Title(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        for t in titles {
            let key = t.raster_key();
            if t.text.is_empty()
                || self.masks.contains_key(&key)
                || self.pending_masks.contains(&key)
            {
                continue;
            }
            self.pending_masks.push(key);
            effects.push(AppEffect::RasterText {
                window,
                text: t.text.clone(),
                size: t.size,
                bold: t.bold,
            });
        }
        // Lines no title uses any more are dropped, so a snapshot does not keep every
        // intermediate spelling of a title that was typed out.
        let used: Vec<String> = self
            .project
            .clips
            .iter()
            .chain(self.undo.iter().flat_map(|p| p.clips.iter()))
            .chain(self.redo.iter().flat_map(|p| p.clips.iter()))
            .filter_map(|c| match &c.source {
                Source::Title(t) => Some(t.raster_key()),
                _ => None,
            })
            .collect();
        self.masks.retain(|k, _| used.contains(k));
        effects
    }
    pub fn text_rasterized(
        &mut self,
        width: u32,
        height: u32,
        alpha: Vec<u8>,
    ) -> Result<(), String> {
        let key = if self.pending_masks.is_empty() {
            return Err("no title text was being drawn".into());
        } else {
            self.pending_masks.remove(0)
        };
        if alpha.len() != width as usize * height as usize {
            return Err("glyph coverage does not match its size".into());
        }
        self.text_problem = None;
        self.masks.insert(
            key,
            TextMask {
                width,
                height,
                alpha: cw_video::Blob::new(alpha),
            },
        );
        Ok(())
    }
    pub fn text_failed(&mut self, reason: &str) {
        if !self.pending_masks.is_empty() {
            self.pending_masks.remove(0);
        }
        self.text_problem = Some(reason.to_owned());
    }
    fn title_mut(&mut self) -> Result<&mut Title, String> {
        let id = self.selected.ok_or("no title is selected")?;
        match &mut self.project.clip_mut(id)?.source {
            Source::Title(t) => Ok(t),
            _ => Err("the selected clip is not a title".into()),
        }
    }
    fn edit_title(
        &mut self,
        window: u64,
        change: impl FnOnce(&mut Title) -> Result<(), String>,
    ) -> Result<Vec<AppEffect>, String> {
        let id = self.selected.ok_or("no title is selected")?;
        let track = self.project.clip(id).ok_or("no such clip")?.track;
        if self.project.track(track).is_some_and(|t| t.locked) {
            return Err("that track is locked".into());
        }
        let before = self.project.clone();
        change(self.title_mut()?)?;
        let text = self.title_mut()?.text.clone();
        if let Ok(c) = self.project.clip_mut(id) {
            c.name = text;
        }
        if self.project != before {
            self.undo.push(before);
            if self.undo.len() > HISTORY {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.modified = true;
        }
        Ok(self.request_masks(window))
    }

    // ---- properties ------------------------------------------------------------------

    /// The selected clip's value of `prop` at the playhead.
    pub fn value(&self, prop: Prop, clock_us: u64) -> Option<i32> {
        if prop == Prop::Transition {
            let id = self.selected_transition?;
            return self
                .project
                .transitions
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.frames as i32);
        }
        let c = self.selected_clip()?;
        let local = self.local(clock_us).unwrap_or(0);
        Some(match prop {
            Prop::Opacity => c.opacity.at(local),
            Prop::X => c.x.at(local),
            Prop::Y => c.y.at(local),
            Prop::Scale => c.scale.at(local),
            Prop::Rotation => c.rotation.at(local),
            Prop::Volume => c.volume.at(local),
            Prop::Brightness => c.grade.brightness,
            Prop::Contrast => c.grade.contrast,
            Prop::Saturation => c.grade.saturation,
            Prop::Temperature => c.grade.temperature,
            Prop::Speed => c.speed as i32,
            Prop::FadeIn => c.fade_in as i32,
            Prop::FadeOut => c.fade_out as i32,
            Prop::CropLeft => i32::from(c.crop.left),
            Prop::CropTop => i32::from(c.crop.top),
            Prop::CropRight => i32::from(c.crop.right),
            Prop::CropBottom => i32::from(c.crop.bottom),
            Prop::TitleSize => match &c.source {
                Source::Title(t) => i32::from(t.size),
                _ => return None,
            },
            Prop::Transition => unreachable!(),
        })
    }
    /// The range a slider for `prop` spans.
    pub fn range(&self, prop: Prop) -> (i32, i32) {
        let (w, h) = (self.project.width as i32, self.project.height as i32);
        let fps = self.project.fps as i32;
        match prop {
            Prop::Opacity => (0, 1000),
            Prop::X => (-w, w),
            Prop::Y => (-h, h),
            Prop::Scale => (100, 4000),
            Prop::Rotation => (-18_000, 18_000),
            Prop::Volume => (0, 200),
            Prop::Brightness | Prop::Contrast | Prop::Saturation | Prop::Temperature => (-100, 100),
            Prop::Speed => (
                cw_video::project::MIN_SPEED as i32,
                cw_video::project::MAX_SPEED as i32,
            ),
            Prop::FadeIn | Prop::FadeOut => (0, 5 * fps),
            Prop::CropLeft | Prop::CropTop | Prop::CropRight | Prop::CropBottom => (0, 450),
            Prop::TitleSize => (8, 96),
            Prop::Transition => (1, 4 * fps),
        }
    }
    fn allowed(&self, prop: Prop) -> Result<(), String> {
        let f = self.product.features(false);
        let ok = match prop {
            Prop::X | Prop::Y | Prop::Scale | Prop::Rotation => f.transform || f.ken_burns,
            Prop::Opacity => f.opacity,
            Prop::Brightness | Prop::Contrast | Prop::Saturation | Prop::Temperature => f.color,
            Prop::CropLeft | Prop::CropTop | Prop::CropRight | Prop::CropBottom => f.crop,
            _ => true,
        };
        if ok {
            Ok(())
        } else {
            Err(format!("{} has no such control", self.product.name()))
        }
    }
    /// Set `prop` on the selected clip (or transition) to `value`.
    fn set_prop(
        &mut self,
        prop: Prop,
        value: i32,
        clock_us: u64,
        record: bool,
    ) -> Result<(), String> {
        self.allowed(prop)?;
        let (lo, hi) = self.range(prop);
        let value = value.clamp(lo, hi);
        if prop == Prop::Transition {
            let id = self
                .selected_transition
                .ok_or("no transition is selected")?;
            let left = self
                .project
                .transitions
                .iter()
                .find(|t| t.id == id)
                .map(|t| (t.left, t.kind))
                .ok_or("no such transition")?;
            let change = |p: &mut Project, _: &Library| {
                p.add_transition(left.0, left.1, value as u32).map(|_| ())
            };
            self.edit_or_live(record, change)?;
            // Re-adding gave the transition a new id; keep it selected.
            self.selected_transition = self
                .project
                .transitions
                .iter()
                .find(|t| t.left == left.0)
                .map(|t| t.id);
            return Ok(());
        }
        let id = self.selected.ok_or("no clip is selected")?;
        let local = self.local(clock_us).unwrap_or(0);
        if prop == Prop::Speed {
            return self.edit_or_live(record, |p, lib| p.set_speed(id, value as u32, lib));
        }
        if prop == Prop::TitleSize {
            let before = self.project.clone();
            self.title_mut()?.size = value as u16;
            if record && self.project != before {
                self.undo.push(before);
                self.redo.clear();
            }
            self.modified = true;
            return Ok(());
        }
        let track = self.project.clip(id).ok_or("no such clip")?.track;
        if self.project.track(track).is_some_and(|t| t.locked) {
            return Err("that track is locked".into());
        }
        let is_audio = self.project.track(track).map(|t| t.kind) == Some(TrackKind::Audio);
        if is_audio
            && !matches!(
                prop,
                Prop::Volume | Prop::FadeIn | Prop::FadeOut | Prop::Speed
            )
        {
            return Err("sound has no picture to change".into());
        }
        self.edit_or_live(record, |p, _| {
            let c = p.clip_mut(id)?;
            match prop {
                Prop::Opacity => c.opacity.set(local, value),
                Prop::X => c.x.set(local, value),
                Prop::Y => c.y.set(local, value),
                Prop::Scale => c.scale.set(local, value),
                Prop::Rotation => c.rotation.set(local, value),
                Prop::Volume => c.volume.set(local, value),
                Prop::Brightness => c.grade.brightness = value,
                Prop::Contrast => c.grade.contrast = value,
                Prop::Saturation => c.grade.saturation = value,
                Prop::Temperature => c.grade.temperature = value,
                Prop::FadeIn => c.fade_in = (value as u32).min(c.length as u32),
                Prop::FadeOut => c.fade_out = (value as u32).min(c.length as u32),
                Prop::CropLeft => c.crop.left = value as u16,
                Prop::CropTop => c.crop.top = value as u16,
                Prop::CropRight => c.crop.right = value as u16,
                Prop::CropBottom => c.crop.bottom = value as u16,
                Prop::Speed | Prop::TitleSize | Prop::Transition => unreachable!(),
            }
            Ok(())
        })
    }
    /// An edit either recorded for undo, or applied live during a drag whose start was
    /// already recorded.
    fn edit_or_live(
        &mut self,
        record: bool,
        change: impl FnOnce(&mut Project, &Library) -> Result<(), String>,
    ) -> Result<(), String> {
        if record {
            self.edit(change)
        } else {
            change(&mut self.project, &self.library)?;
            self.modified = true;
            self.tidy();
            Ok(())
        }
    }
    fn checkpoint(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    fn param_mut(&mut self, prop: Prop) -> Result<&mut Param, String> {
        let id = self.selected.ok_or("no clip is selected")?;
        let c = self.project.clip_mut(id)?;
        Ok(match prop {
            Prop::Opacity => &mut c.opacity,
            Prop::X => &mut c.x,
            Prop::Y => &mut c.y,
            Prop::Scale => &mut c.scale,
            Prop::Rotation => &mut c.rotation,
            Prop::Volume => &mut c.volume,
            _ => return Err("that property cannot be keyframed".into()),
        })
    }
    /// Whether the selected clip has a keyframe for `prop` at the playhead.
    pub fn keyed(&self, prop: Prop, clock_us: u64) -> bool {
        let (Some(c), Some(local)) = (self.selected_clip(), self.local(clock_us)) else {
            return false;
        };
        let p = match prop {
            Prop::Opacity => &c.opacity,
            Prop::X => &c.x,
            Prop::Y => &c.y,
            Prop::Scale => &c.scale,
            Prop::Rotation => &c.rotation,
            Prop::Volume => &c.volume,
            _ => return false,
        };
        p.keys.iter().any(|k| k.frame == local)
    }

    // ---- commands --------------------------------------------------------------------

    pub fn command(
        &mut self,
        window: u64,
        command: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let (head, rest) = command.split_once(':').unwrap_or((command, ""));
        let features = self.product.features(false);
        // Anything but a transport control stops playback where it is.
        if !matches!(head, "play" | "shuttle" | "noop") {
            self.settle(clock_us);
        }
        match head {
            "noop" => {}
            // Transport.
            "play" => {
                if self.play.is_some() {
                    self.settle(clock_us);
                    self.play = None;
                } else {
                    self.start_playing(100, clock_us);
                }
            }
            "stop" => self.seek(0),
            "start" => self.seek(0),
            "end" => self.seek(self.project.duration()),
            "step" => {
                let by: i64 = parse(rest, "step")?;
                self.seek(self.playhead + by);
            }
            "skip" => {
                let by: i64 = parse(rest, "skip")?;
                self.seek(self.playhead + by * SKIP_SECONDS * i64::from(self.project.fps));
            }
            "seek" => {
                let frame: i64 = parse(rest, "frame")?;
                self.seek(frame);
            }
            "shuttle" => {
                if !features.shuttle {
                    return Err(format!("{} has no shuttle control", self.product.name()));
                }
                self.shuttle(rest, clock_us)?;
            }
            // Editing.
            "undo" => self.undo()?,
            "redo" => self.redo()?,
            "split" => {
                let at = self.playhead;
                // The selected clip when the playhead is inside it; otherwise every clip
                // the playhead crosses on an unlocked track.
                let inside = |c: &cw_video::Clip| c.start < at && at < c.end();
                let targets: Vec<u32> = match self.selected_clip().filter(|c| inside(c)) {
                    Some(c) => vec![c.id],
                    None => self
                        .project
                        .clips
                        .iter()
                        .filter(|c| c.start < at && at < c.end())
                        .filter(|c| !self.project.track(c.track).is_some_and(|t| t.locked))
                        .map(|c| c.id)
                        .collect(),
                };
                if targets.is_empty() {
                    return Err("there is no clip under the playhead to split".into());
                }
                self.edit(|p, _| {
                    for id in targets {
                        p.split(id, at)?;
                    }
                    Ok(())
                })?;
            }
            "delete" => {
                if let Some(t) = self.selected_transition {
                    self.edit(|p, _| p.remove_transition(t))?;
                    return Ok(vec![]);
                }
                let id = self.selected.ok_or("nothing is selected")?;
                if features.magnetic {
                    self.edit(|p, _| p.ripple_delete(id))?;
                } else {
                    self.edit(|p, _| p.delete(id))?;
                }
            }
            "ripple-delete" => {
                let id = self.selected.ok_or("no clip is selected")?;
                self.edit(|p, _| p.ripple_delete(id))?;
            }
            "deselect" => {
                self.selected = None;
                self.selected_transition = None;
                self.field = Field::None;
            }
            "zoom-in" => self.zoom = (self.zoom * 3 / 2).min(ZOOM_MAX),
            "zoom-out" => self.zoom = (self.zoom * 2 / 3).max(ZOOM_MIN),
            "zoom-fit" => {
                let width: i64 = parse(rest, "width")?;
                let seconds = (self.project.duration() / i64::from(self.project.fps)).max(1) + 1;
                self.zoom = ((width / seconds) as u32).clamp(ZOOM_MIN, ZOOM_MAX);
                self.scroll = 0;
            }
            "scroll" => {
                let to: i64 = parse(rest, "scroll")?;
                self.scroll = to.clamp(0, self.project.duration().max(0));
            }
            "snap" => self.snapping = !self.snapping,
            // Selection (a click on a drag surface arrives here too).
            "clip" => {
                let id: u32 = parse(rest.split(':').next().unwrap_or(""), "clip")?;
                self.project.clip(id).ok_or("no such clip")?;
                self.selected = Some(id);
                self.selected_transition = None;
                self.field = Field::None;
            }
            "media" => {
                let id: u32 = parse(rest.split(':').next().unwrap_or(""), "media")?;
                if !self.project.media.iter().any(|m| m.id == id) {
                    return Err("no such media".into());
                }
                self.selected_media = Some(id);
            }
            "transition" => {
                let id: u32 = parse(rest, "transition")?;
                if !self.project.transitions.iter().any(|t| t.id == id) {
                    return Err("no such transition".into());
                }
                self.selected_transition = Some(id);
                self.selected = None;
            }
            "trim-in" | "trim-out" => {
                let id: u32 = parse(rest.split(':').next().unwrap_or(""), "clip")?;
                self.project.clip(id).ok_or("no such clip")?;
                self.selected = Some(id);
            }
            "ruler" => {
                // A click without an offset lands on the first frame showing.
                let scroll: i64 = parse(rest.split(':').next().unwrap_or("0"), "scroll")?;
                self.seek(scroll);
            }
            "slider" | "zoom" => {
                // A slider clicked with no position keeps its value.
            }
            // Adding to the timeline.
            "append" => {
                let id: u32 = parse(rest, "media")?;
                let kind = self.media_kind(id).ok_or("no such media")?;
                if !self.library.contains_key(&id) {
                    return Err("that media is offline".into());
                }
                let track_kind = if kind == MediaKind::Audio {
                    TrackKind::Audio
                } else {
                    TrackKind::Video
                };
                let track = self.project.tracks_of(track_kind)[0].id;
                // Phones and iMovie add at the playhead; the others add at the end.
                let at = match self.product {
                    Product::Imovie | Product::VideoEditor => self.playhead,
                    _ => self.project.on_track(track).last().map_or(0, |c| c.end()),
                };
                let mut new = 0;
                self.edit(|p, lib| {
                    new = p.insert(lib, Source::Media { media: id }, track, at)?;
                    Ok(())
                })?;
                self.selected = Some(new);
            }
            "overlay" => {
                // A picture laid over the main track at the playhead: picture in picture.
                let id: u32 = parse(rest, "media")?;
                if self.media_kind(id).is_none_or(|k| !k.visual()) {
                    return Err("only pictures can be overlaid".into());
                }
                if !self.library.contains_key(&id) {
                    return Err("that media is offline".into());
                }
                let at = self.playhead;
                let mut new = 0;
                self.edit(|p, lib| {
                    let track = match p.tracks_of(TrackKind::Video).get(1) {
                        Some(t) => t.id,
                        None => p.add_track(TrackKind::Video)?,
                    };
                    new = p.insert(lib, Source::Media { media: id }, track, at)?;
                    let (w, h) = (p.width as i32, p.height as i32);
                    let c = p.clip_mut(new)?;
                    c.scale = Param::fixed(400);
                    c.x = Param::fixed(w / 4 + 4);
                    c.y = Param::fixed(-h / 4 - 2);
                    Ok(())
                })?;
                self.selected = Some(new);
            }
            "media-remove" => {
                let id: u32 = parse(rest, "media")?;
                self.edit(|p, _| p.remove_media(id))?;
                self.library.remove(&id);
                self.offline.remove(&id);
            }
            "add-title" => {
                let style = rest;
                let (text, size, bold, background, position) = match style {
                    "lower" => (
                        "Your Name",
                        14,
                        true,
                        Some("000000b0".to_owned()),
                        TitlePosition::Lower,
                    ),
                    "headline" => ("Headline", 28, true, None, TitlePosition::Center),
                    "plain" | "" => ("Title", 20, false, None, TitlePosition::Center),
                    "top" => ("Caption", 14, false, None, TitlePosition::Top),
                    "credits" => (
                        "The End",
                        22,
                        true,
                        Some("000000ff".to_owned()),
                        TitlePosition::Center,
                    ),
                    _ => return Err(format!("unknown title style {style}")),
                };
                let at = self.playhead;
                let mut new = 0;
                self.edit(|p, lib| {
                    // Titles sit on the top video track, on a new one if that is the main.
                    let track = match p.tracks_of(TrackKind::Video).get(1) {
                        Some(t) => p.tracks_of(TrackKind::Video).last().map_or(t.id, |t| t.id),
                        None => p.add_track(TrackKind::Video)?,
                    };
                    let title = Title {
                        text: text.into(),
                        size,
                        bold,
                        color: "ffffff".into(),
                        background,
                        position,
                    };
                    new = p.insert(lib, Source::Title(title), track, at)?;
                    Ok(())
                })?;
                self.selected = Some(new);
                self.field = Field::Title;
                return Ok(self.request_masks(window));
            }
            "add-color" => {
                let hex = rest.trim_start_matches('#');
                cw_raster::parse_hex(hex).ok_or("invalid colour")?;
                let at = self.playhead;
                let track = self.project.tracks_of(TrackKind::Video)[0].id;
                let mut new = 0;
                let color = hex.to_owned();
                self.edit(|p, lib| {
                    new = p.insert(lib, Source::Color { color }, track, at)?;
                    Ok(())
                })?;
                self.selected = Some(new);
            }
            "add-transition" => {
                let kind = TransitionKind::parse(rest).ok_or("unknown transition")?;
                let left = self.transition_site()?;
                let frames = self.project.fps;
                let mut id = 0;
                self.edit(|p, _| {
                    id = p.add_transition(left, kind, frames)?;
                    Ok(())
                })?;
                self.selected_transition = Some(id);
                self.selected = None;
            }
            // The selected clip's properties.
            "set" => {
                let (prop, value) = rest.split_once(':').ok_or("set needs a value")?;
                let prop = Prop::parse(prop).ok_or("unknown property")?;
                let value: i32 = parse(value, "value")?;
                self.set_prop(prop, value, clock_us, true)?;
            }
            "nudge" => {
                let (prop, by) = rest.split_once(':').ok_or("nudge needs an amount")?;
                let prop = Prop::parse(prop).ok_or("unknown property")?;
                let by: i32 = parse(by, "amount")?;
                let now = self.value(prop, clock_us).ok_or("nothing is selected")?;
                self.set_prop(prop, now + by, clock_us, true)?;
            }
            "key" => {
                if !features.keyframes {
                    return Err(format!("{} has no keyframes", self.product.name()));
                }
                let prop = Prop::parse(rest)
                    .filter(|p| p.animatable())
                    .ok_or("that property cannot be keyframed")?;
                self.allowed(prop)?;
                let local = self.local(clock_us).ok_or("no clip is selected")?;
                let keyed = self.keyed(prop, clock_us);
                let value = self.value(prop, clock_us).unwrap_or(0);
                let id = self.selected.ok_or("no clip is selected")?;
                self.edit(|p, _| {
                    let c = p.clip_mut(id)?;
                    let param = match prop {
                        Prop::Opacity => &mut c.opacity,
                        Prop::X => &mut c.x,
                        Prop::Y => &mut c.y,
                        Prop::Scale => &mut c.scale,
                        Prop::Rotation => &mut c.rotation,
                        _ => &mut c.volume,
                    };
                    if keyed {
                        param.remove_key(local)
                    } else {
                        param.set_key(local, value, Ease::Linear)
                    }
                })?;
            }
            "ease" => {
                if !features.keyframes {
                    return Err(format!("{} has no keyframes", self.product.name()));
                }
                let prop = Prop::parse(rest)
                    .filter(|p| p.animatable())
                    .ok_or("that property cannot be keyframed")?;
                let local = self.local(clock_us).ok_or("no clip is selected")?;
                self.checkpoint();
                let param = self.param_mut(prop)?;
                let key = param
                    .keys
                    .iter_mut()
                    .find(|k| k.frame == local)
                    .ok_or("there is no keyframe at the playhead")?;
                key.ease = if key.ease == Ease::Linear {
                    Ease::Ease
                } else {
                    Ease::Linear
                };
                self.modified = true;
            }
            "reverse" => {
                if !features.reverse {
                    return Err(format!(
                        "{} cannot play a clip backwards",
                        self.product.name()
                    ));
                }
                let id = self.selected.ok_or("no clip is selected")?;
                let now = self.project.clip(id).ok_or("no such clip")?.reverse;
                self.edit(|p, _| p.set_reverse(id, !now))?;
            }
            "rotate" => {
                // A quarter turn clockwise, as the crop tools' rotate buttons do.
                let now = self
                    .value(Prop::Rotation, clock_us)
                    .ok_or("no clip is selected")?;
                let next = if now >= 18_000 {
                    now - 27_000
                } else {
                    now + 9000
                };
                self.set_prop(Prop::Rotation, next, clock_us, true)?;
            }
            "fit" | "fill" | "ken-burns" => self.framing(head, clock_us)?,
            "pip" => {
                if !features.transform {
                    return Err(format!("{} has no picture in picture", self.product.name()));
                }
                let id = self.selected.ok_or("no clip is selected")?;
                let (w, h) = (self.project.width as i32, self.project.height as i32);
                let (sx, sy) = match rest {
                    "top-left" => (-1, -1),
                    "top-right" => (1, -1),
                    "bottom-left" => (-1, 1),
                    "bottom-right" | "" => (1, 1),
                    _ => return Err("unknown corner".into()),
                };
                self.edit(|p, _| {
                    let c = p.clip_mut(id)?;
                    c.scale = Param::fixed(400);
                    c.x = Param::fixed(sx * (w * 3 / 10));
                    c.y = Param::fixed(sy * (h * 3 / 10));
                    Ok(())
                })?;
            }
            "reset" => {
                let id = self.selected.ok_or("no clip is selected")?;
                self.edit(|p, _| {
                    let c = p.clip_mut(id)?;
                    c.opacity = Param::fixed(1000);
                    c.x = Param::fixed(0);
                    c.y = Param::fixed(0);
                    c.scale = Param::fixed(1000);
                    c.rotation = Param::fixed(0);
                    c.crop = Default::default();
                    c.grade = Default::default();
                    Ok(())
                })?;
            }
            "mute-clip" => {
                let id = self.selected.ok_or("no clip is selected")?;
                let now = self.value(Prop::Volume, clock_us).unwrap_or(100);
                self.set_prop(Prop::Volume, if now == 0 { 100 } else { 0 }, clock_us, true)?;
                let _ = id;
            }
            // Titles.
            "title-text" => {
                self.title_mut()?;
                self.field = Field::Title;
            }
            "title-bold" => {
                return self.edit_title(window, |t| {
                    t.bold = !t.bold;
                    Ok(())
                })
            }
            "title-color" => {
                let hex = rest.trim_start_matches('#').to_owned();
                cw_raster::parse_hex(&hex).ok_or("invalid colour")?;
                return self.edit_title(window, |t| {
                    t.color = hex;
                    Ok(())
                });
            }
            "title-bg" => {
                let bg = match rest {
                    "none" | "" => None,
                    hex => {
                        cw_raster::parse_hex(hex).ok_or("invalid colour")?;
                        Some(hex.to_owned())
                    }
                };
                return self.edit_title(window, |t| {
                    t.background = bg;
                    Ok(())
                });
            }
            "title-pos" => {
                let pos = TitlePosition::parse(rest).ok_or("unknown title position")?;
                return self.edit_title(window, |t| {
                    t.position = pos;
                    Ok(())
                });
            }
            "title-size" => {
                let by: i32 = parse(rest, "size")?;
                return self.edit_title(window, |t| {
                    t.size = (i32::from(t.size) + by).clamp(8, 96) as u16;
                    Ok(())
                });
            }
            // Tracks.
            "track-mute" | "track-hide" | "track-lock" => {
                let allowed = match head {
                    "track-mute" => features.track_mute,
                    "track-hide" => features.track_hide,
                    _ => features.track_lock,
                };
                if !allowed {
                    return Err(format!("{} has no such track control", self.product.name()));
                }
                let id: u32 = parse(rest, "track")?;
                let before = self.project.clone();
                let t = self.project.track_mut(id)?;
                match head {
                    "track-mute" => t.muted = !t.muted,
                    "track-hide" => t.hidden = !t.hidden,
                    _ => t.locked = !t.locked,
                }
                self.undo.push(before);
                self.redo.clear();
                self.modified = true;
            }
            "add-track" => {
                let kind = match rest {
                    "video" => TrackKind::Video,
                    "audio" => TrackKind::Audio,
                    _ => return Err("unknown track kind".into()),
                };
                self.edit(|p, _| p.add_track(kind).map(|_| ()))?;
            }
            // Tabs.
            "tab" => self.tab = rest.to_owned(),
            "inspector" => self.inspector = rest.to_owned(),
            // Files.
            "import" => {
                let folder = if rest.is_empty() {
                    self.folder.clone()
                } else {
                    rest.to_owned()
                };
                return Ok(self.list(window, &folder, false));
            }
            "open" => {
                let folder = self.folder.clone();
                return Ok(self.list(window, &folder, true));
            }
            "browse" => {
                let (folder, project) = match &self.sheet {
                    Some(Sheet::Browse {
                        folder, project, ..
                    }) => (folder.clone(), *project),
                    _ => return Err("no file sheet is showing".into()),
                };
                let next = match rest {
                    ".." => folder
                        .trim_end_matches('/')
                        .rsplit_once('/')
                        .map(|(p, _)| {
                            if p.is_empty() {
                                "/".to_owned()
                            } else {
                                p.to_owned()
                            }
                        })
                        .unwrap_or_else(|| ".".into()),
                    name => join(&folder, name.trim_end_matches('/')),
                };
                return Ok(self.list(window, &next, project));
            }
            "pick" | "pick-add" => {
                let (folder, entries, project) = match &self.sheet {
                    Some(Sheet::Browse {
                        folder,
                        entries,
                        project,
                        ..
                    }) => (folder.clone(), entries.clone(), *project),
                    _ => return Err("no file sheet is showing".into()),
                };
                if !entries.iter().any(|e| e == rest) {
                    return Err("that file is not in the folder".into());
                }
                let path = join(&folder, rest);
                self.folder = folder;
                if project {
                    self.sheet = None;
                    return Ok(vec![self.read(window, &path, Purpose::Project)]);
                }
                // The sheet stays open, as a media browser does, for more imports; a
                // phone's picker adds the clip to the movie and closes.
                let purpose = if head == "pick-add" {
                    Purpose::ImportAdd
                } else {
                    Purpose::Import
                };
                return Ok(vec![self.read(window, &path, purpose)]);
            }
            "close-sheet" => {
                self.sheet = None;
                self.field = Field::None;
            }
            "new" => {
                self.project = Project::new("My Movie", 320, 180, 24, 22_050);
                self.path.clear();
                self.library.clear();
                self.offline.clear();
                self.undo.clear();
                self.redo.clear();
                self.modified = false;
                self.selected = None;
                self.selected_media = None;
                self.selected_transition = None;
                self.playhead = 0;
                self.play = None;
                self.masks.clear();
            }
            "save" => {
                if self.path.is_empty() {
                    self.sheet = Some(Sheet::Save {
                        name: self.project.name.clone(),
                    });
                    self.field = Field::Name;
                } else {
                    let path = self.path.clone();
                    return Ok(self.save_to(window, path));
                }
            }
            "save-as" => {
                self.sheet = Some(Sheet::Save {
                    name: self.project.name.clone(),
                });
                self.field = Field::Name;
            }
            "name" => self.field = Field::Name,
            "project-name" => self.field = Field::Project,
            "save-confirm" => {
                let Some(Sheet::Save { name }) = &self.sheet else {
                    return Err("the save sheet is not showing".into());
                };
                let name = clean_name(name);
                if name.is_empty() {
                    return Err("a project needs a name".into());
                }
                self.project.name = name.clone();
                let path = join(&self.folder, &format!("{name}.{PROJECT_EXT}"));
                return Ok(self.save_to(window, path));
            }
            "export" => {
                if self.export.is_some() {
                    return Err("an export is already running".into());
                }
                if self.project.duration() == 0 {
                    return Err("there is nothing on the timeline to export".into());
                }
                self.sheet = Some(Sheet::Export {
                    name: self.project.name.clone(),
                    width: self.export_size.0,
                    height: self.export_size.1,
                    fps: self.export_fps,
                });
                self.field = Field::Name;
            }
            "export-size" => {
                let (w, h) = rest.split_once('x').ok_or("size is WxH")?;
                let (w, h): (u32, u32) = (parse(w, "width")?, parse(h, "height")?);
                if !EXPORT_SIZES.contains(&(w, h)) {
                    return Err("that size is not offered".into());
                }
                match &mut self.sheet {
                    Some(Sheet::Export { width, height, .. }) => (*width, *height) = (w, h),
                    _ => return Err("the export sheet is not showing".into()),
                }
                self.export_size = (w, h);
            }
            "export-fps" => {
                let f: u32 = parse(rest, "frame rate")?;
                if !EXPORT_RATES.contains(&f) {
                    return Err("that frame rate is not offered".into());
                }
                match &mut self.sheet {
                    Some(Sheet::Export { fps, .. }) => *fps = f,
                    _ => return Err("the export sheet is not showing".into()),
                }
                self.export_fps = f;
            }
            "export-start" => {
                let Some(Sheet::Export {
                    name,
                    width,
                    height,
                    fps,
                }) = self.sheet.clone()
                else {
                    return Err("the export sheet is not showing".into());
                };
                let name = clean_name(&name);
                if name.is_empty() {
                    return Err("the movie needs a name".into());
                }
                let video = join(&self.folder, &format!("{name}.apng"));
                let sound = join(&self.folder, &format!("{name}.wav"));
                self.export = Some(Export::new(
                    &self.project,
                    &video,
                    &sound,
                    width,
                    height,
                    fps,
                )?);
                self.sheet = None;
                self.field = Field::None;
                self.status = format!("Exporting {name}…");
                return Ok(vec![AppEffect::CreateDirectory {
                    window,
                    path: self.folder.clone(),
                }]);
            }
            "export-cancel" => {
                self.export.take().ok_or("no export is running")?;
                self.status = "Export cancelled".into();
            }
            other => return Err(format!("unknown video command {other}")),
        }
        Ok(vec![])
    }

    /// The clip a new transition goes after: the selected clip if another follows it,
    /// else the cut nearest the playhead on the main track.
    fn transition_site(&self) -> Result<u32, String> {
        if let Some(id) = self.selected {
            if self.project.next_touching(id).is_some() {
                return Ok(id);
            }
        }
        let at = self.playhead;
        self.project
            .clips
            .iter()
            .filter(|c| {
                self.project.track(c.track).map(|t| t.kind) == Some(TrackKind::Video)
                    && self.project.next_touching(c.id).is_some()
            })
            .min_by_key(|c| ((c.end() - at).abs(), c.id))
            .map(|c| c.id)
            .ok_or_else(|| "a transition goes between two clips that touch".into())
    }

    /// iMovie's cropping modes: Fit, Crop to Fill and Ken Burns.
    fn framing(&mut self, mode: &str, _clock_us: u64) -> Result<(), String> {
        let f = self.product.features(false);
        if !(f.ken_burns || f.transform) || (mode == "ken-burns" && !f.ken_burns) {
            return Err(format!("{} has no such framing", self.product.name()));
        }
        let id = self.selected.ok_or("no clip is selected")?;
        let clip = self.project.clip(id).ok_or("no such clip")?;
        let fill = match clip.media().and_then(|m| self.library.get(&m)) {
            Some(m) if m.width > 0 && m.height > 0 => {
                // The scale that covers the frame: fit's shortfall on the other axis.
                let (w, h) = (
                    i64::from(self.project.width),
                    i64::from(self.project.height),
                );
                let (mw, mh) = (i64::from(m.width), i64::from(m.height));
                let fit_w = w * mh <= h * mw; // fitted to width
                let cover = if fit_w {
                    (h * mw * 1000).div_euclid(w * mh).max(1000)
                } else {
                    (w * mh * 1000).div_euclid(h * mw).max(1000)
                };
                cover as i32
            }
            _ => 1000,
        };
        let length = clip.length;
        self.edit(|p, _| {
            let c = p.clip_mut(id)?;
            c.x = Param::fixed(0);
            c.y = Param::fixed(0);
            c.rotation = Param::fixed(0);
            match mode {
                "fit" => c.scale = Param::fixed(1000),
                "fill" => c.scale = Param::fixed(fill),
                _ => {
                    // Start on the whole (filled) picture, end a quarter closer, easing.
                    let mut s = Param::fixed(fill);
                    s.set_key(0, fill, Ease::Ease)?;
                    s.set_key(length - 1, fill * 5 / 4, Ease::Linear)?;
                    c.scale = s;
                }
            }
            Ok(())
        })
    }

    fn shuttle(&mut self, key: &str, clock_us: u64) -> Result<(), String> {
        self.settle(clock_us);
        let rate = self.play.map_or(0, |p| p.rate);
        match key {
            "k" => {
                self.settle(clock_us);
                self.play = None;
            }
            "l" => {
                let next = if rate > 0 { (rate * 2).min(400) } else { 100 };
                self.start_playing(next, clock_us);
            }
            "j" => {
                let next = if rate < 0 { (rate * 2).max(-400) } else { -100 };
                if self.playhead == 0 && next < 0 {
                    return Err("already at the start".into());
                }
                self.start_playing(next, clock_us);
            }
            _ => return Err("shuttle is j, k or l".into()),
        }
        Ok(())
    }

    // ---- pointer ---------------------------------------------------------------------

    /// Whether `target` follows the pointer while pressed.
    pub fn drags(target: &str) -> bool {
        let Some(rest) = target
            .strip_prefix(PREFIX)
            .and_then(|t| t.strip_prefix(':'))
        else {
            return false;
        };
        matches!(
            rest.split(':').next().unwrap_or(""),
            "clip" | "media" | "trim-in" | "trim-out" | "ruler" | "slider" | "zoom"
        )
    }

    pub fn pointer(
        &mut self,
        window: u64,
        command: &str,
        phase: PointerPhase,
        x: i32,
        y: i32,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let parts: Vec<&str> = command.split(':').collect();
        let num = |i: usize| -> Result<i64, String> {
            parts
                .get(i)
                .ok_or_else(|| "incomplete drag surface".to_owned())
                .and_then(|v| parse::<i64>(v, "drag geometry"))
        };
        match phase {
            PointerPhase::Down => {
                self.settle(clock_us);
                self.drag = Some(match parts[0] {
                    "media" => {
                        let id = num(1)? as u32;
                        self.command(window, &format!("media:{id}"), clock_us)?;
                        Drag::Media {
                            id,
                            ox: num(2)? as i32,
                            oy: num(3)? as i32,
                            lane: num(4)? as u32,
                            scroll: num(5)?,
                            x,
                            y,
                        }
                    }
                    "clip" => {
                        let id = num(1)? as u32;
                        self.command(window, &format!("clip:{id}"), clock_us)?;
                        Drag::Clip {
                            id,
                            lane: num(2)? as u32,
                            press_x: x,
                            press_y: y,
                            x,
                            y,
                        }
                    }
                    "trim-in" | "trim-out" => {
                        let id = num(1)? as u32;
                        self.project.clip(id).ok_or("no such clip")?;
                        self.selected = Some(id);
                        self.selected_transition = None;
                        Drag::Trim {
                            id,
                            out: parts[0] == "trim-out",
                            press: x,
                            dx: 0,
                        }
                    }
                    "ruler" => {
                        let scroll = num(1)?;
                        self.seek(scroll + self.frames(i64::from(x)));
                        Drag::Scrub { scroll }
                    }
                    "slider" => {
                        let prop = Prop::parse(parts.get(1).copied().unwrap_or(""))
                            .ok_or("unknown slider")?;
                        let width = num(2)?.max(1) as u32;
                        self.checkpoint();
                        let d = Drag::Slider { prop, width };
                        self.drag = Some(d.clone());
                        if let Err(e) = self.slide(prop, width, x, clock_us) {
                            self.undo.pop();
                            self.drag = None;
                            return Err(e);
                        }
                        d
                    }
                    "zoom" => {
                        let width = num(1)?.max(1) as u32;
                        self.zoom_to(width, x);
                        Drag::Zoom { width }
                    }
                    other => return Err(format!("{other} is not a drag surface")),
                });
                Ok(vec![])
            }
            PointerPhase::Move => {
                match self.drag.clone() {
                    Some(Drag::Media { .. }) | Some(Drag::Clip { .. }) => {
                        if let Some(
                            Drag::Media { x: dx, y: dy, .. } | Drag::Clip { x: dx, y: dy, .. },
                        ) = &mut self.drag
                        {
                            *dx = x;
                            *dy = y;
                        }
                    }
                    Some(Drag::Trim { id, out, press, .. }) => {
                        self.drag = Some(Drag::Trim {
                            id,
                            out,
                            press,
                            dx: x - press,
                        })
                    }
                    Some(Drag::Scrub { scroll }) => self.seek(scroll + self.frames(i64::from(x))),
                    Some(Drag::Slider { prop, width }) => self.slide(prop, width, x, clock_us)?,
                    Some(Drag::Zoom { width }) => self.zoom_to(width, x),
                    None => return Err("no drag is in progress".into()),
                }
                Ok(vec![])
            }
            PointerPhase::Up => {
                let drag = self.drag.take().ok_or("no drag is in progress")?;
                self.finish(window, drag, x, y, clock_us)
            }
            PointerPhase::Cancel => {
                if matches!(self.drag, Some(Drag::Slider { .. })) {
                    // Put the value back where the press found it.
                    if let Some(before) = self.undo.pop() {
                        self.project = before;
                    }
                }
                self.drag = None;
                Ok(vec![])
            }
        }
    }

    fn zoom_to(&mut self, width: u32, x: i32) {
        let t = i64::from(x.clamp(0, width as i32)) * 1000 / i64::from(width);
        // Logarithmic feel from a linear slider: square the fraction.
        let span = i64::from(ZOOM_MAX - ZOOM_MIN);
        self.zoom = (i64::from(ZOOM_MIN) + span * t * t / 1_000_000) as u32;
    }

    fn slide(&mut self, prop: Prop, width: u32, x: i32, clock_us: u64) -> Result<(), String> {
        let (lo, hi) = self.range(prop);
        let t = i64::from(x.clamp(0, width as i32));
        let value =
            i64::from(lo) + (i64::from(hi - lo) * t + i64::from(width) / 2) / i64::from(width);
        self.set_prop(prop, value as i32, clock_us, false)
    }

    /// Commit a released drag.
    fn finish(
        &mut self,
        window: u64,
        drag: Drag,
        x: i32,
        y: i32,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        match drag {
            Drag::Media {
                id,
                ox,
                oy,
                lane,
                scroll,
                x: _,
                y: _,
            } => {
                let (lx, ly) = (x - ox, y - oy);
                if lx < 0 {
                    // Released outside the timeline: the click selected the item only.
                    return Ok(vec![]);
                }
                let lane_index = i64::from(ly).div_euclid(i64::from(lane.max(1)));
                let lanes = self.lanes().len() as i64;
                if lane_index < -1 || lane_index > lanes {
                    return Ok(vec![]);
                }
                let kind = self.media_kind(id).ok_or("no such media")?;
                if !self.library.contains_key(&id) {
                    return Err("that media is offline".into());
                }
                let track_kind = if kind == MediaKind::Audio {
                    TrackKind::Audio
                } else {
                    TrackKind::Video
                };
                let at = self.snapped(scroll + self.frames(i64::from(lx)), None);
                let before = self.project.clone();
                let track = self.lane_track(lane_index, track_kind)?;
                let mut new = 0;
                let result = self
                    .project
                    .insert(&self.library, Source::Media { media: id }, track, at)
                    .map(|n| new = n);
                if let Err(e) = result {
                    self.project = before;
                    return Err(e);
                }
                self.undo.push(before);
                self.redo.clear();
                self.modified = true;
                self.selected = Some(new);
                self.selected_transition = None;
                self.status = String::new();
                Ok(vec![])
            }
            Drag::Clip {
                id,
                lane,
                press_x,
                press_y,
                ..
            } => {
                let (dx, dy) = (x - press_x, y - press_y);
                if dx.abs() < 3 && dy.abs() < 3 {
                    return Ok(vec![]);
                }
                let clip = self.project.clip(id).ok_or("no such clip")?.clone();
                let lanes = self.lanes();
                let from = lanes.iter().position(|t| *t == clip.track).unwrap_or(0) as i64;
                let lane_index =
                    from + (i64::from(press_y) + i64::from(dy)).div_euclid(i64::from(lane.max(1)));
                let kind = self
                    .project
                    .track(clip.track)
                    .map(|t| t.kind)
                    .unwrap_or(TrackKind::Video);
                let wanted = clip.start + self.frames(i64::from(dx));
                // Either edge may snap; whichever is nearer wins.
                let by_start = self.snapped(wanted, Some(id));
                let by_end = self.snapped(wanted + clip.length, Some(id)) - clip.length;
                let at = if (by_start - wanted).abs() <= (by_end - wanted).abs() {
                    by_start
                } else {
                    by_end
                };
                let before = self.project.clone();
                let track = match self.lane_track(lane_index.clamp(-1, lanes.len() as i64), kind) {
                    Ok(t) => t,
                    Err(_) => clip.track,
                };
                if let Err(e) = self.project.move_clip(id, track, at.max(0)) {
                    self.project = before;
                    return Err(e);
                }
                if self.project != before {
                    self.undo.push(before);
                    self.redo.clear();
                    self.modified = true;
                }
                Ok(vec![])
            }
            Drag::Trim { id, out, press, .. } => {
                let clip = self.project.clip(id).ok_or("no such clip")?.clone();
                let by = self.frames(i64::from(x - press));
                let (edge, to) = if out {
                    (Edge::Out, self.snapped(clip.end() + by, Some(id)))
                } else {
                    (Edge::In, self.snapped(clip.start + by, Some(id)))
                };
                self.edit(|p, lib| p.trim(id, edge, to, lib))?;
                Ok(vec![])
            }
            Drag::Scrub { scroll } => {
                self.seek(scroll + self.frames(i64::from(x)));
                Ok(vec![])
            }
            Drag::Slider { prop, width } => {
                self.slide(prop, width, x, clock_us)?;
                // The press recorded the value before; nothing changed, nothing to undo.
                if self.undo.last() == Some(&self.project) {
                    self.undo.pop();
                }
                let _ = window;
                Ok(vec![])
            }
            Drag::Zoom { width } => {
                self.zoom_to(width, x);
                Ok(vec![])
            }
        }
    }

    // ---- keyboard --------------------------------------------------------------------

    pub fn text(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
        match self.field {
            Field::Title => {
                let text = text.to_owned();
                self.edit_title(window, |t| {
                    crate::apps::push_bounded(&mut t.text, &text, cw_video::project::TITLE_LIMIT);
                    Ok(())
                })
            }
            Field::Name => {
                let name = match &mut self.sheet {
                    Some(Sheet::Save { name }) | Some(Sheet::Export { name, .. }) => name,
                    _ => return Err("no name field is showing".into()),
                };
                crate::apps::push_bounded(name, text, NAME_LIMIT);
                Ok(vec![])
            }
            Field::Project => {
                crate::apps::push_bounded(&mut self.project.name, text, NAME_LIMIT);
                self.modified = true;
                Ok(vec![])
            }
            // With no field focused, a letter is a keyboard shortcut.
            Field::None => {
                let mut effects = vec![];
                for ch in text.chars() {
                    effects.extend(self.key(window, &ch.to_string(), 0, true)?);
                }
                Ok(effects)
            }
        }
    }
    pub fn accepts_text(&self) -> bool {
        self.field != Field::None
    }

    pub fn key(
        &mut self,
        window: u64,
        key: &str,
        clock_us: u64,
        from_text: bool,
    ) -> Result<Vec<AppEffect>, String> {
        let _ = from_text;
        if self.field != Field::None {
            match key {
                "Backspace" => {
                    match self.field {
                        Field::Title => {
                            return self.edit_title(window, |t| {
                                t.text.pop();
                                Ok(())
                            })
                        }
                        Field::Name => match &mut self.sheet {
                            Some(Sheet::Save { name }) | Some(Sheet::Export { name, .. }) => {
                                name.pop();
                            }
                            _ => {}
                        },
                        Field::Project => {
                            self.project.name.pop();
                        }
                        Field::None => {}
                    }
                    return Ok(vec![]);
                }
                "Enter" => {
                    let field = self.field;
                    self.field = Field::None;
                    return match (&self.sheet, field) {
                        (Some(Sheet::Save { .. }), Field::Name) => {
                            self.command(window, "save-confirm", clock_us)
                        }
                        (Some(Sheet::Export { .. }), Field::Name) => {
                            self.command(window, "export-start", clock_us)
                        }
                        _ => Ok(vec![]),
                    };
                }
                "Escape" => {
                    self.field = Field::None;
                    return Ok(vec![]);
                }
                _ if key.chars().count() == 1 => return self.text(window, key),
                _ => {}
            }
        }
        let p = self.product;
        let f = p.features(false);
        let command = match key {
            " " | "Space" => "play",
            "ArrowLeft" => "step:-1",
            "ArrowRight" => "step:1",
            "Shift+ArrowLeft" if p == Product::Clipchamp => "skip:-1",
            "Shift+ArrowRight" if p == Product::Clipchamp => "skip:1",
            "Home" | "Ctrl+ArrowLeft" | "Meta+ArrowLeft" => "start",
            "End" | "Ctrl+ArrowRight" | "Meta+ArrowRight" => "end",
            "j" | "J" | "k" | "K" | "l" | "L" if f.shuttle => {
                return self.command(
                    window,
                    &format!("shuttle:{}", key.to_ascii_lowercase()),
                    clock_us,
                )
            }
            "s" | "S" if p == Product::Clipchamp => "split",
            "Meta+b" | "Meta+B" if p == Product::Imovie => "split",
            "Shift+r" | "Shift+R" if p == Product::Kdenlive => "split",
            "Delete" | "Backspace" => "delete",
            "Shift+Delete" if p == Product::Kdenlive => "ripple-delete",
            "Ctrl+z" | "Meta+z" => "undo",
            "Ctrl+y" | "Ctrl+Shift+z" | "Ctrl+Shift+Z" | "Meta+Shift+z" | "Meta+Shift+Z" => "redo",
            "Ctrl+=" | "Ctrl++" | "Meta+=" | "Meta++" => "zoom-in",
            "Ctrl+-" | "Meta+-" => "zoom-out",
            "Ctrl+s" | "Meta+s" => "save",
            "Ctrl+i" | "Meta+i" => "import",
            "Ctrl+e" | "Meta+e" if p != Product::Kdenlive => "export",
            "Ctrl+Return" | "Ctrl+Enter" if p == Product::Kdenlive => "export",
            "Escape" => {
                if self.sheet.is_some() {
                    "close-sheet"
                } else {
                    "deselect"
                }
            }
            "s" | "S" if p == Product::Kdenlive => "snap",
            other => return Err(format!("{} has no shortcut for {other}", p.name())),
        };
        self.command(window, command, clock_us)
    }

    // ---- background work -------------------------------------------------------------

    pub fn busy(&self) -> bool {
        self.export.is_some()
    }
    /// One simulation step of an export: encode a few frames, and write the files when
    /// the last is done.
    pub fn background(&mut self, window: u64) -> Vec<AppEffect> {
        let Some(mut job) = self.export.take() else {
            return vec![];
        };
        let comp = Compositor {
            project: &self.project,
            library: &self.library,
            masks: &self.masks,
        };
        if let Err(e) = job.step(&comp) {
            self.status = format!("Export failed: {e}");
            return vec![];
        }
        if !job.finished() {
            self.export = Some(job);
            return vec![];
        }
        match job.files(&self.project, &self.library) {
            Ok((movie, sound)) => {
                self.status = format!(
                    "Exported {}",
                    job.video_path.rsplit('/').next().unwrap_or(&job.video_path)
                );
                vec![
                    AppEffect::WriteBytes {
                        window,
                        path: job.audio_path.clone(),
                        bytes: sound,
                    },
                    AppEffect::WriteBytes {
                        window,
                        path: job.video_path.clone(),
                        bytes: movie,
                    },
                ]
            }
            Err(e) => {
                self.status = format!("Export failed: {e}");
                vec![]
            }
        }
    }

    /// The composited frame at the playhead: what the monitor shows.
    pub fn frame(&self, clock_us: u64) -> cw_raster::Canvas {
        Compositor {
            project: &self.project,
            library: &self.library,
            masks: &self.masks,
        }
        .frame(self.now(clock_us))
    }
    /// The mix's level during the frame at the playhead, 0..=255.
    pub fn level(&self, clock_us: u64) -> u8 {
        if self.play.is_none() {
            return 0;
        }
        audio::level(&self.project, &self.library, self.now(clock_us))
    }

    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: String| cw_protocol::PageAction {
            method: "APP".into(),
            url,
            fields: Default::default(),
        };
        let button = |page: &mut cw_protocol::Page, id: String, text: String| {
            page.elements.push(E::Button {
                id: id.clone(),
                text,
                action: act(id),
            });
        };
        page.elements.push(E::Heading {
            id: "video-project".into(),
            text: self.project.name.clone(),
            level: 2,
        });
        page.elements.push(E::Text {
            id: "video-playhead".into(),
            text: format!(
                "{} / {}{}",
                self.project.timecode(self.playhead),
                self.project.timecode(self.project.duration()),
                if self.play.is_some() {
                    " (playing)"
                } else {
                    ""
                }
            ),
        });
        if !self.status.is_empty() {
            page.elements.push(E::Text {
                id: "video-status".into(),
                text: self.status.clone(),
            });
        }
        if let Some(job) = &self.export {
            page.elements.push(E::Text {
                id: "video-export".into(),
                text: format!("Exporting {} of {} frames", job.done, job.total),
            });
        }
        for (cmd, label) in [
            ("play", "Play/Pause"),
            ("start", "Go to start"),
            ("end", "Go to end"),
            ("split", "Split"),
            ("delete", "Delete"),
            ("undo", "Undo"),
            ("redo", "Redo"),
            ("import", "Import media"),
            ("save", "Save project"),
            ("open", "Open project"),
            ("export", "Export"),
        ] {
            button(page, format!("{PREFIX}:{cmd}"), label.into());
        }
        for m in &self.project.media {
            let name = m.path.rsplit('/').next().unwrap_or(&m.path);
            let state = if self.offline.contains_key(&m.id) {
                " (offline)"
            } else {
                ""
            };
            button(
                page,
                format!("{PREFIX}:append:{}", m.id),
                format!("Add {name}{state} to the timeline"),
            );
        }
        for c in &self.project.clips {
            let track = self.project.track(c.track).map_or("", |t| t.name.as_str());
            button(
                page,
                format!("{PREFIX}:clip:{}", c.id),
                format!(
                    "{} on {track} at {} for {} frames{}",
                    c.name,
                    self.project.timecode(c.start),
                    c.length,
                    if self.selected == Some(c.id) {
                        " (selected)"
                    } else {
                        ""
                    }
                ),
            );
        }
        for t in &self.project.transitions {
            button(
                page,
                format!("{PREFIX}:transition:{}", t.id),
                format!(
                    "{} ({} frames)",
                    self.product.transition_name(t.kind),
                    t.frames
                ),
            );
        }
        if let Some(sheet) = &self.sheet {
            match sheet {
                Sheet::Browse {
                    folder, entries, ..
                } => {
                    page.elements.push(E::Heading {
                        id: "video-sheet".into(),
                        text: folder.clone(),
                        level: 3,
                    });
                    for e in entries {
                        let id = if e.ends_with('/') {
                            format!("{PREFIX}:browse:{e}")
                        } else {
                            format!("{PREFIX}:pick:{e}")
                        };
                        button(page, id, e.clone());
                    }
                    button(page, format!("{PREFIX}:close-sheet"), "Done".into());
                }
                Sheet::Save { name } => {
                    page.elements.push(E::Text {
                        id: "video-save-name".into(),
                        text: name.clone(),
                    });
                    button(page, format!("{PREFIX}:save-confirm"), "Save".into());
                }
                Sheet::Export {
                    name,
                    width,
                    height,
                    fps,
                } => {
                    page.elements.push(E::Text {
                        id: "video-export-settings".into(),
                        text: format!("{name}: {width}x{height} at {fps} fps"),
                    });
                    button(page, format!("{PREFIX}:export-start"), "Export".into());
                }
            }
        }
    }
}

/// Export sizes offered, all 16:9.
pub const EXPORT_SIZES: [(u32, u32); 3] = [(160, 90), (320, 180), (640, 360)];
pub const EXPORT_RATES: [u32; 3] = [12, 24, 30];

macro_rules! video_app {
    ($name:ident, $kind:literal, $product:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name(pub Box<Editor>);
        impl std::ops::Deref for $name {
            type Target = Editor;
            fn deref(&self) -> &Editor {
                &self.0
            }
        }
        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Editor {
                &mut self.0
            }
        }
        impl $name {
            pub const KIND: &'static str = $kind;
            pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
                let (editor, effects) = Editor::launch($product, argument, window);
                (Self(Box::new(editor)), effects)
            }
            pub fn kind(&self) -> &'static str {
                Self::KIND
            }
            pub fn title(&self, _theme: DesktopTheme) -> String {
                self.0.title()
            }
            pub fn document(&self) -> String {
                self.0.path.clone()
            }
            pub fn caption(&self) -> String {
                self.0.caption()
            }
            pub fn modified(&self) -> bool {
                self.0.modified
            }
            pub fn text(&mut self, text: &str) -> Result<(), String> {
                self.0.text(0, text).map(|_| ())
            }
            pub fn key(
                &mut self,
                window: u64,
                key: &str,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                self.0.key(window, key, clock_us, false)
            }
            pub fn click(
                &mut self,
                window: u64,
                target: &str,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                let command = target
                    .strip_prefix(PREFIX)
                    .and_then(|t| t.strip_prefix(':'))
                    .ok_or_else(|| format!("interaction does not belong to {}", Self::KIND))?
                    .to_owned();
                self.0.command(window, &command, clock_us)
            }
            pub fn http(
                &mut self,
                _window: u64,
                _tag: &str,
                _status: u16,
                _body: &str,
            ) -> Result<Vec<AppEffect>, String> {
                Err(format!("{} makes no network requests", Self::KIND))
            }
            pub fn offline(&mut self, _tag: &str, reason: &str) {
                self.0.listing_failed(reason);
            }
            pub fn page(&self, page: &mut cw_protocol::Page) {
                self.0.page(page)
            }
            pub fn render(&self, p: &mut crate::desktop_scene::Painter, env: &crate::AppEnv<'_>) {
                if env.theme.mobile() {
                    mobile::render(&self.0, p, env)
                } else {
                    desktop::render(&self.0, p, env)
                }
            }
        }
    };
}

video_app!(Clipchamp, "clipchamp", Product::Clipchamp);
video_app!(Imovie, "imovie", Product::Imovie);
video_app!(Kdenlive, "kdenlive", Product::Kdenlive);
video_app!(VideoEditor, "videoeditor", Product::VideoEditor);
