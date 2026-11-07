//! Calendar rules shared by API writes and background maintenance.
use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;

pub fn zone(name: &str) -> Result<Tz, chrono_tz::ParseError> {
    name.parse()
}

/// Match PostgreSQL's wall-clock resolution: later instant during an overlap,
/// pre-transition offset during a gap. In particular, never add 24 UTC hours.
pub fn at(zone: Tz, day: NaiveDate, hour: u32) -> DateTime<Utc> {
    let local = day.and_hms_opt(hour, 0, 0).unwrap();
    local_instant(zone, local)
}

pub fn local_instant(zone: Tz, local: NaiveDateTime) -> DateTime<Utc> {
    match zone.from_local_datetime(&local) {
        LocalResult::Single(value) => value.with_timezone(&Utc),
        LocalResult::Ambiguous(a, b) => a.max(b).with_timezone(&Utc),
        LocalResult::None => {
            // IANA transitions can skip an entire date (e.g. Pacific/Apia).
            let mut before = local - Duration::hours(1);
            loop {
                if let Some(value) = zone.from_local_datetime(&before).latest() {
                    return (local
                        - Duration::seconds(i64::from(value.offset().fix().local_minus_utc())))
                    .and_utc();
                }
                before -= Duration::hours(1);
            }
        }
    }
}

pub fn daily_reset(zone: Tz, instant: DateTime<Utc>) -> DateTime<Utc> {
    let day = instant.with_timezone(&zone).date_naive();
    let boundary = at(zone, day, 2);
    if instant < boundary {
        boundary
    } else {
        at(zone, day.succ_opt().unwrap(), 2)
    }
}

pub fn tomorrow_reset(zone: Tz, instant: DateTime<Utc>) -> DateTime<Utc> {
    at(
        zone,
        instant
            .with_timezone(&zone)
            .date_naive()
            .succ_opt()
            .unwrap(),
        2,
    )
}

pub fn planning_date(zone: Tz, instant: DateTime<Utc>) -> NaiveDate {
    daily_reset(zone, instant)
        .with_timezone(&zone)
        .date_naive()
        .pred_opt()
        .unwrap()
}

pub fn morning_schedule(zone: Tz, reset: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let day = reset.with_timezone(&zone).date_naive();
    (at(zone, day, 8), at(zone, day.succ_opt().unwrap(), 2))
}

/// Only the current alert window is actionable after downtime. Each all-day
/// slot ends at the next slot; the last ends at calendar midnight, not 02:00.
pub fn reminder_alert_window(
    zone: Tz,
    due: DateTime<Utc>,
    all_day: bool,
    now: DateTime<Utc>,
) -> Option<(i16, DateTime<Utc>, DateTime<Utc>)> {
    if !all_day {
        let start = due - Duration::hours(1);
        return (start <= now && now < due).then_some((0, start, due));
    }
    let day = due.with_timezone(&zone).date_naive();
    let times = [
        at(zone, day, 21),
        at(zone, day, 23),
        local_instant(zone, day.and_hms_opt(23, 55, 0).unwrap()),
        at(zone, day.succ_opt().unwrap(), 0),
    ];
    times.windows(2).enumerate().find_map(|(index, pair)| {
        (pair[0] <= now && now < pair[1]).then_some((index as i16 + 1, pair[0], pair[1]))
    })
}
