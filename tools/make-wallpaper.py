#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Render Lapin's default wallpapers (original artwork).

Soft, layered translucent arches over a deep blue-to-teal field: calm,
light-catching, and recognisably "ours". Run from the repository root:

    python3 tools/make-wallpaper.py            # writes data/wallpapers/*.jpg
"""
import os
import sys

import numpy as np

from PIL import Image, ImageDraw, ImageFilter

W, H = 3840, 2160


def gradient(top, bottom, glow_center, glow_color, glow_radius):
    ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
    t = (ys / (H - 1))[..., None]
    base = np.array(top, np.float32) * (1 - t) + np.array(bottom, np.float32) * t
    gx, gy = glow_center
    d = np.hypot(xs - gx, (ys - gy) * 1.25) / glow_radius
    g = (np.clip(1.0 - d, 0.0, 1.0) ** 2.2 * 0.85)[..., None]
    out = base * (1 - g) + np.array(glow_color, np.float32) * g
    # A touch of noise prevents banding on 8-bit displays.
    out += np.random.default_rng(7).normal(0, 0.8, out.shape).astype(np.float32)
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGB")


def arch_path(cx, base_y, w, h):
    """Points of an arch outline: two verticals joined by a half-ellipse."""
    pts = [(cx - w / 2, base_y)]
    top = base_y - h
    r = w / 2
    pts.append((cx - w / 2, top + r))
    for i in range(0, 181, 3):
        a = np.radians(180 - i)
        pts.append((cx + r * np.cos(a), top + r - r * np.sin(a)))
    pts.append((cx + w / 2, base_y))
    return pts


def arches(img, color, count, cx, base_y, width0, step, height_ratio, alpha_lo, alpha_hi, fill_alpha):
    """Nested arches: faint fills plus luminous outlines, brighter inward."""
    fill = Image.new("RGBA", (W, H), color + (0,))
    glow = Image.new("RGBA", (W, H), color + (0,))
    line = Image.new("RGBA", (W, H), color + (0,))
    df, dg, dl = ImageDraw.Draw(fill), ImageDraw.Draw(glow), ImageDraw.Draw(line)
    for i in range(count):
        w = width0 - i * step
        if w <= 0:
            break
        h = w * height_ratio
        t = i / max(1, count - 1)
        a = int(alpha_lo + (alpha_hi - alpha_lo) * t)
        pts = arch_path(cx, base_y, w, h)
        df.polygon(pts, fill=color + (fill_alpha,))
        dg.line(pts, fill=color + (a,), width=26, joint="curve")
        dl.line(pts, fill=color + (min(255, a * 2),), width=3, joint="curve")
    img.alpha_composite(fill.filter(ImageFilter.GaussianBlur(4)))
    img.alpha_composite(glow.filter(ImageFilter.GaussianBlur(22)))
    img.alpha_composite(line.filter(ImageFilter.GaussianBlur(1)))


def render(dark: bool):
    if dark:
        top, bottom, glow = (6, 14, 32), (5, 44, 70), (48, 132, 214)
        arch = (140, 200, 255)
        lo, hi, fill_a = 14, 60, 10
        sky_a = 90
    else:
        top, bottom, glow = (186, 214, 242), (234, 243, 252), (255, 255, 255)
        arch = (40, 110, 200)
        lo, hi, fill_a = 10, 46, 8
        sky_a = 200
    img = gradient(top, bottom, (W * 0.5, H * 0.66), glow, H * 0.8).convert("RGBA")
    # Distant wide arches, then the main nested set.
    arches(img, arch, 3, W * 0.5, H * 1.02, W * 1.15, W * 0.16, 0.85, lo // 2, lo, fill_a // 2)
    arches(img, arch, 6, W * 0.5, H * 1.0, W * 0.56, W * 0.075, 1.12, lo, hi, fill_a)
    # Skylight: soft light source above the arches. Transparent white base
    # so the blur does not create a dark halo.
    sky = Image.new("RGBA", (W, H), (255, 255, 255, 0))
    ImageDraw.Draw(sky).ellipse([W / 2 - 70, H * 0.1 - 70, W / 2 + 70, H * 0.1 + 70], fill=(255, 255, 255, sky_a))
    img.alpha_composite(sky.filter(ImageFilter.GaussianBlur(36)))
    return img.convert("RGB")


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "data/wallpapers"
    os.makedirs(out, exist_ok=True)
    for dark in (True, False):
        name = "lapin-dark.jpg" if dark else "lapin-light.jpg"
        render(dark).save(os.path.join(out, name), quality=88, optimize=True, progressive=True)
        print("wrote", os.path.join(out, name))


if __name__ == "__main__":
    main()
