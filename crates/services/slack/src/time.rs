//! Civil time from Slack's clock, by integer arithmetic only.
//!
//! Slack's clock counts microseconds from `HISTORY` before the world's epoch of
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
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
pub const MINUTE_US: u64 = 60_000_000;
pub const HOUR_US: u64 = 60 * MINUTE_US;
pub const DAY_US: u64 = 24 * HOUR_US;
/// Minutes into the day at which Slack's clock starts: the world epoch is 09:00, and the
/// history offset is a whole number of days, so the phase is the same.
const EPOCH_MINUTE: u64 = 9 * 60;
/// Days per 400-year cycle; a whole cycle also preserves the weekday phase.
const CYCLE_DAYS: u64 = 146_097;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: u64,
    /// 1-12.
    pub month: u64,
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
    /// Days since Slack's epoch.
    pub index: u64,
    /// 0 is Sunday.
    pub weekday: u64,
}
impl Civil {
    pub fn month_name(&self) -> &'static str {
        MONTHS[(self.month as usize - 1) % 12]
    }
    pub fn weekday_name(&self) -> &'static str {
        WEEKDAYS[(self.weekday as usize) % 7]
    }
    /// "9:41 AM", the way Slack stamps a message.
    pub fn clock(&self) -> String {
        let (hour, half) = match self.hour {
            0 => (12, "AM"),
            h if h < 12 => (h, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        format!("{hour}:{:02} {half}", self.minute)
    }
    /// "Monday, September 14th", the label on a day older than a week.
    pub fn long_date(&self) -> String {
        let suffix = match self.day {
            11..=13 => "th",
            d if d % 10 == 1 => "st",
            d if d % 10 == 2 => "nd",
            d if d % 10 == 3 => "rd",
            _ => "th",
        };
        format!(
            "{}, {} {}{suffix}",
            self.weekday_name(),
            self.month_name(),
            self.day
        )
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
    // Slack's day zero is `HISTORY` before the world's 17 September; the offset is a
    // whole number of days, so it is a day count back from that date.
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
    // The world epoch is a Thursday (4); day zero is `world_epoch` days earlier.
    let weekday = (4 + 7 + index - world_epoch % 7) % 7;
    Civil {
        year,
        month,
        day,
        hour: minute_of_day / 60,
        minute: minute_of_day % 60,
        index,
        weekday,
    }
}
/// The label on the divider above a day's messages, seen from `now`.
pub fn day_label(at: u64, now: u64) -> String {
    let (day, today) = (civil(at), civil(now));
    match today.index.saturating_sub(day.index) {
        0 if day.index >= today.index => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => day.weekday_name().into(),
        _ => day.long_date(),
    }
}
/// "2h ago", the age of a thread's last reply.
pub fn ago(at: u64, now: u64) -> String {
    let gap = now.saturating_sub(at);
    if gap < MINUTE_US {
        "just now".into()
    } else if gap < HOUR_US {
        format!("{}m ago", gap / MINUTE_US)
    } else if gap < DAY_US {
        format!("{}h ago", gap / HOUR_US)
    } else {
        format!("{}d ago", gap / DAY_US)
    }
}
/// Slack's clock reading for a seed author: the day (0 is day zero), then the wall clock.
#[cfg(test)]
pub fn at(day: u64, hour: u64, minute: u64) -> u64 {
    (day * 1440 + hour * 60 + minute - EPOCH_MINUTE) * MINUTE_US
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_world_epoch_is_thursday_the_seventeenth_at_nine() {
        let epoch = civil(HISTORY);
        assert_eq!(
            (epoch.year, epoch.month, epoch.day, epoch.hour, epoch.minute),
            (2026, 9, 17, 9, 0)
        );
        assert_eq!(epoch.weekday_name(), "Thursday");
        assert_eq!(epoch.clock(), "9:00 AM");
        let zero = civil(0);
        assert_eq!((zero.month, zero.day), (9, 17 - HISTORY / DAY_US));
        assert_eq!(civil(at(7, 14, 5)).clock(), "2:05 PM");
        assert_eq!(civil(at(7, 0, 30)).clock(), "12:30 AM");
        assert_eq!(civil(at(7, 12, 0)).clock(), "12:00 PM");
    }
    #[test]
    fn day_labels_read_the_way_slack_writes_them() {
        let now = HISTORY + HOUR_US;
        assert_eq!(day_label(HISTORY, now), "Today");
        assert_eq!(day_label(HISTORY - DAY_US, now), "Yesterday");
        assert_eq!(day_label(HISTORY - 3 * DAY_US, now), "Monday");
        assert_eq!(day_label(0, now), "Thursday, September 10th");
        assert_eq!(ago(now - 2 * HOUR_US - 5 * MINUTE_US, now), "2h ago");
        assert_eq!(ago(now - 30 * MINUTE_US, now), "30m ago");
        assert_eq!(ago(now - 3 * DAY_US, now), "3d ago");
        assert_eq!(ago(now, now), "just now");
    }
}
