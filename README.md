# Frontline

A CoD4/MW2-inspired multiplayer FPS made for LAN parties. It's written in Rust on Bevy, lightyear and avian3d.

- Design: `IDEA.md`, `docs/PRD-lan-party.md`
- Glossary: `CONTEXT.md`
- Decisions: `docs/adr/`

## Run

One binary does everything. Launched without flags it opens the main menu: HOST GAME opens the Host setup screen (below) and then drops you in, JOIN GAME opens the Server Browser, which lists the servers on your LAN (click one) and also joins by IP (`192.168.1.20` or `192.168.1.20:5000`). If a Join fails you land back in the browser with the reason.

```sh
cargo run                                                # main menu
cargo run -- --host --name Alice                         # skip the menu and Host: server + play in this window
cargo run -- --connect 192.168.1.20:5000 --name Bob      # skip the menu and join a server
cargo run -- --dedicated --name "Bob's server"           # headless Dedicated Server
cargo run -- --dedicated --rules hardcore                # Dedicated Server running the Hardcore preset
cargo run -- --dedicated --map arena --mode sd           # Dedicated Server playing Search & Destroy on the arena
cargo run -- --dedicated --rotation rotation.ron         # Dedicated Server cycling through a Rotation file
cargo run -- --host --map arena                          # Host on a given Map (id from data/maps.ron)
cargo run -- --host --look night_ops                     # Host with another look preset (dev/test)
```

Servers listen on UDP 5000 (`--port <n>` changes it; it's also the port HOST GAME uses). Every server answers LAN discovery on UDP 5001; a second server on the same machine can't take that port, so it logs a warning and only shows up when joined by IP. `--name` sets your player name for this run (overriding Settings) or a Dedicated Server's name in the browser. `--rules <preset>` picks the Match Rules set a server (Dedicated, or Host) runs: `default` unless given, `hardcore` (30 HP, no regen, friendly fire, no crosshair, hitmarkers or damage direction, kill feed 6 s late), or one of your custom sets (Match Rules, below). An unknown name stops it at startup with the list of sets. The server sends the rule *values* to every client with the Match state, so a client needs neither the preset file nor your custom set. For faster rebuilds during development, add `--features dev` (dynamic linking).

Settings (name, mouse sensitivity, field of view, master volume) are saved to `<config dir>/frontline/settings.ron`: `~/Library/Application Support/` on macOS, `~/.config/` on Linux, `%APPDATA%` on Windows. The Profile (custom Loadouts and the chosen Loadout) is saved next to it as `profile.ron`.

`--mode <key>` picks the Game Mode of a single-entry game: `tdm` (Team Deathmatch, the default), `dom` (Domination), `hq` (Headquarters) or `sd` (Search & Destroy). The Map must support it (`modes` in `data/maps.ron`; an unsupported pair stops at startup with the list). `--rotation <file.ron>` replaces `--map`, `--mode` and `--rules` with a Rotation: a RON list of entries that the server plays in order, one per Match, then repeats, e.g.

```ron
[
    (map: "arena", mode: tdm),
    (map: "crossroads", mode: dom, rules: "hardcore"),   // rules default to "default"
]
```

`--map <id>` picks the Map a server (Dedicated, or Host) starts on: `arena` unless given. It's also the Map behind the main menu. An unknown id stops it at startup with the list of Maps. Clients load whatever Map the server names, so they need the same Map files (a client without a Homage Map leaves with an explanation).

`--look <preset>` (dev/test) replaces the Map's own look with a preset from `data/looks.ron` for that run: `overcast_urban`, `desert_dust`, `dawn_cold`, `jungle_humid` or `night_ops`. It applies to the window of a Host; a Dedicated Server accepts it but has nothing to render, and it can't be combined with `--connect`. An unknown preset stops a windowed run at startup with the list.

A shipped build needs its `assets/` folder (maps, models, fonts) next to the executable; only the surface textures and faction emblems are compiled in. Under `cargo run` the assets come from `crates/frontline/assets/`.

## Game Modes

Every Match runs one Game Mode (`data/game_modes.ron`; every number below is data). After a Match the server moves on to the next Rotation entry (changing Map and mode if the entry says so) and the end screen shows `Next: <Map> - <Mode>`.

| Mode | Key | Rules |
|---|---|---|
| Team Deathmatch | `tdm` | Each kill scores a point. 50 points or 10 min. |
| Domination | `dom` | Three Flags (A, B, C). Stand in a Flag's zone to capture it: ~10 s alone, up to 3x faster with teammates; both teams on it freezes it, and an enemy Flag is neutralised first. Each held Flag scores a point every 5 s. 200 points or 10 min. Spawns lean toward your Flags. The HUD shows A/B/C icons under the score, a progress bar while you capture, and markers over the Flags. |
| Headquarters | `hq` | One HQ is active at a time (rotating through the Map's HQ sites, ~60 s each). Capture it by standing in it (~10 s); the holders score a point per second and **don't respawn** while they hold it, so the other team must destroy it (stand in it ~10 s) before they are all dead. 250 points or 10 min. |
| Search & Destroy | `sd` | Rounds without respawns, attackers against defenders; sides swap after round 3, first to 4 rounds wins. A random attacker starts with the bomb (dropped on death, picked up by walking over it). The carrier plants it at site A or B by **holding F** for ~5 s (frozen in place), defenders defuse by holding F ~7 s, the fuse is ~45 s and the explosion kills everyone within 20 m. Defenders are never told which site holds the planted bomb (both site markers look the same all round): they find it by its positional beep, which speeds up as the fuse runs down. A round ends on elimination, detonation, defusal or the 2:30 clock (defenders win on time unless it is planted). Dead players spectate a teammate (click to switch). |

### Host setup

HOST GAME opens a setup screen: server name, max players, the Rotation, per-mode score and time limits (in Search & Destroy: rounds to win and minutes per round), then START. The Rotation is a table under the column headers MAP / MODE / RULES: each entry is a row and each cell a picker that shows its current value between ◀ ▶ arrows (a Map that lacks the entry's Game Mode brings its first one); `^` `v` reorder a row and `X` removes it; ADD ENTRY copies the last entry (up to 8). MATCH RULES opens the rules editor and returns to the same setup. Score limits step by 5 points (Search & Destroy: whole rounds), time limits by a minute. Your choices are saved with Settings. `--host` skips the screen and plays the `--map`/`--mode`/`--rules` entry.

### Match Rules

The built-in presets (`default`, `hardcore`, from `data/match_rules.ron`) are read-only starting points. The rules editor (SETTINGS > CUSTOM RULE SETS, or MATCH RULES on the Host setup screen) lists them with your own sets: select one, type a name (or leave it empty for `<set> copy`) and COPY SELECTED makes a custom set, which you edit field by field: max health, health regen on/off with its delay and rate, respawn delay and the earliest early respawn, damage scale, friendly fire, where fall damage starts and where it is lethal, every HUD toggle (crosshair, hitmarkers, kill hitmarker, kill feed, damage direction, name tags) and the kill feed delay. DONE saves your sets to `<config dir>/frontline/rules.ron`; BACK (or Esc) discards the edits. Custom sets then appear in the Rotation's RULES column next to the presets. A Match's rule values travel to the clients in `MatchState`, so only the Host needs the file; a Dedicated Server reads its own `rules.ron`.

## Controls

On joining, pick a Team (and then a Loadout). The mouse is grabbed while you play and freed whenever a menu is open; left-click grabs it again if the window lost focus.

| Action | Key |
|---|---|
| Move | WASD |
| Sprint | Left Shift (forward only, limited stamina) |
| Jump / stand up | Space |
| Mantle | Move into a ledge up to ~1.6 m (jumping helps); steps and ramps (stairs) up to 0.45 m per step are simply walked up at full speed |
| Crouch | C |
| Prone (while sprinting: dive) | Left Ctrl or Z |
| Fire (full auto, or one round per click for semi/bolt-action) | Left mouse |
| Aim down sights | Right mouse (hold) |
| Reload | R (sprinting cancels it). Takes longer from an empty magazine (AR 2.5 / 3.1 s, SMG 2.4 / 2.9, Sniper 3.5 / 4.0, Pistol 1.7 / 2.1; Sleight of Hand x0.7) |
| Scoreboard | Tab (hold) |
| Plant / defuse the bomb (Search & Destroy; hold) | F |
| Switch Team | T (switching kills you, no credit to anyone, and the death screen shows until you respawn on the new Team's spawns; in Search & Destroy you wait for the next round) |
| Change Loadout (applies on next spawn, or at once within 5 s of spawning) | B |
| Primary / Secondary (lowers one and raises the other, about 0.4 s) | 1 / 2, or the mouse wheel |
| Respawn early (dead, once `min_respawn_secs` have passed; the death screen says when) | Space or click |
| Pause menu (Resume, Change Team, Change Loadout, Respawn, Settings, Leave Game) | Esc; Esc again steps back. RESPAWN is for getting unstuck: it kills you with no credit (a death in your stats, "X respawned" in the kill feed) and respawns you as usual; not in Search & Destroy |

Fall damage (CoD style, from the peak of the fall): none below `min_height` (4.5 m by default, 3.5 m in hardcore), then linear up to a lethal fall at `max_height` (12 m, 8 m in hardcore). The kill feed says "X fell to their death". The Tab Scoreboard's header shows the win condition, e.g. `FIRST TO 50 · 10:00 LIMIT` (Search & Destroy: `FIRST TO 4 ROUNDS`), with the Host's limit overrides applied.

Tuning lives in `data/`: `weapons.ron` (including per-Weapon Hit Zone damage multipliers), `attachments.ron` (Optics and Muzzles as stat modifiers), `perks.ron` (the six Perks and the ammo pack), `loadouts.ron` (Default Loadouts, the instant-swap window and the Weapon switch time), `movement.ron`, `match_rules.ron` (presets: health, regen, respawn delay and the earliest early respawn, damage scale, friendly fire, fall damage and the HUD block), `game_modes.ron` (score and time limits, capture, HQ and bomb tuning of every Game Mode), `factions.ron`, `maps.ron` (every built-in Map: file, look, Game Modes, and the two Factions its theater plays), `looks.ron` (per-Map look presets: grade, fog, sun, ambient, sky), `presentation.ron` (how long bodies stay and how many at once) and `progression.ron` (XP awards, the Level curve, unlocks and camo Challenges). The game reads them at startup from `./data/` (relative to where you launch it), so edit and restart to retune. Without that folder it uses the copies built into the binary. Both machines should use the same files: damage and score are decided on the server, but movement, recoil, spread and fire rate come from each client's own copy.

## Loadouts

A Loadout is a Primary (AR, SMG or Sniper) with an Optic and a Muzzle Attachment, a Secondary (the M9 pistol), one Perk per Perk Slot and a camo for the Primary (`none`, or one of the camos under Progression). It applies on your next spawn, or at once within 5 s of spawning. The Loadout screen (B, or Change Loadout in the pause menu) lists three read-only Default Loadouts and the three custom ones in your Profile; CHOOSE picks one.

**Create a Class** (button on the Loadout screen) edits one of the three custom slots: pick the slot, type a name, then cycle the Primary, its camo, Optic, Muzzle, Secondary and Perks 1 to 3 with the arrows. Everything is listed, but what your Level or a camo Challenge hasn't unlocked is greyed with what it takes (`Unlocks at Level 4`, `Locked: 34/50 kills`), and SAVE refuses a class that holds any of it. The stat bars follow the Attachments (an Optic or Muzzle the Primary can't take, like an Optic on the Sniper's built-in scope, shows as unavailable). SAVE writes the class into the Profile and returns to the Loadout screen on it; BACK or Esc discards.

The Profile is a client-local file, `<config dir>/frontline/profile.ron`, next to `settings.ron`: the custom classes, the chosen Loadout, your XP (the Level follows from it) and kills and headshot kills per Weapon for the camo Challenges. The first run starts with the Default Loadout your old Settings had chosen. It follows you to any server you join.

| Slot | Choices |
|---|---|
| Optic | none (iron sights), Red Dot, ACOG (about 4x zoom, slower to aim, swayier; not on the Sniper) |
| Muzzle | none, Silencer (no muzzle flash, a quieter and duller shot, damage falls off at 0.75x the distance) |
| Perk 1 | Sleight of Hand (reload 30% faster), Scavenger (walk over a dead player's ammo pack to refill ammo) |
| Perk 2 | Stopping Power (+25% bullet damage), Lightweight (+10% move speed) |
| Perk 3 | Marathon (sprint 2.5x as long), Ninja (your footsteps make no sound) |

Other players' footsteps are heard as a quiet thud while they walk or run; crouched and prone players, and Ninja, are silent. The server applies the Attachments' falloff and the Perks' damage; clients tell it their Attachments with every shot and their Perks when they spawn (clients are trusted on a LAN, docs/adr/0002). Fire Pings and the Minimap don't exist yet, so the Silencer only changes the flash, the sound and the damage range.

## Progression

The server awards XP for what you do and sends it to you as an `XpAward` (amount and reason); your client adds it to your Profile and saves it (clients are trusted, docs/adr/0002), so your Level follows you to any server. Every number is in `data/progression.ron`; a Host's numbers apply to everyone on it.

| Event | XP |
|---|---|
| Kill | 100 |
| Headshot (on top of the kill) | +50 |
| Assist (damage you dealt to an enemy someone else killed, this life; under 10 HP earns none) | 1 per HP of damage |
| Multikill (kills within 4 s of your previous one) | double +50, triple +100, quad +150, then +200 |
| Flag capture (everyone on the Flag, Domination) | 150 |
| Flag-defend kill (killing an enemy in a Flag zone your team holds) | +50 |
| HQ capture or destroy (everyone on it, Headquarters) | 150 |
| Bomb plant or defuse (Search & Destroy) | 200 |
| Match played / Match won | 200 / +500 |

Level n+1 needs `n * 1500` XP in all (Level 2 at 1,500, Level 3 at 3,000, ...), up to Level 30 for now. The HUD shows a `+100 KILL` popup stack under the crosshair for each award (it fades after about two seconds), a small LEVEL UP toast (the result banner at half size, naming what the Level unlocked) and a CAMO UNLOCKED toast. The end-of-Match Scoreboard ends with your XP summary: XP earned, your Level (`LEVEL 2 > 3  LEVEL UP` when it rose) and a progress bar. The Loadout screen shows your Level and XP.

**Unlocks** are a table in `progression.ron` (anything not listed is there from Level 1, and the Default Loadouts are always usable):

| Level | Unlocks |
|---|---|
| 1 | AR, SMG, Sniper, Pistol, Red Dot, Sleight of Hand, Stopping Power, Marathon |
| 2 | Custom classes (editing and choosing the three custom slots) |
| 3 | ACOG |
| 4 | Silencer |
| 5 / 6 / 8 | Scavenger / Lightweight / Ninja (the second Perks) |

**Camos** are material variants of the Primary's body parts in the first-person viewmodel (barrel, sights and bolt stay bare; the third-person gun comes later with remote players' guns). Six ship: Woodland, Desert, Urban (grey digital), Tiger (stripes), Arctic and a metallic Gold. Each Weapon unlocks them separately by a **Challenge** counted in your Profile on that Weapon: Woodland 25 kills, Desert 50 kills, Urban 25 headshot kills, Tiger 100 kills, Arctic 50 headshot kills, and Gold once the other five are unlocked on it. The editor's Camo picker shows the progress of a locked one (`34/50 kills`). The textures are 128 px tiling PNGs under `crates/frontline/assets/camos/` (embedded in the binary), made by `tools/camos/camos.py`:

```sh
python3 -m venv /tmp/camo && /tmp/camo/bin/pip install Pillow
/tmp/camo/bin/python tools/camos/camos.py                    # all six, into assets/camos/
/tmp/camo/bin/python tools/camos/camos.py --sheet /tmp/camos.png   # plus a contact sheet
```

A camo is a `progression.ron` entry (id, name, texture file, editor swatch, optional `metallic` and `roughness`, and its challenge: `Kills(n)`, `Headshots(n)` or `AllOthers`); a new texture also goes into the list in `src/assets.rs`.

## Maps

Maps are TrenchBroom `.map` files (Valve 220 format), loaded with bevy_trenchbroom. 1 TrenchBroom unit is 1 inch, as in Call of Duty's own map tools, so 39.37 units make a metre (a standing player is 71 units tall, mantles reach 63, the prone gap is 24).

- Built-in Maps live in `crates/frontline/assets/maps/` and are listed in `data/maps.ron`.
- Homage Maps live in the private content folder (below): `content/private/maps/*.map`, loaded through the `private://` asset source. Every `.map` there is playable by its file name (`--map vacant` for `maps/vacant.map`); `content/private/maps.ron`, in the same format as `data/maps.ron` with files relative to `content/private/`, adds names, looks, Game Modes and Factions. Without the folder there are simply no Homage Maps. Like `data/`, `content/private/` is found relative to where you launch the game.

### Editing in TrenchBroom

1. Install [TrenchBroom](https://trenchbroom.github.io/) and start it once, so its user folder exists.
2. From the repository root, write Frontline's game config into TrenchBroom: `cargo run -- --write-trenchbroom-config` (or set `FRONTLINE_WRITE_TRENCHBROOM_CONFIG=1`). bevy_trenchbroom writes `GameConfig.cfg` and `frontline.fgd` (every entity class below) into TrenchBroom's `games/frontline/` folder and sets the game path in TrenchBroom's preferences to the current directory, with `crates/frontline/assets/` as its search path. Run it again whenever the entity classes change. Normal launches never touch TrenchBroom's files.
3. In TrenchBroom, open or create a map for the game "Frontline" in the Valve format.

Textures come from `crates/frontline/assets/textures/<surface>/color.jpg` (concrete, plaster, metal, corrugated, wood, ground, brick). A face's texture picks the in-game surface by its folder name, and the game draws it with that surface's full material (normal and roughness maps, palette tint). The texture scale is real-world: a scale of 0.0846 on concrete repeats it every 2.2 m, as `tools/mapgen` sets it.

Brushes in worldspawn or any brush entity (`func_group`, `func_generic`) become solid, bullet-stopping geometry, one convex collider per brush. Entities:

| Class | Properties | Purpose |
|---|---|---|
| `worldspawn` | `name`, `look` (from `data/looks.ron`) | Map settings; `data/maps.ron` may override the look |
| `spawn_point` | `team` (alpha, bravo, any), `modes` (Game Mode keys `tdm dom hq sd`, empty for all), `angle` | Where players spawn: origin at the feet, `angle` the facing |
| `dom_flag` | `label` (A, B, C) | Domination Flag |
| `hq_site` | | Headquarters site |
| `bomb_site` | `label` (A, B) | Search & Destroy bomb site |
| `prop` | `model` (glTF asset path), `collider` (none, box, convex) | A model, e.g. `models/downtown/Prop_Bollard.glb`; `box` collides as one box around it, `convex` as a hull per mesh. Homage Maps point at their own copy, `private://models/...` |
| `light_point`, `light_spot`, `light_directional` | Bevy light settings | Extra lights (the sun, sky and fog come from the look). A `light_point` with `targetname` `practical` is a lamp: its intensity is scaled by the look's `practical_lights` (night looks switch it on, day looks leave it dark) |

### Generated maps

`tools/mapgen/` is a small Python 3 library (no dependencies) that writes Valve 220 `.map` text from layouts in game space (metres, y up): boxes, turned boxes, ramps (`wedge`) and entities. `kit.py` builds on it with a `Level` (walls with door and window gaps, slabs with stairwell holes, ramps, vehicle and barrier blockouts, `prop` entities, optional 180-degree mirroring, and `shifted(dx, dz)` to move a part of a layout apart from the rest) and a walkability checker (`--check`) that flood-fills the floor on foot to find unreachable objectives, trapped spots and closed-off pockets without launching the game. The same check measures the playable extents and the longest spawn-to-spawn walk (shortest on-foot path of each Alpha-Bravo pair, longest pair; at 5.5 m/s a 22 s crossing is ~120 m), fails when that is outside the size a Map is meant to have, and flags floating geometry (brushes not attached to the ground, props with nothing under their base). Writing a Map also fails when objectives sit too close: HQ sites and bomb sites 30 m apart, Domination Flags 25 m. Maps are spread out rather than scaled up (players stay real size): buildings and props keep their size, the gaps and yards between them grow, and the new space gets cover. Walk lengths now: Arena 115 m, Crossroads 130 m, Backlot 119 m, Vacant 112 m. `arena.py` describes the arena and `crossroads.py` the Remix Map Crossroads (an original urban layout: three lanes, a two-storey building in the middle, ~22 s to cross); regenerate them with:

```sh
python3 tools/mapgen/arena.py                # writes crates/frontline/assets/maps/arena.map
python3 tools/mapgen/crossroads.py --check   # writes crossroads.map, then checks it can be walked
```

Generated maps stay editable in TrenchBroom, but regenerating overwrites hand edits, so pick one source per map.

### Props

Props are glTF models from the CC0 Downtown City MegaKit (Quaternius), processed into one `.glb` each (textures at most 1024 px, shared ones in `models/downtown/textures/`) under `crates/frontline/assets/models/downtown/`; credits in `assets/models/CREDITS.md`. The raw pack stays in the git-ignored `content/assets/`. `tools/assets/import_props.py` picks the pieces out of the zip, writes each pack's `bounds.json` (the model bounds the generators place by: `Level.prop(..., y)` puts a model's *base* at `y`, wherever its origin is; `--bounds-only` rewrites it from the `.glb` files already there), and with `--from-maps` imports exactly the ones the maps use:

```sh
python3 tools/assets/import_props.py --zip "content/assets/Downtown City MegaKit[Standard].zip" \
    --pack downtown --from-maps crates/frontline/assets/maps/*.map --clean
```

The kit has no vehicles, barriers or lamp posts, so the generators build those as brushes. Homage Maps keep their own copy in `content/private/models/` (`--prefix private://models --out content/private/models`), so the public repo carries nothing selected for them.

## Layout

- `crates/frontline/src/flow.rs`: screen states (main menu, in game, overlays), flow commands, Settings file
- `crates/frontline/src/menus/`: title, Host setup, Server Browser, Connecting, Pause, Settings and Match Rules editor screens
- `crates/frontline/src/ui/`: shared menu look: fonts, palette, buttons, panels, sliders, text fields
- `crates/frontline/src/match_hud/`: team select (Faction cards), top-centre score with Faction emblems, the objective HUD, Tab Scoreboard and end screen, friendly name tags
- `crates/frontline/src/combat/`: server-side shots, per-Hit-Zone hitboxes, damage, health/regen, death and respawn, spawn selection, the Match flow (Rotation, scores, end) and the Bevy systems of the objective modes (`mode_systems.rs`)
- `crates/frontline/src/modes/`: pure, unit-tested rules of Domination, Headquarters and Search & Destroy (capture, scoring, rounds), the replicated `ModeView`, and the client visuals (zone rings, markers, explosion, spectating)
- `crates/frontline/src/rotation.rs`: Rotation entries, the server's `ServerConfig` and the Host setup that Settings remembers
- `crates/frontline/src/rules.rs`: Custom Match Rules: the `rules.ron` file, validation, naming, merging the custom sets into `GameData` (unit-tested)
- `crates/frontline/src/data.rs`: loads the `data/*.ron` tuning files
- `crates/frontline/src/net/`: lightyear protocol, server, client, Host/Join/Leave sessions, LAN discovery
- `crates/frontline/src/player/`: first-person controller, input bindings, damage/kill-feed/death HUD, remote player visuals and their bodies
- `crates/frontline/src/weapon/`: local Weapon (firing, ammo, aiming, recoil, bolt cycle), per-weapon viewmodels, shot effects, crosshair/hitmarker/ammo/scope HUD
- `crates/frontline/src/gear.rs`: Attachments, Perks and the Loadout, and the pure rules that turn them into effective Weapon stats and Perk effects (unit-tested)
- `crates/frontline/src/profile.rs`: the Profile file (`profile.ron`): custom Loadouts, the chosen one, XP and per-Weapon kill counters, and applying an award to them
- `crates/frontline/src/progression/`: XP values, the Level curve, the unlock table and camo Challenges (pure, unit-tested); client-side award handling, and the XP popups and toasts
- `crates/frontline/src/combat/xp.rs`: server XP awarding, the per-life damage ledger (Assists) and the Multikill chain
- `crates/frontline/src/loadout/`: the Loadout screen, the Create a Class editor, stat bars, the CoD swap rule
- `crates/frontline/src/maps/`: Map registry (`data/maps.ron`, private Maps), TrenchBroom loading (units, textures, colliders, entity classes), loading the Match's Map and swapping it (`ChangeMap`), spawn points
- `crates/frontline/src/world.rs`: physics
- `crates/frontline/src/materials.rs`: material library (one tinted material per `SurfaceKind` texture set)
- `crates/frontline/src/atmosphere.rs`: camera look (tonemapping, bloom, SSAO) and per-Map look presets (`data/looks.ron`)
- `crates/frontline/src/assets.rs`: files embedded in the binary (faction emblems, camo textures, surface textures; CC0 credits in `assets/textures/CREDITS.md`)
- `tools/mapgen/`: Python generator for `.map` files (the arena, Crossroads) and its building kit
- `tools/camos/`: generates the weapon camo textures (`camos.py`, Pillow only)
- `tools/assets/`: imports prop models from raw asset packs (`import_props.py`, also their `bounds.json`)
- `crates/frontline/src/shared.rs`: pose and stance types shared by netcode and gameplay

Private content such as Homage Maps lives in the separate private repo `Bjeaurn/frontline-private`, cloned into the git-ignored `content/private/` (see `docs/adr/0003`):

```sh
git clone https://github.com/Bjeaurn/frontline-private.git content/private
```
