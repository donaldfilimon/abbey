use super::*;
use chrono::Datelike;
#[derive(Clone, Debug)]
pub(super) struct Calendar {
    pub minutes: Vec<u32>,
    pub hours: Vec<u32>,
    dom: Vec<u32>,
    months: Vec<u32>,
    dow: Vec<u32>,
    dom_any: bool,
    dow_any: bool,
}
impl Calendar {
    pub fn parse(expression: &str) -> Result<Self, ScheduleError> {
        let bad = ScheduleError::InvalidSchedule;
        if expression.len() > 128 {
            return Err(bad);
        }
        let f: Vec<_> = expression.split_ascii_whitespace().collect();
        if f.len() != 5 {
            return Err(bad);
        }
        let c = Self {
            minutes: field(f[0], 0, 59)?,
            hours: field(f[1], 0, 23)?,
            dom: field(f[2], 1, 31)?,
            months: field(f[3], 1, 12)?,
            dow: field(f[4], 0, 7)?.into_iter().map(|v| v % 7).collect(),
            dom_any: f[2].starts_with('*'),
            dow_any: f[4].starts_with('*'),
        };
        // A full Gregorian cycle proves that at least one calendar date can match.
        let mut day = NaiveDate::from_ymd_opt(2000, 1, 1).ok_or(bad)?;
        for _ in 0..146097 {
            if c.matches(day) {
                return Ok(c);
            }
            day = day.succ_opt().ok_or(bad)?;
        }
        Err(bad)
    }
    pub fn matches(&self, day: NaiveDate) -> bool {
        if !self.months.contains(&day.month()) {
            return false;
        }
        let dom = self.dom.contains(&day.day());
        let dow = self.dow.contains(&day.weekday().num_days_from_sunday());
        if self.dom_any || self.dow_any {
            dom && dow
        } else {
            dom || dow
        }
    }
}
fn number(s: &str) -> Result<u32, ScheduleError> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ScheduleError::InvalidSchedule);
    }
    s.parse().map_err(|_| ScheduleError::InvalidSchedule)
}
fn field(s: &str, min: u32, max: u32) -> Result<Vec<u32>, ScheduleError> {
    let bad = ScheduleError::InvalidSchedule;
    let mut values = Vec::new();
    for part in s.split(',') {
        let mut split = part.split('/');
        let base = split.next().ok_or(bad)?;
        let step = split.next().map(number).transpose()?;
        if split.next().is_some() || step == Some(0) {
            return Err(bad);
        }
        let (lo, hi) = if base == "*" {
            (min, max)
        } else if let Some((a, b)) = base.split_once('-') {
            (number(a)?, number(b)?)
        } else {
            if step.is_some() {
                return Err(bad);
            }
            let n = number(base)?;
            (n, n)
        };
        if lo < min || hi > max || lo > hi {
            return Err(bad);
        }
        for n in lo..=hi {
            if (n - lo) % step.unwrap_or(1) == 0 {
                values.push(n)
            }
        }
    }
    values.sort_unstable();
    values.dedup();
    if values.is_empty() {
        return Err(bad);
    }
    Ok(values)
}
