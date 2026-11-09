#!/usr/bin/env python3
"""Imports the CC0 surface texture sets from ambientCG into crates/frontline/assets/textures/.

Each entry of SURFACES below is one surface: a folder with `color.jpg`, `normal.jpg` (OpenGL) and
`roughness.jpg`, made from the asset's 1K-JPG download. Colour and normal are 1024 px, roughness
512 px, re-encoded as JPEG. The downloads are kept in the git-ignored content/assets/ambientcg/.

The same list (name, metres one repeat covers, image size) must be in the surface tables:
`surfaces!` in crates/frontline/src/materials.rs and SURFACES in tools/mapgen/mapgen.py (a unit
test in materials.rs compares the two). `--credits` prints the rows for textures/CREDITS.md.

Needs ImageMagick (`magick`); standard library otherwise.

    python3 tools/assets/import_textures.py                 import every surface that is missing
    python3 tools/assets/import_textures.py --force NAME    re-import one
    python3 tools/assets/import_textures.py --credits       print the CREDITS.md table rows
"""
import subprocess
import sys
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "crates/frontline/assets/textures"
CACHE = ROOT / "content/assets/ambientcg"

# surface folder -> ambientCG asset id. The first eight were imported by hand earlier (see
# CREDITS.md for how they were cropped) and are listed for the credits only.
HAND_MADE = {
    "concrete": "Concrete042A",
    "plaster": "PaintedPlaster017",
    "metal": "PaintedMetal012",
    "corrugated": "CorrugatedSteel005",
    "wood": "Planks023A",
    "ground": "Ground109",
    "brick": "Bricks105",
    "grass": "Grass001",
}
SURFACES = {
    # Walls.
    "plaster_yellow": "PaintedPlaster018",
    "plaster_ochre": "PaintedPlaster007",
    "plaster_blue": "PaintedPlaster008",
    "plaster_white": "PaintedPlaster016",
    "plaster_green": "PaintedPlaster009",
    "brick_old": "Bricks097",
    "stone": "Bricks109A",
    "brick_hollow": "Bricks088",
    "concrete_damaged": "Concrete044C",
    "wood_painted": "PaintedWood007B",
    "wood_siding": "WoodSiding011",
    # Floors and ground.
    "asphalt": "Road012A",
    "sand": "Ground054",
    "gravel": "Gravel022",
    "paving": "PavingStones128",
    "paving_brick": "PavingStones152",
    "tiles": "Tiles141",
    "wood_floor": "WoodFloor041",
    # Roofs.
    "roof_tiles": "RoofingTiles012B",
    "roof_slate": "RoofingTiles013B",
    # Metal.
    "metal_rust": "MetalPlates011",
    "metal_green": "PaintedMetal006",
    "metal_white": "PaintedMetal011",
    "corrugated_rust": "CorrugatedSteel007C",
    "corrugated_blue": "CorrugatedSteel007B",
}


def download(asset: str) -> Path:
    CACHE.mkdir(parents=True, exist_ok=True)
    path = CACHE / f"{asset}_1K-JPG.zip"
    if not path.exists():
        url = f"https://ambientcg.com/get?file={asset}_1K-JPG.zip"
        request = urllib.request.Request(url, headers={"User-Agent": "frontline-import-textures"})
        with urllib.request.urlopen(request, timeout=180) as response:
            path.write_bytes(response.read())
    return path


def magick(*args: str) -> None:
    subprocess.run(["magick", *args], check=True)


def import_surface(name: str, asset: str) -> None:
    out = OUT / name
    out.mkdir(parents=True, exist_ok=True)
    tmp = CACHE / "tmp" / asset
    tmp.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(download(asset)) as archive:
        members = {member.rsplit("_", 1)[-1].lower(): member for member in archive.namelist() if member.lower().endswith(".jpg")}
        archive.extractall(tmp)
    color = tmp / members["color.jpg"]
    normal = tmp / members["normalgl.jpg"]
    magick(str(color), "-resize", "1024x1024!", "-strip", "-quality", "80", str(out / "color.jpg"))
    magick(str(normal), "-resize", "1024x1024!", "-strip", "-quality", "70", str(out / "normal.jpg"))
    if "roughness.jpg" in members:
        magick(str(tmp / members["roughness.jpg"]), "-resize", "512x512!", "-colorspace", "Gray", "-strip", "-quality", "80", str(out / "roughness.jpg"))
    else:
        # A set without a roughness map: an even, fairly rough surface.
        magick("-size", "512x512", "xc:gray(80%)", "-quality", "80", str(out / "roughness.jpg"))
    size = sum(f.stat().st_size for f in out.iterdir()) // 1024
    print(f"{name}: {asset}, {size} KB")


def main() -> None:
    if "--credits" in sys.argv:
        for name, asset in {**HAND_MADE, **SURFACES}.items():
            print(f"| `{name}/` | [{asset}](https://ambientcg.com/view?id={asset}) |")
        return
    forced = sys.argv[sys.argv.index("--force") + 1 :] if "--force" in sys.argv else []
    for name, asset in SURFACES.items():
        if name in forced or not (OUT / name / "color.jpg").exists():
            import_surface(name, asset)


if __name__ == "__main__":
    main()
