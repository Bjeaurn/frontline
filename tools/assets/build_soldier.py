#!/usr/bin/env python3
"""Builds `crates/frontline/assets/models/characters/soldier.glb`: makes the small fabric texture
(a seeded woven-cloth noise, original, CC0), then runs `blender_soldier.py` in headless Blender.

    python3 tools/assets/build_soldier.py [--preview /tmp/prefix]

Needs `blender` on the PATH (5.x). With `--preview` also renders the rest pose from several sides
(`blender_soldier_preview.py`).
"""

import argparse
import os
import random
import struct
import subprocess
import tempfile
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(ROOT, "crates/frontline/assets/models/characters/soldier.glb")
GUN = os.path.join(ROOT, "crates/frontline/assets/models/weapons/m4.glb")
SIZE = 64


def fabric_png():
    """A tileable greyscale weave: threads along both axes, light noise, mostly bright so the
    Faction's tint carries the colour."""
    rng = random.Random(7)
    rows = []
    for y in range(SIZE):
        row = bytearray([0])
        for x in range(SIZE):
            thread = 0.5 + 0.5 * (1 if (x // 2 + y // 2) % 2 else -1) * 0.35
            value = 0.78 + 0.14 * thread + rng.uniform(-0.07, 0.07)
            row.append(max(0, min(255, int(value * 255))))
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 0, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--preview", help="prefix of the preview renders")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        fabric = os.path.join(tmp, "fabric.png")
        with open(fabric, "wb") as f:
            f.write(fabric_png())
        script = os.path.join(HERE, "blender_soldier.py")
        subprocess.run(
            ["blender", "-b", "--python", script, "--", "--fabric", fabric, "--out", OUT],
            check=True,
        )
    if args.preview:
        subprocess.run(
            [
                "blender",
                "-b",
                "--python",
                os.path.join(HERE, "blender_soldier_preview.py"),
                "--",
                "--glb",
                OUT,
                "--gun",
                GUN,
                "--out",
                args.preview,
            ],
            check=True,
        )


main()
