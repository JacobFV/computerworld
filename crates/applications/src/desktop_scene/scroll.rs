//! Scrolling as a platform service. A window keeps one offset per named pane
//! (`Scroll`); an application paints a pane's content shifted by its offset between
//! `Painter::pane` and `Painter::end_pane`, which clips the content to the viewport,
//! clamps an offset the content has shrunk under, publishes the pane as a
//! `cw_scene::ScrollArea` and paints a scroll bar whose thumb is the real extent.
//!
//! The pointer side is equally shared: a wheel turn or a phone swipe over a published
//! area moves its offset (see the environment's router), and the scroll bar is a drag
//! surface, `pane:<name>:<track>:<thumb>:<max>`, handled here for every application.
//!
//! A pane may scroll sideways instead (`Painter::hpane`): a shelf of album covers. Its
//! offset runs along x, it is published `horizontal`, the wheel's `delta_x` (or Shift
//! with the wheel) and a sideways swipe move it, and its bar runs along the bottom edge
//! as `hpane:<name>:<track>:<thumb>:<max>`.
use super::{DesktopTheme, Painter};
use crate::PointerPhase;
use cw_scene::{Color, Node, Primitive, Rect, ScrollArea, Semantic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Panes a window remembers offsets for. Names come from the application, not the
/// actor, but a bound keeps a window's state small whatever it paints.
pub const PANE_LIMIT: usize = 64;
/// Space left under the last thing painted in a pane measured automatically, so the
/// last row does not sit flush on the viewport's edge when scrolled to the end.
const END_PADDING: u32 = 12;
/// Shortest thumb a scroll bar paints, so a very long list still has one to grab.
const MIN_THUMB: u32 = 28;

/// Where each pane of one window is scrolled to, in pixels from its top.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scroll {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub offsets: BTreeMap<String, i32>,
    /// A scroll bar held by the pointer: its pane, and how far down the thumb it was
    /// grabbed, so the thumb stays under the pointer while it moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grab: Option<(String, i32)>,
    /// A pane a finger is pulling past one of its ends, and by how many pixels its
    /// content is displaced there (positive: pulled down past the top; negative: pulled
    /// up past the end). The rubber band of iOS and the overscroll of Android; it
    /// springs back to nothing when the finger lifts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stretch: Option<(String, i32)>,
    /// A pane whose caret was just moved by an edit: it is painted scrolled as little as
    /// it takes to show the caret, until it is scrolled again. The view is otherwise
    /// independent of the caret, as in every text editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<String>,
}
impl Scroll {
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
            && self.grab.is_none()
            && self.stretch.is_none()
            && self.reveal.is_none()
    }
    /// How far `pane`'s content is displaced past an end by a finger pulling on it.
    pub fn stretch_of(&self, pane: &str) -> i32 {
        self.stretch
            .as_ref()
            .filter(|(p, _)| p == pane)
            .map_or(0, |(_, s)| *s)
    }
    /// Whether `pane` must be painted showing its caret (see `reveal`).
    pub fn reveals(&self, pane: &str) -> bool {
        self.reveal.as_deref() == Some(pane)
    }
    /// The pane's offset as last set; painting clamps it to what the content allows.
    pub fn offset(&self, pane: &str) -> i32 {
        self.offsets.get(pane).copied().unwrap_or(0).max(0)
    }
    /// Scroll `pane` to `offset` (at least 0). Returns whether the stored offset
    /// changed. A pane that opens at its end (`Painter::pane_from_end`) is only at its
    /// end until it is first scrolled, so an offset, even 0, is kept once set.
    pub fn set(&mut self, pane: &str, offset: i32) -> bool {
        let offset = offset.max(0);
        // Scrolled on purpose, the view no longer chases the caret.
        if self.reveals(pane) {
            self.reveal = None;
        }
        if self.offsets.get(pane) == Some(&offset) {
            return false;
        }
        if !self.offsets.contains_key(pane) && self.offsets.len() >= PANE_LIMIT {
            if let Some(first) = self.offsets.keys().next().cloned() {
                self.offsets.remove(&first);
            }
        }
        self.offsets.insert(pane.to_owned(), offset);
        true
    }
    /// Forget where `pane` was: the content it showed was replaced (another folder,
    /// another mailbox), and a new list starts at its top.
    pub fn reset(&mut self, pane: &str) {
        self.offsets.remove(pane);
    }
    /// A press, move or release on a pane's scroll bar at (`x`, `y`) from the bar's
    /// top-left; only the coordinate along its track counts. A press on the thumb grabs
    /// it where it was pressed; a press elsewhere on the track centres the thumb there
    /// (GTK's and macOS's "jump to the spot") and grabs it by its middle.
    pub fn drag(
        &mut self,
        target: &str,
        phase: PointerPhase,
        x: i32,
        y: i32,
    ) -> Result<bool, String> {
        let bar = ScrollBar::parse(target).ok_or("not a scroll bar")?;
        let y = if bar.horizontal { x } else { y };
        let travel = bar.travel();
        let top = bar.thumb_top(self.offset(bar.pane).min(bar.max));
        match phase {
            PointerPhase::Down => {
                let grab = if (top..top + bar.thumb as i32).contains(&y) {
                    y - top
                } else {
                    bar.thumb as i32 / 2
                };
                self.grab = Some((bar.pane.to_owned(), grab));
            }
            PointerPhase::Cancel => {
                self.grab = None;
                return Ok(false);
            }
            _ => {}
        }
        let grab = self
            .grab
            .as_ref()
            .filter(|(pane, _)| pane == bar.pane)
            .map_or(bar.thumb as i32 / 2, |(_, grab)| *grab);
        let at = (y - grab).clamp(0, travel);
        let offset = (i64::from(at) * i64::from(bar.max) / i64::from(travel.max(1))) as i32;
        let moved = self.set(bar.pane, offset);
        if phase == PointerPhase::Up {
            self.grab = None;
        }
        Ok(moved)
    }
}

/// A scroll bar's target, which carries the geometry a drag needs so the drag reads
/// exactly the bar that was painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollBar<'a> {
    pub pane: &'a str,
    /// Length of the track, in pixels.
    pub track: u32,
    /// Length of the thumb.
    pub thumb: u32,
    /// The pane's furthest offset.
    pub max: i32,
    /// The bar runs along the bottom of a sideways-scrolling pane.
    pub horizontal: bool,
}
impl<'a> ScrollBar<'a> {
    pub fn target(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            if self.horizontal { "hpane" } else { "pane" },
            self.pane,
            self.track,
            self.thumb,
            self.max
        )
    }
    pub fn parse(target: &'a str) -> Option<Self> {
        let (horizontal, rest) = match target.strip_prefix("hpane:") {
            Some(rest) => (true, rest),
            None => (false, target.strip_prefix("pane:")?),
        };
        let mut parts = rest.split(':');
        let pane = parts.next().filter(|p| !p.is_empty())?;
        let track: u32 = parts.next()?.parse().ok()?;
        let thumb: u32 = parts.next()?.parse().ok()?;
        let max: i32 = parts.next()?.parse().ok()?;
        if parts.next().is_some() || thumb > track || max <= 0 {
            return None;
        }
        Some(Self {
            pane,
            track,
            thumb,
            max,
            horizontal,
        })
    }
    fn travel(&self) -> i32 {
        self.track.saturating_sub(self.thumb).max(1) as i32
    }
    /// Where the thumb starts along the track at `offset`.
    pub fn thumb_top(&self, offset: i32) -> i32 {
        (i64::from(offset.clamp(0, self.max)) * i64::from(self.travel())
            / i64::from(self.max.max(1))) as i32
    }
}

/// A pane being painted. Content drawn for it is placed `offset` pixels higher than it
/// would be unscrolled: start it at `top()`.
#[derive(Clone, Debug)]
pub struct Pane {
    pub name: String,
    pub viewport: Rect,
    /// The offset asked for; `end_pane` clamps it once the content's extent is known.
    pub offset: i32,
    mark: usize,
    scrolls: usize,
    title: Option<(String, u32)>,
    /// Scrolls sideways (`Painter::hpane`).
    pub horizontal: bool,
}
impl Pane {
    /// Where content that begins at the pane's top edge is painted.
    pub fn top(&self) -> i32 {
        self.viewport.y - self.offset
    }
    /// Where content that begins at a sideways pane's left edge is painted.
    pub fn left(&self) -> i32 {
        self.viewport.x - self.offset
    }
    /// Whether a column painted at `x`, `width` wide, is at least partly in view of a
    /// sideways pane.
    pub fn shows_x(&self, x: i32, width: u32) -> bool {
        x + width as i32 > self.viewport.x && x < self.viewport.right()
    }
    pub fn bottom(&self) -> i32 {
        self.viewport.bottom()
    }
    /// Whether a band painted at `y`, `height` tall, is at least partly in view: rows
    /// wholly outside need not be painted at all.
    pub fn shows(&self, y: i32, height: u32) -> bool {
        y + height as i32 > self.viewport.y && y < self.viewport.bottom()
    }
    /// The content starts with a large title `height` tall that collapses into the
    /// navigation bar once it has scrolled away, as on iOS.
    pub fn titled(mut self, title: &str, height: u32) -> Self {
        self.title = Some((title.to_owned(), height));
        self
    }
}

impl Painter {
    /// Open pane `name` over `viewport`. Nodes painted until `end_pane` belong to it.
    pub fn pane(&mut self, name: &str, viewport: Rect) -> Pane {
        debug_assert!(!name.contains(':'), "pane names are single segments");
        Pane {
            name: name.to_owned(),
            offset: self.scroll.offset(name),
            viewport,
            mark: self.scene.nodes.len(),
            scrolls: self.scene.scrolls.len(),
            title: None,
            horizontal: false,
        }
    }
    /// Open a pane that scrolls sideways over `viewport`: a shelf of cards. Content
    /// starts at `Pane::left()`.
    pub fn hpane(&mut self, name: &str, viewport: Rect) -> Pane {
        let mut pane = self.pane(name, viewport);
        pane.horizontal = true;
        pane
    }
    /// Open a pane that shows its end until it is scrolled: a conversation opens on
    /// its newest message.
    pub fn pane_from_end(&mut self, name: &str, viewport: Rect) -> Pane {
        let mut pane = self.pane(name, viewport);
        if !self.scroll.offsets.contains_key(name) {
            // Pulled back to the real end by `end_pane` once the extent is known.
            pane.offset = i32::MAX / 4;
        }
        pane
    }
    /// Close a pane. `extent` is the height of everything it holds; `None` measures
    /// it from what was painted. Content is clipped to the viewport, rows scrolled
    /// wholly out of it are dropped, an offset beyond the end is pulled back, and the
    /// pane is published with a scroll bar when its content does not fit.
    pub fn end_pane(&mut self, pane: Pane, extent: Option<u32>) -> ScrollArea {
        let view = pane.viewport;
        let across = pane.horizontal;
        let span = if across { view.width } else { view.height };
        let start = if across { pane.left() } else { pane.top() };
        let extent = extent.unwrap_or_else(|| {
            let end = self.scene.nodes[pane.mark..]
                .iter()
                .map(|n| {
                    let b = n.transform.bounds(n.bounds);
                    if across {
                        b.right()
                    } else {
                        b.bottom()
                    }
                })
                .max()
                .unwrap_or(start);
            let measured = (end - start).max(0) as u32;
            // The padding only matters to content that overflows; a layout sized to
            // fit the viewport does not become scrollable by it.
            if measured <= span {
                measured
            } else {
                measured + END_PADDING
            }
        });
        let max = extent.saturating_sub(span) as i32;
        let offset = pane.offset.clamp(0, max);
        // A finger pulling past an end displaces the content by the rubber band; only
        // at that end, and never by more than the viewport along the pane's own axis.
        let stretch = match self.scroll.stretch_of(&pane.name) {
            s if s > 0 && offset == 0 => s,
            s if s < 0 && offset == max => s,
            _ => 0,
        }
        .clamp(-(span as i32), span as i32);
        // Content painted for an offset the extent does not allow comes back.
        let shift = pane.offset - offset + stretch;
        let content = self.scene.nodes.split_off(pane.mark);
        for mut n in content {
            if shift != 0 && across {
                n.transform.tx = n.transform.tx.saturating_add(shift);
                if let Some(clip) = &mut n.clip {
                    clip.x = clip.x.saturating_add(shift);
                }
                if let Some(rounded) = &mut n.rounded_clip {
                    rounded.rect.x = rounded.rect.x.saturating_add(shift);
                }
            } else if shift != 0 {
                n.transform.ty = n.transform.ty.saturating_add(shift);
                if let Some(clip) = &mut n.clip {
                    clip.y = clip.y.saturating_add(shift);
                }
                if let Some(rounded) = &mut n.rounded_clip {
                    rounded.rect.y = rounded.rect.y.saturating_add(shift);
                }
            }
            let clip = n.clip.unwrap_or(view).intersection(view);
            let painted = clip.and_then(|c| n.transform.bounds(n.bounds).intersection(c));
            if painted.is_none() {
                continue;
            }
            n.clip = clip;
            self.scene.nodes.push(n);
        }
        let inner = self.scene.scrolls.split_off(pane.scrolls);
        for mut area in inner {
            if across {
                area.bounds.x = area.bounds.x.saturating_add(shift);
            } else {
                area.bounds.y = area.bounds.y.saturating_add(shift);
            }
            if let Some(bounds) = area.bounds.intersection(view) {
                area.bounds = bounds;
                self.scene.scrolls.push(area);
            }
        }
        let (title, title_height) = pane.title.clone().unwrap_or_default();
        let area = ScrollArea {
            target: format!("pane:{}", pane.name),
            window: None,
            bounds: view,
            offset,
            extent,
            title: pane.title.as_ref().map(|_| title),
            title_height,
            horizontal: across,
        };
        if max > 0 {
            if across {
                self.hscroll_bar(&pane.name, view, offset, extent);
            } else {
                self.scroll_bar(&pane.name, view, offset, extent);
            }
        }
        self.scene.scrolls.push(area.clone());
        area
    }
    /// The platform's scroll bar along the right edge of `view`. Desktops get a thumb
    /// that can be dragged and a track that jumps; a phone shows none at rest (its
    /// panes scroll under the finger), which is what the platforms do.
    pub(crate) fn scroll_bar(&mut self, pane: &str, view: Rect, offset: i32, extent: u32) {
        if self.theme.mobile() || view.height < 24 || view.width < 24 {
            return;
        }
        let track = view.height.saturating_sub(4);
        let thumb = (u64::from(track) * u64::from(view.height) / u64::from(extent.max(1)))
            .clamp(u64::from(MIN_THUMB.min(track)), u64::from(track)) as u32;
        let bar = ScrollBar {
            pane,
            track,
            thumb,
            max: extent.saturating_sub(view.height) as i32,
            horizontal: false,
        };
        let top = view.y + 2 + bar.thumb_top(offset);
        let (width, color) = match self.theme {
            // Overlay scrollers: a slim rounded thumb over the content.
            DesktopTheme::Macos => (7, Color(96, 96, 100, 150)),
            DesktopTheme::Windows => (6, Color(110, 110, 110, 170)),
            _ => (6, Color(100, 100, 100, 150)),
        };
        let z = self.z;
        self.z += 3;
        self.box_(
            Rect::new(view.right() - width as i32 - 3, top, width, thumb),
            color,
            width / 2,
        );
        let mut n = Node::new(
            self.next,
            Rect::new(view.right() - 14, view.y + 2, 14, track),
            Primitive::Region,
        );
        self.next += 1;
        n.z = self.z;
        n.interaction = Some(bar.target());
        n.semantic = Some(Semantic {
            role: "scrollbar".into(),
            label: format!("Scroll {pane}"),
            value: Some(format!("{offset} of {}", bar.max)),
            disabled: false,
            focusable: false,
        });
        self.scene.nodes.push(n);
        self.z = z;
    }
    /// A sideways pane's bar, along its bottom edge: the same thumb and track, turned.
    fn hscroll_bar(&mut self, pane: &str, view: Rect, offset: i32, extent: u32) {
        if self.theme.mobile() || view.height < 24 || view.width < 24 {
            return;
        }
        let track = view.width.saturating_sub(4);
        let thumb = (u64::from(track) * u64::from(view.width) / u64::from(extent.max(1)))
            .clamp(u64::from(MIN_THUMB.min(track)), u64::from(track)) as u32;
        let bar = ScrollBar {
            pane,
            track,
            thumb,
            max: extent.saturating_sub(view.width) as i32,
            horizontal: true,
        };
        let left = view.x + 2 + bar.thumb_top(offset);
        let (height, color) = match self.theme {
            DesktopTheme::Macos => (7, Color(96, 96, 100, 150)),
            DesktopTheme::Windows => (6, Color(110, 110, 110, 170)),
            _ => (6, Color(100, 100, 100, 150)),
        };
        let z = self.z;
        self.z += 3;
        self.box_(
            Rect::new(left, view.bottom() - height as i32 - 3, thumb, height),
            color,
            height / 2,
        );
        let mut n = Node::new(
            self.next,
            Rect::new(view.x + 2, view.bottom() - 14, track, 14),
            Primitive::Region,
        );
        self.next += 1;
        n.z = self.z;
        n.interaction = Some(bar.target());
        n.semantic = Some(Semantic {
            role: "scrollbar".into(),
            label: format!("Scroll {pane} sideways"),
            value: Some(format!("{offset} of {}", bar.max)),
            disabled: false,
            focusable: false,
        });
        self.scene.nodes.push(n);
        self.z = z;
    }
}

/// iOS's rubber band: a finger `excess` pixels past an end of a pane `dimension` tall
/// moves the content this much, ever less the further it pulls and never the whole
/// viewport. UIScrollView's curve, `(1 - 1 / (x * c / d + 1)) * d` with c = 0.55,
/// kept in integers so it is the same everywhere. Android's overscroll stretch resists
/// the same way and is modelled by the same curve.
pub fn rubber_band(excess: i32, dimension: u32) -> i32 {
    if excess == 0 || dimension == 0 {
        return 0;
    }
    let x = i64::from(excess.unsigned_abs());
    let d = i64::from(dimension);
    let moved = (x * 55 * d / (x * 55 + d * 100)) as i32;
    moved * excess.signum()
}
/// Slowest finger, in pixels per second, that still flings a list on release
/// (Android's `ViewConfiguration` minimum fling velocity, 50 dp/s).
pub const MIN_FLING: i64 = 50;
/// Fastest fling a list takes (Android's maximum, 8000 dp/s); faster is clamped.
pub const MAX_FLING: i64 = 8000;
/// How far a list keeps moving after a finger leaves it at `velocity` pixels per
/// second (signed; the result has the same sign), as each platform decelerates it.
///
/// iOS decelerates exponentially at UIScrollView's normal rate, 0.998 per
/// millisecond, which travels `v / 1000 * r / (1 - r)` = 0.499 v in all. Android's
/// OverScroller follows its spline, `f * c * exp(D / (D - 1) * ln(0.35 v / (f * c)))`
/// with friction f = 0.015, D = ln 0.78 / ln 0.9 and c the physical coefficient of a
/// 160 dpi pixel. Both are pure functions of the velocity, and the transcendental
/// functions are cw-determinism's, so a fling lands on the same pixel on every host.
pub fn fling_distance(android: bool, velocity: i64) -> i32 {
    let speed = velocity.unsigned_abs().min(i64::MAX as u64) as i64;
    if speed < MIN_FLING {
        return 0;
    }
    let speed = speed.min(MAX_FLING);
    let distance = if android {
        use cw_determinism::math::{exp, ln};
        const FRICTION: f64 = 0.015;
        // GRAVITY_EARTH * 39.37 in/m * 160 px/in * 0.84: OverScroller's constant.
        const PHYSICAL: f64 = 9.806_65 * 39.37 * 160.0 * 0.84;
        let decel = ln(0.78) / ln(0.9);
        let l = ln(0.35 * speed as f64 / (FRICTION * PHYSICAL));
        (FRICTION * PHYSICAL * exp(decel / (decel - 1.0) * l)) as i64
    } else {
        speed * 499 / 1000
    };
    distance as i32 * velocity.signum() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pulled_end_stretches_and_resists_and_a_fling_decelerates_per_platform() {
        // The content follows the finger less and less past an end, never a viewport.
        assert_eq!(rubber_band(0, 600), 0);
        let small = rubber_band(40, 600);
        let large = rubber_band(400, 600);
        assert!(small > 0 && small < 40, "{small}");
        assert!(large > small && large < 400, "{large}");
        assert!(rubber_band(1_000_000, 600) < 600);
        assert_eq!(rubber_band(-40, 600), -small);
        // Painted, a stretch at the top moves the content down, and only at the top.
        for (offset, expected) in [(0, 230), (50, 150)] {
            let mut p = Painter::themed(DesktopTheme::Ios, 300, 400, 1);
            p.scroll.set("list", offset);
            p.scroll.stretch = Some(("list".into(), 30));
            let pane = p.pane("list", Rect::new(0, 100, 300, 200));
            p.button(
                Rect::new(0, pane.top() + 100, 280, 40),
                Color::WHITE,
                0,
                "row:0",
                "Row",
            );
            p.end_pane(pane, Some(800));
            let row = p
                .scene
                .nodes
                .iter()
                .find(|n| n.interaction.as_deref() == Some("row:0"))
                .unwrap();
            assert_eq!(row.transform.bounds(row.bounds).y, expected);
        }
        // Flings: iOS travels about half the release speed; Android's spline goes
        // less far; both keep the sign and ignore a finger that was barely moving.
        assert_eq!(fling_distance(false, 1000), 499);
        assert_eq!(fling_distance(false, -1000), -499);
        let android = fling_distance(true, 1200);
        assert!((200..340).contains(&android), "{android}");
        assert_eq!(fling_distance(true, 30), 0);
        assert_eq!(
            fling_distance(false, 100_000),
            fling_distance(false, MAX_FLING)
        );
    }

    fn painted(offset: i32, rows: i32) -> (Painter, ScrollArea) {
        let mut p = Painter::themed(DesktopTheme::Macos, 300, 400, 1);
        p.scroll.set("list", offset);
        let pane = p.pane("list", Rect::new(0, 100, 300, 200));
        for i in 0..rows {
            p.button(
                Rect::new(0, pane.top() + i * 40, 280, 40),
                Color::WHITE,
                0,
                &format!("row:{i}"),
                "Row",
            );
        }
        let area = p.end_pane(pane, None);
        (p, area)
    }

    #[test]
    fn a_pane_clips_culls_and_publishes_its_real_extent() {
        let (p, area) = painted(0, 20);
        assert_eq!(area.extent, 20 * 40 + END_PADDING);
        assert_eq!(area.max_offset(), 800 + END_PADDING as i32 - 200);
        // Five rows fit in 200 px; the rest are not in the scene at all.
        let rows: Vec<_> = p
            .scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .filter(|i| i.starts_with("row:"))
            .collect();
        assert_eq!(rows, ["row:0", "row:1", "row:2", "row:3", "row:4"]);
        assert_eq!(p.scene.scrolls, vec![area]);
        // The bar is a real control reporting where the view is.
        let bar = p
            .scene
            .nodes
            .iter()
            .find(|n| {
                n.interaction
                    .as_deref()
                    .is_some_and(|i| i.starts_with("pane:"))
            })
            .unwrap();
        assert_eq!(bar.semantic.as_ref().unwrap().role, "scrollbar");
    }

    #[test]
    fn scrolling_moves_rows_and_an_overlong_offset_is_pulled_back() {
        let (p, area) = painted(120, 20);
        assert_eq!(area.offset, 120);
        let first = p
            .scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("row:3"))
            .unwrap();
        assert_eq!(first.transform.bounds(first.bounds).y, 100 + 3 * 40 - 120);
        let (_, area) = painted(10_000, 20);
        assert_eq!(area.offset, area.max_offset());
        // Content that fits cannot scroll and paints no bar.
        let (p, area) = painted(50, 3);
        assert_eq!(area.offset, 0);
        assert!(!p.scene.nodes.iter().any(|n| n
            .interaction
            .as_deref()
            .is_some_and(|i| i.starts_with("pane:"))));
    }

    #[test]
    fn dragging_the_thumb_and_jumping_on_the_track() {
        let bar = ScrollBar {
            pane: "list",
            track: 200,
            thumb: 50,
            max: 600,
            horizontal: false,
        };
        let target = bar.target();
        assert_eq!(ScrollBar::parse(&target), Some(bar));
        let mut s = Scroll::default();
        // Grab the thumb at its top edge and pull it half way down its travel.
        s.drag(&target, PointerPhase::Down, 0, 2).unwrap();
        assert_eq!(s.offset("list"), 0);
        s.drag(&target, PointerPhase::Move, 0, 77).unwrap();
        assert_eq!(s.offset("list"), 300);
        s.drag(&target, PointerPhase::Up, 0, 1000).unwrap();
        assert_eq!(s.offset("list"), 600);
        assert!(s.grab.is_none());
        // A press on the track away from the thumb centres the thumb there.
        let mut s = Scroll::default();
        s.drag(&target, PointerPhase::Down, 0, 100).unwrap();
        assert_eq!(s.offset("list"), 300);
        assert!(ScrollBar::parse("pane:list:10:20:5").is_none());
    }

    #[test]
    fn a_sideways_pane_clips_along_x_and_its_bar_drags_along_the_bottom() {
        let mut p = Painter::themed(DesktopTheme::Macos, 800, 400, 1);
        p.scroll.set("shelf", 250);
        let pane = p.hpane("shelf", Rect::new(20, 100, 400, 200));
        for i in 0..8 {
            p.button(
                Rect::new(pane.left() + i * 170, 100, 160, 180),
                Color::WHITE,
                0,
                &format!("card:{i}"),
                "Card",
            );
        }
        let area = p.end_pane(pane, None);
        assert!(area.horizontal);
        assert_eq!(area.extent, 8 * 170 - 10 + END_PADDING);
        assert_eq!(area.offset, 250);
        // Card 1 starts at 170 - 250 from the left edge; card 0 is gone.
        let x = |i: usize| {
            p.scene
                .nodes
                .iter()
                .find(|n| n.interaction.as_deref() == Some(&format!("card:{i}")))
                .map(|n| n.transform.bounds(n.bounds).x)
        };
        assert_eq!(x(0), None);
        assert_eq!(x(1), Some(20 + 170 - 250));
        assert_eq!(x(7), None, "past the right edge");
        let bar = p
            .scene
            .nodes
            .iter()
            .find_map(|n| n.interaction.as_deref().and_then(ScrollBar::parse))
            .unwrap();
        assert!(bar.horizontal && bar.target().starts_with("hpane:shelf:"));
        let mut s = Scroll::default();
        s.drag(&bar.target(), PointerPhase::Down, bar.track as i32, 0)
            .unwrap();
        assert_eq!(
            s.offset("shelf"),
            bar.max,
            "a press at the far end goes there"
        );
    }

    #[test]
    fn offsets_are_bounded_and_the_top_is_the_default() {
        let mut s = Scroll::default();
        for i in 0..PANE_LIMIT + 5 {
            s.set(&format!("p{i:03}"), 10);
        }
        assert_eq!(s.offsets.len(), PANE_LIMIT);
        // The oldest names made room for the newest.
        assert!(!s.offsets.contains_key("p000"));
        assert_eq!(s.offset("never"), 0);
        assert!(s.set("p010", -5));
        assert_eq!(s.offset("p010"), 0);
        assert!(!s.set("p010", 0));
    }

    #[test]
    fn a_pane_from_its_end_opens_on_the_last_row_until_scrolled() {
        let mut p = Painter::themed(DesktopTheme::Macos, 300, 400, 1);
        let pane = p.pane_from_end("chat", Rect::new(0, 0, 300, 100));
        for i in 0..10 {
            p.box_(Rect::new(0, pane.top() + i * 40, 300, 40), Color::WHITE, 0);
        }
        let area = p.end_pane(pane, Some(400));
        assert_eq!(area.offset, 300);
        // Scrolled all the way up, it stays there rather than snapping back.
        p = Painter::themed(DesktopTheme::Macos, 300, 400, 1);
        p.scroll.set("chat", 0);
        let pane = p.pane_from_end("chat", Rect::new(0, 0, 300, 100));
        assert_eq!(p.end_pane(pane, Some(400)).offset, 0);
    }
}
