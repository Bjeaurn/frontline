#!/usr/bin/env python3
"""Builds the prop models in crates/frontline/assets/models/props/ (a `prop` entity's
`models/props/<name>.glb`).

Most are CC0 models from Poly Haven (polyhaven.com): the 1k glTF is downloaded into the git-ignored
content/assets/polyhaven/, then Blender (headless, `tools/assets/blender_prop.py`) joins it into
one mesh, decimates it to the triangle budget below, stands it on the ground plane, shrinks its
textures and exports a .glb. A few are built from scratch by `tools/assets/blender_builds.py`
with the game's own surface textures (`BUILT`). `bounds.json` (what `tools/mapgen` places props
by) is rewritten at the end.

Needs Blender on PATH (`blender`); standard library otherwise.

    python3 tools/assets/import_polyhaven.py                  build every prop that is missing
    python3 tools/assets/import_polyhaven.py --force NAME...  rebuild these
    python3 tools/assets/import_polyhaven.py --preview DIR    also render a preview PNG of each built
    python3 tools/assets/import_polyhaven.py --credits        print the CREDITS.md rows
"""
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "crates/frontline/assets/models/props"
CACHE = ROOT / "content/assets/polyhaven"
BLENDER_SCRIPT = Path(__file__).with_name("blender_prop.py")
TEXTURES = ROOT / "crates/frontline/assets/textures"

# prop name -> (Poly Haven asset id, triangle budget, texture size, extra blender_prop.py options)
PROPS: dict[str, tuple[str, int, int, list[str]]] = {
    "barrel_red": ("Barrel_01", 900, 512, []),
    "barrel_blue": ("barrel_03", 800, 512, []),
    "barrel_rusty": ("barrel_stove", 1200, 512, []),
    "barrier": ("concrete_road_barrier_02", 700, 512, []),
    "barrier_long": ("concrete_road_barrier", 900, 512, []),
    "car_covered": ("covered_car", 2500, 1024, []),
    "tyre": ("old_tyre", 500, 512, []),
    "trash_can": ("metal_trash_can", 1000, 512, ["--keep", "rust"]),
    "trashbag": ("trashbag", 500, 512, []),
    "aircon": ("exterior_aircon_unit", 1200, 512, ["--keep", "rusted"]),
    "utility_box": ("utility_box_01", 900, 512, []),
    "utility_box_tall": ("utility_box_02", 900, 512, []),
    "power_box": ("power_box_01", 1000, 512, []),
    "street_lamp": ("street_lamp_01", 1400, 512, []),
    "hydrant": ("fire_hydrant", 900, 512, ["--keep", "aged"]),
    "cement_bag": ("cement_bag", 850, 512, []),
    "military_crate": ("old_military_crate", 1000, 512, ["--keep", "_a$"]),
    "wooden_crate": ("wooden_crate_01", 700, 512, []),
    "ammo_box": ("ammo_box", 600, 512, []),
    "cardboard_box": ("cardboard_box_01", 400, 512, []),
    "shutter_door": ("rollershutter_door", 600, 512, ["--drop", "graffiti"]),
    "shutter_door_graffiti": ("rollershutter_door", 600, 512, ["--keep", "graffiti"]),
    "shutter_window": ("rollershutter_window_01", 600, 512, ["--drop", "graffiti"]),
    "shutter_window_graffiti": ("rollershutter_window_01", 600, 512, ["--keep", "graffiti"]),
    "plastic_chair": ("plastic_monobloc_chair_01", 900, 512, []),
    "propane_tank": ("propane_tank", 800, 512, []),
    "jerrycan": ("metal_jerrycan_green", 700, 512, []),
    "generator": ("portable_generator", 1800, 512, []),
    "ladder": ("wooden_ladder", 700, 512, []),
    "bench": ("painted_wooden_bench", 700, 512, []),
    "picnic_table": ("wooden_picnic_table", 1200, 512, []),
    "planter": ("planter_box_01", 1000, 512, []),
}
# prop name -> generator in blender_builds.py (made from the game's own surface textures).
BUILT: dict[str, str] = {"dumpster": "dumpster", "pallet": "pallet", "sandbags": "sandbags"}


def fetch(url: str, path: Path) -> None:
    if path.exists():
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(url, headers={"User-Agent": "frontline-import-polyhaven"})
    with urllib.request.urlopen(request, timeout=300) as response:
        path.write_bytes(response.read())


def download(asset: str) -> Path:
    """The asset's 1k glTF with its buffer and textures, in the cache; returns the .gltf."""
    folder = CACHE / asset
    listing = folder / "files.json"
    fetch(f"https://api.polyhaven.com/files/{asset}", listing)
    gltf = json.loads(listing.read_text())["gltf"]["1k"]["gltf"]
    target = folder / gltf["url"].rsplit("/", 1)[-1]
    fetch(gltf["url"], target)
    for relative, info in gltf.get("include", {}).items():
        fetch(info["url"], folder / relative)
    return target


def blender(args: list[str]) -> None:
    result = subprocess.run(["blender", "-b", "--python", str(BLENDER_SCRIPT), "--", *args], capture_output=True, text=True)
    lines = [line for line in result.stdout.splitlines() if line.startswith("PROP ")]
    if result.returncode != 0 or not lines:
        sys.exit(f"blender failed:\n{result.stdout[-3000:]}\n{result.stderr[-3000:]}")
    print(lines[-1])


def write_bounds() -> None:
    sys.path.insert(0, str(Path(__file__).parent))
    import import_props

    import_props.write_bounds(OUT)


def main() -> None:
    if "--credits" in sys.argv:
        for name, (asset, *_rest) in PROPS.items():
            print(f"| `{name}.glb` | [{asset}](https://polyhaven.com/a/{asset}) | CC0 |")
        for name, build in BUILT.items():
            print(f"| `{name}.glb` | built by `tools/assets/blender_builds.py` (`{build}`) from the surface textures | CC0 |")
        return
    OUT.mkdir(parents=True, exist_ok=True)
    forced = [arg for arg in sys.argv[sys.argv.index("--force") + 1 :] if not arg.startswith("--")] if "--force" in sys.argv else []
    preview = Path(sys.argv[sys.argv.index("--preview") + 1]) if "--preview" in sys.argv else None
    if preview:
        preview.mkdir(parents=True, exist_ok=True)

    def wanted(name: str) -> bool:
        return name in forced or not (OUT / f"{name}.glb").exists()

    def extra(name: str) -> list[str]:
        return ["--preview", str(preview / f"{name}.png")] if preview else []

    for name, (asset, tris, texture, options) in PROPS.items():
        if wanted(name):
            source = download(asset)
            blender(["--source", str(source), "--out", str(OUT / f"{name}.glb"), "--tris", str(tris), "--texture", str(texture), *options, *extra(name)])
    for name, build in BUILT.items():
        if wanted(name):
            blender(["--build", build, "--out", str(OUT / f"{name}.glb"), "--textures", str(TEXTURES), *extra(name)])
    write_bounds()


if __name__ == "__main__":
    main()
