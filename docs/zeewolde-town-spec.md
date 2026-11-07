# Z-Town: map spec

A public **Remix Map** (ADR 0003): original layout in the centre of the Dutch polder town of Zeewolde, urban, 6-10 players, all four Game Modes (`tdm dom hq sd`). The layout follows the user's drawing on a Google Maps screenshot; the buildings are simplified massing, not plans.

Status: **phases 1-3 built** (blockout, the recognisable parts, the HQ rotation rule). Left: what the playtest finds, then dressing (see section 6's "not built"). The vision, interview answers and photo notes are in `docs/zeewolde-town/notes.md`; the screenshots and the numbered drawing are in `docs/zeewolde-town/`; the playtest guide is `docs/zeewolde-town/playtest.md`.

## 1. Frame and scale

- Scale: the user measured 32 m from the church entrance to the north end of the green Kerkplein, which is about 12 px per metre in the 1568-px view of the screenshots.
- The town's street grid is turned about 15.6 degrees against the screenshot. The Map is turned back, so every street and wall is axis-aligned: **x east, z south** (-z is "north" of the Map; true north is about 16 degrees off). The origin is the church's west front.
- Playfield x -66..58, z -44..54 (124 x 98 m), closed by a 14 m brick perimeter.
- Alpha to Bravo on foot: **90 m (16 s)**, a bit under the 110-140 m of the built-in Maps, because the real scale was kept. The longest walk between any two spawns is 102 m.

## 2. Layout

| Zone | Bounds (x; z) | Notes |
|---|---|---|
| **Kerkplein** | -31..-5; -33..-3 | The market square, the main fight. Seven stalls in two rows and two columns round a clear middle (Flag A), one market van, pines, light poles, planters. |
| Kerkstraat | -37..-31; -33..54 | The long shopping street on the west; 6 m wide, cars and barriers break the sightline. |
| **FlevoMeer library** | -66..-42; 3..26 | Enterable, ground and first floor, stair, loopholes ("boekenbajes" slits) towards Kerkstraat and Marktstraat, 11 m tall. Phase 2 adds the 5 m arcade and the glass box. |
| **Cafe De Mens** | -31..-16; 2..13 | Enterable, ground floor, doors on Marktstraat, Kerkstraat and the front lane. |
| Blocks A and B | -31..-16; 13..24 and 27..40 | Solid, with a 3 m alley between (z 24..27) from Kerkstraat to the front lane. |
| Front lane | -16..-6; -3..40 | Paved lane along the church's west front (the paved square of the photos). |
| **Open Haven church** | -6..24; -5..32 | Three double-height halls: entrance hall (x -6..8, z 1..11), north hall (x 8..24, z -2..11), the parish's south hall (x -6..24, z 14..32). The tower comes in phase 2. |
| Church lane (the arch) | -6..24; 11..14 | Open lane between the halls, from the front to the east path; roofed for its first 8 m. |
| Covered walks | west x -9..-6, z -5..26; north z -5..-2 | 3 m wide on columns, along the church's west and north fronts; doors open into them. |
| Arch over the lane | -16..-6; 28..32 | A gateway from block B to the church's south-west corner; the lane passes under it (3.3 m). |
| North lane | -6..46; -11..-5 | Between the church and the shops; the Insectenhotel pavilion (cover) stands on it. |
| Raadhuisstraat | -5..58; -41..-35 | The north street, with the north row of buildings on its far side. |
| Shops (Hair Flair) | 1..46; -34..-11 | Solid, with two 3 m alleys (x 14..17 and 31..34) between Raadhuisstraat and the north lane. |
| East paths | 24..29 and 32..37; -5..38 | Path 9 along the church's east side; path 8 with bushes and defilade; hedges (1.3 m) between them. |
| Car park, Open Hof, east shops | 37..46 / 46..49 / 49..58 | Bravo's start in the car park; two enterable shops on Open Hof (HQ, Flag C, bomb site B). |
| South lane, S block | -16..46; 32..38; S block -4..36, 38..54 | The south lane is Alpha's route east. The S block closes the south street so the starts don't see each other. |
| South street | -37..-4; 40..50 | Alpha's start, at Kerkstraat's south end. |

## 3. Routes

- **Alpha** (south-west) to the centre: north along Kerkstraat; through the alley between blocks A and B to the front lane; north up the front lane and under the arch; through the library or the cafe; east along the south lane to the paths.
- **Bravo** (east) to the centre: west along the north lane or Raadhuisstraat; down path 9 or path 8 (bushes); through the church (south hall west door, or the lane); through an east shop.
- The church lane and the covered walks give routes round the church. The shops' alleys connect the north to the lane.

## 4. Entities

- **Spawns:** 6 per team (`modes: "tdm dom hq sd"`), 8 free-for-all. Alpha along the south street, Bravo in the car park, all facing the church. No Alpha spawn sees a Bravo spawn (`--check` verifies).
- **Domination** (at least 25 m apart): A the market (-19,-18); B the library (-54,14); C the east shop (53,24).
- **Search & Destroy:** bomb site A the church entrance hall (1,6); B the east shop (53,22).
- **Headquarters** (five sites, at least 30 m apart): the market (-15,-30), Cafe De Mens (-23,7), the library (-56,20), the church north hall (16,3), the east shop (53,6).
- **Ammo crates (tdm):** six: the market's east side, Kerkstraat, the front lane, path 9, the north lane, the north-east bend.
- **Lights:** practical pole lamps over Kerkplein and in the streets; interior lights in the enterable buildings.

## 5. Look and surfaces

`overcast_urban`; factions `baekho` and `volkov` (swap and playtest). Surfaces: brick (streets, walls, terraces), concrete (the library, church roofs, plaza), plaster bands, wood and metal (stalls, cover), ground (hedges). There is no colour; the Maps' MegaKit buildings are not used (they read American).

## 6. Phase 2 (built)

- **Library:** the 5 m deep arcade on the Kerkstraat side with thin dark columns, the dark glass box over it (first floor, big windows), dark window bands on the upper concrete, loopholes in the concrete first floor, and the **roof terrace** (7.2 m, parapet, a gap where the climb arrives). Not built: the slanted dark roof edge and the medallion.
- **Church:** the lattice tower over the entrance hall (to about 36 m, with clocks and the boat-and-cross vane), a stepped base, the green-mullion glass front, tall poles with a wire in front. **The tower deck (10 m up) is reachable by trick-jump only.** Not built: chamfered brick arches (the arches are plain openings), the blue canopy, the cable net.
- **Roof climbs by trickery**, each step within the 1.6 m mantle and verified by `--check` (`kit.climb_problems`, the `CLIMBS` table): the church roof and the tower's two steps (ground, two crates, the covered walk's roof, two ledges on the glass front, the roof, the tower's steps: 1.2, 2.2, 3.6, 5.0, 6.3, 7.3, 8.7, 10.0 m; the roof has no cover), the library terrace (1.2, 2.2, 3.4, 4.7, 6.0, 7.2 m), Cafe De Mens' roof and the two east shops' roofs (1.2, 2.2, 3.6 m, each with a parapet and a gap where the crates arrive). There is no ladder in the game: a ladder is a chain of ledges.

## 7. Phase 3: Headquarters rotation (built; a game rule, not map data)

The next HQ site is a **random one of the sites at least 60% of the farthest site's distance from the current one**, never the same twice in a row, so the HQ moves around the Map without a fixed order (`next_site` in `crates/frontline/src/modes/hq.rs`: pure, unit-tested; the roll comes from the HQ's own seeded random generator). The 60% is `relocate_far_fraction` in `data/game_modes.ron`.

## 8. Commands and acceptance

```sh
python3 tools/mapgen/z_town.py --check                      # walkability, spawns, spacing, fences
python3 tools/mapgen/lint.py crates/frontline/assets/maps/z_town.map
cargo run -p mapedit -- --map z_town --top                  # look at it
cargo run -- --host --map z_town --mode dom                 # play it
```

Phases 1-3 pass: `check: ok` (every climb verified), lint clean, 605 brushes, the HQ rule's unit tests pass. The playtest guide and interview questions are in `docs/zeewolde-town/playtest.md`.
