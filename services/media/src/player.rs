//! The listening session: what is loaded, where in it the listener is, and how the world
//! clock moves that position. Nothing here makes a sound; everything here is state.
//!
//! A player never runs on a timer. It records the position it had at one tick (`tick`) and
//! whether it was running; at any later tick the position is that one plus the time since,
//! carried across track boundaries by the queue and the repeat mode. `settle` is that one
//! calculation, so the service, a page and a native player that repeat it at the same tick
//! all agree without anything having to wake up and advance playback.
use serde::{Deserialize, Serialize};

/// Ticks are microseconds; positions are milliseconds.
const TICKS_PER_MS: u64 = 1_000;
/// "Previous" restarts the track once it has played this long, as every player does.
pub const RESTART_MS: u64 = 3_000;
/// A queue longer than this is truncated when it is built. A catalogue may be any size.
pub const QUEUE_LIMIT: usize = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}
impl Repeat {
    /// The order every player's repeat button cycles through.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "all" => Some(Self::All),
            "one" => Some(Self::One),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    /// The current track. Kept beside `queue[index]` because older seeds only carry this.
    pub item: String,
    /// World tick at which `position_ms` was true.
    #[serde(default)]
    pub tick: u64,
    /// Playlist the session was started from, when it was one (the original seed field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<String>,
    /// What was played: `album:<id>`, `playlist:<id>`, `artist:<id>`, `station:<id>`,
    /// `library`, `liked` or `track`.
    #[serde(default)]
    pub context: String,
    /// Play order, shuffled or not.
    #[serde(default)]
    pub queue: Vec<String>,
    /// The context's own order, so switching shuffle off can restore it.
    #[serde(default)]
    pub source: Vec<String>,
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub position_ms: u64,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub repeat: Repeat,
}

impl Player {
    /// A session that has been started from `queue`, at `index`, at `now`.
    pub fn start(context: &str, source: Vec<String>, index: usize, now: u64) -> Self {
        let mut source = source;
        source.truncate(QUEUE_LIMIT);
        let index = index.min(source.len().saturating_sub(1));
        Self {
            item: source.get(index).cloned().unwrap_or_default(),
            tick: now,
            list: context.strip_prefix("playlist:").map(str::to_owned),
            context: context.to_owned(),
            queue: source.clone(),
            source,
            index,
            position_ms: 0,
            playing: true,
            shuffle: false,
            repeat: Repeat::Off,
        }
    }
    /// Older seeds only name the track (and maybe the list); give them a queue.
    pub fn normalize(mut self) -> Self {
        if self.queue.is_empty() && !self.item.is_empty() {
            self.queue = vec![self.item.clone()];
        }
        if self.source.is_empty() {
            self.source = self.queue.clone();
        }
        if self.context.is_empty() {
            self.context = match &self.list {
                Some(list) => format!("playlist:{list}"),
                None => "track".into(),
            };
        }
        if let Some(at) = self.queue.iter().position(|id| *id == self.item) {
            if self.queue.get(self.index) != Some(&self.item) {
                self.index = at;
            }
        }
        self.index = self.index.min(self.queue.len().saturating_sub(1));
        self.item = self.queue.get(self.index).cloned().unwrap_or_default();
        self
    }
    pub fn current(&self) -> Option<&str> {
        self.queue.get(self.index).map(String::as_str)
    }
    /// Where playback is at `now`: the recorded position plus the time since, carried
    /// through the queue. Pure, so every reader at the same tick sees the same thing.
    pub fn settle(&self, now: u64, duration_ms: impl Fn(&str) -> u64) -> Self {
        let mut p = self.clone().normalize();
        if p.queue.is_empty() {
            p.playing = false;
            p.tick = now.max(p.tick);
            return p;
        }
        if !p.playing || now <= p.tick {
            p.tick = now.max(p.tick);
            return p;
        }
        let length = |p: &Self, i: usize| duration_ms(&p.queue[i]).max(1_000);
        let mut elapsed = (now - p.tick) / TICKS_PER_MS;
        // Every pass either finishes or moves to another track; a wrap folds the rest of
        // the time into one lap first, so the loop is bounded by the queue's length.
        for _ in 0..=p.queue.len() * 2 + 2 {
            let length_now = length(&p, p.index);
            let left = length_now.saturating_sub(p.position_ms);
            if elapsed < left {
                p.position_ms += elapsed;
                break;
            }
            elapsed -= left;
            p.position_ms = 0;
            match p.repeat {
                Repeat::One => elapsed %= length_now,
                _ if p.index + 1 < p.queue.len() => p.index += 1,
                Repeat::All => {
                    p.index = 0;
                    let lap: u64 = (0..p.queue.len()).map(|i| length(&p, i)).sum();
                    elapsed %= lap.max(1);
                }
                Repeat::Off => {
                    // The queue ran out: the last track stays loaded, stopped at its start.
                    p.playing = false;
                    break;
                }
            }
        }
        p.tick = now;
        p.item = p.queue[p.index].clone();
        p
    }
    pub fn toggle(&mut self) {
        self.playing = !self.playing;
    }
    pub fn skip(&mut self) -> Result<(), String> {
        if self.index + 1 < self.queue.len() {
            self.index += 1;
        } else if self.repeat == Repeat::All && !self.queue.is_empty() {
            self.index = 0;
        } else {
            return Err("nothing is queued after this track".into());
        }
        self.jump_here();
        Ok(())
    }
    pub fn previous(&mut self) {
        if self.position_ms > RESTART_MS || self.queue.is_empty() {
            self.position_ms = 0;
            return;
        }
        if self.index > 0 {
            self.index -= 1;
        } else if self.repeat == Repeat::All {
            self.index = self.queue.len() - 1;
        }
        self.jump_here();
    }
    pub fn jump(&mut self, index: usize) -> Result<(), String> {
        if index >= self.queue.len() {
            return Err("that queue position does not exist".into());
        }
        self.index = index;
        self.jump_here();
        self.playing = true;
        Ok(())
    }
    fn jump_here(&mut self) {
        self.position_ms = 0;
        self.item = self.queue[self.index].clone();
    }
    pub fn seek(&mut self, position_ms: u64, duration_ms: u64) {
        self.position_ms = position_ms.min(duration_ms.saturating_sub(1));
    }
    /// Shuffle keeps the current track playing and deals the rest in a seeded order.
    pub fn set_shuffle(&mut self, on: bool, seed: u64) {
        self.shuffle = on;
        let current = self.current().map(str::to_owned);
        if on {
            let mut rest: Vec<String> = self
                .source
                .iter()
                .filter(|id| Some(id.as_str()) != current.as_deref())
                .cloned()
                .collect();
            deal(&mut rest, seed);
            self.queue = current.into_iter().chain(rest).collect();
            self.index = 0;
        } else {
            self.queue = self.source.clone();
            self.index = current
                .and_then(|c| self.queue.iter().position(|id| *id == c))
                .unwrap_or(0);
        }
    }
    /// "Play Next" puts the track straight after this one; "Play Last" at the end.
    pub fn enqueue(&mut self, item: &str, next: bool) {
        if self.queue.len() >= QUEUE_LIMIT {
            return;
        }
        let at = if next {
            (self.index + 1).min(self.queue.len())
        } else {
            self.queue.len()
        };
        self.queue.insert(at, item.to_owned());
        if !self.source.iter().any(|id| id == item) {
            self.source.push(item.to_owned());
        }
    }
}

/// Fisher–Yates driven by SplitMix64: the same seed deals the same order everywhere.
fn deal(items: &mut [String], seed: u64) {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    };
    for i in (1..items.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: u64 = 1_000_000;
    fn queue() -> Vec<String> {
        ["a", "b", "c"].iter().map(|s| s.to_string()).collect()
    }
    /// a is 10 s, b 20 s, c 30 s.
    fn length(id: &str) -> u64 {
        match id {
            "a" => 10_000,
            "b" => 20_000,
            _ => 30_000,
        }
    }
    #[test]
    fn the_clock_moves_the_position_and_carries_it_into_the_next_track() {
        let p = Player::start("album:x", queue(), 0, 100);
        let later = p.settle(100 + 4 * S, length);
        assert_eq!((later.index, later.position_ms), (0, 4_000));
        let later = p.settle(100 + 15 * S, length);
        assert_eq!((later.item.as_str(), later.position_ms), ("b", 5_000));
        // Past the end with repeat off: stopped, last track loaded at its start.
        let done = p.settle(100 + 90 * S, length);
        assert_eq!((done.index, done.playing, done.position_ms), (2, false, 0));
        // A paused player does not move.
        let mut paused = p.clone();
        paused.toggle();
        assert_eq!(paused.settle(100 + 50 * S, length).position_ms, 0);
    }
    #[test]
    fn repeat_all_laps_and_repeat_one_holds_the_track() {
        let mut p = Player::start("album:x", queue(), 0, 0);
        p.repeat = Repeat::All;
        // One lap is 60 s; 125 s is two laps and 5 s into a.
        let lapped = p.settle(125 * S, length);
        assert_eq!(
            (lapped.index, lapped.position_ms, lapped.playing),
            (0, 5_000, true)
        );
        p.repeat = Repeat::One;
        let held = p.settle(34 * S, length);
        assert_eq!((held.index, held.position_ms), (0, 4_000));
    }
    #[test]
    fn previous_restarts_a_track_that_has_played_and_next_stops_at_the_end() {
        let mut p = Player::start("album:x", queue(), 1, 0);
        p.position_ms = 8_000;
        p.previous();
        assert_eq!((p.index, p.position_ms), (1, 0));
        p.previous();
        assert_eq!(p.index, 0);
        p.skip().unwrap();
        p.skip().unwrap();
        assert!(p.skip().is_err(), "nothing after the last track");
        p.repeat = Repeat::All;
        p.skip().unwrap();
        assert_eq!(p.index, 0);
    }
    #[test]
    fn shuffle_keeps_the_current_track_and_restores_the_order_after() {
        let long: Vec<String> = (0..12).map(|i| format!("t{i}")).collect();
        let mut p = Player::start("playlist:x", long.clone(), 4, 0);
        p.set_shuffle(true, 7);
        assert_eq!(p.current(), Some("t4"));
        assert_eq!(p.index, 0);
        assert_ne!(p.queue, long, "a seeded deal moves something");
        let mut again = Player::start("playlist:x", long.clone(), 4, 0);
        again.set_shuffle(true, 7);
        assert_eq!(p.queue, again.queue, "the same seed deals the same order");
        p.skip().unwrap();
        let now = p.current().unwrap().to_owned();
        p.set_shuffle(false, 7);
        assert_eq!(p.queue, long);
        assert_eq!(p.current(), Some(now.as_str()));
    }
    #[test]
    fn enqueue_and_seek_and_a_legacy_seed_normalizes() {
        let mut p = Player::start("album:x", queue(), 0, 0);
        p.enqueue("z", true);
        assert_eq!(p.queue, ["a", "z", "b", "c"]);
        p.enqueue("y", false);
        assert_eq!(p.queue.last().unwrap(), "y");
        p.seek(99_000, 10_000);
        assert_eq!(p.position_ms, 9_999);
        let legacy: Player =
            serde_json::from_str(r#"{"item":"cold-start","tick":9,"list":"ship-it"}"#).unwrap();
        let legacy = legacy.normalize();
        assert_eq!(legacy.queue, ["cold-start"]);
        assert_eq!(legacy.context, "playlist:ship-it");
        assert!(!legacy.playing);
    }
}
