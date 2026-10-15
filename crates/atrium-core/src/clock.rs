// SPDX-License-Identifier: GPL-3.0-only
//! Taskbar clock: time on the first line, short date on the second, in the
//! conventions of the user's locale (12/24-hour, date order and separator).

use chrono::{Datelike, NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateOrder {
    Mdy,
    Dmy,
    Ymd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockStyle {
    pub hour12: bool,
    pub order: DateOrder,
    pub separator: char,
    pub seconds: bool,
}

impl Default for ClockStyle {
    fn default() -> Self {
        Self { hour12: false, order: DateOrder::Dmy, separator: '/', seconds: false }
    }
}

impl ClockStyle {
    /// Pick defaults from a POSIX locale like `en_US.UTF-8` or `de_DE`.
    pub fn from_locale(locale: &str) -> Self {
        let tag = locale.split(['.', '@']).next().unwrap_or("");
        let (lang, region) = tag.split_once(['_', '-']).unwrap_or((tag, ""));
        let s = |hour12, order, separator| ClockStyle { hour12, order, separator, seconds: false };
        match (lang, region) {
            ("en", "US") | ("en", "PH") => s(true, DateOrder::Mdy, '/'),
            ("en", "CA") => s(true, DateOrder::Ymd, '-'),
            ("en", "AU") | ("en", "IN") | ("en", "NZ") => s(true, DateOrder::Dmy, '/'),
            ("en", "GB") | ("en", "IE") => s(false, DateOrder::Dmy, '/'),
            ("fil", _) | ("tl", _) => s(true, DateOrder::Mdy, '/'),
            ("zh", "TW") => s(true, DateOrder::Ymd, '/'),
            ("ja", _) | ("zh", _) => s(false, DateOrder::Ymd, '/'),
            ("ko", _) => s(true, DateOrder::Ymd, '.'),
            ("de", _) | ("ru", _) | ("pl", _) | ("fi", _) | ("cs", _) | ("tr", _) => s(false, DateOrder::Dmy, '.'),
            ("sv", _) | ("lt", _) => s(false, DateOrder::Ymd, '-'),
            ("nl", _) => s(false, DateOrder::Dmy, '-'),
            ("C", _) | ("POSIX", _) | ("", _) => ClockStyle::default(),
            _ => ClockStyle::default(),
        }
    }

    /// Read LC_ALL / LC_TIME / LANG the way glibc prioritises them.
    pub fn from_env() -> Self {
        let pick = ["LC_ALL", "LC_TIME", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        Self::from_locale(&pick)
    }

    pub fn time(&self, t: &NaiveDateTime) -> String {
        let (h, m, sec) = (t.hour(), t.minute(), t.second());
        let tail = if self.seconds { format!(":{sec:02}") } else { String::new() };
        if self.hour12 {
            let (h12, ampm) = match h {
                0 => (12, "AM"),
                1..=11 => (h, "AM"),
                12 => (12, "PM"),
                _ => (h - 12, "PM"),
            };
            format!("{h12}:{m:02}{tail} {ampm}")
        } else {
            format!("{h:02}:{m:02}{tail}")
        }
    }

    pub fn date(&self, t: &NaiveDateTime) -> String {
        let (y, mo, d, sep) = (t.year(), t.month(), t.day(), self.separator);
        match self.order {
            DateOrder::Mdy => format!("{mo}{sep}{d}{sep}{y}"),
            DateOrder::Dmy => format!("{d:02}{sep}{mo:02}{sep}{y}"),
            DateOrder::Ymd => format!("{y}{sep}{mo:02}{sep}{d:02}"),
        }
    }

    /// Long form for the tooltip, e.g. "Thursday, October 15, 2026".
    pub fn long_date(&self, t: &NaiveDateTime) -> String {
        match self.order {
            DateOrder::Mdy => t.format("%A, %B %-d, %Y").to_string(),
            _ => t.format("%A, %-d %B %Y").to_string(),
        }
    }
}

/// Whether calendars should start on Sunday for this locale (otherwise Monday).
pub fn week_starts_sunday(locale: &str) -> bool {
    let tag = locale.split(['.', '@']).next().unwrap_or("");
    let region = tag.split_once(['_', '-']).map(|(_, r)| r).unwrap_or("");
    matches!(
        region,
        "US" | "CA" | "PH" | "JP" | "TW" | "HK" | "KR" | "IL" | "BR" | "MX" | "IN" | "ZA" | "SA" | "AU"
    )
}

/// Same, reading the environment like [`ClockStyle::from_env`].
pub fn week_starts_sunday_env() -> bool {
    let pick = ["LC_ALL", "LC_TIME", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    week_starts_sunday(&pick)
}

#[cfg(test)]
mod tests {
    #[test]
    fn first_weekday() {
        assert!(week_starts_sunday("en_US.UTF-8"));
        assert!(week_starts_sunday("en_PH.UTF-8"));
        assert!(!week_starts_sunday("de_DE.UTF-8"));
        assert!(!week_starts_sunday("en_GB"));
        assert!(!week_starts_sunday("C"));
    }

    use super::*;
    use chrono::NaiveDate;

    fn at(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 5).unwrap().and_hms_opt(h, m, 7).unwrap()
    }

    #[test]
    fn us_and_ph() {
        for loc in ["en_US.UTF-8", "en_PH.UTF-8", "fil_PH"] {
            let s = ClockStyle::from_locale(loc);
            assert_eq!(s.time(&at(10, 42)), "10:42 AM");
            assert_eq!(s.time(&at(0, 5)), "12:05 AM");
            assert_eq!(s.time(&at(12, 0)), "12:00 PM");
            assert_eq!(s.time(&at(23, 59)), "11:59 PM");
            assert_eq!(s.date(&at(0, 0)), "10/5/2026");
        }
    }

    #[test]
    fn twenty_four_hour_locales() {
        let de = ClockStyle::from_locale("de_DE.UTF-8");
        assert_eq!(de.time(&at(9, 3)), "09:03");
        assert_eq!(de.date(&at(0, 0)), "05.10.2026");
        let ja = ClockStyle::from_locale("ja_JP.UTF-8");
        assert_eq!(ja.date(&at(0, 0)), "2026/10/05");
        let tw = ClockStyle::from_locale("zh_TW.UTF-8");
        assert!(tw.hour12);
        assert_eq!(ClockStyle::from_locale("sv_SE").date(&at(0, 0)), "2026-10-05");
    }

    #[test]
    fn fallback_and_seconds() {
        let mut c = ClockStyle::from_locale("C");
        assert_eq!(c, ClockStyle::default());
        c.seconds = true;
        assert_eq!(c.time(&at(8, 1)), "08:01:07");
    }

    #[test]
    fn long_date() {
        let us = ClockStyle::from_locale("en_US");
        assert_eq!(us.long_date(&at(0, 0)), "Monday, October 5, 2026");
        let gb = ClockStyle::from_locale("en_GB");
        assert_eq!(gb.long_date(&at(0, 0)), "Monday, 5 October 2026");
    }
}
