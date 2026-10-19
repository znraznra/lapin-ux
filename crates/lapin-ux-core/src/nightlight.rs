// SPDX-License-Identifier: GPL-3.0-only
//! Night light: colour temperature from the strength setting, and the
//! schedule (turn on at one time of day, off at another).

/// Kelvin used when no strength is set (Windows' default strength, 50%).
pub const DEFAULT_KELVIN: u32 = 4000;
pub const WARMEST: u32 = 1200;
pub const COOLEST: u32 = 6500;

/// The configured temperature, 0 meaning the default.
pub fn kelvin(setting: u32) -> u32 {
    if setting == 0 { DEFAULT_KELVIN } else { setting.clamp(WARMEST, COOLEST) }
}

/// Strength 0..=100 (Windows' slider) to kelvin and back.
pub fn strength_to_kelvin(strength: u8) -> u32 {
    let s = strength.min(100) as u32;
    COOLEST - (COOLEST - WARMEST) * s / 100
}

pub fn kelvin_to_strength(kelvin: u32) -> u8 {
    let k = kelvin.clamp(WARMEST, COOLEST);
    (((COOLEST - k) * 100 + (COOLEST - WARMEST) / 2) / (COOLEST - WARMEST)) as u8
}

/// Whether the schedule says "on" at `now` (minutes since midnight). The
/// window may wrap past midnight (21:00 to 07:00).
pub fn scheduled_on(now: u16, from: u16, to: u16) -> bool {
    if from == to {
        return false;
    }
    if from < to { now >= from && now < to } else { now >= from || now < to }
}

/// Did the schedule switch between minute `prev` and `now`? Gives the new
/// state; manual changes in between are kept until the next switch.
pub fn crossed(prev: u16, now: u16, from: u16, to: u16) -> Option<bool> {
    let (a, b) = (scheduled_on(prev, from, to), scheduled_on(now, from, to));
    (a != b).then_some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_wraps_midnight() {
        let (from, to) = (21 * 60, 7 * 60);
        assert!(scheduled_on(22 * 60, from, to));
        assert!(scheduled_on(3 * 60, from, to));
        assert!(!scheduled_on(12 * 60, from, to));
        assert_eq!(crossed(21 * 60 - 1, 21 * 60, from, to), Some(true));
        assert_eq!(crossed(7 * 60 - 1, 7 * 60, from, to), Some(false));
        assert_eq!(crossed(100, 101, from, to), None);
        assert!(scheduled_on(10 * 60, 9 * 60, 17 * 60));
    }

    #[test]
    fn strength() {
        assert_eq!(strength_to_kelvin(0), 6500);
        assert_eq!(strength_to_kelvin(100), 1200);
        assert_eq!(kelvin_to_strength(strength_to_kelvin(48)), 48);
        assert_eq!(kelvin(0), DEFAULT_KELVIN);
    }
}
