// SPDX-License-Identifier: GPL-3.0-only
//! Snap layouts: ways to split the screen, and which space to fill next.

/// Part of the work area: x, y, width, height as fractions.
pub type Zone = [f32; 4];

const T: f32 = 1.0 / 3.0;

/// The benchmark's layouts: halves, two-thirds + third, thirds, half +
/// two quarters, quarters, and quarter-half-quarter for wide screens.
pub const LAYOUTS: &[&[Zone]] = &[
    &[[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 0.5, 1.0]],
    &[[0.0, 0.0, 2.0 * T, 1.0], [2.0 * T, 0.0, T, 1.0]],
    &[[0.0, 0.0, T, 1.0], [T, 0.0, T, 1.0], [2.0 * T, 0.0, T, 1.0]],
    &[[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 0.5, 0.5], [0.5, 0.5, 0.5, 0.5]],
    &[[0.0, 0.0, 0.5, 0.5], [0.5, 0.0, 0.5, 0.5], [0.0, 0.5, 0.5, 0.5], [0.5, 0.5, 0.5, 0.5]],
    &[[0.0, 0.0, 0.25, 1.0], [0.25, 0.0, 0.5, 1.0], [0.75, 0.0, 0.25, 1.0]],
];

/// Layouts that suit a screen of this aspect ratio (the last one only on wide screens).
pub fn layouts_for(width: u32, height: u32) -> usize {
    if height > 0 && width as f32 / height as f32 >= 2.0 { LAYOUTS.len() } else { LAYOUTS.len() - 1 }
}

/// The next space Snap assist offers to fill, in layout order.
pub fn next_zone(layout: usize, filled: &[usize]) -> Option<usize> {
    let zones = LAYOUTS.get(layout)?;
    (0..zones.len()).find(|z| !filled.contains(z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_cover_the_screen() {
        for l in LAYOUTS {
            let area: f32 = l.iter().map(|z| z[2] * z[3]).sum();
            assert!((area - 1.0).abs() < 1e-4);
            assert!(l.iter().all(|z| z[0] + z[2] <= 1.0001 && z[1] + z[3] <= 1.0001));
        }
        assert_eq!(next_zone(2, &[0]), Some(1));
        assert_eq!(next_zone(2, &[1, 0]), Some(2));
        assert_eq!(next_zone(0, &[0, 1]), None);
        assert_eq!(layouts_for(1920, 1080), 5);
        assert_eq!(layouts_for(3440, 1440), 6);
    }
}
