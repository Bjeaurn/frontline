# Zeewolde-Town: playtest guide and interview

Run it: `cargo run -- --host --map zeewolde_town --mode dom` (also `hq`, `sd`, `tdm`). Look from above first: `cargo run -p mapedit -- --map zeewolde_town --top`. Alpha spawns in the south-west (the south street), Bravo in the east (the Open Hof car park); switch teams from the pause menu.

## What changed since the first test (phase 2 and 3)

- The church has its **tower** (over the entrance hall), a green-mullion **glass front**, tall poles with a wire in front, and the covered walks.
- The **library** has the 5 m arcade, the dark glass box over it, the dark window bands, the first-floor loopholes and a **roof terrace**.
- **Trick-jump routes** (each step within the mantle, checked by the generator): the church roof and the tower's two steps (the perch, 10 m up), the library terrace, Cafe De Mens' roof, and the two east shops' roofs.
- **Headquarters** now jumps to a random far site instead of the next in the list.

## Things to try on purpose

1. **Recognition:** from the plaza, is the church the church (tower, glass front, poles)? From Kerkstraat, is the library the library?
2. **The climbs** (each is two crates, then ledges):
   - Church roof: the crate stack in the front lane at the covered walk's edge (about x -9.7, z 10.8), onto the walk roof, two ledges on the glass front, the roof, then the tower's two steps.
   - Library terrace: the crate stack in Marktstraat at the library's north-east corner (about x -38.6, z 1.4), then three ledges.
   - Cafe De Mens' roof from Kerkstraat (crates at about x -31.7, z 9.4); the two east shops' roofs from Open Hof (crates at x 48.4, z 11 and z 20.8).
3. **Headquarters mode** for a few minutes: watch where the HQ appears each time. Does it hop between areas and vary the spot?
4. **Search & Destroy:** the bomb sites are the church entrance hall and the east shop. Can attackers reach them in time, and defend them?
5. **The routes:** Alpha along Kerkstraat, through the alley (z 24..27), up the front lane, under the arch (z 28..32), through the library or the cafe, along the south lane. Bravo along the north lane or Raadhuisstraat, down the two east paths, through the church lane, through an east shop.

## Interview questions

**Recognition and look**
1. Does the church read as Open Haven now? What is the first thing you'd change (tower slimness, glass front, the arches, the cable-net poles)?
2. The library: does the box, arcade and glass box give you the "boekenbajes" feeling? Is the dark band pattern too busy, too faint, or fine?
3. Does the map feel Dutch? What would make it feel more like Zeewolde (colour, signage, greenery, water at the edge)?

**Flow**
4. How did the walk from your start feel (too short, too long)? It is 90 m from Alpha to Bravo, shorter than the other maps.
5. Which route did you use most, and which did nobody use? Which lane felt like a shooting gallery?
6. Were the starts safe enough (spawn camping from the lanes, the car park from the east shops or the paths)?
7. Did the market on the green Kerkplein work as the main fight: enough cover, enough ways to flank, or too crowded or too open? Did the van and the stalls behave?

**The new routes and roofs**
8. Were the roof climbs findable and fun, or frustrating? Were any too easy (a free sniper spot) or too hard? Which perch was best: the tower deck, the library terrace, the cafe roof or an east shop roof?
9. Is the church roof with no cover the right balance? Is the tower deck worth the climb?
10. Did you ever get stuck (a ledge you could not mantle off, a crate gap, a window you could not vault)? Where?

**Modes and objectives**
11. Domination: are the three Flags (market, library, east shop) well placed? Which team has the easier flags?
12. Headquarters: does the far-first rotation feel good (varied but fair)? Is 60% right, or should the HQ move farther or sometimes stay near?
13. Search & Destroy: are the church hall and the east shop good sites? Which side wins more?
14. Team Deathmatch: do the six ammo crates matter? Any dead zone with no fights?

**Performance and feel**
15. Frame rate: any dips, and where (the plaza, inside the church, looking down Kerkstraat)?
16. Lighting and fog: too dark, too foggy, too flat? Would a different look (`--look dawn_cold` or `night_ops`) suit it better?
17. Anything you wanted to do and could not (a door, a window, a vault, a route)?

## Noting findings

Write what you find in `docs/zeewolde-town/notes.md` under a "Playtest" heading, or tell me; I'll retune from there. Known gaps: no chamfered arches (the brick arches are plain openings), no medallion on the library, no cable net over the plaza, no signage, no colour accents.
