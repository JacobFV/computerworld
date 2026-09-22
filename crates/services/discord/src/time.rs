//! Civil time from Discord's clock, by integer arithmetic only.
//!
//! Discord's clock counts microseconds from `HISTORY` before the world's epoch of
//! 2026-09-17 09:00 (a Thursday), the epoch the Calendar and Mail services share, so a
//! seed can hold the week before the world starts. No host clock, no timezone, no date
//! library: a tick renders the same on every machine.
use crate::HISTORY;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const MINUTE_US: u64 = 60_000_000;
pub const HOUR_US: u64 = 60 * MINUTE_US;
pub const DAY_US: u64 = 24 * HOUR_US;
/// Minutes into the day at which Discord's clock starts: the world epoch is 09:00, and
/// the history offset is a whole number of days, so the phase is the same.
const EPOCH_MINUTE: u64 = 9 * 60;
/// Days per 400-year cycle.
const CYCLE_DAYS: u64 = 146_097;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: u64,
    /// 1-12.
    pub month: u64,
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
    /// Days since Discord's day zero.
    pub index: u64,
}
impl Civil {
    pub fn month_name(&self) -> &'static str {
        MONTHS[(self.month as usize - 1) % 12]
    }
    /// "9:41 AM", the wall clock Discord stamps a message with.
    pub fn clock(&self) -> String {
        let (hour, half) = match self.hour {
            0 => (12, "AM"),
            h if h < 12 => (h, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        format!("{hour}:{:02} {half}", self.minute)
    }
    /// "September 15, 2026", the label on the divider above a day's messages.
    pub fn long_date(&self) -> String {
        format!("{} {}, {}", self.month_name(), self.day, self.year)
    }
    /// "09/15/2026", the date on a message older than yesterday.
    pub fn short_date(&self) -> String {
        format!("{:02}/{:02}/{}", self.month, self.day, self.year)
    }
}
pub fn civil(us: u64) -> Civil {
    let minutes = us / MINUTE_US + EPOCH_MINUTE;
    let (index, minute_of_day) = (minutes / 1440, minutes % 1440);
    let leap = |y: u64| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let length = |y: u64, m: u64| match m {
        4 | 6 | 9 | 11 => 30,
        2 if leap(y) => 29,
        2 => 28,
        _ => 31,
    };
    // Day zero is `HISTORY` before the world's 17 September; the offset is a whole
    // number of days, so it is a day count back from that date.
    let world_epoch = HISTORY / DAY_US;
    let (mut year, mut month, mut day) =
        (2026 + index / CYCLE_DAYS * 400, 9, 17 - world_epoch.min(16));
    let mut remaining = index % CYCLE_DAYS;
    while remaining > 0 {
        let left = length(year, month) - day;
        if remaining <= left {
            day += remaining;
            break;
        }
        remaining -= left + 1;
        day = 1;
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
    Civil {
        year,
        month,
        day,
        hour: minute_of_day / 60,
        minute: minute_of_day % 60,
        index,
    }
}
/// "Today at 9:41 AM", "Yesterday at 3:12 PM" or "09/15/2026 3:12 PM": the stamp beside
/// an author's name, seen from `now`.
pub fn stamp(at: u64, now: u64) -> String {
    let (when, today) = (civil(at), civil(now));
    match today.index.saturating_sub(when.index) {
        0 if when.index >= today.index => format!("Today at {}", when.clock()),
        1 => format!("Yesterday at {}", when.clock()),
        _ => format!("{} {}", when.short_date(), when.clock()),
    }
}
/// The label on the divider above a day's messages.
pub fn day_label(at: u64) -> String {
    civil(at).long_date()
}
/// Discord's clock reading for a seed author: the day (0 is day zero), then the wall
/// clock.
#[cfg(test)]
pub fn at(day: u64, hour: u64, minute: u64) -> u64 {
    (day * 1440 + hour * 60 + minute - EPOCH_MINUTE) * MINUTE_US
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_world_epoch_is_the_seventeenth_at_nine() {
        let epoch = civil(HISTORY);
        assert_eq!(
            (epoch.year, epoch.month, epoch.day, epoch.hour, epoch.minute),
            (2026, 9, 17, 9, 0)
        );
        assert_eq!(epoch.clock(), "9:00 AM");
        assert_eq!(civil(0).long_date(), "September 10, 2026");
        assert_eq!(civil(at(7, 14, 5)).clock(), "2:05 PM");
        assert_eq!(civil(at(7, 0, 30)).clock(), "12:30 AM");
        assert_eq!(civil(at(7, 12, 0)).clock(), "12:00 PM");
    }
    #[test]
    fn stamps_read_the_way_discord_writes_them() {
        let now = HISTORY + HOUR_US;
        assert_eq!(stamp(HISTORY, now), "Today at 9:00 AM");
        assert_eq!(stamp(at(6, 15, 12), now), "Yesterday at 3:12 PM");
        assert_eq!(stamp(at(5, 15, 12), now), "09/15/2026 3:12 PM");
        assert_eq!(day_label(at(5, 15, 12)), "September 15, 2026");
        assert_eq!(day_label(at(30, 8, 0)), "October 10, 2026");
    }
}
