#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Build Lapin UX's colour emoji font from Microsoft's Fluent Emoji (MIT).

    python3 tools/make-fluent-emoji.py <fluentui-emoji checkout> <out dir>

Then copy LapinFluentEmoji.ttf and LICENSE-fluentui-emoji to data/fonts/fluent-emoji/
and fluent-emoji.txt to crates/lapin-ux-shell/src/clipboard/.

Takes every emoji's "Color" artwork (and each skin tone), names the files by
code point and builds a CBDT colour-bitmap font with nanoemoji
(pip install nanoemoji). CBDT is what COSMIC's text engine, GTK, Qt and the
browsers all draw in colour. The font is named "Lapin Fluent Emoji".
"""
import glob
import json
import os
import shutil
import subprocess
import sys

TONES = ["Light", "Medium-Light", "Medium", "Medium-Dark", "Dark"]
FAMILY = "Lapin Fluent Emoji"


def seq_name(seq: str) -> str:
    # nanoemoji wants emoji_u<cp>_<cp>.svg; VS16 (fe0f) is matched by the font's
    # cmap without it, so drop it from names.
    cps = [c for c in seq.lower().split() if c != "fe0f"]
    return "emoji_u" + "_".join(cps) + ".svg"


def main():
    src, out = sys.argv[1], sys.argv[2]
    work = os.path.join(out, "svg")
    os.makedirs(work, exist_ok=True)
    wanted = set()
    count = 0
    for meta in sorted(glob.glob(os.path.join(src, "assets", "*", "metadata.json"))):
        d = json.load(open(meta, encoding="utf-8"))
        base = os.path.dirname(meta)
        tones = d.get("unicodeSkintones")
        if tones:
            pairs = [(os.path.join(base, "Default", "Color"), tones[0])]
            pairs += [(os.path.join(base, t, "Color"), tones[i + 1]) for i, t in enumerate(TONES) if i + 1 < len(tones)]
        else:
            pairs = [(os.path.join(base, "Color"), d["unicode"])]
        for folder, seq in pairs:
            svgs = glob.glob(os.path.join(folder, "*.svg"))
            if not svgs:
                continue
            name = seq_name(seq)
            if name in wanted:
                continue
            wanted.add(name)
            dst = os.path.join(work, name)
            # Only touch files that changed, so a rebuild stays incremental.
            data = open(svgs[0], "rb").read()
            if not os.path.exists(dst) or open(dst, "rb").read() != data:
                with open(dst, "wb") as fh:
                    fh.write(data)
            count += 1
    for stale in set(os.listdir(work)) - wanted:
        os.remove(os.path.join(work, stale))
    print(f"{count} emoji")
    files = sorted(glob.glob(os.path.join(work, "*.svg")))
    # The list of sequences the font has, for the shell's emoji panel.
    seqs = sorted(
        "".join(chr(int(c, 16)) for c in os.path.basename(f)[len("emoji_u"):-len(".svg")].split("_")) for f in files
    )
    with open(os.path.join(out, "fluent-emoji.txt"), "w", encoding="utf-8") as fh:
        fh.write("\n".join(seqs) + "\n")
    build = os.path.join(out, "build")  # kept between runs: ninja only redoes what changed
    nano = shutil.which("nanoemoji") or os.path.join(os.path.dirname(sys.executable), "nanoemoji")
    subprocess.run(
        [nano, "--family", FAMILY, "--color_format", "cbdt", "--bitmap_resolution", "136",
         # pngquant shrinks the bitmaps; zopfli would take hours for a few % more.
         "--nouse_zopflipng",
         "--output_file", "LapinFluentEmoji.ttf", "--build_dir", build, *files],
        check=True,
    )
    shutil.copy(os.path.join(build, "LapinFluentEmoji.ttf"), os.path.join(out, "LapinFluentEmoji.ttf"))
    shutil.copy(os.path.join(src, "LICENSE"), os.path.join(out, "LICENSE-fluentui-emoji"))
    print("wrote", os.path.join(out, "LapinFluentEmoji.ttf"))


if __name__ == "__main__":
    main()
