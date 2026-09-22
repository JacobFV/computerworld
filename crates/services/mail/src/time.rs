//! Civil time from simulation time, by integer arithmetic only.
//!
//! The world clock counts microseconds from 2026-09-17 09:00, the same epoch the native
//! Calendar app reads (`cw_applications::desktop_scene::shared::CalendarDate::from_clock`).
//! No host clock, no timezone, no date library: a tick renders the same on every machine.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
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
    /// Days since the epoch.
    pub index: u64,
    /// 0 is Sunday; the epoch day is a Thursday.
    pub weekday: u64,
}
impl Civil {
    pub fn month_name(&self) -> &'static str {
        MONTHS[(self.month as usize - 1) % 12]
    }
    /// "10:30", zero-padded so column widths never jump between ticks.
    pub fn clock(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
    /// "17 Sep" — short enough for a message-list column.
    pub fn short(&self) -> String {
        format!("{} {}", self.day, self.month_name())
    }
}
pub fn civil(us: u64) -> Civil {
    let minutes = us / 60_000_000 + EPOCH_MINUTE;
    let (index, minute_of_day) = (minutes / 1440, minutes % 1440);
    let leap = |y: u64| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let length = |y: u64, m: u64| match m {
        4 | 6 | 9 | 11 => 30,
        2 if leap(y) => 29,
        2 => 28,
        _ => 31,
    };
    let (mut year, mut month, mut day) = (2026 + index / CYCLE_DAYS * 400, 9, 17);
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
        weekday: (4 + index) % 7,
    }
}
/// Message-list stamp: the clock time on the epoch day, the date on any other day.
pub fn stamp(us: u64) -> String {
    let at = civil(us);
    if at.index == 0 {
        at.clock()
    } else {
        at.short()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const DAY: u64 = 86_400_000_000;
    #[test]
    fn epoch_is_nine_in_the_morning_on_a_thursday() {
        let at = civil(0);
        assert_eq!(
            (at.year, at.month, at.day, at.hour, at.minute),
            (2026, 9, 17, 9, 0)
        );
        assert_eq!(at.weekday, 4);
        assert_eq!(stamp(3_600_000_000), "10:00");
    }
    #[test]
    fn days_roll_over_months_and_years() {
        assert_eq!(stamp(14 * DAY), "1 Oct");
        let new_year = civil(106 * DAY);
        assert_eq!((new_year.year, new_year.month, new_year.day), (2027, 1, 1));
        // 2028 is a leap year, so the day count to 2029 is one longer than to 2027.
        assert_eq!(civil(106 * DAY + 365 * DAY).year, 2028);
        assert_eq!(civil(106 * DAY + (365 + 366) * DAY).year, 2029);
    }
}
