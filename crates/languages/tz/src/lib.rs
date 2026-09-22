//! A compiled-in subset of the IANA time zone database.
//!
//! The simulated machines have no `/usr/share/zoneinfo` and no host clock, so
//! the zones a program can name are the ones in [`data`], generated from the
//! host's database by `tools/generate.py` and checked in. Each zone carries its
//! transitions between 1970 and 2050: the instant, the offset east of UTC, the
//! abbreviation and whether the offset is a daylight one. Outside that range the
//! first (or last) transition's offset stands, which is what a zone without
//! future rules does anyway.
//!
//! Everything here is pure data and arithmetic: no clock, no files, no host.
pub mod data;

/// One transition: `(seconds since the previous one, offset east of UTC,
/// abbreviation index, is daylight)`.
pub type Transition = (i64, i32, u16, u8);

/// What a zone was doing at an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offset {
    /// Seconds east of UTC (`-18000` for New York in winter).
    pub seconds: i32,
    /// The abbreviation in use (`EST`, `CEST`, `+0530`).
    pub abbreviation: &'static str,
    pub is_dst: bool,
}

/// Resolves an alias (`US/Eastern`, `Asia/Calcutta`) to the zone it names.
pub fn canonical(name: &str) -> Option<&'static str> {
    if let Some((n, _)) = data::ZONES.iter().find(|(n, _)| *n == name) {
        return Some(n);
    }
    let target = data::ALIASES
        .iter()
        .find(|(a, _)| a.eq_ignore_ascii_case(name))
        .map(|(_, t)| *t)?;
    data::ZONES
        .iter()
        .find(|(n, _)| *n == target)
        .map(|(n, _)| *n)
}

/// Whether the database knows this zone (or an alias of it).
pub fn exists(name: &str) -> bool {
    canonical(name).is_some()
}

/// Every zone, in the order the generator wrote them (`UTC` first, then
/// alphabetically).
pub fn zones() -> impl Iterator<Item = &'static str> {
    data::ZONES.iter().map(|(n, _)| *n)
}

fn table(name: &str) -> Option<&'static [Transition]> {
    let name = canonical(name)?;
    data::ZONES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
}

fn entry(t: &Transition) -> Offset {
    Offset {
        seconds: t.1,
        abbreviation: data::ABBREVIATIONS
            .get(t.2 as usize)
            .copied()
            .unwrap_or("UTC"),
        is_dst: t.3 != 0,
    }
}

/// The offset a zone was on at `utc_seconds` (seconds since the epoch).
pub fn offset_at(zone: &str, utc_seconds: i64) -> Option<Offset> {
    let t = table(zone)?;
    let mut at = 0i64;
    let mut current = t.first()?;
    for row in t {
        at += row.0;
        if at > utc_seconds {
            break;
        }
        current = row;
    }
    Some(entry(current))
}

/// The instant a zone's offset last changed at or before `utc_seconds`, and the
/// one it changes next: what `zoneinfo` needs to fold local times.
pub fn transition_around(zone: &str, utc_seconds: i64) -> Option<(i64, Option<i64>)> {
    let t = table(zone)?;
    let mut at = 0i64;
    let mut last = i64::MIN;
    for row in t {
        at += row.0;
        if at > utc_seconds {
            return Some((last, Some(at)));
        }
        last = at;
    }
    Some((last, None))
}

/// The offset for a *local* time, as `zoneinfo` and `Intl` resolve it: the
/// offset in force at that wall clock. A local time that happens twice (the end
/// of daylight saving) takes the earlier offset unless `fold` is set; one that
/// never happens (the start) takes the offset before the gap, which moves the
/// instant forward, as CPython and V8 both do.
pub fn offset_for_local(zone: &str, local_seconds: i64, fold: bool) -> Option<Offset> {
    let t = table(zone)?;
    // Candidates: the offsets whose own local time lands where we are.
    let mut at = 0i64;
    let mut rows: Vec<(i64, &Transition)> = Vec::with_capacity(t.len());
    for row in t {
        at += row.0;
        rows.push((at, row));
    }
    let mut matches: Vec<&Transition> = vec![];
    for (i, (start, row)) in rows.iter().enumerate() {
        let end = rows.get(i + 1).map(|(s, _)| *s).unwrap_or(i64::MAX);
        let utc = local_seconds - row.1 as i64;
        let after_start = *start == rows[0].0 || utc >= *start;
        if after_start && utc < end {
            matches.push(row);
        }
    }
    let chosen = match matches.len() {
        0 => {
            // A local time in a gap: the offset in force before it.
            let mut best = rows.first()?.1;
            for (start, row) in &rows {
                if local_seconds - row.1 as i64 >= *start {
                    best = row;
                }
            }
            best
        }
        1 => matches[0],
        _ => {
            if fold {
                matches[matches.len() - 1]
            } else {
                matches[0]
            }
        }
    };
    Some(entry(chosen))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-06-15T12:00:00Z and 2026-01-15T12:00:00Z.
    const SUMMER: i64 = 1_781_006_400;
    const WINTER: i64 = 1_768_478_400;

    #[test]
    fn offsets_follow_daylight_saving() {
        let ny = offset_at("America/New_York", SUMMER).unwrap();
        assert_eq!(
            (ny.seconds, ny.abbreviation, ny.is_dst),
            (-14400, "EDT", true)
        );
        let ny = offset_at("America/New_York", WINTER).unwrap();
        assert_eq!(
            (ny.seconds, ny.abbreviation, ny.is_dst),
            (-18000, "EST", false)
        );
        let berlin = offset_at("Europe/Berlin", SUMMER).unwrap();
        assert_eq!(
            (berlin.seconds, berlin.abbreviation, berlin.is_dst),
            (7200, "CEST", true)
        );
        // Zones that do not change.
        for (zone, seconds) in [
            ("UTC", 0),
            ("Asia/Tokyo", 32400),
            ("Asia/Kolkata", 19800),
            ("Asia/Kathmandu", 20700),
            ("Australia/Brisbane", 36000),
            ("Pacific/Honolulu", -36000),
        ] {
            assert_eq!(offset_at(zone, SUMMER).unwrap().seconds, seconds, "{zone}");
            assert_eq!(offset_at(zone, WINTER).unwrap().seconds, seconds, "{zone}");
        }
        // The southern hemisphere is the other way round.
        assert_eq!(
            offset_at("Australia/Sydney", SUMMER).unwrap().seconds,
            36000
        );
        assert_eq!(
            offset_at("Australia/Sydney", WINTER).unwrap().seconds,
            39600
        );
    }

    #[test]
    fn aliases_resolve() {
        assert_eq!(canonical("US/Eastern"), Some("America/New_York"));
        assert_eq!(canonical("Asia/Calcutta"), Some("Asia/Kolkata"));
        assert_eq!(canonical("Etc/UTC"), Some("UTC"));
        assert_eq!(canonical("Mars/Olympus"), None);
        assert!(exists("Europe/London") && !exists("Europe/Atlantis"));
    }

    #[test]
    fn local_times_resolve_with_their_fold() {
        // 2026-11-01 01:30 in New York happens twice.
        let ambiguous = local_seconds(2026, 11, 1, 1, 30);
        let early = offset_for_local("America/New_York", ambiguous, false).unwrap();
        let late = offset_for_local("America/New_York", ambiguous, true).unwrap();
        assert_eq!((early.abbreviation, late.abbreviation), ("EDT", "EST"));
        // 2026-03-08 02:30 never happens.
        let gap = local_seconds(2026, 3, 8, 2, 30);
        let g = offset_for_local("America/New_York", gap, false).unwrap();
        assert_eq!(g.abbreviation, "EST");
    }

    /// Seconds since the epoch for a local wall clock, ignoring zones.
    fn local_seconds(y: i64, m: i64, d: i64, h: i64, min: i64) -> i64 {
        let days = days_from_civil(y, m, d);
        days * 86400 + h * 3600 + min * 60
    }

    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }
}
