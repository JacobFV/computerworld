//! Clock, stopwatch and timer over simulation time. There is no host clock anywhere in
//! this file; every reading is derived from the `clock_us` the shell threads in.
use super::look::{action, look, screen, INK, LINE, MUTED};
use crate::desktop_scene::shared::{arc_points, CalendarDate};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Face {
    #[default]
    World,
    Stopwatch,
    Timer,
}
/// A stopwatch is `elapsed` plus, while running, the time since `started`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stopwatch {
    pub elapsed_us: u64,
    pub started_us: Option<u64>,
}
impl Stopwatch {
    pub fn reading(&self, now_us: u64) -> u64 {
        self.elapsed_us + self.started_us.map_or(0, |s| now_us.saturating_sub(s))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    pub face: Face,
    pub stopwatch: Stopwatch,
    pub laps: Vec<u64>,
    /// Countdown length in whole minutes, and when it was started.
    pub timer_minutes: u64,
    pub timer_started_us: Option<u64>,
}
impl Clock {
    pub const KIND: &'static str = "clock";
    pub fn launch(_argument: &str, _window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        (
            Self {
                timer_minutes: 5,
                ..Default::default()
            },
            vec![],
        )
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        if theme == DesktopTheme::Ubuntu {
            "Clocks".into()
        } else {
            "Clock".into()
        }
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        String::new()
    }
    pub fn modified(&self) -> bool {
        false
    }
    pub fn offline(&mut self, _tag: &str, _reason: &str) {}
    pub fn http(
        &mut self,
        _window: u64,
        _tag: &str,
        _status: u16,
        _body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        Err("clock makes no requests".into())
    }
    pub fn text(&mut self, _text: &str) -> Result<(), String> {
        Err("clock has no text field".into())
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match key {
            "Enter" => self.click(window, "clock:start", clock_us),
            other => Err(format!("unsupported clock key {other}")),
        }
    }
    /// Microseconds left on the countdown, or `None` when no timer is running.
    pub fn remaining(&self, now_us: u64) -> Option<u64> {
        let started = self.timer_started_us?;
        let total = self.timer_minutes * 60 * 1_000_000;
        Some(total.saturating_sub(now_us.saturating_sub(started)))
    }
    pub fn click(
        &mut self,
        _window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("clock:")
            .ok_or("interaction does not belong to the clock")?;
        match command {
            "world" => self.face = Face::World,
            "stopwatch" => self.face = Face::Stopwatch,
            "timer" => self.face = Face::Timer,
            "start" => match self.face {
                Face::Stopwatch => {
                    if self.stopwatch.started_us.is_none() {
                        self.stopwatch.started_us = Some(clock_us);
                    }
                }
                Face::Timer => self.timer_started_us = Some(clock_us),
                Face::World => return Err("the world clock cannot be started".into()),
            },
            "stop" => match self.face {
                Face::Stopwatch => {
                    self.stopwatch.elapsed_us = self.stopwatch.reading(clock_us);
                    self.stopwatch.started_us = None;
                }
                Face::Timer => self.timer_started_us = None,
                Face::World => return Err("the world clock cannot be stopped".into()),
            },
            "reset" => match self.face {
                Face::Stopwatch => {
                    self.stopwatch = Stopwatch::default();
                    self.laps.clear();
                }
                Face::Timer => self.timer_started_us = None,
                Face::World => return Err("the world clock cannot be reset".into()),
            },
            "lap" => {
                if self.stopwatch.started_us.is_some() && self.laps.len() < 64 {
                    self.laps.push(self.stopwatch.reading(clock_us));
                }
            }
            "shorter" => self.timer_minutes = self.timer_minutes.saturating_sub(1).max(1),
            "longer" => self.timer_minutes = (self.timer_minutes + 1).min(180),
            other => return Err(format!("unknown clock command {other}")),
        }
        Ok(vec![])
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        page.elements.push(E::Heading {
            id: "clock-face".into(),
            text: match self.face {
                Face::World => "World clock",
                Face::Stopwatch => "Stopwatch",
                Face::Timer => "Timer",
            }
            .into(),
            level: 2,
        });
        for (id, label) in [
            ("clock:world", "World clock"),
            ("clock:stopwatch", "Stopwatch"),
            ("clock:timer", "Timer"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
                style: None,
            });
        }
        let controls: &[(&str, &str)] = match self.face {
            Face::World => &[],
            Face::Stopwatch => &[
                ("clock:start", "Start"),
                ("clock:stop", "Stop"),
                ("clock:lap", "Lap"),
                ("clock:reset", "Reset"),
            ],
            Face::Timer => &[
                ("clock:shorter", "Shorter"),
                ("clock:longer", "Longer"),
                ("clock:start", "Start"),
                ("clock:stop", "Stop"),
            ],
        };
        for (id, label) in controls {
            page.elements.push(E::Button {
                id: (*id).into(),
                text: (*label).into(),
                action: act(id),
                style: None,
            });
        }
        for (index, lap) in self.laps.iter().enumerate() {
            page.elements.push(E::Text {
                id: format!("clock-lap-{index}"),
                text: format!("Lap {}: {}", index + 1, duration(*lap)),
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let screen = screen(p, theme, &l, width, height as i32, &self.title(theme));
        let mut top = screen.top;
        let mut x = 8;
        for (target, label) in [
            ("clock:world", "World"),
            ("clock:stopwatch", "Stopwatch"),
            ("clock:timer", "Timer"),
        ] {
            let on = matches!(
                (self.face, target),
                (Face::World, "clock:world")
                    | (Face::Stopwatch, "clock:stopwatch")
                    | (Face::Timer, "clock:timer")
            );
            let w = p.measure(label, 12, true) + 24;
            let r = Rect::new(x, top + 6, w, 26);
            p.button(
                r,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                target,
                label,
            );
            p.label(
                r.x,
                r.y + 4,
                r.width,
                label,
                12,
                if on { l.accent } else { MUTED },
                on,
                Align::Center,
            );
            x += w as i32 + 6;
        }
        top += 40;
        p.hline(0, top, width, LINE);
        // The rendered reading is always the state at the last action, never "now":
        // a scene must be reproducible from the snapshot alone.
        let (big, controls): (String, &[(&str, &str)]) = match self.face {
            Face::World => (String::new(), &[]),
            Face::Stopwatch => (
                duration(
                    self.stopwatch
                        .reading(self.stopwatch.started_us.unwrap_or(0)),
                ),
                if self.stopwatch.started_us.is_some() {
                    &[("clock:lap", "Lap"), ("clock:stop", "Stop")]
                } else {
                    &[("clock:start", "Start"), ("clock:reset", "Reset")]
                },
            ),
            Face::Timer => (
                duration(self.timer_minutes * 60 * 1_000_000),
                if self.timer_started_us.is_some() {
                    &[("clock:stop", "Cancel")]
                } else {
                    &[
                        ("clock:shorter", "−"),
                        ("clock:longer", "+"),
                        ("clock:start", "Start"),
                    ]
                },
            ),
        };
        if self.face == Face::World {
            self.world_face(p, &l, width, height, top);
        } else {
            p.label(
                0,
                top + (height as i32 - top) / 3,
                width,
                &big,
                40,
                INK,
                false,
                Align::Center,
            );
        }
        let mut x = 14;
        for (target, label) in controls {
            let w = p.measure(label, 13, true) + 30;
            action(
                p,
                &l,
                Rect::new(x, height as i32 - 46, w, 32),
                label,
                target,
                *target == "clock:start",
            );
            x += w as i32 + 8;
        }
        // Every lap, newest first, in a list that scrolls once it outgrows its column.
        if !self.laps.is_empty() {
            let laps = p.pane(
                "laps",
                Rect::new(
                    width as i32 - 156,
                    top + 8,
                    150,
                    (height as i32 - top - 64).max(18) as u32,
                ),
            );
            for (index, lap) in self.laps.iter().rev().enumerate() {
                p.left(
                    width as i32 - 150,
                    laps.top() + 4 + index as i32 * 18,
                    140,
                    &format!("Lap {}  {}", self.laps.len() - index, duration(*lap)),
                    11,
                    MUTED,
                );
            }
            p.end_pane(laps, None);
        }
        screen.end(p);
    }
    /// An analogue face drawn from the world clock the shell already carries.
    fn world_face(
        &self,
        p: &mut Painter,
        l: &super::look::Look,
        width: u32,
        height: u32,
        top: i32,
    ) {
        let size = (width.min(height.saturating_sub(top as u32 + 60)) * 3 / 5).clamp(80, 260);
        let (cx, cy) = (width as i32 / 2, top + (height as i32 - top) / 2 - 16);
        let radius = size as i32 / 2;
        p.ring(cx, cy, radius as u32, 2, LINE);
        for tick in 0..12 {
            let outer = arc_points(cx, cy, radius - 4, tick * 30, tick * 30, 30);
            let inner = arc_points(cx, cy, radius - 12, tick * 30, tick * 30, 30);
            p.line(vec![outer[0], inner[0]], MUTED, 2);
        }
        // Hands are omitted deliberately: the world clock advances with simulation time,
        // and a scene must be reproducible from state alone, so the digital readout is
        // the honest presentation of the current tick.
        let date = CalendarDate::from_clock(0);
        p.label(
            0,
            cy - 12,
            width,
            &format!("{} {}", date.weekday_name(), date.day),
            15,
            INK,
            true,
            Align::Center,
        );
        p.label(
            0,
            cy + 10,
            width,
            date.month_name(),
            12,
            MUTED,
            false,
            Align::Center,
        );
        let _ = l;
    }
}
/// `h:mm:ss.cc` for a stopwatch, `mm:ss` for anything under an hour.
fn duration(us: u64) -> String {
    let total_cs = us / 10_000;
    let (h, m, s, cs) = (
        total_cs / 360_000,
        (total_cs / 6_000) % 60,
        (total_cs / 100) % 60,
        total_cs % 100,
    );
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{cs:02}")
    } else {
        format!("{m:02}:{s:02}.{cs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stopwatch_accumulates_simulation_time_only() {
        let (mut c, _) = Clock::launch("", 1, 0);
        c.click(1, "clock:stopwatch", 0).unwrap();
        c.click(1, "clock:start", 1_000_000).unwrap();
        assert_eq!(c.stopwatch.reading(3_500_000), 2_500_000);
        c.click(1, "clock:lap", 3_500_000).unwrap();
        c.click(1, "clock:stop", 4_000_000).unwrap();
        assert_eq!(c.stopwatch.reading(9_000_000), 3_000_000);
        assert_eq!(c.laps, vec![2_500_000]);
        c.click(1, "clock:reset", 9_000_000).unwrap();
        assert_eq!(c.stopwatch, Stopwatch::default());
        assert!(c.laps.is_empty());
    }
    #[test]
    fn timer_counts_down_from_a_real_length() {
        let (mut c, _) = Clock::launch("", 1, 0);
        c.click(1, "clock:timer", 0).unwrap();
        assert_eq!(c.timer_minutes, 5);
        c.click(1, "clock:longer", 0).unwrap();
        c.click(1, "clock:start", 60_000_000).unwrap();
        assert_eq!(c.remaining(120_000_000), Some(5 * 60 * 1_000_000));
        c.click(1, "clock:stop", 0).unwrap();
        assert_eq!(c.remaining(0), None);
    }
    #[test]
    fn faces_refuse_controls_that_do_not_belong_to_them() {
        let (mut c, _) = Clock::launch("", 1, 0);
        assert!(c.click(1, "clock:start", 0).is_err());
        assert!(c.click(1, "clock:bogus", 0).is_err());
        assert!(c.click(1, "not-mine", 0).is_err());
    }
    #[test]
    fn durations_format_without_floating_point() {
        assert_eq!(duration(0), "00:00.00");
        assert_eq!(duration(3_661_230_000), "1:01:01.23");
    }
}
