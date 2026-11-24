#!/usr/bin/env python3
"""Tests the cursor-pack download checks in tools/lapin-ux-cursors with
made-up zips (no network). Run: python3 -I tools/test-cursors/run.py"""
import importlib.machinery
import importlib.util
import io
import os
import sys
import tempfile
import zipfile

here = os.path.dirname(os.path.abspath(__file__))
path = os.path.join(here, "..", "lapin-ux-cursors")
loader = importlib.machinery.SourceFileLoader("lapin_ux_cursors", path)
spec = importlib.util.spec_from_loader("lapin_ux_cursors", loader)
cur = importlib.util.module_from_spec(spec)
loader.exec_module(cur)

failed = 0


def check(name, ok):
    global failed
    print(("  ok   " if ok else "  FAIL ") + name)
    failed |= not ok


def make_zip(entries):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in entries:
            if isinstance(name, zipfile.ZipInfo):
                z.writestr(name, data)
            else:
                z.writestr(name, data)
    return buf.getvalue()


def digest_of(data):
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        return cur.tree_digest(z)


def unpack(data, expected):
    with tempfile.TemporaryDirectory() as d:
        part = os.path.join(d, "pack")
        try:
            cur.unpack_verified(data, part, expected)
        except ValueError as e:
            # look at the disk now, before the temporary folder goes away
            left = sorted(os.listdir(d))
            return str(e), part, left
        files = sorted(os.path.relpath(os.path.join(r, f), part)
                       for r, _, fs in os.walk(part) for f in fs)
        return None, part, files


good = make_zip([("pack-abc/assets/light/a.cur", b"one"), ("pack-abc/assets/dark/b.cur", b"two")])
want = digest_of(good)
err, _, files = unpack(good, want)
check("a pack with the expected digest unpacks", err is None and files == ["assets/dark/b.cur", "assets/light/a.cur"])

renamed = make_zip([("other-name/assets/light/a.cur", b"one"), ("other-name/assets/dark/b.cur", b"two")])
check("the top folder's name doesn't change the digest", digest_of(renamed) == want)

changed = make_zip([("pack-abc/assets/light/a.cur", b"ONE"), ("pack-abc/assets/dark/b.cur", b"two")])
err, part, _ = unpack(changed, want)
check("changed file content is refused", err is not None and "don't match" in err)

extra = make_zip([("pack-abc/assets/light/a.cur", b"one"), ("pack-abc/assets/dark/b.cur", b"two"),
                  ("pack-abc/assets/dark/c.cur", b"three")])
err, _, _ = unpack(extra, want)
check("an extra file is refused", err is not None)

err, _, _ = unpack(b"this is not a zip", want)
check("a file that isn't a zip is refused", err is not None and "not a zip" in err)

# The archives carry the expected files plus one bad member. The extra member
# would also change the digest, so the tests check the error text to prove
# the path check is what refused them, not a digest mismatch.
for label, name, why in (("a '..' path", "pack-abc/../../evil", "unsafe path"),
                         ("an absolute path", "/etc/evil", "unsafe path"),
                         ("a backslash path", "pack-abc\\evil", "unsafe path"),
                         ("a file outside the top folder", "evil", "outside the pack's folder")):
    bad = make_zip([("pack-abc/assets/light/a.cur", b"one"), ("pack-abc/assets/dark/b.cur", b"two"),
                    (name, b"x")])
    err, _, left = unpack(bad, want)
    check(f"{label} is refused by the path check", err is not None and why in err)
    check(f"{label} leaves nothing on disk", left == [])

link = zipfile.ZipInfo("pack-abc/assets/link")
link.external_attr = 0o120777 << 16
bad = make_zip([("pack-abc/assets/light/a.cur", b"one"), (link, b"/etc/passwd")])
err, _, _ = unpack(bad, want)
check("a symlink entry is refused", err is not None and "plain file" in err)

old = cur.MAX_ENTRIES
cur.MAX_ENTRIES = 1
err, _, _ = unpack(good, want)
cur.MAX_ENTRIES = old
check("too many entries are refused", err is not None and "too many" in err)

old = cur.MAX_UNPACKED_BYTES
cur.MAX_UNPACKED_BYTES = 4
err, _, _ = unpack(good, want)
cur.MAX_UNPACKED_BYTES = old
check("a pack that unpacks too big is refused", err is not None and "more than expected" in err)

err, part, files = unpack(changed, want)
check("nothing is written when the digest doesn't match", err is not None and files == [])

try:
    cur.read_limited(io.BytesIO(b"x" * 11), 10)
    check("a download over the limit is refused", False)
except ValueError:
    check("a download over the limit is refused", True)
check("a download at the limit is read", cur.read_limited(io.BytesIO(b"x" * 10), 10) == b"x" * 10)

sys.exit(failed)
