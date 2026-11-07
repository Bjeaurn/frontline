# Jelmos' House: map spec (skeleton, awaiting your specs)

Only the name is known so far. This file holds the pipeline facts and constraints that apply to any built-in Map. Every design field is **TBD** until you supply it. Nothing here is invented.

## 1. Inputs needed from you

| # | Topic | Status |
|---|---|---|
| 1 | Setting and scale: a single house, a house plus grounds, or a neighbourhood? Interior-heavy or outdoor? Real place or fictional? | TBD |
| 2 | Public Remix Map or private Homage Map (ADR 0003)? Zeewolde-Town is public. | TBD |
| 3 | Player count (the built-in Maps target 6-10) | TBD |
| 4 | Game Modes: `tdm dom hq sd`, all four or a subset | TBD |
| 5 | Layout: symmetric (`mirrored()`) or asymmetric, number of lanes, a main focus area, recognition points | TBD |
| 6 | Look preset: `overcast_urban`, `desert_dust`, `dawn_cold`, `jungle_humid`, `green_dusk` or `night_ops`, or a new one | TBD |
| 7 | Factions: any two ids from `data/factions.ron` | TBD |
| 8 | Style, materials, props, special features (verticality, vaulting windows, snipe perches, vehicles) | TBD |
| 9 | Map id (default `jelmos_house`) and display name (default "Jelmos' House") | TBD |

## 2. Facts that apply to any map (verified in the repo)

**Format and scale.** TrenchBroom `.map` (Valve 220). 1 unit = 1 inch (39.37 per metre). Game space is metres, y up, -z forward. A standing player is 71 units (1.8 m) tall. Mantles reach 1.6 m.

**Generator.** `tools/mapgen/` (`mapgen.py`, `kit.py` with `Level`). Crossroads (`tools/mapgen/crossroads.py`) is the structure to copy. Surfaces: concrete, plaster, metal, corrugated, wood, ground, brick.

**Entities**
- `spawn_point` with `team` (alpha, bravo, any), `modes` and `angle`.
- `dom_flag` (A, B, C): exactly three, at least 25 m apart.
- `hq_site`: at least one, at least 30 m apart.
- `bomb_site` (A, B): two, at least 30 m apart.
- `ammo_crate` (Team Deathmatch only): at least 8 m apart, at least 12 m from any tdm spawn, not touching a solid.
- `prop`, `light_point` (`practical` lamps on at night looks).

**Interiors, relevant for a house**
- Walls with `door()` gaps (2.4 m by default) and `window()` gaps.
- A vaultable window has a 0.9 m sill, a 2.3 m head and is at least 1.0 m wide. Walls up to 0.6 m thick can be vaulted.
- Floor slabs with stairwell holes (`Level.slab`), ramps, `slab_ramp`, a 1.5 m mantle.

**Rules the tooling enforces**
- Walk time: Crossroads uses a spawn-to-spawn path of 110-140 m, about 22 s at 5.5 m/s. A house-scale map may need a different target.
- No Alpha spawn may see a Bravo spawn.
- No coplanar overlapping faces. `lint.py` must be clean.
- A perimeter wall flush with a model facade hides windows, so recess it 0.5 m.

**Known gaps in the kit**
- No colours beyond the 7 textures.
- The MegaKit models are American downtown. There are no furniture, kitchen or bedroom props, so a house interior would be brush blockouts (tables, beds, sofas) and `crate`, `jersey`, `dumpster` and similar helpers.

## 3. Deliverables (fixed by the pipeline)

| Item | Path |
|---|---|
| Generator | `tools/mapgen/jelmos_house.py` |
| Map | `crates/frontline/assets/maps/jelmos_house.map` (opens in TrenchBroom) |
| Registry | `data/maps.ron` entry: name, file, look, modes, factions |
| Docs | README mentions of built-in Maps |

**Acceptance (same as Zeewolde-Town).**
1. `python3 tools/mapgen/jelmos_house.py --check` prints `check: ok`.
2. `python3 tools/mapgen/lint.py crates/frontline/assets/maps/jelmos_house.map` is clean.
3. `cargo run -- --host --map jelmos_house --mode <mode>` starts for each supported mode.

## 4. Design

TBD until section 1 is filled.
