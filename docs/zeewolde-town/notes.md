# Z-Town: vision notes (interview in progress)

Collected from the reference images and the interview. The spec (`../zeewolde-town-spec.md`) is rewritten from these once the interview is done. **The reference images (the Google Maps screenshots, the numbered drawing, the church photos) are kept locally and not committed: the repo is public, and they are Google's imagery and personal photos.** They sat in this folder as `overlay-gmaps-*.png`, `overlay-gmaps-lines-numbered.svg` and `church*.HEIC`.

## Decided

- Straight black lines on `overlay-gmaps-lines.png` are **main walking routes**, not map edges.
- **Crop to the core**: the church, Kerkplein and the lanes next to them at real size, not the whole drawn area (about 190 x 250 m by eye).
- Church entrance and tower face **NW onto Kerkplein**.
- No "OPEN HAVEN" lettering.
- Performance and a working map come before looks: recognisable vibe, not accurate models.

## Reading of the numbered lines (`overlay-gmaps-lines-numbered.svg`), unconfirmed

| # | Reading |
|---|---|
| 1 | Route: Raadhuisstraat, north |
| 2 | Route: east side |
| 3 | Route: curve at the Open Hof bend, NE |
| 4 | Route: north side of the church |
| 5 | Building outline, west side of the church (freehand) |
| 6 | ? (long curve SW of the church) |
| 7 | Route: Kerkstraat, west |
| 8 | Walking path east of the church, with defilade and bushes |
| 9 | ? (straight diagonal, east: a second east path, or "the diagonal") |
| 10 | Route: south bend |
| 11 | South edge or route |
| T | Tower base (dark ellipse) |

The user also said: two paths in the east part, one with defilade and bushes, the other through the car park (a possible spawn, as the Open Hof street suggests).

## Reference photos (copyright Fleurbaaij Kunst & Cultuur: for looking at, not to be committed)

FlevoMeer Bibliotheek, Kerkstraat 2, Koen van Velsen, 1989 (flevolanderfgoed.nl, "Bibliotheek" under Zeewolde):

- A large pale concrete box, about three storeys, with a regular grid of small square window holes: vertical rows on the front and back, horizontal rows on the long sides. The grey panels hang above a strip of glass, so the box looks lifted off the ground.
- At the entrance end a **glass box on thin steel legs** sticks out over a **5 m deep arcade** (the other shops have 2 m).
- A dark, slanted timber roof edge overhangs the entrance end, with a steel ladder up the glass box and a skylight on the roof terrace.
- A round concrete medallion on a side facade, and a curved (barrel) roof showing at the back.
- Nicknamed the "boekenbajes" (book jail) for its small windows.

### Plan: a cheap version that reads the same

Goal: recognisable from the plaza at a glance, few brushes.

| Feature | Build | Cost |
|---|---|---|
| Concrete box | one block of `concrete` (pale), about 40 x 18 x 11 m | 1 brush |
| Window grid | **dark strips, not holes**: a few thin `metal` bands standing 5 cm proud of the wall, horizontal on the long sides, vertical on the ends | about 8 brushes per facade |
| Loopholes | a handful of real 0.5 m slits at 1.5 m (shots pass, a body can't): the "boekenbajes" joke, and sniper ports | few |
| Glass box on legs | a dark `metal` box about 2.6 m tall cantilevered over the arcade on 4-6 thin legs | about 8 brushes |
| 5 m arcade | open ground under the box: columns, cover, a walkway along the front | few |
| Dark roof edge | a `wedge` ramp brush as the slanted overhang at the entrance end | 1 brush |
| Medallion | an 8-sided flat prism on a side facade, or skipped | 1 brush or none |
| Ground floor | enterable: a door and a stair to a first floor with slit windows | rooms as before |

About 60-100 brushes, no props and no extra lights beyond a few interior ones. It replaces the earlier "Combinatiegebouw" as the enterable west-lane building (sniping, HQ or bomb site), since the library really stands on Kerkstraat at the Marktstraat corner. Open question: whether it keeps that role (see the chat).

## Interview answers, round 2

- **Library** replaces the Combinatiegebouw, and the whole layout is rebuilt around the drawn core and the library (the earlier test layout felt wrong).
- **Line 6**: an open pedestrian lane with some cover and **a covered arch** (as in the photos), not a roofed walkway. Roof climbing by "climbing trickery" is wanted if it can be found: chains of steps within the 1.6 m mantle (the existing `--check` verifies such routes).
- **Line 9**: a second path in the east part.
- **Alpha** starts **south-west along the shops (Kerkstraat end)**, with several routes to the centre.
- **Bravo** starts **east among the shops / the Open Hof car park** (the car park may hold respawns), with a similar number of routes, including **through a building or two** and sniper spots. Whether the **tower** is a sniper spot is an open question.
- **Arches** pass through to the back of the church (plaza to the east paths).
- **Market stalls and the one van** go on **the green Kerkplein itself**.
- **Church interior**: the entrance hall and one wing enterable.

## Interview answers, round 3

- **Scale**: the user will measure a real distance in Google Maps; until then the crop is unsized. Alpha (SW, Kerkstraat end) to Bravo (east, Open Hof) is about 215 m by my rough scale, far over the 110-140 m target, so the crop and/or the compression are decided once the measurement is in.
- **Crop**: medium: the core plus Raadhuisstraat to the north and the south shops.
- **Tower**: reachable by **trick-jump only**; not a stair, not a landmark only.
- **Climbable roofs by trickery**: the library roof terrace (via the ladder on the glass box), the shop roofs on Kerkstraat, the shop roofs in the east. Not the church wing roof.
- **Line 6 covered arch**: at the SW corner, between the church and the Geldmaat.
- **Bravo's "through a building"**: no building picked yet (the question was left empty).

## Interview answers, round 4

- **Bravo's "through a building"**: left to me: I pick one or two east buildings from the map (proposal: the RK parish wing and a shop on Open Hof) and show them.
- **Domination Flags**: the market on the green Kerkplein, the library (ground floor or arcade), the east shops / Open Hof car park.
- **Search & Destroy bomb sites**: the church entrance hall or wing, and an east shop.
- **Headquarters candidates**: the library, the church wing, an east shop, Grand cafe De Mens, and the marketplace. The game wants HQ sites at least 30 m apart; the library, the cafe and the market are close together, so some of these may have to merge or move.

## Interview answers, round 5

- **Measurement**: 32 m from the church entrance to the end (north) of the marketplace. My reading: entrance pin (about x 860, y 640 in the 1568-px view) to the north end of the green Kerkplein (about x 700, y 290) is about 385 px, so about 12 px/m, which would make the drawn area about 80 x 104 m. To be confirmed.
- **HQ rotation** (a game-rule change in `crates/frontline/src/modes/hq.rs`, not map data): today the next site is `(site + 1) % sites.len()`, in file order. Wanted: prefer a site far from the current one, picking randomly among the far ones, so the HQ moves around an area without always landing on the same spot.
- **Church wing roof**: not climbable, unless `--check` verifies a trick-jump route, or there is an interior ladder up with no cover on the roof. The game has no ladder: a ladder would be a short chain of ledges (each within the 1.5 m mantle).

## Built since (phase 2 and 3)

- Church: tower over the entrance hall (trick-jump deck at 10 m), glass front, poles; library: arcade, glass box, window bands, loopholes, roof terrace; trick-jump roofs on the library, Cafe De Mens and the two east shops, all verified by `--check`.
- Headquarters: the next site is a random one of the far sites (60% of the farthest distance, never the same twice), `next_site` in `modes/hq.rs`.
- Playtest guide and interview questions: `playtest.md` in this folder.
