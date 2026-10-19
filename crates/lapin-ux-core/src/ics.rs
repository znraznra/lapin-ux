// SPDX-License-Identifier: GPL-3.0-only
//! Calendar events from iCalendar (.ics) files for the agenda under the
//! calendar: single events and the common repeat rules (daily, weekly on
//! given days, monthly, yearly; interval, count, until; excluded dates).

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday};

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub summary: String,
    pub location: String,
    /// Local start; all-day events start at midnight.
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    pub rule: Option<Rule>,
    pub exdates: Vec<NaiveDate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<NaiveDate>,
    pub by_day: Vec<Weekday>,
}

/// One showing of an event on a day.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    pub summary: String,
    pub location: String,
    pub start: Option<NaiveTime>,
    pub end: Option<NaiveTime>,
}

/// Unfold continuation lines (RFC 5545 §3.1).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t'))
            && let Some(last) = out.last_mut()
        {
            last.push_str(rest);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

fn unescape(v: &str) -> String {
    v.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

/// "20261019", "20261019T090000", "20261019T090000Z" → local time.
fn parse_dt(params: &str, v: &str) -> Option<(NaiveDateTime, bool)> {
    let v = v.trim();
    if params.contains("VALUE=DATE") || v.len() == 8 {
        let d = NaiveDate::parse_from_str(&v[..8.min(v.len())], "%Y%m%d").ok()?;
        return Some((d.and_hms_opt(0, 0, 0)?, true));
    }
    let (body, utc) = match v.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (v, false),
    };
    let naive = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    if utc {
        let local: DateTime<Local> = Utc.from_utc_datetime(&naive).with_timezone(&Local);
        return Some((local.naive_local(), false));
    }
    // TZID times are taken as local time (the common case for personal calendars).
    Some((naive, false))
}

fn weekday(s: &str) -> Option<Weekday> {
    // Ignore ordinals like "1MO" / "-1FR".
    let code: String = s.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    Some(match code.as_str() {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

fn parse_rule(v: &str) -> Option<Rule> {
    let mut freq = None;
    let mut rule = Rule { freq: Freq::Daily, interval: 1, count: None, until: None, by_day: Vec::new() };
    for part in v.split(';') {
        let (k, val) = part.split_once('=')?;
        match k {
            "FREQ" => {
                freq = Some(match val {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None, // hourly etc.: not shown
                })
            }
            "INTERVAL" => rule.interval = val.parse().unwrap_or(1).max(1),
            "COUNT" => rule.count = val.parse().ok(),
            "UNTIL" => rule.until = parse_dt("", val).map(|(d, _)| d.date()),
            "BYDAY" => rule.by_day = val.split(',').filter_map(weekday).collect(),
            _ => {}
        }
    }
    rule.freq = freq?;
    Some(rule)
}

pub fn parse(text: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut cur: Option<(Option<(NaiveDateTime, bool)>, Option<NaiveDateTime>, Option<Duration>, Event)> = None;
    let blank = || Event {
        summary: String::new(),
        location: String::new(),
        start: NaiveDateTime::default(),
        end: NaiveDateTime::default(),
        all_day: false,
        rule: None,
        exdates: Vec::new(),
    };
    for line in unfold(text) {
        if line == "BEGIN:VEVENT" {
            cur = Some((None, None, None, blank()));
            continue;
        }
        if line == "END:VEVENT" {
            if let Some((Some((start, all_day)), end, dur, mut e)) = cur.take() {
                e.start = start;
                e.all_day = all_day;
                e.end = end.or_else(|| dur.map(|d| start + d)).unwrap_or(if all_day { start + Duration::days(1) } else { start });
                events.push(e);
            }
            continue;
        }
        let Some((start, end, dur, e)) = cur.as_mut() else { continue };
        let Some((key, value)) = line.split_once(':') else { continue };
        let (name, params) = key.split_once(';').unwrap_or((key, ""));
        match name {
            "SUMMARY" => e.summary = unescape(value),
            "LOCATION" => e.location = unescape(value),
            "DTSTART" => *start = parse_dt(params, value),
            "DTEND" => *end = parse_dt(params, value).map(|(d, _)| d),
            "DURATION" => *dur = parse_duration(value),
            "RRULE" => e.rule = parse_rule(value),
            "EXDATE" => e.exdates.extend(value.split(',').filter_map(|v| parse_dt(params, v)).map(|(d, _)| d.date())),
            _ => {}
        }
    }
    events
}

/// "PT1H30M", "P1D", "-PT15M" (negative is ignored).
fn parse_duration(v: &str) -> Option<Duration> {
    let v = v.strip_prefix('+').unwrap_or(v).strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut num = String::new();
    for c in v.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => {}
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match c {
                    'W' => Duration::weeks(n),
                    'D' => Duration::days(n),
                    'H' => Duration::hours(n),
                    'M' => Duration::minutes(n),
                    _ => Duration::seconds(n),
                };
            }
            _ => return None,
        }
    }
    Some(total)
}

/// Does the event happen on `day` (starting that day)?
fn occurs_on(e: &Event, day: NaiveDate) -> bool {
    let first = e.start.date();
    if day < first || e.exdates.contains(&day) {
        return false;
    }
    let Some(r) = &e.rule else {
        // Multi-day events show on each day they cover.
        let last = if e.end > e.start { (e.end - Duration::seconds(1)).date() } else { first };
        return day <= last;
    };
    if r.until.is_some_and(|u| day > u) {
        return false;
    }
    let iv = r.interval as i64;
    let matches = |d: NaiveDate| -> bool {
        match r.freq {
            Freq::Daily => (d - first).num_days() % iv == 0,
            Freq::Weekly => {
                let days = if r.by_day.is_empty() { vec![first.weekday()] } else { r.by_day.clone() };
                let weeks = (week_start(d) - week_start(first)).num_days() / 7;
                days.contains(&d.weekday()) && weeks % iv == 0
            }
            Freq::Monthly => {
                let months = (d.year() - first.year()) as i64 * 12 + d.month() as i64 - first.month() as i64;
                d.day() == first.day() && months % iv == 0
            }
            Freq::Yearly => d.month() == first.month() && d.day() == first.day() && (d.year() - first.year()) as i64 % iv == 0,
        }
    };
    if !matches(day) {
        return false;
    }
    match r.count {
        // Count occurrences up to `day` (bounded walk; counts are small in practice).
        Some(c) => {
            let mut n = 0;
            let mut d = first;
            while d <= day {
                if matches(d) && !e.exdates.contains(&d) {
                    n += 1;
                }
                if n > c {
                    return false;
                }
                d += Duration::days(1);
            }
            n <= c
        }
        None => true,
    }
}

fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// Events on `day`, all-day ones first, then by start time.
pub fn agenda(events: &[Event], day: NaiveDate) -> Vec<Occurrence> {
    let mut out: Vec<Occurrence> = events
        .iter()
        .filter(|e| occurs_on(e, day))
        .map(|e| {
            let timed = !e.all_day;
            Occurrence {
                summary: if e.summary.is_empty() { "(No title)".into() } else { e.summary.clone() },
                location: e.location.clone(),
                start: timed.then(|| e.start.time()),
                end: (timed && e.end > e.start).then(|| e.end.time()),
            }
        })
        .collect();
    out.sort_by(|a, b| a.start.cmp(&b.start).then(a.summary.cmp(&b.summary)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Stand-up\r\nDTSTART:20261005T093000\r\nDTEND:20261005T094500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\r\nEXDATE:20261009T093000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Dentist\\, Dr. Cruz\r\nLOCATION:Makati\r\nDTSTART:20261019T140000\r\nDURATION:PT1H\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Holiday trip\r\nDTSTART;VALUE=DATE:20261018\r\nDTEND;VALUE=DATE:20261021\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Rent\r\nDTSTART;VALUE=DATE:20260119\r\nRRULE:FREQ=MONTHLY;COUNT=12\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Long\r\n  title\r\nDTSTART:20261019T080000\r\nRRULE:FREQ=DAILY;INTERVAL=2;UNTIL=20261031T000000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn agenda_for_days() {
        let ev = parse(ICS);
        assert_eq!(ev.len(), 5);
        let names = |day| agenda(&ev, day).into_iter().map(|o| o.summary).collect::<Vec<_>>();
        // Monday 19 Oct: all-day trip, rent (19th monthly), long title, stand-up, dentist.
        assert_eq!(names(d(2026, 10, 19)), vec!["Holiday trip", "Rent", "Long title", "Stand-up", "Dentist, Dr. Cruz"]);
        // Friday 9th is excluded; Wednesday 7th isn't.
        assert!(!names(d(2026, 10, 9)).contains(&"Stand-up".to_string()));
        assert!(names(d(2026, 10, 7)).contains(&"Stand-up".to_string()));
        // Tuesday: no stand-up; trip ends before the 21st.
        assert_eq!(names(d(2026, 10, 20)), vec!["Holiday trip"]);
        assert_eq!(names(d(2026, 10, 21)), vec!["Long title", "Stand-up"]);
        // Rent: 12 months from January, so not in 2027.
        assert!(names(d(2026, 12, 19)).contains(&"Rent".to_string()));
        assert!(!names(d(2027, 1, 19)).contains(&"Rent".to_string()));
        let dentist = agenda(&ev, d(2026, 10, 19)).into_iter().find(|o| o.location == "Makati").unwrap();
        assert_eq!((dentist.start, dentist.end), (NaiveTime::from_hms_opt(14, 0, 0), NaiveTime::from_hms_opt(15, 0, 0)));
    }
}
