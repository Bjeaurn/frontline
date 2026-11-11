# Z-Town: map spec

A public **Remix Map** (ADR 0003): original layout in the centre of a Dutch polder town, urban, 6-10 players, all four Game Modes (`tdm dom hq sd`). The layout follows the user's drawing on a Google Maps screenshot; the buildings are simplified massing, not plans.

Status: **phases 1-3 built** (blockout, the recognisable parts, the HQ rotation rule) and the **street dressing** (props, section 6). Left: what the playtest finds. The vision, interview answers and photo notes are in `docs/z-town/notes.md`; the screenshots and the numbered drawing are in `docs/z-town/`; the playtest guide is `docs/z-town/playtest.md`.

## 1. Frame and scale

- Scale: the user measured 32 m from the church entrance to the north end of the green Kerkplein, which is about 12 px per metre in the 1568-px view of the screenshots.
- The town's street grid is turned about 15.6 degrees against the screenshot. The Map is turned back, so every street and wall is axis-aligned: **x east, z south** (-z is "north" of the Map; true north is about 16 degrees off). The origin is the church's west front.
- Playfield x -66..58, z -44..54 (124 x 98 m), closed by a 14 m brick perimeter.
- Alpha to Bravo on foot: **90 m (16 s)**, a bit under the 110-140 m of the built-in Maps, because the real scale was kept. The longest walk between any two spawns is 102 m.

## 2. Layout

| Zone | Bounds (x; z) | Notes |
|---|---|---|
| **Kerkplein** | -31..-5; -33..-3 | The market square, the main fight. Six stalls round a clear middle (Flag A), one market van, pines with foliage, light poles, planters, a lawn frame. |
| Kerkstraat | -37..-31; -33..54 | The long shopping street on the west; 6 m wide, cars and barriers break the sightline. |
| **FlevoMeer library** | -66..-42; 3..26 | Enterable, ground and first floor, stair, loopholes ("boekenbajes" slits) towards Kerkstraat and Marktstraat, 11 m tall. Phase 2 adds the 5 m arcade and the glass box. |
| **Cafe De Mens** | -31..-16; 2..13 | Enterable, ground floor, doors on Marktstraat, Kerkstraat and the front lane. |
| Blocks A and B | -31..-16; 13..24 and 27..40 | Solid, with a 3 m alley between (z 24..27) from Kerkstraat to the front lane. |
| Front lane | -16..-6; -3..40 | Paved lane along the church's west front (the paved square of the photos). |
| **Open Haven church** | -6..24; -5..31 | Three double-height halls: entrance hall (x -6..8, z 1..11), north hall (x 8..24, z -2..11), the parish's smaller south hall (x -6..20, z 14..28, windows on three sides), joined by a solid block (z 11..14, no lane). A sacristy annex (x 16..24, z 28..31) closes the south-east corner. |
| Covered walks | west x -8..-6, z -4..13; north z -4..-2 | 2 m wide, green metal canopies on columns; the west one is the canopy over the entrance. No door opens into them except the entrance. |
| Arch over the lane | -16..-6; 28..32 | A gateway from block B to the church's south-west corner, 6 m wide between two piers (3.3 m high). |
| North lane | -6..46; -11..-5 | Between the church and the shops; the Insectenhotel pavilion (cover) stands on it. |
| Raadhuisstraat | -5..58; -41..-30 | The north street, about 11 m wide (9 m open plus the 2 m arcades), a grass strip with trees along the north row, planters, benches and a kiosk. |
| Shops (Hair Flair) | 1..46; -30..-11 | Arcade fronts (green metal awnings on columns), two 3 m alleys (x 14..17 and 31..34). The shops at x 1..14 and x 34..46 are enterable (street door and an alley door); the middle one is solid. |
| East paths | 24..29 and 32..37; -5..38 | Path 9 along the church's east side; path 8 with bushes and defilade; hedges (1.3 m) between them. |
| Car park, Open Hof, east shops | 37..46 / 46..49 / 49..58 | Bravo's start in the car park; two enterable shops on Open Hof (HQ, Flag C, bomb site B). |
| South lane, S block | -16..46; 32..38; S block -4..36, 38..54 | The south lane is Alpha's route east. The S block closes the south street so the starts don't see each other. |
| South street | -37..-4; 40..50 | Alpha's start, at Kerkstraat's south end. |

## 3. Routes

- **Alpha** (south-west) to the centre: north along Kerkstraat; through the alley between blocks A and B to the front lane; north up the front lane and under the arch; through the library or the cafe; east along the south lane to the paths.
- **Bravo** (east) to the centre: west along the north lane or Raadhuisstraat (under the arcades); down path 9 or path 8 (bushes); through the church (south hall east door or windows); through an east shop.
- The covered walks give routes round the church; the south hall's windows add ways through. The shops' alleys connect the north to the lane.

## 4. Entities

- **Spawns:** 6 per team (`modes: "tdm dom hq sd"`), 8 free-for-all. Alpha along the south street, Bravo in the car park, all facing the church. No Alpha spawn sees a Bravo spawn (`--check` verifies).
- **Domination** (at least 25 m apart): A the market (-19,-18); B the library (-54,14); C the east shop (53,24).
- **Search & Destroy:** bomb site A the church entrance hall (1,6); B the east shop (53,22).
- **Headquarters** (five sites, at least 30 m apart): the market (-15,-30), Cafe De Mens (-23,7), the library (-56,20), the church north hall (16,3), the east shop (53,6).
- **Ammo crates (tdm):** six: the market's east side, Kerkstraat, the front lane, path 9, the north lane, the north-east bend.
- **Lights:** practical pole lamps over Kerkplein and in the streets; interior lights in the enterable buildings.

## 5. Look and surfaces

`dawn_cold` (a `z_town_night` entry on the same file uses `night_ops`); factions `baekho` and `volkov` (swap and playtest). Surfaces: brick (streets, walls, terraces), concrete (the library, church roofs, plaza), plaster bands, wood and metal (stalls, cover, the green awnings and canopies), and the `grass` surface (ambientCG Grass001, CC0) for the plaza's lawn frame, hedges, the tree strip along Raadhuisstraat and every tree's foliage. The Maps' MegaKit buildings are not used (they read American).

## 6. Phase 2 (built)

- **Library:** the 5 m deep arcade on the Kerkstraat side with thin dark columns, the dark glass box over it (first floor, big windows), dark window bands on the upper concrete, loopholes in the concrete first floor, and the **roof terrace** (7.2 m, parapet, a gap where the climb arrives). Not built: the slanted dark roof edge and the medallion.
- **Church:** the lattice tower over the entrance hall (to about 36 m, with clocks and the boat-and-cross vane), a stepped base, the green-mullion glass front, tall poles with a wire in front. **The tower deck (10 m up) is reachable by trick-jump only.** Not built: chamfered brick arches (the arches are plain openings), the blue canopy, the cable net.
- **Roof climbs by trickery**, each step within the 1.6 m mantle and verified by `--check` (`kit.climb_problems`, the `CLIMBS` table): the church roof and the tower's two steps (ground, two crates, the covered walk's roof, two ledges on the glass front, the roof, the tower's steps: 1.2, 2.2, 3.6, 5.0, 6.3, 7.3, 8.7, 10.0 m; the roof has no cover), the library terrace (1.2, 2.2, 3.4, 4.7, 6.0, 7.2 m), Cafe De Mens' roof and the two east shops' roofs (1.2, 2.2, 3.6 m, each with a parapet and a gap where the crates arrive). There is no ladder in the game: a ladder is a chain of ledges.

**Street dressing** (`street_life` in `tools/mapgen/z_town.py`, models from `assets/models/props/`, civil on purpose: no sandbags anywhere): crates, pallets loaded with cement bags and barrels beside the market stalls; benches on Kerkplein's lawn strips, bins by its light poles and planter boxes on its south edge; chairs on the pavement outside Cafe De Mens and propane cylinders at its corner; bins and barrels in the alley between blocks A and B (2.4 m clear each way); on the blank brick of Kerkstraat, the front lane, Marktstraat, the north lane, Raadhuisstraat's north row, Open Hof, the south lane and the south street: roller shutters (graffiti ones too), utility and power boxes at the foot of the wall, air-conditioning units above head height, a generator or two; a picnic table on path 9; some of the parked cars are the tarp-covered car model, and every dumpster and jersey barrier is the prop model. Wall fittings and shutters have no collider; the solid props (bins, boxes, barrels, crates, benches, the table) carry `clip` blocks of their bounds, keep off the doorways and alley mouths and (`--check`) 0.8 m from every spawn.

## 7. Phase 3: Headquarters rotation (built; a game rule, not map data)

The next HQ site is a **random one of the sites at least 60% of the farthest site's distance from the current one**, never the same twice in a row, so the HQ moves around the Map without a fixed order (`next_site` in `crates/frontline/src/modes/hq.rs`: pure, unit-tested; the roll comes from the HQ's own seeded random generator). The 60% is `relocate_far_fraction` in `data/game_modes.ron`.

## 8. Commands and acceptance

```sh
python3 tools/mapgen/z_town.py --check                      # walkability, spawns, spacing, fences
python3 tools/mapgen/lint.py crates/frontline/assets/maps/z_town.map
cargo run -p mapedit -- --map z_town --top                  # look at it
cargo run -- --host --map z_town --mode dom                 # play it
```

Phases 1-3 pass: `check: ok` (every climb verified), lint clean, 605 brushes, the HQ rule's unit tests pass. The playtest guide and interview questions are in `docs/z-town/playtest.md`.
