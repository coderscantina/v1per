//! Expected values are reasoned from Kuma 2.5.0 `server/model/maintenance.js` (line numbers
//! below) and, for DST, from croner's documented Vixie behaviour.

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use v1per_maintenance_schedule::{Definition, Status, Window, next_change, status_at, windows};

fn utc(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

fn win(start: &str, end: &str) -> Window {
    Window {
        start: utc(start),
        end: utc(end),
    }
}

fn def(row: Value) -> Definition {
    serde_json::from_value(row).unwrap()
}

fn list(definition: &Definition, from: &str, to: &str) -> Vec<Window> {
    windows(definition, utc(from), utc(to)).collect()
}

#[test]
fn manual_follows_the_active_flag() {
    // :426-428 under maintenance whenever active, no windows.
    let mut manual = def(json!({"strategy": "manual", "active": true}));
    assert_eq!(
        status_at(&manual, utc("2026-06-01T00:00:00Z")),
        Status::UnderMaintenance
    );
    assert!(list(&manual, "2026-01-01T00:00:00Z", "2027-01-01T00:00:00Z").is_empty());
    assert_eq!(next_change(&manual, utc("2026-06-01T00:00:00Z")), None);
    manual.active = false;
    assert_eq!(
        status_at(&manual, utc("2026-06-01T00:00:00Z")),
        Status::Inactive
    );
}

#[test]
fn single_runs_between_its_dates_in_its_timezone() {
    // Vienna is UTC+2 in June. Start and end are strict comparisons at :431 and :436; the end
    // instant itself is already "ended" here.
    let single = def(json!({
        "strategy": "single", "active": true, "timezone": "Europe/Vienna",
        "start_date": "2026-06-01 22:00:00", "end_date": "2026-06-02T02:00",
    }));
    assert_eq!(
        status_at(&single, utc("2026-06-01T19:59:59Z")),
        Status::Scheduled
    );
    assert_eq!(
        status_at(&single, utc("2026-06-01T20:00:00Z")),
        Status::UnderMaintenance
    );
    assert_eq!(
        status_at(&single, utc("2026-06-01T23:59:59Z")),
        Status::UnderMaintenance
    );
    assert_eq!(
        status_at(&single, utc("2026-06-02T00:00:00Z")),
        Status::Ended
    );
    assert_eq!(
        list(&single, "2026-06-01T00:00:00Z", "2026-06-03T00:00:00Z"),
        [win("2026-06-01T20:00:00Z", "2026-06-02T00:00:00Z")]
    );
    assert_eq!(
        next_change(&single, utc("2026-06-01T00:00:00Z")),
        Some(utc("2026-06-01T20:00:00Z"))
    );
    assert_eq!(
        next_change(&single, utc("2026-06-01T20:00:00Z")),
        Some(utc("2026-06-02T00:00:00Z"))
    );
    assert_eq!(next_change(&single, utc("2026-06-02T00:00:00Z")), None);
}

#[test]
fn weekday_window_crosses_midnight() {
    // :474-477 cron `0 22 * * 1,5`; :131-136 duration 02:00 - 22:00 + 24h = 4h. The weekday is
    // the one the window starts on.
    let weekday = def(json!({
        "strategy": "recurring-weekday", "active": true, "timezone": "UTC",
        "weekdays": "[5,1]", "start_time": "22:00", "end_time": "02:00",
    }));
    assert_eq!(
        list(&weekday, "2026-06-01T00:00:00Z", "2026-06-08T00:00:00Z"),
        [
            win("2026-06-01T22:00:00Z", "2026-06-02T02:00:00Z"),
            win("2026-06-05T22:00:00Z", "2026-06-06T02:00:00Z"),
        ]
    );
    assert_eq!(
        status_at(&weekday, utc("2026-06-02T01:00:00Z")),
        Status::UnderMaintenance
    );
    assert_eq!(
        status_at(&weekday, utc("2026-06-03T01:00:00Z")),
        Status::Scheduled
    );
    assert_eq!(
        next_change(&weekday, utc("2026-06-02T01:00:00Z")),
        Some(utc("2026-06-02T02:00:00Z"))
    );
    assert_eq!(
        next_change(&weekday, utc("2026-06-02T02:00:00Z")),
        Some(utc("2026-06-05T22:00:00Z"))
    );
}

#[test]
fn interval_counts_days_from_the_start_date_across_a_dst_change() {
    // :462-468 daily cron `0 4 * * *`, :292-298 start only every third day. Vienna switches to
    // summer time on 2026-03-29, so the same wall clock time moves from 03:00Z to 02:00Z.
    let interval = def(json!({
        "strategy": "recurring-interval", "active": true, "timezone": "Europe/Vienna",
        "interval_day": 3, "start_date": "2026-03-27 00:00:00",
        "start_time": "04:00", "end_time": "05:30",
    }));
    assert_eq!(
        list(&interval, "2026-03-26T00:00:00Z", "2026-04-04T00:00:00Z"),
        [
            win("2026-03-27T03:00:00Z", "2026-03-27T04:30:00Z"),
            win("2026-03-30T02:00:00Z", "2026-03-30T03:30:00Z"),
            win("2026-04-02T02:00:00Z", "2026-04-02T03:30:00Z"),
        ]
    );
}

#[test]
fn day_of_month_skips_missing_days_and_handles_leap_day() {
    // :486-494 `lastDay1` becomes cron `L`, duplicates are dropped. 29 and 31 do not exist in
    // February 2027, so only `L` (the 28th) fires. In 2028 the 29th is both.
    let days = json!("[29, 31, \"lastDay1\"]");
    let month = def(json!({
        "strategy": "recurring-day-of-month", "active": true, "timezone": "UTC",
        "days_of_month": days, "start_time": "10:00", "end_time": "11:00",
    }));
    assert_eq!(
        list(&month, "2027-02-01T00:00:00Z", "2027-03-01T00:00:00Z"),
        [win("2027-02-28T10:00:00Z", "2027-02-28T11:00:00Z")]
    );
    assert_eq!(
        list(&month, "2028-02-01T00:00:00Z", "2028-03-01T00:00:00Z"),
        [win("2028-02-29T10:00:00Z", "2028-02-29T11:00:00Z")]
    );
    assert_eq!(
        list(&month, "2027-04-01T00:00:00Z", "2027-05-01T00:00:00Z"),
        [
            win("2027-04-29T10:00:00Z", "2027-04-29T11:00:00Z"),
            win("2027-04-30T10:00:00Z", "2027-04-30T11:00:00Z"),
        ]
    );

    // lastDay2 to lastDay4 are dropped, which leaves Kuma with an invalid cron (:488-491).
    let only_dropped = def(json!({
        "strategy": "recurring-day-of-month", "active": true,
        "days_of_month": ["lastDay2"], "start_time": "10:00", "end_time": "11:00",
    }));
    assert!(only_dropped.validate().is_err());
    assert_eq!(
        status_at(&only_dropped, utc("2027-02-28T10:30:00Z")),
        Status::Unknown
    );
}

#[test]
fn cron_with_weekday_range_and_optional_seconds() {
    // :180-183 duration is minutes * 60; the expression is used as written. 2026-06-01 is a
    // Monday, so 1-5 skips the weekend of June 6 and 7.
    let weekdays = def(json!({
        "strategy": "cron", "active": true, "timezone": "UTC",
        "cron": "0 22 * * 1-5", "duration": 5400,
    }));
    let found = list(&weekdays, "2026-06-01T00:00:00Z", "2026-06-08T00:00:00Z");
    assert_eq!(found.len(), 5);
    assert_eq!(
        found[0],
        win("2026-06-01T22:00:00Z", "2026-06-01T23:30:00Z")
    );
    assert_eq!(
        found[4],
        win("2026-06-05T22:00:00Z", "2026-06-05T23:30:00Z")
    );
    assert_eq!(
        status_at(&weekdays, utc("2026-06-01T23:29:59Z")),
        Status::UnderMaintenance
    );
    assert_eq!(
        status_at(&weekdays, utc("2026-06-01T23:30:00Z")),
        Status::Scheduled
    );

    // croner takes a leading seconds field: 09:00:30 on Mondays.
    let with_seconds = def(json!({
        "strategy": "cron", "active": true, "timezone": "UTC",
        "cron": "30 0 9 * * 1", "duration": 60,
    }));
    assert_eq!(
        list(
            &with_seconds,
            "2026-06-01T00:00:00Z",
            "2026-06-09T00:00:00Z"
        ),
        [
            win("2026-06-01T09:00:30Z", "2026-06-01T09:01:30Z"),
            win("2026-06-08T09:00:30Z", "2026-06-08T09:01:30Z"),
        ]
    );

    let broken =
        def(json!({"strategy": "cron", "active": true, "cron": "61 * * * *", "duration": 60}));
    assert_eq!(
        status_at(&broken, utc("2026-06-01T00:00:00Z")),
        Status::Unknown
    );
}

#[test]
fn date_range_cuts_windows_at_both_ends() {
    // :431 and :436 decide scheduled and ended before the window state is looked at. The
    // window that began before start_date still counts after it (the timer is running), and
    // :366-376 cuts the last window at end_date.
    let ranged = def(json!({
        "strategy": "cron", "active": true, "timezone": "UTC", "cron": "0 22 * * *", "duration": 7200,
        "start_date": "2026-06-01 23:00:00", "end_date": "2026-06-03 23:00:00",
    }));
    assert_eq!(
        list(&ranged, "2026-06-01T00:00:00Z", "2026-06-10T00:00:00Z"),
        [
            win("2026-06-01T23:00:00Z", "2026-06-02T00:00:00Z"),
            win("2026-06-02T22:00:00Z", "2026-06-03T00:00:00Z"),
            win("2026-06-03T22:00:00Z", "2026-06-03T23:00:00Z"),
        ]
    );
    assert_eq!(
        status_at(&ranged, utc("2026-06-01T22:30:00Z")),
        Status::Scheduled
    );
    assert_eq!(
        status_at(&ranged, utc("2026-06-01T23:30:00Z")),
        Status::UnderMaintenance
    );
    assert_eq!(
        status_at(&ranged, utc("2026-06-03T23:00:00Z")),
        Status::Ended
    );
    assert_eq!(
        next_change(&ranged, utc("2026-06-01T22:30:00Z")),
        Some(utc("2026-06-01T23:00:00Z"))
    );
    assert_eq!(
        next_change(&ranged, utc("2026-06-03T22:30:00Z")),
        Some(utc("2026-06-03T23:00:00Z"))
    );
    assert_eq!(next_change(&ranged, utc("2026-06-03T23:00:00Z")), None);
}

#[test]
fn spring_forward_fires_after_the_gap_and_keeps_the_absolute_duration() {
    // Vienna 2026-03-29: 02:00 CET jumps to 03:00 CEST. A 02:30 start does not exist and
    // fires at 03:00 CEST (01:00Z). Other days keep 02:30 local (01:30Z in CET, 00:30Z in CEST).
    let daily = def(json!({
        "strategy": "cron", "active": true, "timezone": "Europe/Vienna",
        "cron": "30 2 * * *", "duration": 3600,
    }));
    assert_eq!(
        list(&daily, "2026-03-28T00:00:00Z", "2026-03-31T00:00:00Z"),
        [
            win("2026-03-28T01:30:00Z", "2026-03-28T02:30:00Z"),
            win("2026-03-29T01:00:00Z", "2026-03-29T02:00:00Z"),
            win("2026-03-30T00:30:00Z", "2026-03-30T01:30:00Z"),
        ]
    );

    // 02:30 to 04:30 is a two hour duration (:131-136 subtracts clock times), so the window
    // ends at 05:00 CEST, not 04:30.
    let sunday = def(json!({
        "strategy": "recurring-weekday", "active": true, "timezone": "Europe/Vienna",
        "weekdays": [0], "start_time": "02:30", "end_time": "04:30",
    }));
    assert_eq!(
        list(&sunday, "2026-03-29T00:00:00Z", "2026-03-30T00:00:00Z"),
        [win("2026-03-29T01:00:00Z", "2026-03-29T03:00:00Z")]
    );

    // New York 2026-03-08: 02:00 EST jumps to 03:00 EDT.
    let new_york = def(json!({
        "strategy": "cron", "active": true, "timezone": "America/New_York",
        "cron": "30 2 * * *", "duration": 3600,
    }));
    assert_eq!(
        list(&new_york, "2026-03-07T00:00:00Z", "2026-03-10T00:00:00Z"),
        [
            win("2026-03-07T07:30:00Z", "2026-03-07T08:30:00Z"),
            win("2026-03-08T07:00:00Z", "2026-03-08T08:00:00Z"),
            win("2026-03-09T06:30:00Z", "2026-03-09T07:30:00Z"),
        ]
    );

    // A date inside the gap moves forward by the gap, like dayjs: 02:30 becomes 03:30 EDT.
    let single = def(json!({
        "strategy": "single", "active": true, "timezone": "America/New_York",
        "start_date": "2026-03-08 02:30:00", "end_date": "2026-03-08 05:00:00",
    }));
    assert_eq!(
        list(&single, "2026-03-08T00:00:00Z", "2026-03-09T00:00:00Z"),
        [win("2026-03-08T07:30:00Z", "2026-03-08T09:00:00Z")]
    );
}

#[test]
fn fall_back_fires_once_in_the_repeated_hour() {
    // Vienna 2026-10-25: 03:00 CEST falls back to 02:00 CET, so 02:30 happens twice. It fires
    // once, at the first one (00:30Z). The window is three real hours, ending 03:30Z.
    let daily = def(json!({
        "strategy": "cron", "active": true, "timezone": "Europe/Vienna",
        "cron": "30 2 * * *", "duration": 10800,
    }));
    assert_eq!(
        list(&daily, "2026-10-24T00:00:00Z", "2026-10-27T00:00:00Z"),
        [
            win("2026-10-24T00:30:00Z", "2026-10-24T03:30:00Z"),
            win("2026-10-25T00:30:00Z", "2026-10-25T03:30:00Z"),
            win("2026-10-26T01:30:00Z", "2026-10-26T04:30:00Z"),
        ]
    );
    assert_eq!(
        status_at(&daily, utc("2026-10-25T02:45:00Z")),
        Status::UnderMaintenance
    );
    assert_eq!(
        status_at(&daily, utc("2026-10-25T03:30:00Z")),
        Status::Scheduled
    );

    // New York 2026-11-01: 02:00 EDT falls back to 01:00 EST, so 01:30 happens twice.
    let new_york = def(json!({
        "strategy": "cron", "active": true, "timezone": "America/New_York",
        "cron": "30 1 * * *", "duration": 3600,
    }));
    assert_eq!(
        list(&new_york, "2026-10-31T00:00:00Z", "2026-11-03T00:00:00Z"),
        [
            win("2026-10-31T05:30:00Z", "2026-10-31T06:30:00Z"),
            win("2026-11-01T05:30:00Z", "2026-11-01T06:30:00Z"),
            win("2026-11-02T06:30:00Z", "2026-11-02T07:30:00Z"),
        ]
    );
}

#[test]
fn kuma_rows_deserialize_and_same_as_server_uses_the_server_timezone() {
    // Columns as Kuma stores them: JSON text lists, HH:mm times, SAME_AS_SERVER (:402-407).
    let row = json!({
        "strategy": "recurring-day-of-month", "active": true, "timezone": "SAME_AS_SERVER",
        "start_date": "2026-01-01 00:00:00", "end_date": null, "weekdays": "[]",
        "days_of_month": "[1,\"lastDay1\"]", "start_time": "09:00", "end_time": "10:00",
    });
    let mut stored = def(row.clone());
    stored.server_timezone = chrono_tz::Europe::Vienna;
    assert_eq!(
        list(&stored, "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z"),
        [win("2026-01-01T08:00:00Z", "2026-01-01T09:00:00Z")]
    );

    let mut unknown_zone = row;
    unknown_zone["timezone"] = json!("Mars/Olympus");
    assert!(serde_json::from_value::<Definition>(unknown_zone).is_err());
}
