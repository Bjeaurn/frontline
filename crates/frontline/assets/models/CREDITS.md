# Prop models

## `downtown/`

Pieces of the **Downtown City MegaKit [Standard]** by [Quaternius](https://quaternius.com),
licensed [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/) (public domain; no
attribution required, credited anyway): facades, complete buildings, street tiles, road decals and
street furniture.

Only the pieces the built-in Maps use are here. `tools/assets/import_props.py` made each `.glb`
from the kit's glTF files: one file per piece with its geometry in the binary chunk, every texture
downscaled to at most 1024 px and re-encoded as JPEG (PNG where it has transparency). The kit's few
textures are shared by all pieces and live in `downtown/textures/`.

## `weapons/`

First-person guns and grenades from the **Styloo Guns Asset Pack GLTF FBX V1.1** by
[Styloo](https://styloo.itch.io/), from the user's own copy (`content/assets/`, never committed).
**Licence: not verified (user-owned pack)**: the pack ships no licence file; the project owner
chose to use it. Embedded in the binary (`src/assets.rs`), used by the viewmodel (`src/weapon/viewmodel.rs`,
`src/weapon/rig.rs`) and, for the guns, by remote players' held weapons.

| File here | Original file (Normal version, GLB) | Used for |
| --- | --- | --- |
| `m4.glb` | (built by `tools/assets/build_rifles.py`, see below) | M4 (Carbine), and the rifle remote players carry |
| `m16.glb` | (built by `tools/assets/build_rifles.py`, see below) | M16A4 (Rifle) |
| `mac10.glb` | `mac10.glb` | MP5 (Smg) |
| `awp.glb` | `awp.glb` | M40 (Sniper) |
| `pew.glb` | `pew.glb` | M9 (Pistol) |
| `nade_low.glb` | `nade_low.glb` | Frag Grenade (and the launcher's 40mm round, smaller) |
| `flashbang_low.glb` | `flashbang_low.glb` | Flashbang |
| `smoke_low.glb` | `smoke_low.glb` | Smoke Grenade |

`tools/assets/import_weapons.py` made them: one `.glb` per piece with the textures embedded, at most
1024 px and re-encoded as JPEG, the colour map graded duller and darker (the pack's own colours are
cartoon bright) and the material given some metal and gloss. The arms, gloves and sleeves are
procedural meshes (no asset).

### The M4 and M16A4 (`m4.glb`, `m16.glb`)

The pack has no AR-15 rifle (its only rifle, `ak47`, read as an AK: curved magazine, wood, slab
receiver, and was used for both until a playtest said so), and none of the user's other packs
(FPS Lite, LowPoly FPS Lite, NiceLabs) hold a gun. So these two are **original geometry, no third
party asset in them**: `tools/assets/build_rifles.py` writes them from boxes, lofts and cylinders
(no texture, no input file; four flat-coloured materials). Same licence as the rest of the project;
nothing from the Styloo pack is in them. The M4 is a collapsed-stock carbine with a quad-rail handguard, flat-top rail,
STANAG magazine and flip-up iron sights; the M16A4 has the fixed A2 stock, a round full-length
handguard, the removable carry handle and the long barrel.

## Soldiers

Remote players and their bodies are procedural: `src/player/soldier.rs` builds them at startup from
primitives (no model file, no texture), dressed by `look` in `data/factions.ron`. None of the CC0
character packs (Quaternius, Kenney) or the user's packs hold a military character we could use
(they are fantasy, cartoon or modular untextured figures, and the user's packs only have guns and
props). The rifle a soldier carries is the first-person `m4.glb` above.
