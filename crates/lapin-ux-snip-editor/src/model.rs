// SPDX-License-Identifier: GPL-3.0-only
//! The editor's pure parts: strokes and how they are painted, the crop
//! rectangle and its handles, how the picture fits the window, and undo.
//! Nothing here touches the screen, so all of it is covered by unit tests.

use image::{Rgba, RgbaImage};

/// What a stroke looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Pen,
    Highlighter,
}

impl Ink {
    /// How solid the ink is (the highlighter lets the picture show through).
    pub fn alpha(self) -> f32 {
        match self {
            Ink::Pen => 1.0,
            Ink::Highlighter => 0.4,
        }
    }

    /// The highlighter is a lot wider than the pen at the same size setting.
    pub fn width_factor(self) -> f32 {
        match self {
            Ink::Pen => 1.0,
            Ink::Highlighter => 4.0,
        }
    }
}

/// One pen or highlighter stroke, in picture pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub ink: Ink,
    pub color: [u8; 3],
    /// Line width in picture pixels.
    pub width: f32,
    pub points: Vec<(f32, f32)>,
}

/// Width in picture pixels for a size setting (1 to 12): scales with the
/// picture so a mark looks the same on a small snip and on a 4K one.
pub fn stroke_width(setting: f32, ink: Ink, img_w: u32, img_h: u32) -> f32 {
    let scale = (img_w.max(img_h) as f32 / 1000.0).max(1.0);
    setting.max(1.0) * ink.width_factor() * scale
}

/// Distance from `p` to the segment `a`-`b`.
pub fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= f32::EPSILON { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// How much of a pixel at distance `d` from the stroke's centre line is
/// covered by a line of radius `r` (one pixel of soft edge).
pub fn coverage(d: f32, r: f32) -> f32 {
    (r + 0.5 - d).clamp(0.0, 1.0)
}

/// Paints a stroke on `img`. `scale` converts picture pixels to the pixels of
/// `img` (1.0 for the real picture, less for the preview). The stroke is
/// first collected in a coverage mask and blended once, so a highlighter
/// stroke that crosses itself stays one even colour.
pub fn draw_stroke(img: &mut RgbaImage, s: &Stroke, scale: f32) {
    if s.points.is_empty() || img.width() == 0 || img.height() == 0 {
        return;
    }
    let r = (s.width * scale / 2.0).max(0.5);
    let pts: Vec<(f32, f32)> = s.points.iter().map(|p| (p.0 * scale, p.1 * scale)).collect();
    let pad = r + 1.0;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &pts {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
        x1 = x1.max(p.0);
        y1 = y1.max(p.1);
    }
    let (w, h) = (img.width() as i64, img.height() as i64);
    let bx0 = ((x0 - pad).floor() as i64).clamp(0, w);
    let by0 = ((y0 - pad).floor() as i64).clamp(0, h);
    let bx1 = ((x1 + pad).ceil() as i64).clamp(0, w);
    let by1 = ((y1 + pad).ceil() as i64).clamp(0, h);
    if bx1 <= bx0 || by1 <= by0 {
        return;
    }
    let (mw, mh) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
    let mut mask = vec![0.0f32; mw * mh];
    let segs = if pts.len() == 1 { 1 } else { pts.len() - 1 };
    for i in 0..segs {
        let a = pts[i];
        let b = if pts.len() == 1 { pts[0] } else { pts[i + 1] };
        let sx0 = ((a.0.min(b.0) - pad).floor() as i64).clamp(bx0, bx1);
        let sy0 = ((a.1.min(b.1) - pad).floor() as i64).clamp(by0, by1);
        let sx1 = ((a.0.max(b.0) + pad).ceil() as i64).clamp(bx0, bx1);
        let sy1 = ((a.1.max(b.1) + pad).ceil() as i64).clamp(by0, by1);
        for y in sy0..sy1 {
            for x in sx0..sx1 {
                let c = coverage(dist_to_segment((x as f32 + 0.5, y as f32 + 0.5), a, b), r);
                if c > 0.0 {
                    let m = &mut mask[(y - by0) as usize * mw + (x - bx0) as usize];
                    if c > *m {
                        *m = c;
                    }
                }
            }
        }
    }
    let alpha = s.ink.alpha();
    for my in 0..mh {
        for mx in 0..mw {
            let c = mask[my * mw + mx];
            if c > 0.0 {
                let px = img.get_pixel_mut((bx0 as usize + mx) as u32, (by0 as usize + my) as u32);
                blend(px, s.color, c * alpha);
            }
        }
    }
}

/// Lays `color` over a pixel with strength `a` (0 to 1).
pub fn blend(px: &mut Rgba<u8>, color: [u8; 3], a: f32) {
    let a = a.clamp(0.0, 1.0);
    for (i, c) in color.iter().enumerate() {
        px.0[i] = (px.0[i] as f32 * (1.0 - a) + *c as f32 * a).round() as u8;
    }
    px.0[3] = (px.0[3] as f32 + (255.0 - px.0[3] as f32) * a).round() as u8;
}

/// Smallest side of the crop rectangle, in picture pixels.
pub const MIN_CROP: f32 = 8.0;

/// The crop rectangle in picture pixels (x0 < x1, y0 < y1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Crop {
    pub fn full(w: u32, h: u32) -> Self {
        Crop { x0: 0.0, y0: 0.0, x1: w as f32, y1: h as f32 }
    }

    /// Whether the rectangle already covers the whole picture.
    pub fn is_full(&self, w: u32, h: u32) -> bool {
        self.pixels(w, h) == (0, 0, w, h)
    }

    /// The rectangle as whole pixels: (x, y, width, height), inside the picture
    /// and at least 1 by 1.
    pub fn pixels(&self, w: u32, h: u32) -> (u32, u32, u32, u32) {
        let x0 = (self.x0.round().max(0.0) as u32).min(w.saturating_sub(1));
        let y0 = (self.y0.round().max(0.0) as u32).min(h.saturating_sub(1));
        let x1 = (self.x1.round().max(0.0) as u32).clamp(x0 + 1, w.max(1));
        let y1 = (self.y1.round().max(0.0) as u32).clamp(y0 + 1, h.max(1));
        (x0, y0, x1 - x0, y1 - y0)
    }
}

/// Which part of the crop rectangle is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Move,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// What a press at `p` grabs: a corner, an edge, the inside, or nothing.
/// `tol` is how close counts as on an edge, in picture pixels.
pub fn grip_at(c: &Crop, p: (f32, f32), tol: f32) -> Option<Grip> {
    let tol = tol.min((c.x1 - c.x0) / 3.0).min((c.y1 - c.y0) / 3.0);
    let near_l = (p.0 - c.x0).abs() <= tol;
    let near_r = (p.0 - c.x1).abs() <= tol;
    let near_t = (p.1 - c.y0).abs() <= tol;
    let near_b = (p.1 - c.y1).abs() <= tol;
    let in_x = p.0 >= c.x0 - tol && p.0 <= c.x1 + tol;
    let in_y = p.1 >= c.y0 - tol && p.1 <= c.y1 + tol;
    match (near_l, near_r, near_t, near_b) {
        (true, _, true, _) => Some(Grip::TopLeft),
        (_, true, true, _) => Some(Grip::TopRight),
        (true, _, _, true) => Some(Grip::BottomLeft),
        (_, true, _, true) => Some(Grip::BottomRight),
        (true, ..) if in_y => Some(Grip::Left),
        (_, true, ..) if in_y => Some(Grip::Right),
        (_, _, true, _) if in_x => Some(Grip::Top),
        (_, _, _, true) if in_x => Some(Grip::Bottom),
        _ if p.0 > c.x0 && p.0 < c.x1 && p.1 > c.y0 && p.1 < c.y1 => Some(Grip::Move),
        _ => None,
    }
}

/// The rectangle after dragging `grip` from `from` to `to`, starting from
/// `start`. It stays inside the `w` by `h` picture and at least `MIN_CROP` big.
pub fn drag_crop(start: Crop, grip: Grip, from: (f32, f32), to: (f32, f32), w: u32, h: u32) -> Crop {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let (fw, fh) = (w as f32, h as f32);
    let mut c = start;
    if grip == Grip::Move {
        let (cw, ch) = (start.x1 - start.x0, start.y1 - start.y0);
        c.x0 = (start.x0 + dx).clamp(0.0, (fw - cw).max(0.0));
        c.y0 = (start.y0 + dy).clamp(0.0, (fh - ch).max(0.0));
        c.x1 = c.x0 + cw;
        c.y1 = c.y0 + ch;
        return c;
    }
    if matches!(grip, Grip::Left | Grip::TopLeft | Grip::BottomLeft) {
        c.x0 = (start.x0 + dx).clamp(0.0, (start.x1 - MIN_CROP).max(0.0));
    }
    if matches!(grip, Grip::Right | Grip::TopRight | Grip::BottomRight) {
        c.x1 = (start.x1 + dx).clamp((start.x0 + MIN_CROP).min(fw), fw);
    }
    if matches!(grip, Grip::Top | Grip::TopLeft | Grip::TopRight) {
        c.y0 = (start.y0 + dy).clamp(0.0, (start.y1 - MIN_CROP).max(0.0));
    }
    if matches!(grip, Grip::Bottom | Grip::BottomLeft | Grip::BottomRight) {
        c.y1 = (start.y1 + dy).clamp((start.y0 + MIN_CROP).min(fh), fh);
    }
    c
}

/// Paints the crop frame on a preview: the outside dimmed, a border and a
/// square at every corner and edge middle. `scale` is preview pixels per
/// picture pixel.
pub fn draw_crop_frame(img: &mut RgbaImage, c: &Crop, scale: f32) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let (x0, y0) = ((c.x0 * scale).clamp(0.0, w), (c.y0 * scale).clamp(0.0, h));
    let (x1, y1) = ((c.x1 * scale).clamp(0.0, w), (c.y1 * scale).clamp(0.0, h));
    for y in 0..img.height() {
        for x in 0..img.width() {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if fx < x0 || fx >= x1 || fy < y0 || fy >= y1 {
                blend(img.get_pixel_mut(x, y), [0, 0, 0], 0.5);
            }
        }
    }
    let mut rect = |ax: f32, ay: f32, bx: f32, by: f32, color: [u8; 3]| {
        let (ix0, iy0) = (ax.max(0.0) as u32, ay.max(0.0) as u32);
        let (ix1, iy1) = ((bx.max(0.0) as u32).min(img.width()), (by.max(0.0) as u32).min(img.height()));
        for y in iy0..iy1 {
            for x in ix0..ix1 {
                blend(img.get_pixel_mut(x, y), color, 1.0);
            }
        }
    };
    let t = 2.0;
    rect(x0, y0, x1, y0 + t, [255; 3]);
    rect(x0, y1 - t, x1, y1, [255; 3]);
    rect(x0, y0, x0 + t, y1, [255; 3]);
    rect(x1 - t, y0, x1, y1, [255; 3]);
    let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let hs = 5.0;
    for (cx, cy) in [(x0, y0), (mx, y0), (x1, y0), (x0, my), (x1, my), (x0, y1), (mx, y1), (x1, y1)] {
        rect(cx - hs - 1.0, cy - hs - 1.0, cx + hs + 1.0, cy + hs + 1.0, [40; 3]);
        rect(cx - hs, cy - hs, cx + hs, cy + hs, [255; 3]);
    }
}

/// The picture as shown in an `avail_w` by `avail_h` area: as big as fits
/// without being stretched past twice its size.
pub fn fit(avail_w: f32, avail_h: f32, img_w: u32, img_h: u32) -> (f32, f32) {
    if img_w == 0 || img_h == 0 {
        return (0.0, 0.0);
    }
    let s = (avail_w / img_w as f32).min(avail_h / img_h as f32).clamp(0.0, 2.0);
    (img_w as f32 * s, img_h as f32 * s)
}

/// A point on the shown picture, in picture pixels, kept inside the picture.
pub fn to_picture(p: (f32, f32), shown: (f32, f32), img_w: u32, img_h: u32) -> (f32, f32) {
    if shown.0 <= 0.0 || shown.1 <= 0.0 {
        return (0.0, 0.0);
    }
    ((p.0 * img_w as f32 / shown.0).clamp(0.0, img_w as f32), (p.1 * img_h as f32 / shown.1).clamp(0.0, img_h as f32))
}

/// Undo and redo for any cloneable state.
#[derive(Debug)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
}

/// How many steps back are kept.
const HISTORY_LIMIT: usize = 50;

impl<T> Default for History<T> {
    fn default() -> Self {
        History { undo: Vec::new(), redo: Vec::new() }
    }
}

impl<T> History<T> {
    /// Remembers `before` as the state to go back to. A new change ends the redo line.
    pub fn record(&mut self, before: T) {
        self.undo.push(before);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Steps back: returns the state to show, remembering `current` for redo.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    /// Steps forward again: returns the state to show, remembering `current` for undo.
    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]))
    }

    fn stroke(ink: Ink, pts: &[(f32, f32)], width: f32) -> Stroke {
        Stroke { ink, color: [200, 0, 0], width, points: pts.to_vec() }
    }

    #[test]
    fn distance_to_a_segment() {
        assert_eq!(dist_to_segment((5.0, 3.0), (0.0, 0.0), (10.0, 0.0)), 3.0);
        // Past the end the nearest point is the end itself.
        assert_eq!(dist_to_segment((13.0, 4.0), (0.0, 0.0), (10.0, 0.0)), 5.0);
        // A segment of length zero is a point.
        assert_eq!(dist_to_segment((3.0, 4.0), (0.0, 0.0), (0.0, 0.0)), 5.0);
    }

    #[test]
    fn coverage_fades_over_one_pixel() {
        assert_eq!(coverage(0.0, 2.0), 1.0);
        assert_eq!(coverage(2.5, 2.0), 0.0);
        assert!((coverage(2.0, 2.0) - 0.5).abs() < 1e-6 || coverage(2.0, 2.0) == 0.5);
    }

    #[test]
    fn pen_paints_its_line_and_leaves_the_rest() {
        let mut img = white(20, 20);
        draw_stroke(&mut img, &stroke(Ink::Pen, &[(2.0, 10.0), (18.0, 10.0)], 4.0), 1.0);
        assert_eq!(img.get_pixel(10, 10).0, [200, 0, 0, 255]);
        assert_eq!(img.get_pixel(10, 3).0, [255, 255, 255, 255]);
    }

    #[test]
    fn a_single_point_makes_a_dot() {
        let mut img = white(20, 20);
        draw_stroke(&mut img, &stroke(Ink::Pen, &[(10.0, 10.0)], 6.0), 1.0);
        assert_eq!(img.get_pixel(10, 10).0, [200, 0, 0, 255]);
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn highlighter_lets_the_picture_through() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        let mut s = stroke(Ink::Highlighter, &[(2.0, 10.0), (18.0, 10.0)], 6.0);
        s.color = [255, 255, 0];
        draw_stroke(&mut img, &s, 1.0);
        let p = img.get_pixel(10, 10).0;
        assert_eq!(p, [102, 102, 0, 255]);
    }

    #[test]
    fn a_highlighter_stroke_crossing_itself_stays_one_colour() {
        let mut img = RgbaImage::from_pixel(30, 30, Rgba([0, 0, 0, 255]));
        let mut s = stroke(Ink::Highlighter, &[(5.0, 15.0), (25.0, 15.0), (5.0, 15.0)], 6.0);
        s.color = [255, 255, 0];
        draw_stroke(&mut img, &s, 1.0);
        assert_eq!(img.get_pixel(15, 15).0, [102, 102, 0, 255]);
    }

    #[test]
    fn strokes_off_the_picture_do_not_panic() {
        let mut img = white(10, 10);
        draw_stroke(&mut img, &stroke(Ink::Pen, &[(-50.0, -50.0), (500.0, 500.0)], 8.0), 1.0);
        draw_stroke(&mut img, &stroke(Ink::Pen, &[(-50.0, 300.0)], 8.0), 1.0);
        draw_stroke(&mut img, &stroke(Ink::Pen, &[], 8.0), 1.0);
    }

    #[test]
    fn preview_scale_moves_the_stroke() {
        let mut img = white(20, 20);
        draw_stroke(&mut img, &stroke(Ink::Pen, &[(20.0, 20.0)], 4.0), 0.5);
        // The dot is drawn around (10, 10), half way to where it was set.
        assert!(img.get_pixel(10, 10).0[1] < 200);
        assert_eq!(img.get_pixel(19, 19).0, [255, 255, 255, 255]);
    }

    #[test]
    fn width_follows_picture_size() {
        assert_eq!(stroke_width(3.0, Ink::Pen, 800, 600), 3.0);
        assert_eq!(stroke_width(3.0, Ink::Pen, 3000, 2000), 9.0);
        assert_eq!(stroke_width(2.0, Ink::Highlighter, 1000, 500), 8.0);
    }

    #[test]
    fn crop_pixels_are_whole_and_inside() {
        let c = Crop { x0: 10.4, y0: 5.6, x1: 50.5, y1: 400.0 };
        assert_eq!(c.pixels(100, 100), (10, 6, 41, 94));
        assert!(Crop::full(100, 80).is_full(100, 80));
        assert!(!c.is_full(100, 100));
        // Never empty, even when squeezed to nothing.
        let t = Crop { x0: 99.9, y0: 99.9, x1: 99.9, y1: 99.9 };
        assert_eq!(t.pixels(100, 100), (99, 99, 1, 1));
    }

    #[test]
    fn grips_are_found_where_expected() {
        let c = Crop { x0: 10.0, y0: 10.0, x1: 90.0, y1: 70.0 };
        assert_eq!(grip_at(&c, (10.0, 10.0), 5.0), Some(Grip::TopLeft));
        assert_eq!(grip_at(&c, (91.0, 69.0), 5.0), Some(Grip::BottomRight));
        assert_eq!(grip_at(&c, (50.0, 11.0), 5.0), Some(Grip::Top));
        assert_eq!(grip_at(&c, (9.0, 40.0), 5.0), Some(Grip::Left));
        assert_eq!(grip_at(&c, (50.0, 40.0), 5.0), Some(Grip::Move));
        assert_eq!(grip_at(&c, (200.0, 40.0), 5.0), None);
    }

    #[test]
    fn dragging_an_edge_changes_only_that_edge() {
        let c = Crop { x0: 10.0, y0: 10.0, x1: 90.0, y1: 70.0 };
        let d = drag_crop(c, Grip::Left, (10.0, 40.0), (30.0, 55.0), 100, 100);
        assert_eq!(d, Crop { x0: 30.0, ..c });
    }

    #[test]
    fn dragging_stops_at_the_picture_and_the_minimum() {
        let c = Crop { x0: 10.0, y0: 10.0, x1: 90.0, y1: 70.0 };
        let d = drag_crop(c, Grip::BottomRight, (90.0, 70.0), (500.0, 500.0), 100, 100);
        assert_eq!((d.x1, d.y1), (100.0, 100.0));
        let d = drag_crop(c, Grip::Right, (90.0, 40.0), (-500.0, 40.0), 100, 100);
        assert_eq!(d.x1, c.x0 + MIN_CROP);
        let d = drag_crop(c, Grip::TopLeft, (10.0, 10.0), (-30.0, -30.0), 100, 100);
        assert_eq!((d.x0, d.y0), (0.0, 0.0));
    }

    #[test]
    fn moving_the_crop_keeps_its_size() {
        let c = Crop { x0: 10.0, y0: 10.0, x1: 50.0, y1: 40.0 };
        let d = drag_crop(c, Grip::Move, (20.0, 20.0), (200.0, 25.0), 100, 100);
        assert_eq!(d, Crop { x0: 60.0, y0: 15.0, x1: 100.0, y1: 45.0 });
    }

    #[test]
    fn crop_frame_dims_outside_and_keeps_inside() {
        let mut img = white(40, 40);
        draw_crop_frame(&mut img, &Crop { x0: 10.0, y0: 10.0, x1: 30.0, y1: 30.0 }, 1.0);
        assert_eq!(img.get_pixel(1, 1).0, [128, 128, 128, 255]);
        assert_eq!(img.get_pixel(20, 20).0, [255, 255, 255, 255]);
    }

    #[test]
    fn picture_fits_the_window() {
        assert_eq!(fit(500.0, 500.0, 1000, 500), (500.0, 250.0));
        // Small pictures grow at most to twice their size.
        assert_eq!(fit(5000.0, 5000.0, 100, 50), (200.0, 100.0));
        assert_eq!(fit(100.0, 100.0, 0, 0), (0.0, 0.0));
    }

    #[test]
    fn pointer_maps_back_to_the_picture() {
        assert_eq!(to_picture((250.0, 125.0), (500.0, 250.0), 1000, 500), (500.0, 250.0));
        assert_eq!(to_picture((-10.0, 900.0), (500.0, 250.0), 1000, 500), (0.0, 500.0));
        assert_eq!(to_picture((1.0, 1.0), (0.0, 0.0), 10, 10), (0.0, 0.0));
    }

    #[test]
    fn undo_and_redo_walk_the_states() {
        let mut h = History::default();
        h.record(1);
        h.record(2);
        assert_eq!(h.undo(3), Some(2));
        assert_eq!(h.undo(2), Some(1));
        assert_eq!(h.undo(1), None);
        assert_eq!(h.redo(1), Some(2));
        assert!(h.can_undo() && h.can_redo());
        // A new change ends the redo line.
        h.record(9);
        assert!(!h.can_redo());
    }

    #[test]
    fn history_is_capped() {
        let mut h = History::default();
        for i in 0..200 {
            h.record(i);
        }
        let mut n = 0;
        let mut cur = 200;
        while let Some(p) = h.undo(cur) {
            cur = p;
            n += 1;
        }
        assert_eq!(n, HISTORY_LIMIT);
    }
}
