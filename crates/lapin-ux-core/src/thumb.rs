// SPDX-License-Identifier: GPL-3.0-only
//! Window thumbnails: shrink a captured frame to preview size.

/// Byte order of a captured pixel in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelOrder {
    /// wl_shm ARGB8888 / XRGB8888 on little-endian: B, G, R, A.
    Bgra,
    /// wl_shm ABGR8888 / XBGR8888 on little-endian: R, G, B, A.
    Rgba,
}

/// A captured frame.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes per row.
    pub stride: u32,
    pub order: PixelOrder,
    /// Force alpha to 255 (X formats carry garbage there).
    pub opaque: bool,
}

/// Box-filter `frame` down to fit within `max_w`×`max_h`, keeping the
/// aspect ratio. Returns (width, height, RGBA).
pub fn shrink(frame: Frame<'_>, max_w: u32, max_h: u32) -> Option<(u32, u32, Vec<u8>)> {
    let Frame { data: src, width: w, height: h, stride, order, opaque } = frame;
    if w == 0 || h == 0 || (stride as usize) < w as usize * 4 || src.len() < stride as usize * (h as usize - 1) + w as usize * 4 {
        return None;
    }
    let scale = (max_w as f64 / w as f64).min(max_h as f64 / h as f64).min(1.0);
    let tw = ((w as f64 * scale).round() as u32).max(1);
    let th = ((h as f64 * scale).round() as u32).max(1);
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for ty in 0..th {
        let y0 = (ty as u64 * h as u64 / th as u64) as u32;
        let y1 = (((ty + 1) as u64 * h as u64).div_ceil(th as u64) as u32).clamp(y0 + 1, h);
        for tx in 0..tw {
            let x0 = (tx as u64 * w as u64 / tw as u64) as u32;
            let x1 = (((tx + 1) as u64 * w as u64).div_ceil(tw as u64) as u32).clamp(x0 + 1, w);
            // Sample at most 4×4 points per output pixel: plenty for a preview.
            let sx = ((x1 - x0) / 4).max(1);
            let sy = ((y1 - y0) / 4).max(1);
            let (mut acc, mut n) = ([0u32; 4], 0u32);
            let mut y = y0;
            while y < y1 {
                let row = (y * stride) as usize;
                let mut x = x0;
                while x < x1 {
                    let p = &src[row + x as usize * 4..row + x as usize * 4 + 4];
                    let (r, g, b, a) = match order {
                        PixelOrder::Bgra => (p[2], p[1], p[0], p[3]),
                        PixelOrder::Rgba => (p[0], p[1], p[2], p[3]),
                    };
                    acc[0] += r as u32;
                    acc[1] += g as u32;
                    acc[2] += b as u32;
                    acc[3] += if opaque { 255 } else { a as u32 };
                    n += 1;
                    x += sx;
                }
                y += sy;
            }
            out.extend(acc.iter().map(|c| (c / n) as u8));
        }
    }
    Some((tw, th, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_and_converts() {
        // 4×2 BGRA image: left half red, right half blue; stride with padding.
        let (w, h, stride) = (4u32, 2u32, 20u32);
        let mut src = vec![0u8; (stride * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = (y * stride + x * 4) as usize;
                let px = if x < 2 { [0, 0, 255, 0] } else { [255, 0, 0, 0] };
                src[i..i + 4].copy_from_slice(&px);
            }
        }
        let (tw, th, rgba) = shrink(Frame { data: &src, width: w, height: h, stride, order: PixelOrder::Bgra, opaque: true }, 2, 2).unwrap();
        assert_eq!((tw, th), (2, 1));
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[0, 0, 255, 255]);
        // Never enlarges.
        let (tw, th, _) = shrink(Frame { data: &src, width: w, height: h, stride, order: PixelOrder::Bgra, opaque: true }, 100, 100).unwrap();
        assert_eq!((tw, th), (4, 2));
        assert!(shrink(Frame { data: &src[..10], width: w, height: h, stride, order: PixelOrder::Bgra, opaque: true }, 2, 2).is_none());
    }
}
