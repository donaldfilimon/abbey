//! Pure scheduling calculations. No clock, persistence, or execution authority.
//!
//! Numeric five-field cron uses Vixie day matching: when either day field starts
//! with `*` (including stepped wildcards), both allowed sets must match; otherwise
//! day-of-month and day-of-week are ORed. Weekday 0 and 7 mean Sunday.
//! Local folds select the earlier instant once; gaps advance to the first valid
//! minute, at most 48 hours later. Labels are resolved local RFC3339 timestamps.
//! Searches inspect calendar days and allowed hours/minutes within eight years
//! (8 × 366 days plus skipped-date padding); exhaustion returns `None`.
//! Zone names are exact, case-sensitive names from chrono-tz's IANA database,
//! including its registered IANA links. No fixed-offset expressions are parsed.
use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
mod cron;
use cron::Calendar;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleKind {
    Once { at_ms: u64 },
    Hourly { minute: u8 },
    Daily { hour: u8, minute: u8 },
    Weekly { weekday: u8, hour: u8, minute: u8 },
    Cron { expression: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleSpec {
    pub timezone: String,
    pub kind: ScheduleKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub scheduled_at_ms: u64,
    pub local_label: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissedWindow {
    pub after_ms: u64,
    pub through_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatchUp {
    pub latest_due: Option<Occurrence>,
    pub missed_window: Option<MissedWindow>,
    pub next: Option<Occurrence>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    InvalidSchedule,
    InvalidTimestamp,
    InvalidCount,
    InvalidInterval,
    TimezoneGap,
}
impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidSchedule => "invalid schedule",
            Self::InvalidTimestamp => "invalid timestamp",
            Self::InvalidCount => "invalid preview count",
            Self::InvalidInterval => "invalid interval",
            Self::TimezoneGap => "unsupported timezone gap",
        })
    }
}
impl std::error::Error for ScheduleError {}
#[derive(Clone, Debug)]
pub struct CompiledSchedule {
    zone: Tz,
    once: Option<u64>,
    calendar: Calendar,
}
fn local_datetime(zone: Tz, ms: u64) -> Result<NaiveDateTime, ScheduleError> {
    let utc = timestamp(ms)?;
    utc.naive_utc()
        .checked_add_signed(Duration::seconds(i64::from(
            zone.offset_from_utc_datetime(&utc.naive_utc())
                .fix()
                .local_minus_utc(),
        )))
        .ok_or(ScheduleError::InvalidTimestamp)
}
fn timestamp(ms: u64) -> Result<DateTime<Utc>, ScheduleError> {
    if ms > 9_007_199_254_740_991 {
        return Err(ScheduleError::InvalidTimestamp);
    }
    DateTime::from_timestamp_millis(i64::try_from(ms).map_err(|_| ScheduleError::InvalidTimestamp)?)
        .ok_or(ScheduleError::InvalidTimestamp)
}
impl CompiledSchedule {
    pub fn new(spec: &ScheduleSpec) -> Result<Self, ScheduleError> {
        let bad = ScheduleError::InvalidSchedule;
        if spec.timezone.is_empty() || spec.timezone.len() > 64 {
            return Err(bad);
        }
        let zone: Tz = spec.timezone.parse().map_err(|_| bad)?;
        if zone.name() != spec.timezone {
            return Err(bad);
        }
        let expression = match &spec.kind {
            ScheduleKind::Once { at_ms } => {
                local_datetime(zone, *at_ms)?;
                "* * * * *".into()
            }
            ScheduleKind::Hourly { minute } if *minute < 60 => format!("{minute} * * * *"),
            ScheduleKind::Daily { hour, minute } if *hour < 24 && *minute < 60 => {
                format!("{minute} {hour} * * *")
            }
            ScheduleKind::Weekly {
                weekday,
                hour,
                minute,
            } if *weekday < 7 && *hour < 24 && *minute < 60 => {
                format!("{minute} {hour} * * {weekday}")
            }
            ScheduleKind::Cron { expression } => expression.clone(),
            _ => return Err(bad),
        };
        Ok(Self {
            zone,
            once: if let ScheduleKind::Once { at_ms } = spec.kind {
                Some(at_ms)
            } else {
                None
            },
            calendar: Calendar::parse(&expression)?,
        })
    }
    fn occurrence(&self, ms: u64) -> Result<Occurrence, ScheduleError> {
        local_datetime(self.zone, ms)?;
        Ok(Occurrence {
            scheduled_at_ms: ms,
            local_label: timestamp(ms)?.with_timezone(&self.zone).to_rfc3339(),
        })
    }
    pub fn next_after(&self, after_ms: u64) -> Result<Option<Occurrence>, ScheduleError> {
        timestamp(after_ms)?;
        self.find(after_ms, false)
    }
    pub fn preview(&self, after_ms: u64, count: u8) -> Result<Vec<Occurrence>, ScheduleError> {
        if !(1..=50).contains(&count) {
            return Err(ScheduleError::InvalidCount);
        }
        timestamp(after_ms)?;
        let mut result = Vec::new();
        let mut after = after_ms;
        for _ in 0..count {
            let Some(item) = self.next_after(after)? else {
                break;
            };
            after = item.scheduled_at_ms;
            result.push(item)
        }
        Ok(result)
    }
    pub fn catch_up(&self, from_exclusive_ms: u64, now_ms: u64) -> Result<CatchUp, ScheduleError> {
        timestamp(from_exclusive_ms)?;
        timestamp(now_ms)?;
        if from_exclusive_ms > now_ms {
            return Err(ScheduleError::InvalidInterval);
        }
        let latest_due = self
            .find(now_ms, true)?
            .filter(|o| o.scheduled_at_ms > from_exclusive_ms);
        let previous = match &latest_due {
            Some(o) if o.scheduled_at_ms > 0 => self
                .find(o.scheduled_at_ms - 1, true)?
                .filter(|p| p.scheduled_at_ms > from_exclusive_ms),
            _ => None,
        };
        Ok(CatchUp {
            latest_due,
            missed_window: previous.map(|o| MissedWindow {
                after_ms: from_exclusive_ms,
                through_ms: o.scheduled_at_ms,
            }),
            next: self.next_after(now_ms)?,
        })
    }
    /// Calendar-day search bounded to eight years (including leap-century gaps).
    /// Two extra days account for timezone gap advancement across skipped dates.
    fn find(&self, bound: u64, reverse: bool) -> Result<Option<Occurrence>, ScheduleError> {
        if let Some(ms) = self.once {
            return if (reverse && ms <= bound) || (!reverse && ms > bound) {
                self.occurrence(ms).map(Some)
            } else {
                Ok(None)
            };
        }
        let local = local_datetime(self.zone, bound)?.date();
        let mut day = if reverse {
            local
        } else {
            local.checked_sub_signed(Duration::days(2)).unwrap_or(local)
        };
        let mut best: Option<u64> = None;
        let mut remaining = None;
        for _ in 0..(8 * 366 + 4) {
            if self.calendar.matches(day) {
                for &hour in &self.calendar.hours {
                    for &minute in &self.calendar.minutes {
                        let Some(wall) = day.and_hms_opt(hour, minute, 0) else {
                            continue;
                        };
                        if let Some(ms) = self.resolve(wall)?
                            && ((reverse && ms <= bound) || (!reverse && ms > bound))
                        {
                            best = Some(match best {
                                Some(old) if reverse => old.max(ms),
                                Some(old) => old.min(ms),
                                None => ms,
                            });
                        }
                    }
                }
            }
            if best.is_some() {
                let n = remaining.get_or_insert(2);
                if *n == 0 {
                    break;
                }
                *n -= 1;
            }
            let Some(next) = day.checked_add_signed(Duration::days(if reverse { -1 } else { 1 }))
            else {
                break;
            };
            day = next;
        }
        best.map(|ms| self.occurrence(ms)).transpose()
    }
    fn resolve(&self, mut wall: NaiveDateTime) -> Result<Option<u64>, ScheduleError> {
        for _ in 0..=48 * 60 {
            let instant = match self.zone.from_local_datetime(&wall) {
                LocalResult::Single(t) => Some(t),
                LocalResult::Ambiguous(a, b) => Some(a.min(b)),
                LocalResult::None => None,
            };
            if let Some(t) = instant {
                let ms = t.timestamp_millis();
                return Ok(u64::try_from(ms)
                    .ok()
                    .filter(|ms| *ms <= 9_007_199_254_740_991));
            }
            let Some(next) = wall.checked_add_signed(Duration::minutes(1)) else {
                return Ok(None);
            };
            wall = next;
        }
        Err(ScheduleError::TimezoneGap)
    }
}
