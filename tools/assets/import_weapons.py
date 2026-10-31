"""Imports the first-person weapon and grenade models from the Styloo Guns Asset Pack into
crates/frontline/assets/models/weapons/ (embedded in the binary, see src/assets.rs).

    python3 tools/assets/import_weapons.py \\
        --zip "content/assets/Styloo Guns Asset Pack GLTF FBX V1.1.zip"

One self-contained `.glb` per piece (textures embedded): the colour map is graded towards a duller,
darker military palette (the pack's own colours are cartoon bright), every texture is at most
--max-texture px and re-encoded as JPEG, and the material gets a little metal and gloss (the pack's
own glTF has none). The pack ships no licence we could verify (docs: assets/models/CREDITS.md).
The M4 and M16A4 are not from the pack (it has no AR-15): tools/assets/build_rifles.py builds them.
Reuses tools/assets/import_props.py; needs ImageMagick (`magick`). Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import sys
import zipfile
from pathlib import Path, PurePosixPath

sys.path.insert(0, str(Path(__file__).resolve().parent))
import import_props as ip  # noqa: E402

# (brightness %, saturation %, hue %) for ImageMagick `-modulate`, and (metallic, roughness).
PIECES = {
    "awp": ((70, 40, 100), (0.35, 0.6)),
    "mac10": ((70, 100, 100), (0.6, 0.45)),
    "pew": ((68, 12, 100), (0.2, 0.6)),
    "nade_low": ((85, 70, 100), (0.3, 0.6)),
    "flashbang_low": ((85, 70, 100), (0.4, 0.5)),
    "smoke_low": ((85, 70, 100), (0.3, 0.6)),
}


def is_colour(image: dict) -> bool:
    return "color" in (image.get("name") or "").lower()


def grade(gltf: ip.Gltf, modulate: tuple[int, int, int], factors: tuple[float, float]) -> None:
    """Grades the colour image in place (as a new buffer view at the end of the binary) and sets
    the material's metallic and roughness factors."""
    binary = bytearray(gltf.binary)
    views = gltf.document["bufferViews"]
    for image in gltf.document["images"]:
        if not is_colour(image):
            continue
        graded = ip.run(
            "magick", "-", "-modulate", ",".join(map(str, modulate)), "png:-",
            stdin=ip.image_source(gltf, image, None),
        )
        binary += b"\0" * (-len(binary) % 4)
        views.append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(graded)})
        image["bufferView"] = len(views) - 1
        binary += graded
    gltf.binary = bytes(binary)
    for material in gltf.document["materials"]:
        pbr = material.setdefault("pbrMetallicRoughness", {})
        pbr["metallicFactor"], pbr["roughnessFactor"] = factors


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--zip", type=Path, required=True)
    parser.add_argument("--max-texture", type=int, default=1024)
    parser.add_argument("--out", type=Path, default=ip.MODELS_DIR / "weapons")
    parser.add_argument("pieces", nargs="*", help=f"file stems (default: {' '.join(PIECES)})")
    args = parser.parse_args()

    names = args.pieces or list(PIECES)
    args.out.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.zip) as archive:
        for name in names:
            modulate, factors = PIECES[name]
            members = [
                m for m in archive.namelist()
                if PurePosixPath(m).name == f"{name}.glb" and "Normal version" in m
            ]
            if len(members) != 1:
                sys.exit(f"{name}.glb: expected one 'Normal version' member, found {members}")
            gltf = ip.read_glb(archive.read(members[0]))
            grade(gltf, modulate, factors)
            glb = ip.convert(gltf, None, None, args.max_texture)
            (args.out / f"{name}.glb").write_bytes(glb)
            print(f"{name}.glb {len(glb) / 1024:.0f} KB")


if __name__ == "__main__":
    main()
