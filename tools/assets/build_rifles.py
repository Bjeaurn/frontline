"""Builds the first-person M4 and M16A4 models (`m4.glb`, `m16.glb`) in
crates/frontline/assets/models/weapons/ by running Blender headless on blender_rifles.py:

    python3 tools/assets/build_rifles.py [--blender /path/to/blender] [--out DIR] [--only m4|m16]

The Styloo pack (the other guns, see import_weapons.py) has no AR-15 family rifle, so these two are
original geometry written in blender_rifles.py (bevelled, smooth-shaded receivers, rails, barrels,
curved magazine; a few PBR materials, no textures): the M4 is a collapsed-stock carbine with a
quad-rail handguard, flat-top rail and a flip-up rear sight; the M16A4 is the long rifle with a
fixed stock, a fluted round full-length handguard and the removable carry handle.

The node names are what `rig.rs` refers to (magazine, trigger, the sights that go when an Optic is
fitted); `rig.rs` has a test that every name it uses is in the file. The model is written turned
to the pack's convention (authored along +X, +Y up), which `GunRig::model_transform` turns back.
blender_rifles_preview.py renders contact sheets and the rig-point overlay of the result.
Needs Blender 4.2 or newer (developed on 5.2); Python 3 standard library only here.
"""

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
OUT = REPO / "crates/frontline/assets/models/weapons"
MAC_APP = "/Applications/Blender.app/Contents/MacOS/Blender"


def find_blender(given: str | None) -> str:
    for candidate in (given, os.environ.get("BLENDER"), shutil.which("blender"), MAC_APP):
        if candidate and Path(candidate).exists() or candidate and shutil.which(candidate):
            return candidate
    sys.exit("Blender not found: pass --blender, set BLENDER or put blender on PATH")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--blender")
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument("--only", choices=("m4", "m16"))
    args = parser.parse_args()
    command = [find_blender(args.blender), "-b", "--python", str(HERE / "blender_rifles.py"), "--", "--out", str(args.out)]
    if args.only:
        command += ["--only", args.only]
    result = subprocess.run(command, capture_output=True, text=True)
    for line in result.stdout.splitlines():
        if line.startswith("RIFLE "):
            print(line[len("RIFLE ") :])
    if result.returncode != 0:
        sys.exit(result.stdout + result.stderr)


if __name__ == "__main__":
    main()
