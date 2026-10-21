use super::*;
fn ms(s: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .unwrap()
        .timestamp_millis() as u64
}
fn schedule(zone: &str, kind: ScheduleKind) -> CompiledSchedule {
    CompiledSchedule::new(&ScheduleSpec {
        timezone: zone.into(),
        kind,
    })
    .unwrap()
}
fn cron(zone: &str, expression: &str) -> CompiledSchedule {
    schedule(
        zone,
        ScheduleKind::Cron {
            expression: expression.into(),
        },
    )
}
fn next(s: &CompiledSchedule, after: &str, expected: &str) {
    assert_eq!(
        s.next_after(ms(after)).unwrap().unwrap().scheduled_at_ms,
        ms(expected)
    );
}
#[test]
fn once_is_exact_and_exclusive() {
    let at = ms("2026-01-01T00:00:00Z");
    let s = schedule("UTC", ScheduleKind::Once { at_ms: at });
    assert_eq!(s.next_after(at - 1).unwrap().unwrap().scheduled_at_ms, at);
    assert_eq!(s.next_after(at).unwrap(), None);
}
#[test]
fn wall_clock_kinds() {
    next(
        &schedule("UTC", ScheduleKind::Hourly { minute: 15 }),
        "2026-01-01T00:15:00Z",
        "2026-01-01T01:15:00Z",
    );
    next(
        &schedule(
            "UTC",
            ScheduleKind::Daily {
                hour: 2,
                minute: 30,
            },
        ),
        "2026-01-01T03:00:00Z",
        "2026-01-02T02:30:00Z",
    );
    next(
        &schedule(
            "UTC",
            ScheduleKind::Weekly {
                weekday: 0,
                hour: 1,
                minute: 0,
            },
        ),
        "2026-01-01T00:00:00Z",
        "2026-01-04T01:00:00Z",
    );
}
#[test]
fn cron_grammar_and_calendar() {
    let s = cron("UTC", "*/15 1-3/2 * 1,2 *");
    let p = s.preview(ms("2026-01-01T01:00:00Z"), 5).unwrap();
    assert_eq!(p[0].scheduled_at_ms, ms("2026-01-01T01:15:00Z"));
    assert_eq!(p[3].scheduled_at_ms, ms("2026-01-01T03:00:00Z"));
    next(
        &cron("UTC", "0 0 31 2 7"),
        "2026-02-01T00:00:00Z",
        "2026-02-08T00:00:00Z",
    );
    next(
        &cron("UTC", "0 0 29 2 *"),
        "2096-03-01T00:00:00Z",
        "2104-02-29T00:00:00Z",
    );
}
#[test]
fn invalid_inputs_are_safe() {
    for e in [
        "0 0 31 2 *",
        "* * * *",
        "0 0 * * MON",
        "*/0 * * * *",
        "1/2 * * * *",
        "9-1 * * * *",
        "1,,2 * * * *",
        "0 0 0 * *",
        "0 0 * * 8",
    ] {
        let err = CompiledSchedule::new(&ScheduleSpec {
            timezone: "UTC".into(),
            kind: ScheduleKind::Cron {
                expression: e.into(),
            },
        })
        .err()
        .unwrap();
        assert!(!err.to_string().contains(e));
    }
    assert!(
        CompiledSchedule::new(&ScheduleSpec {
            timezone: "bad".into(),
            kind: ScheduleKind::Hourly { minute: 0 }
        })
        .is_err()
    );
}
#[test]
fn dst_gap_fold_and_skipped_day() {
    next(
        &schedule(
            "America/New_York",
            ScheduleKind::Daily {
                hour: 2,
                minute: 30,
            },
        ),
        "2026-03-08T00:00:00Z",
        "2026-03-08T07:00:00Z",
    );
    let s = schedule(
        "America/New_York",
        ScheduleKind::Daily {
            hour: 1,
            minute: 30,
        },
    );
    next(&s, "2026-11-01T00:00:00Z", "2026-11-01T05:30:00Z");
    next(&s, "2026-11-01T05:30:00Z", "2026-11-02T06:30:00Z");
    next(
        &schedule(
            "Australia/Lord_Howe",
            ScheduleKind::Daily {
                hour: 2,
                minute: 15,
            },
        ),
        "2026-10-03T00:00:00Z",
        "2026-10-03T15:30:00Z",
    );
    next(
        &schedule(
            "Pacific/Apia",
            ScheduleKind::Daily {
                hour: 12,
                minute: 0,
            },
        ),
        "2011-12-30T00:00:00Z",
        "2011-12-30T10:00:00Z",
    );
}
#[test]
fn gap_collisions_are_deduplicated() {
    let s = cron("America/New_York", "0,30 2,3 * * *");
    let p = s.preview(ms("2026-03-08T06:59:00Z"), 2).unwrap();
    assert_eq!(p[0].scheduled_at_ms, ms("2026-03-08T07:00:00Z"));
    assert_eq!(p[1].scheduled_at_ms, ms("2026-03-08T07:30:00Z"));
}
#[test]
fn catchup_is_latest_only() {
    let s = cron("UTC", "*/15 * * * *");
    let from = ms("2000-01-01T00:00:00Z");
    let now = ms("2026-01-01T00:07:00Z");
    let c = s.catch_up(from, now).unwrap();
    assert_eq!(
        c.latest_due.unwrap().scheduled_at_ms,
        ms("2026-01-01T00:00:00Z")
    );
    assert_eq!(
        c.missed_window,
        Some(MissedWindow {
            after_ms: from,
            through_ms: ms("2025-12-31T23:45:00Z")
        })
    );
    assert_eq!(c.next.unwrap().scheduled_at_ms, ms("2026-01-01T00:15:00Z"));
    assert!(s.catch_up(now, from).is_err());
    assert_eq!(s.catch_up(now, now).unwrap().latest_due, None);
}
#[test]
fn bounds_and_edges() {
    let s = cron("UTC", "* * * * *");
    assert!(s.preview(0, 0).is_err());
    assert!(s.preview(0, 51).is_err());
    assert!(s.next_after(u64::MAX).is_err());
    assert!(s.next_after(9_007_199_254_740_991).is_err());
    assert_eq!(s.next_after(0).unwrap().unwrap().scheduled_at_ms, 60_000);
}
#[test]
fn local_datetime_edges_do_not_panic() {
    let maximum = chrono::DateTime::<chrono::Utc>::MAX_UTC.timestamp_millis() as u64;
    let s = schedule("Pacific/Kiritimati", ScheduleKind::Hourly { minute: 0 });
    assert!(s.next_after(maximum).is_err());
    assert!(
        CompiledSchedule::new(&ScheduleSpec {
            timezone: "Pacific/Kiritimati".into(),
            kind: ScheduleKind::Once { at_ms: maximum }
        })
        .is_err()
    );
}
#[test]
fn stepped_wildcard_keeps_vixie_day_semantics() {
    next(
        &cron("UTC", "0 9 */1 * 1"),
        "2026-01-01T00:00:00Z",
        "2026-01-05T09:00:00Z",
    );
    next(
        &cron("UTC", "0 9 1-31 * 1"),
        "2026-01-01T00:00:00Z",
        "2026-01-01T09:00:00Z",
    );
    next(
        &schedule("America/New_York", ScheduleKind::Hourly { minute: 0 }),
        "1970-01-01T00:00:00Z",
        "1970-01-01T01:00:00Z",
    );
}
#[test]
fn stepped_day_wildcards_still_filter_their_values() {
    next(
        &cron("UTC", "0 9 */2 * 1"),
        "2026-01-06T00:00:00Z",
        "2026-01-19T09:00:00Z",
    );
}
#[test]
fn catchup_exact_boundaries() {
    let s = cron("UTC", "0 * * * *");
    let at = ms("2026-01-01T00:00:00Z");
    let c = s.catch_up(at, at).unwrap();
    assert_eq!(c.latest_due, None);
    assert_eq!(c.missed_window, None);
    let c = s.catch_up(at, at + 3_600_000).unwrap();
    assert_eq!(c.latest_due.unwrap().scheduled_at_ms, at + 3_600_000);
    assert_eq!(c.missed_window, None);
}

#[test]
fn validation_rejects_out_of_bounds_and_accepts_preview_limit() {
    for kind in [
        ScheduleKind::Hourly { minute: 60 },
        ScheduleKind::Daily {
            hour: 24,
            minute: 0,
        },
        ScheduleKind::Weekly {
            weekday: 7,
            hour: 0,
            minute: 0,
        },
        ScheduleKind::Once { at_ms: u64::MAX },
    ] {
        assert!(
            CompiledSchedule::new(&ScheduleSpec {
                timezone: "UTC".into(),
                kind
            })
            .is_err()
        );
    }
    for expression in [
        "0 0 * * * *".to_owned(),
        "*".repeat(129),
        "0 0 * 13 *".to_owned(),
        "0 0 * * 1-0".to_owned(),
    ] {
        assert!(
            CompiledSchedule::new(&ScheduleSpec {
                timezone: "UTC".into(),
                kind: ScheduleKind::Cron { expression }
            })
            .is_err()
        );
    }
    let s = cron("UTC", "* * * * *");
    assert_eq!(s.preview(0, 50).unwrap().len(), 50);
    let maximum = chrono::DateTime::<chrono::Utc>::MAX_UTC.timestamp_millis() as u64;
    assert_eq!(s.next_after(maximum).unwrap(), None);
    let once = schedule("America/New_York", ScheduleKind::Once { at_ms: 0 });
    let c = once.catch_up(0, 1).unwrap();
    assert_eq!(c.latest_due, None);
    assert_eq!(c.next, None);
}
