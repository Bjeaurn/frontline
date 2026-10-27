# Frontline

A CoD4/MW2-inspired multiplayer FPS made for LAN parties. It's written in Rust on Bevy, lightyear and avian3d.

- Design: `IDEA.md`, `docs/PRD-lan-party.md`
- Glossary: `CONTEXT.md`
- Decisions: `docs/adr/`

## Run

One binary does everything. Launched without flags it opens the main menu: HOST GAME starts a server and drops you in, JOIN GAME opens the Server Browser, which lists the servers on your LAN (click one) and also joins by IP (`192.168.1.20` or `192.168.1.20:5000`). If a Join fails you land back in the browser with the reason.

```sh
cargo run                                                # main menu
cargo run -- --host --name Alice                         # skip the menu and Host: server + play in this window
cargo run -- --connect 192.168.1.20:5000 --name Bob      # skip the menu and join a server
cargo run -- --dedicated --name "Bob's server"           # headless Dedicated Server
cargo run -- --dedicated --rules hardcore                # Dedicated Server running the Hardcore preset
cargo run -- --host --map arena                          # Host on a given Map (id from data/maps.ron)
```

Servers listen on UDP 5000 (`--port <n>` changes it; it's also the port HOST GAME uses). Every server answers LAN discovery on UDP 5001; a second server on the same machine can't take that port, so it logs a warning and only shows up when joined by IP. `--name` sets your player name for this run (overriding Settings) or a Dedicated Server's name in the browser. `--rules <preset>` picks the Match Rules preset a server (Dedicated, or Host) runs, from `data/match_rules.ron`: `default` unless given, or `hardcore` (30 HP, no regen, friendly fire, no crosshair, hitmarkers or damage direction, kill feed 6 s late). An unknown name stops it at startup with the list of presets. Clients take the HUD settings of that preset from their own copy of the file. For faster rebuilds during development, add `--features dev` (dynamic linking).

Settings (name, mouse sensitivity, field of view, master volume) are saved to `<config dir>/frontline/settings.ron`: `~/Library/Application Support/` on macOS, `~/.config/` on Linux, `%APPDATA%` on Windows.

`--map <id>` picks the Map a server (Dedicated, or Host) starts on: `arena` unless given. It's also the Map behind the main menu. An unknown id stops it at startup with the list of Maps. Clients load whatever Map the server names, so they need the same Map files (a client without a Homage Map leaves with an explanation).

A shipped build needs its `assets/` folder (maps, models, fonts) next to the executable; only the surface textures and faction emblems are compiled in. Under `cargo run` the assets come from `crates/frontline/assets/`.

## Controls

On joining, pick a Team (and then a Loadout). The mouse is grabbed while you play and freed whenever a menu is open; left-click grabs it again if the window lost focus.

| Action | Key |
|---|---|
| Move | WASD |
| Sprint | Left Shift (forward only, limited stamina) |
| Jump / stand up | Space |
| Mantle | Move into a ledge up to ~1.6 m (jumping helps) |
| Crouch | C |
| Prone | Left Ctrl or Z |
| Fire (full auto, or one round per click for semi/bolt-action) | Left mouse |
| Aim down sights | Right mouse (hold) |
| Reload | R (sprinting cancels it) |
| Scoreboard | Tab (hold) |
| Switch Team | T (dying counts as a switch, no credit to anyone) |
| Change Loadout (applies on next spawn, or at once within 5 s of spawning) | B |
| Pause menu (Resume, Change Team, Change Loadout, Settings, Leave Game) | Esc; Esc again steps back |

Tuning lives in `data/`: `weapons.ron` (including per-Weapon Hit Zone damage multipliers), `loadouts.ron` (Default Loadouts and the instant-swap window), `movement.ron`, `match_rules.ron` (presets, each with its HUD block), `game_modes.ron` (Team Deathmatch: 50 kills or 10 min), `factions.ron`, `maps.ron` (every built-in Map: file, look, Game Modes, and the two Factions its theater plays), `looks.ron` (per-Map look presets: grade, fog, sun, ambient, sky) and `presentation.ron` (how long bodies stay and how many at once). The game reads them at startup from `./data/` (relative to where you launch it), so edit and restart to retune. Without that folder it uses the copies built into the binary. Both machines should use the same files: damage and score are decided on the server, but movement, recoil, spread and fire rate come from each client's own copy.

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
| `prop` | `model` (glTF asset path), `collider` (none, box, convex) | A model; `box` collides as one box around it, `convex` as a hull per mesh |
| `light_point`, `light_spot`, `light_directional` | Bevy light settings | Extra lights (the sun, sky and fog come from the look) |

### Generated maps

`tools/mapgen/` is a small Python 3 library (no dependencies) that writes Valve 220 `.map` text from layouts in game space (metres, y up): boxes, turned boxes, ramps (`wedge`) and entities. `tools/mapgen/arena.py` describes the arena; regenerate it with:

```sh
python3 tools/mapgen/arena.py   # writes crates/frontline/assets/maps/arena.map
```

Generated maps stay editable in TrenchBroom, but regenerating overwrites hand edits, so pick one source per map.

## Layout

- `crates/frontline/src/flow.rs`: screen states (main menu, in game, overlays), flow commands, Settings file
- `crates/frontline/src/menus/`: title, Server Browser, Connecting, Pause and Settings screens
- `crates/frontline/src/ui/`: shared menu look: fonts, palette, buttons, panels, sliders, text fields
- `crates/frontline/src/match_hud/`: team select (Faction cards), top-centre score with Faction emblems, Tab Scoreboard and end screen, friendly name tags
- `crates/frontline/src/combat/`: server-side shots, per-Hit-Zone hitboxes, damage, health/regen, death and respawn, spawn selection
- `crates/frontline/src/data.rs`: loads the `data/*.ron` tuning files
- `crates/frontline/src/net/`: lightyear protocol, server, client, Host/Join/Leave sessions, LAN discovery
- `crates/frontline/src/player/`: first-person controller, input bindings, damage/kill-feed/death HUD, remote player visuals and their bodies
- `crates/frontline/src/weapon/`: local Weapon (firing, ammo, aiming, recoil, bolt cycle), per-weapon viewmodels, shot effects, crosshair/hitmarker/ammo/scope HUD
- `crates/frontline/src/loadout/`: chosen Loadout (saved with Settings), CoD swap rule, Loadout screen
- `crates/frontline/src/maps/`: Map registry (`data/maps.ron`, private Maps), TrenchBroom loading (units, textures, colliders, entity classes), loading the Match's Map and swapping it (`ChangeMap`), spawn points
- `crates/frontline/src/world.rs`: physics
- `crates/frontline/src/materials.rs`: material library (one tinted material per `SurfaceKind` texture set)
- `crates/frontline/src/atmosphere.rs`: camera look (tonemapping, bloom, SSAO) and per-Map look presets (`data/looks.ron`)
- `crates/frontline/src/assets.rs`: files embedded in the binary (faction emblems, surface textures; CC0 credits in `assets/textures/CREDITS.md`)
- `tools/mapgen/`: Python generator for `.map` files (the arena)
- `crates/frontline/src/shared.rs`: pose and stance types shared by netcode and gameplay

Private content such as Homage Maps lives in the separate private repo `Bjeaurn/frontline-private`, cloned into the git-ignored `content/private/` (see `docs/adr/0003`):

```sh
git clone https://github.com/Bjeaurn/frontline-private.git content/private
```
