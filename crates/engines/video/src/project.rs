//! The edit: tracks, clips on them, transitions between neighbours, and every change an
//! editor can make to them. This is also the saved project format (see
//! `docs/video-editing.md`); nothing here refers to pixels.
//!
//! Time on the timeline is counted in whole frames at the project rate. A clip's place
//! in its source is counted in *centiframes* — hundredths of a timeline frame — so a
//! clip played at any speed from 25% to 400% advances a whole number of them per frame
//! and every trim, split and speed change is exact.
use crate::media::{Media, MediaKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FORMAT: &str = "computerworld-video-project";
pub const VERSION: u32 = 1;
pub const MIN_SPEED: u32 = 25;
pub const MAX_SPEED: u32 = 400;
/// Bounds a project refuses beyond, so a hostile file cannot ask for unbounded work.
pub const MAX_CLIPS: usize = 512;
pub const MAX_TRACKS: usize = 16;
pub const MAX_FRAMES: i64 = 24 * 60 * 60;
pub const MAX_KEYS: usize = 64;
pub const TITLE_LIMIT: usize = 120;

pub type Library = BTreeMap<u32, Media>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: u32,
    pub kind: TrackKind,
    pub name: String,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub locked: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitlePosition {
    Top,
    #[default]
    Center,
    /// The lower third, where a name caption sits.
    Lower,
    Bottom,
}
impl TitlePosition {
    pub const ALL: [TitlePosition; 4] = [Self::Top, Self::Center, Self::Lower, Self::Bottom];
    pub fn id(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Center => "center",
            Self::Lower => "lower",
            Self::Bottom => "bottom",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Title {
    pub text: String,
    /// Glyph height in canvas pixels.
    pub size: u16,
    #[serde(default)]
    pub bold: bool,
    /// `rrggbb` or `rrggbbaa`.
    pub color: String,
    /// A box behind the text, when set.
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub position: TitlePosition,
}
impl Title {
    /// The key a rasterised line of this title is cached under.
    pub fn raster_key(&self) -> String {
        format!("{}|{}|{}", self.size, u8::from(self.bold), self.text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    Media {
        media: u32,
    },
    /// A solid background, `rrggbb`.
    Color {
        color: String,
    },
    Title(Title),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    /// Smoothstep: slow out of one keyframe and into the next.
    Ease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    /// Clip-local frame.
    pub frame: i64,
    pub value: i32,
    /// How the value travels from this key to the next.
    #[serde(default)]
    pub ease: Ease,
}

/// A property that is either one value or animated between keyframes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub value: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<Key>,
}
impl Param {
    pub fn fixed(value: i32) -> Self {
        Self {
            value,
            keys: vec![],
        }
    }
    pub fn animated(&self) -> bool {
        !self.keys.is_empty()
    }
    /// Value at clip-local `frame`. Before the first key it holds the first, after the
    /// last the last; between two it interpolates by the earlier key's easing.
    pub fn at(&self, frame: i64) -> i32 {
        let Some(first) = self.keys.first() else {
            return self.value;
        };
        if frame <= first.frame {
            return first.value;
        }
        let last = self.keys.last().unwrap();
        if frame >= last.frame {
            return last.value;
        }
        let i = self.keys.partition_point(|k| k.frame <= frame) - 1;
        let (a, b) = (self.keys[i], self.keys[i + 1]);
        let span = b.frame - a.frame;
        // Progress in 16.16 fixed point.
        let mut t = ((frame - a.frame) << 16) / span;
        if a.ease == Ease::Ease {
            // 3t² - 2t³
            let t2 = (t * t) >> 16;
            let t3 = (t2 * t) >> 16;
            t = 3 * t2 - 2 * t3;
        }
        let delta = i64::from(b.value) - i64::from(a.value);
        let num = delta * t;
        let q = if num >= 0 {
            (num + (1 << 15)) >> 16
        } else {
            -((-num + (1 << 15)) >> 16)
        };
        (i64::from(a.value) + q) as i32
    }
    /// Add or replace the key at `frame`.
    pub fn set_key(&mut self, frame: i64, value: i32, ease: Ease) -> Result<(), String> {
        match self.keys.binary_search_by_key(&frame, |k| k.frame) {
            Ok(i) => self.keys[i] = Key { frame, value, ease },
            Err(i) => {
                if self.keys.len() >= MAX_KEYS {
                    return Err("this property has as many keyframes as it can hold".into());
                }
                self.keys.insert(i, Key { frame, value, ease });
            }
        }
        Ok(())
    }
    pub fn remove_key(&mut self, frame: i64) -> Result<(), String> {
        let i = self
            .keys
            .binary_search_by_key(&frame, |k| k.frame)
            .map_err(|_| "there is no keyframe there")?;
        let key = self.keys.remove(i);
        if self.keys.is_empty() {
            self.value = key.value;
        }
        Ok(())
    }
    /// Set the value where the playhead is: a key when animated, the value otherwise.
    pub fn set(&mut self, frame: i64, value: i32) {
        if self.animated() {
            let ease = self
                .keys
                .iter()
                .find(|k| k.frame == frame)
                .map(|k| k.ease)
                .unwrap_or_default();
            let _ = self.set_key(frame, value, ease);
        } else {
            self.value = value;
        }
    }
    fn shift(&mut self, by: i64) {
        for k in &mut self.keys {
            k.frame += by;
        }
    }
    /// Split at clip-local `at`: `self` keeps what is before, the result what is after,
    /// each with a key at the cut holding the value there so neither half jumps.
    fn split(&mut self, at: i64) -> Param {
        if !self.animated() {
            return self.clone();
        }
        let here = self.at(at);
        let ease = self
            .keys
            .iter()
            .rev()
            .find(|k| k.frame <= at)
            .map(|k| k.ease)
            .unwrap_or_default();
        let mut after = Param {
            value: here,
            keys: self.keys.iter().filter(|k| k.frame > at).copied().collect(),
        };
        after.keys.insert(
            0,
            Key {
                frame: at,
                value: here,
                ease,
            },
        );
        after.shift(-at);
        self.keys.retain(|k| k.frame < at);
        self.keys.push(Key {
            frame: at,
            value: here,
            ease,
        });
        after
    }
    fn scale_time(&mut self, num: i64, den: i64) {
        for k in &mut self.keys {
            k.frame = k.frame * num / den;
        }
        self.keys.dedup_by_key(|k| k.frame);
    }
}

/// Crop from each edge, in thousandths of the source's width or height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crop {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}
impl Crop {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Colour correction, each -100..=100.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grade {
    pub brightness: i32,
    pub contrast: i32,
    pub saturation: i32,
    pub temperature: i32,
}
impl Grade {
    pub fn is_neutral(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clip {
    pub id: u32,
    pub track: u32,
    pub source: Source,
    /// First timeline frame.
    pub start: i64,
    /// Frames on the timeline.
    pub length: i64,
    /// Where in the source the clip's span begins, in centiframes.
    #[serde(default)]
    pub offset: i64,
    /// Percent: 100 is real time.
    pub speed: u32,
    #[serde(default)]
    pub reverse: bool,
    /// Thousandths: 1000 opaque.
    pub opacity: Param,
    /// Offset of the picture's centre from the canvas centre, in canvas pixels.
    pub x: Param,
    pub y: Param,
    /// Thousandths of the size that fits the canvas.
    pub scale: Param,
    /// Hundredths of a degree, clockwise.
    pub rotation: Param,
    /// Percent gain: 100 unity, 0 silent, 200 doubled.
    pub volume: Param,
    #[serde(default)]
    pub crop: Crop,
    #[serde(default)]
    pub grade: Grade,
    /// Frames of fade from and to transparent (or silence).
    #[serde(default)]
    pub fade_in: u32,
    #[serde(default)]
    pub fade_out: u32,
    pub name: String,
}
impl Clip {
    pub fn end(&self) -> i64 {
        self.start + self.length
    }
    pub fn contains(&self, frame: i64) -> bool {
        frame >= self.start && frame < self.end()
    }
    /// Centiframes of source the clip spans.
    pub fn span(&self) -> i64 {
        self.length * i64::from(self.speed)
    }
    /// Source position (centiframes) shown at clip-local frame `local`. Out-of-range
    /// locals are extrapolated; callers clamp to what the source holds.
    pub fn source_at(&self, local: i64) -> i64 {
        let step = i64::from(self.speed);
        if self.reverse {
            self.offset + (self.length - 1 - local) * step
        } else {
            self.offset + local * step
        }
    }
    pub fn media(&self) -> Option<u32> {
        match self.source {
            Source::Media { media } => Some(media),
            _ => None,
        }
    }
    fn shift(&mut self, by: i64) {
        self.start += by;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    CrossDissolve,
    DipToBlack,
    DipToWhite,
    Wipe,
    Slide,
}
impl TransitionKind {
    pub const ALL: [TransitionKind; 5] = [
        Self::CrossDissolve,
        Self::DipToBlack,
        Self::DipToWhite,
        Self::Wipe,
        Self::Slide,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::CrossDissolve => "cross_dissolve",
            Self::DipToBlack => "dip_to_black",
            Self::DipToWhite => "dip_to_white",
            Self::Wipe => "wipe",
            Self::Slide => "slide",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

/// A transition across the cut between two clips that touch on one track. It is
/// centred on the cut: the outgoing clip plays on past its end, and the incoming one
/// starts before its start, for half the duration each.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub id: u32,
    pub left: u32,
    pub right: u32,
    pub kind: TransitionKind,
    pub frames: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaRef {
    pub id: u32,
    pub path: String,
    pub kind: MediaKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub sample_rate: u32,
    /// What shows where no clip covers the canvas, `rrggbb`.
    pub background: String,
    pub media: Vec<MediaRef>,
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    pub transitions: Vec<Transition>,
    pub next_id: u32,
}

/// Which edge of a clip a trim moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    In,
    Out,
}

impl Project {
    /// An empty project with one video and one audio track.
    pub fn new(name: &str, width: u32, height: u32, fps: u32, sample_rate: u32) -> Self {
        let mut p = Self {
            format: FORMAT.into(),
            version: VERSION,
            name: name.into(),
            width,
            height,
            fps,
            sample_rate,
            background: "000000".into(),
            media: vec![],
            tracks: vec![],
            clips: vec![],
            transitions: vec![],
            next_id: 1,
        };
        p.add_track(TrackKind::Video).unwrap();
        p.add_track(TrackKind::Audio).unwrap();
        p
    }
    fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn track(&self, id: u32) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }
    pub fn track_mut(&mut self, id: u32) -> Result<&mut Track, String> {
        self.tracks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| "no such track".into())
    }
    pub fn clip(&self, id: u32) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }
    pub fn clip_mut(&mut self, id: u32) -> Result<&mut Clip, String> {
        self.clips
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| "no such clip".into())
    }
    /// Tracks of one kind, bottom-most (composited first) first.
    pub fn tracks_of(&self, kind: TrackKind) -> Vec<&Track> {
        self.tracks.iter().filter(|t| t.kind == kind).collect()
    }
    /// Clips on a track in time order.
    pub fn on_track(&self, track: u32) -> Vec<&Clip> {
        let mut v: Vec<&Clip> = self.clips.iter().filter(|c| c.track == track).collect();
        v.sort_by_key(|c| (c.start, c.id));
        v
    }
    /// Frames from the first frame to the end of the last clip.
    pub fn duration(&self) -> i64 {
        self.clips.iter().map(Clip::end).max().unwrap_or(0)
    }
    /// Centiframes of a source that lasts `us` microseconds.
    pub fn centiframes(&self, us: u64) -> i64 {
        (u128::from(us) * 100 * u128::from(self.fps) / 1_000_000) as i64
    }
    /// Microseconds at a source position in centiframes.
    pub fn micros(&self, centiframes: i64) -> i64 {
        (i128::from(centiframes) * 1_000_000 / (100 * i128::from(self.fps.max(1)))) as i64
    }
    /// Source sample at a position in centiframes, at the project sample rate.
    pub fn sample_of(&self, centiframes: i64) -> i64 {
        (i128::from(centiframes) * i128::from(self.sample_rate)
            / (100 * i128::from(self.fps.max(1)))) as i64
    }
    /// First sample of timeline frame `frame`.
    pub fn frame_sample(&self, frame: i64) -> i64 {
        (i128::from(frame) * i128::from(self.sample_rate) / i128::from(self.fps.max(1))) as i64
    }
    /// Timecode `HH:MM:SS:FF`.
    pub fn timecode(&self, frame: i64) -> String {
        let fps = i64::from(self.fps.max(1));
        let f = frame.max(0);
        let s = f / fps;
        format!(
            "{:02}:{:02}:{:02}:{:02}",
            s / 3600,
            s / 60 % 60,
            s % 60,
            f % fps
        )
    }

    pub fn add_track(&mut self, kind: TrackKind) -> Result<u32, String> {
        if self.tracks.len() >= MAX_TRACKS {
            return Err("the timeline has as many tracks as it can hold".into());
        }
        let n = self.tracks.iter().filter(|t| t.kind == kind).count() + 1;
        let id = self.id();
        self.tracks.push(Track {
            id,
            kind,
            name: match kind {
                TrackKind::Video => format!("V{n}"),
                TrackKind::Audio => format!("A{n}"),
            },
            muted: false,
            hidden: false,
            locked: false,
        });
        Ok(id)
    }
    /// Remove an empty track.
    pub fn remove_track(&mut self, id: u32) -> Result<(), String> {
        let track = self.track(id).ok_or("no such track")?;
        if self.tracks_of(track.kind).len() == 1 {
            return Err("the timeline keeps at least one track of each kind".into());
        }
        if self.clips.iter().any(|c| c.track == id) {
            return Err("only an empty track can be removed".into());
        }
        self.tracks.retain(|t| t.id != id);
        Ok(())
    }

    /// Register a file the project uses; the same path twice is the same media.
    pub fn add_media(&mut self, path: &str, kind: MediaKind) -> u32 {
        if let Some(m) = self.media.iter().find(|m| m.path == path) {
            return m.id;
        }
        let id = self.id();
        self.media.push(MediaRef {
            id,
            path: path.into(),
            kind,
        });
        id
    }
    /// Forget a file no clip uses.
    pub fn remove_media(&mut self, id: u32) -> Result<(), String> {
        if self.clips.iter().any(|c| c.media() == Some(id)) {
            return Err("that media is used in the timeline".into());
        }
        let before = self.media.len();
        self.media.retain(|m| m.id != id);
        if self.media.len() == before {
            return Err("no such media".into());
        }
        Ok(())
    }

    fn source_kind(&self, source: &Source) -> Result<TrackKind, String> {
        Ok(match source {
            Source::Media { media } => {
                let m = self
                    .media
                    .iter()
                    .find(|m| m.id == *media)
                    .ok_or("no such media")?;
                if m.kind == MediaKind::Audio {
                    TrackKind::Audio
                } else {
                    TrackKind::Video
                }
            }
            _ => TrackKind::Video,
        })
    }
    /// How much source a clip's media holds, in centiframes; `None` when unbounded.
    fn extent(&self, clip: &Clip, library: &Library) -> Option<i64> {
        let media = library.get(&clip.media()?)?;
        media.length_us().map(|us| self.centiframes(us))
    }
    fn writable(&self, track: u32) -> Result<&Track, String> {
        let t = self.track(track).ok_or("no such track")?;
        if t.locked {
            return Err(format!("track {} is locked", t.name));
        }
        Ok(t)
    }

    /// Put a new clip of `source` on `track` at `at`, rippling later clips if it lands
    /// on them. Returns the clip's id.
    pub fn insert(
        &mut self,
        library: &Library,
        source: Source,
        track: u32,
        at: i64,
    ) -> Result<u32, String> {
        if self.clips.len() >= MAX_CLIPS {
            return Err("the timeline has as many clips as it can hold".into());
        }
        let kind = self.source_kind(&source)?;
        if self.writable(track)?.kind != kind {
            return Err(match kind {
                TrackKind::Audio => "sound goes on an audio track".into(),
                TrackKind::Video => "pictures go on a video track".into(),
            });
        }
        let fps = i64::from(self.fps);
        let (length, name) = match &source {
            Source::Media { media } => {
                let m = library.get(media).ok_or("that media is not loaded")?;
                let length = match m.length_us() {
                    Some(us) => (self.centiframes(us) / 100).max(1),
                    None => 4 * fps,
                };
                (length, m.name().to_owned())
            }
            Source::Color { .. } => (4 * fps, "Color".into()),
            Source::Title(t) => (4 * fps, t.text.clone()),
        };
        let id = self.id();
        self.clips.push(Clip {
            id,
            track,
            source,
            start: at.max(0),
            length,
            offset: 0,
            speed: 100,
            reverse: false,
            opacity: Param::fixed(1000),
            x: Param::fixed(0),
            y: Param::fixed(0),
            scale: Param::fixed(1000),
            rotation: Param::fixed(0),
            volume: Param::fixed(100),
            crop: Crop::default(),
            grade: Grade::default(),
            fade_in: 0,
            fade_out: 0,
            name,
        });
        self.place(id, track, at);
        Ok(id)
    }

    /// Settle clip `id` on `track` at `at`. A drop onto another clip lands at that
    /// clip's nearer edge; anything then in the way is pushed later (ripple insert).
    fn place(&mut self, id: u32, track: u32, at: i64) {
        let length = self.clip(id).map(|c| c.length).unwrap_or(1);
        let mut at = at.max(0);
        if let Some(hit) = self
            .clips
            .iter()
            .find(|c| c.id != id && c.track == track && c.start < at && at < c.end())
        {
            at = if at - hit.start < hit.end() - at {
                hit.start
            } else {
                hit.end()
            };
        }
        let push = self
            .clips
            .iter()
            .filter(|c| c.id != id && c.track == track && c.start >= at)
            .map(|c| c.start)
            .min()
            .map_or(0, |first| (at + length - first).max(0));
        for c in self.clips.iter_mut() {
            if c.id != id && c.track == track && c.start >= at {
                c.shift(push);
            }
        }
        if let Ok(c) = self.clip_mut(id) {
            c.track = track;
            c.start = at;
        }
        self.settle();
    }

    /// Move a clip to `track` at `at`.
    pub fn move_clip(&mut self, id: u32, track: u32, at: i64) -> Result<(), String> {
        let clip = self.clip(id).ok_or("no such clip")?;
        let from = clip.track;
        let kind = self.source_kind(&clip.source.clone())?;
        self.writable(from)?;
        if self.writable(track)?.kind != kind {
            return Err("a clip stays on a track of its own kind".into());
        }
        // A clip that leaves its place leaves its transitions.
        let (start, t) = (clip.start, clip.track);
        if start != at || t != track {
            self.transitions.retain(|x| x.left != id && x.right != id);
        }
        self.place(id, track, at);
        Ok(())
    }

    /// How far a clip's edge may move outward: into the neighbour's gap on its track
    /// and into what the source holds beyond the used span.
    pub fn room(&self, id: u32, edge: Edge, library: &Library) -> Result<i64, String> {
        let clip = self.clip(id).ok_or("no such clip")?;
        let neighbours = self.on_track(clip.track);
        let gap = match edge {
            Edge::In => {
                clip.start
                    - neighbours
                        .iter()
                        .filter(|c| c.id != id && c.end() <= clip.start)
                        .map(|c| c.end())
                        .max()
                        .unwrap_or(0)
            }
            Edge::Out => neighbours
                .iter()
                .filter(|c| c.id != id && c.start >= clip.end())
                .map(|c| c.start - clip.end())
                .min()
                .unwrap_or(i64::MAX / 4),
        };
        let source = match self.extent(clip, library) {
            None => i64::MAX / 4,
            Some(extent) => {
                // Source before the span, and after it.
                let (before, after) = (clip.offset, extent - clip.offset - clip.span());
                // A reversed clip shows the end of its span first.
                let spare = match (edge, clip.reverse) {
                    (Edge::In, false) | (Edge::Out, true) => before,
                    _ => after,
                };
                spare.max(0) / i64::from(clip.speed)
            }
        };
        Ok(gap.min(source).max(0))
    }

    /// Move one edge of a clip to timeline frame `to`, within [`Project::room`].
    pub fn trim(&mut self, id: u32, edge: Edge, to: i64, library: &Library) -> Result<(), String> {
        let clip = self.clip(id).ok_or("no such clip")?.clone();
        self.writable(clip.track)?;
        let room = self.room(id, edge, library)?;
        let c = self.clip_mut(id)?;
        let step = i64::from(c.speed);
        match edge {
            Edge::In => {
                let to = to.clamp(clip.start - room, clip.end() - 1);
                let d = to - clip.start;
                c.start = to;
                c.length -= d;
                if !c.reverse {
                    c.offset += d * step;
                }
                for p in [
                    &mut c.opacity,
                    &mut c.x,
                    &mut c.y,
                    &mut c.scale,
                    &mut c.rotation,
                    &mut c.volume,
                ] {
                    p.shift(-d);
                }
            }
            Edge::Out => {
                let to = to.clamp(clip.start + 1, clip.end() + room);
                let d = to - clip.end();
                c.length += d;
                if c.reverse {
                    c.offset -= d * step;
                }
            }
        }
        c.fade_in = c.fade_in.min(c.length as u32);
        c.fade_out = c.fade_out.min(c.length as u32);
        self.settle();
        Ok(())
    }

    /// Cut clip `id` in two at timeline frame `at`. Returns the new right-hand clip.
    pub fn split(&mut self, id: u32, at: i64) -> Result<u32, String> {
        if self.clips.len() >= MAX_CLIPS {
            return Err("the timeline has as many clips as it can hold".into());
        }
        let clip = self.clip(id).ok_or("no such clip")?;
        self.writable(clip.track)?;
        if at <= clip.start || at >= clip.end() {
            return Err("the playhead is not inside that clip".into());
        }
        let new = self.id();
        let c = self.clip_mut(id)?;
        let local = at - c.start;
        let mut right = c.clone();
        right.id = new;
        right.start = at;
        right.length = c.length - local;
        let step = i64::from(c.speed);
        if c.reverse {
            // The left half shows the later source; the right half keeps the offset.
            c.offset += right.length * step;
        } else {
            right.offset = c.offset + local * step;
        }
        c.length = local;
        c.fade_out = 0;
        right.fade_in = 0;
        right.fade_out = right.fade_out.min(right.length as u32);
        c.fade_in = c.fade_in.min(c.length as u32);
        for (l, r) in [
            (&mut c.opacity, &mut right.opacity),
            (&mut c.x, &mut right.x),
            (&mut c.y, &mut right.y),
            (&mut c.scale, &mut right.scale),
            (&mut c.rotation, &mut right.rotation),
            (&mut c.volume, &mut right.volume),
        ] {
            *r = l.split(local);
        }
        // A transition into the right-hand neighbour now belongs to the new clip.
        for t in &mut self.transitions {
            if t.left == id {
                t.left = new;
            }
        }
        self.clips.push(right);
        self.settle();
        Ok(new)
    }

    /// Remove a clip, leaving its place empty.
    pub fn delete(&mut self, id: u32) -> Result<(), String> {
        let clip = self.clip(id).ok_or("no such clip")?;
        self.writable(clip.track)?;
        self.clips.retain(|c| c.id != id);
        self.settle();
        Ok(())
    }

    /// Remove a clip and close the gap: everything later on its track moves up.
    pub fn ripple_delete(&mut self, id: u32) -> Result<(), String> {
        let clip = self.clip(id).ok_or("no such clip")?.clone();
        self.writable(clip.track)?;
        self.clips.retain(|c| c.id != id);
        for c in self.clips.iter_mut() {
            if c.track == clip.track && c.start >= clip.end() {
                c.shift(-clip.length);
            }
        }
        self.settle();
        Ok(())
    }

    /// Change a clip's speed. The source it plays stays the same, so its length on the
    /// timeline changes; later clips on the track are pushed if it grows into them.
    pub fn set_speed(&mut self, id: u32, speed: u32, library: &Library) -> Result<(), String> {
        if !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
            return Err(format!("speed is {MIN_SPEED}% to {MAX_SPEED}%"));
        }
        let clip = self.clip(id).ok_or("no such clip")?.clone();
        self.writable(clip.track)?;
        let span = clip.span();
        let mut length = (span / i64::from(speed)).max(1);
        if let Some(extent) = self.extent(&clip, library) {
            while clip.offset + length * i64::from(speed) > extent && length > 1 {
                length -= 1;
            }
        }
        let next = self
            .on_track(clip.track)
            .iter()
            .filter(|c| c.start >= clip.end() && c.id != id)
            .map(|c| c.start)
            .min();
        let c = self.clip_mut(id)?;
        let old = c.length;
        c.speed = speed;
        c.length = length;
        for p in [
            &mut c.opacity,
            &mut c.x,
            &mut c.y,
            &mut c.scale,
            &mut c.rotation,
            &mut c.volume,
        ] {
            p.scale_time(length, old);
        }
        c.fade_in = c.fade_in.min(length as u32);
        c.fade_out = c.fade_out.min(length as u32);
        let end = clip.start + length;
        if let Some(next) = next {
            let push = end - next;
            if push > 0 {
                for c in self.clips.iter_mut() {
                    if c.track == clip.track && c.id != id && c.start >= next {
                        c.shift(push);
                    }
                }
            }
        }
        self.settle();
        Ok(())
    }

    pub fn set_reverse(&mut self, id: u32, reverse: bool) -> Result<(), String> {
        let track = self.clip(id).ok_or("no such clip")?.track;
        self.writable(track)?;
        self.clip_mut(id)?.reverse = reverse;
        Ok(())
    }

    /// The clip that starts exactly where `id` ends, on the same track.
    pub fn next_touching(&self, id: u32) -> Option<&Clip> {
        let clip = self.clip(id)?;
        self.clips
            .iter()
            .find(|c| c.track == clip.track && c.id != id && c.start == clip.end())
    }
    /// Put a transition on the cut after clip `left`, replacing one already there.
    pub fn add_transition(
        &mut self,
        left: u32,
        kind: TransitionKind,
        frames: u32,
    ) -> Result<u32, String> {
        let a = self.clip(left).ok_or("no such clip")?;
        self.writable(a.track)?;
        if self.track(a.track).map(|t| t.kind) != Some(TrackKind::Video) {
            return Err("transitions go between video clips".into());
        }
        let b = self
            .next_touching(left)
            .ok_or("a transition needs a clip directly after this one")?;
        let right = b.id;
        let most = a.length.min(b.length).max(1) as u32;
        let frames = frames.clamp(1, most);
        self.transitions
            .retain(|t| !(t.left == left && t.right == right));
        let id = self.id();
        self.transitions.push(Transition {
            id,
            left,
            right,
            kind,
            frames,
        });
        Ok(id)
    }
    pub fn remove_transition(&mut self, id: u32) -> Result<(), String> {
        let before = self.transitions.len();
        self.transitions.retain(|t| t.id != id);
        if self.transitions.len() == before {
            return Err("no such transition".into());
        }
        Ok(())
    }
    /// Frames `[start, end)` a transition covers.
    pub fn transition_span(&self, t: &Transition) -> Option<(i64, i64)> {
        let cut = self.clip(t.left)?.end();
        let before = i64::from(t.frames) / 2;
        Some((cut - before, cut - before + i64::from(t.frames)))
    }

    /// Drop transitions whose clips no longer touch, and clamp what is left.
    fn settle(&mut self) {
        let clips = self.clips.clone();
        let find = |id: u32| clips.iter().find(|c| c.id == id);
        self.transitions.retain_mut(|t| {
            let (Some(a), Some(b)) = (find(t.left), find(t.right)) else {
                return false;
            };
            if a.track != b.track || a.end() != b.start {
                return false;
            }
            t.frames = t.frames.clamp(1, a.length.min(b.length).max(1) as u32);
            true
        });
        self.clips.sort_by_key(|c| (c.track, c.start, c.id));
    }

    /// Places an edge dragged near `frame` should settle on: the start, the playhead,
    /// and every clip edge but those of `exclude`.
    pub fn snap(&self, frame: i64, exclude: Option<u32>, playhead: i64, within: i64) -> i64 {
        let mut best = frame;
        let mut distance = within + 1;
        let points = [0, playhead].into_iter().chain(
            self.clips
                .iter()
                .filter(|c| Some(c.id) != exclude)
                .flat_map(|c| [c.start, c.end()]),
        );
        for p in points {
            let d = (p - frame).abs();
            if d < distance {
                distance = d;
                best = p;
            }
        }
        best
    }

    /// Check a loaded project, refusing what the editor could not honour.
    pub fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err("not a video project".into());
        }
        if self.version != VERSION {
            return Err(format!("project version {} is not supported", self.version));
        }
        if !(1..=120).contains(&self.fps)
            || !(8000..=96_000).contains(&self.sample_rate)
            || cw_raster::check_size(self.width, self.height).is_err()
            || self.width > 1920
            || self.height > 1080
        {
            return Err("project settings are out of range".into());
        }
        if self.clips.len() > MAX_CLIPS || self.tracks.len() > MAX_TRACKS {
            return Err("project is too large".into());
        }
        for kind in [TrackKind::Video, TrackKind::Audio] {
            if self.tracks_of(kind).is_empty() {
                return Err("a project needs a video and an audio track".into());
            }
        }
        let mut ids: Vec<u32> = self
            .tracks
            .iter()
            .map(|t| t.id)
            .chain(self.clips.iter().map(|c| c.id))
            .chain(self.media.iter().map(|m| m.id))
            .chain(self.transitions.iter().map(|t| t.id))
            .collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) || ids.last().is_some_and(|m| *m >= self.next_id) {
            return Err("project ids are inconsistent".into());
        }
        for c in &self.clips {
            let track = self.track(c.track).ok_or("a clip is on a missing track")?;
            if self.source_kind(&c.source)? != track.kind {
                return Err("a clip is on a track of the wrong kind".into());
            }
            if c.length < 1
                || c.start < 0
                || c.end() > MAX_FRAMES
                || !(MIN_SPEED..=MAX_SPEED).contains(&c.speed)
                || c.offset < 0
            {
                return Err(format!("clip {} is out of range", c.name));
            }
            for p in [&c.opacity, &c.x, &c.y, &c.scale, &c.rotation, &c.volume] {
                if p.keys.len() > MAX_KEYS || p.keys.windows(2).any(|w| w[0].frame >= w[1].frame) {
                    return Err("keyframes are out of order".into());
                }
            }
            if let Source::Title(t) = &c.source {
                if t.text.len() > TITLE_LIMIT || !(6..=200).contains(&t.size) {
                    return Err("a title is out of range".into());
                }
            }
        }
        for track in &self.tracks {
            let on = self.on_track(track.id);
            if on.windows(2).any(|w| w[0].end() > w[1].start) {
                return Err("clips overlap on a track".into());
            }
        }
        Ok(())
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a project always serializes")
    }
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut p: Project = serde_json::from_str(text).map_err(|e| e.to_string())?;
        p.validate()?;
        p.settle();
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Media;

    fn library(p: &mut Project) -> (Library, u32, u32) {
        let mut lib = Library::new();
        // A 2 s movie at 12 fps and a 3 s sound.
        let frames: Vec<cw_raster::Canvas> = (0..24)
            .map(|i| cw_raster::Canvas::filled(16, 9, [i * 10, 0, 0, 255]))
            .collect();
        let movie = Media::import(
            "m.apng",
            &crate::apng::encode(&frames, 12).unwrap(),
            p.sample_rate,
        )
        .unwrap();
        let sound = Media::audio("s.wav", vec![0; 3 * p.sample_rate as usize], p.sample_rate);
        let m = p.add_media("m.apng", MediaKind::Video);
        let s = p.add_media("s.wav", MediaKind::Audio);
        lib.insert(m, movie);
        lib.insert(s, sound);
        (lib, m, s)
    }
    fn project() -> (Project, Library, u32, u32, u32, u32) {
        let mut p = Project::new("Test", 320, 180, 24, 24_000);
        let (lib, m, s) = library(&mut p);
        let v = p.tracks_of(TrackKind::Video)[0].id;
        let a = p.tracks_of(TrackKind::Audio)[0].id;
        (p, lib, m, s, v, a)
    }

    #[test]
    fn clips_take_their_source_length_and_ripple_what_they_land_on() {
        let (mut p, lib, m, s, v, a) = project();
        let one = p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        assert_eq!(p.clip(one).unwrap().length, 48); // 2 s at 24 fps
                                                     // Dropped into the middle of the first: lands after it (nearer edge).
        let two = p.insert(&lib, Source::Media { media: m }, v, 30).unwrap();
        assert_eq!(p.clip(two).unwrap().start, 48);
        // Dropped near the start of the first: lands before it and pushes both along.
        let three = p.insert(&lib, Source::Media { media: m }, v, 5).unwrap();
        assert_eq!(p.clip(three).unwrap().start, 0);
        assert_eq!(p.clip(one).unwrap().start, 48);
        assert_eq!(p.clip(two).unwrap().start, 96);
        assert_eq!(p.duration(), 144);
        // Kinds stay on their own tracks.
        assert!(p.insert(&lib, Source::Media { media: s }, v, 0).is_err());
        assert!(p.insert(&lib, Source::Media { media: m }, a, 0).is_err());
        let sound = p.insert(&lib, Source::Media { media: s }, a, 0).unwrap();
        assert_eq!(p.clip(sound).unwrap().length, 72);
        p.validate().unwrap();
    }

    #[test]
    fn trimming_moves_the_source_window_and_stops_at_the_media() {
        let (mut p, lib, m, _, v, _) = project();
        let c = p.insert(&lib, Source::Media { media: m }, v, 24).unwrap();
        p.trim(c, Edge::In, 30, &lib).unwrap();
        let clip = p.clip(c).unwrap().clone();
        assert_eq!((clip.start, clip.length, clip.offset), (30, 42, 600));
        // Out further than the source holds: clamped to the end of the media.
        p.trim(c, Edge::Out, 500, &lib).unwrap();
        assert_eq!(p.clip(c).unwrap().end(), 72);
        // In before the source starts: clamped to its first frame.
        p.trim(c, Edge::In, 0, &lib).unwrap();
        let clip = p.clip(c).unwrap();
        assert_eq!((clip.start, clip.offset, clip.length), (24, 0, 48));
        // A neighbour limits the trim too.
        let d = p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        assert_eq!(p.clip(d).unwrap().end(), 48);
        let c_start = p.clip(c).unwrap().start;
        assert_eq!(c_start, 48, "inserted before and rippled");
        p.trim(d, Edge::Out, 100, &lib).unwrap();
        assert_eq!(p.clip(d).unwrap().end(), 48);
    }

    #[test]
    fn splitting_is_exact_and_the_halves_play_the_same_source() {
        let (mut p, lib, m, _, v, _) = project();
        let c = p.insert(&lib, Source::Media { media: m }, v, 10).unwrap();
        p.set_speed(c, 150, &lib).unwrap();
        let before: Vec<i64> = (0..p.clip(c).unwrap().length)
            .map(|l| p.clip(c).unwrap().source_at(l))
            .collect();
        let start = p.clip(c).unwrap().start;
        let right = p.split(c, start + 13).unwrap();
        let after: Vec<i64> = (0..p.clip(c).unwrap().length)
            .map(|l| p.clip(c).unwrap().source_at(l))
            .chain((0..p.clip(right).unwrap().length).map(|l| p.clip(right).unwrap().source_at(l)))
            .collect();
        assert_eq!(before, after);
        // Reversed too.
        let (mut p, lib, m, _, v, _) = project();
        let c = p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        p.set_reverse(c, true).unwrap();
        let before: Vec<i64> = (0..48).map(|l| p.clip(c).unwrap().source_at(l)).collect();
        assert_eq!(before[0], 4700);
        assert_eq!(before[47], 0);
        let right = p.split(c, 20).unwrap();
        let after: Vec<i64> = (0..20)
            .map(|l| p.clip(c).unwrap().source_at(l))
            .chain((0..28).map(|l| p.clip(right).unwrap().source_at(l)))
            .collect();
        assert_eq!(before, after);
        assert!(p.split(c, 0).is_err());
    }

    #[test]
    fn speed_changes_the_length_and_pushes_the_next_clip() {
        let (mut p, lib, m, _, v, _) = project();
        let a = p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        let b = p.insert(&lib, Source::Media { media: m }, v, 48).unwrap();
        p.set_speed(a, 50, &lib).unwrap();
        assert_eq!(p.clip(a).unwrap().length, 96);
        assert_eq!(p.clip(b).unwrap().start, 96);
        p.set_speed(a, 400, &lib).unwrap();
        assert_eq!(p.clip(a).unwrap().length, 12);
        // Shrinking leaves a gap rather than pulling the next clip.
        assert_eq!(p.clip(b).unwrap().start, 96);
        assert!(p.set_speed(a, 10, &lib).is_err());
        assert!(p.set_speed(a, 500, &lib).is_err());
    }

    #[test]
    fn ripple_delete_closes_the_gap_and_plain_delete_leaves_it() {
        let (mut p, lib, m, _, v, _) = project();
        let a = p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        let b = p.insert(&lib, Source::Media { media: m }, v, 48).unwrap();
        let c = p.insert(&lib, Source::Media { media: m }, v, 96).unwrap();
        p.delete(b).unwrap();
        assert_eq!(p.clip(c).unwrap().start, 96);
        p.ripple_delete(a).unwrap();
        assert_eq!(p.clip(c).unwrap().start, 48);
    }

    #[test]
    fn locked_tracks_refuse_edits_and_transitions_follow_their_clips() {
        let (mut p, lib, m, _, v, _) = project();
        let a = p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        let b = p.insert(&lib, Source::Media { media: m }, v, 48).unwrap();
        let t = p
            .add_transition(a, TransitionKind::CrossDissolve, 12)
            .unwrap();
        assert_eq!(p.transition_span(&p.transitions[0].clone()), Some((42, 54)));
        // Too long a transition is clamped to the shorter clip.
        p.add_transition(a, TransitionKind::Wipe, 500).unwrap();
        assert_eq!(p.transitions.len(), 1);
        assert_eq!(p.transitions[0].frames, 48);
        assert!(
            p.remove_transition(t).is_err(),
            "replaced, so the old id is gone"
        );
        // Splitting the left clip hands the transition to its right half.
        let right = p.split(a, 10).unwrap();
        assert_eq!(p.transitions[0].left, right);
        // Moving a clip away drops it.
        p.move_clip(b, v, 200).unwrap();
        assert!(p.transitions.is_empty());
        p.track_mut(v).unwrap().locked = true;
        assert!(p.delete(a).is_err());
        assert!(p.split(a, 5).is_err());
        assert!(p.move_clip(b, v, 0).is_err());
    }

    #[test]
    fn keyframes_interpolate_linearly_and_with_easing() {
        let mut param = Param::fixed(0);
        assert_eq!(param.at(5), 0);
        param.set_key(0, 0, Ease::Linear).unwrap();
        param.set_key(10, 1000, Ease::Ease).unwrap();
        param.set_key(20, 0, Ease::Linear).unwrap();
        assert_eq!(param.at(-3), 0);
        assert_eq!(param.at(5), 500);
        assert_eq!(param.at(1), 100);
        // Eased: slow at the ends, exact half at the middle.
        assert_eq!(param.at(15), 500);
        assert_eq!(param.at(11), 972);
        assert_eq!(param.at(25), 0);
        // Setting while animated edits keys; removing the last returns to fixed.
        param.set(10, 800);
        assert_eq!(param.at(10), 800);
        let after = param.split(5);
        assert_eq!(param.at(5), 400);
        assert_eq!(after.at(0), 400);
        assert_eq!(after.at(5), 800);
        let mut one = Param::fixed(3);
        one.set_key(4, 9, Ease::Linear).unwrap();
        one.remove_key(4).unwrap();
        assert_eq!((one.at(0), one.animated()), (9, false));
    }

    #[test]
    fn projects_round_trip_and_bad_files_are_refused() {
        let (mut p, lib, m, s, v, a) = project();
        p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        p.insert(&lib, Source::Media { media: s }, a, 0).unwrap();
        p.insert(
            &lib,
            Source::Title(Title {
                text: "Hello".into(),
                size: 24,
                bold: true,
                color: "ffffff".into(),
                background: None,
                position: TitlePosition::Lower,
            }),
            v,
            48,
        )
        .unwrap();
        let json = p.to_json();
        assert_eq!(Project::from_json(&json).unwrap(), p);
        assert!(Project::from_json(&json.replace(FORMAT, "other")).is_err());
        assert!(Project::from_json("{").is_err());
        let mut bad = p.clone();
        bad.clips[0].start = 10;
        assert!(Project::from_json(&bad.to_json()).is_err(), "overlap");
    }

    #[test]
    fn snapping_prefers_the_nearest_edge_within_reach() {
        let (mut p, lib, m, _, v, _) = project();
        let a = p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        assert_eq!(p.snap(50, None, 100, 3), 48);
        assert_eq!(p.snap(60, None, 100, 3), 60);
        assert_eq!(p.snap(98, None, 100, 3), 100);
        assert_eq!(p.snap(47, Some(a), 100, 3), 47);
        assert_eq!(p.timecode(24 * 61 + 5), "00:01:01:05");
    }
}
