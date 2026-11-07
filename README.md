# Frontline

A CoD4/MW2-inspired multiplayer FPS made for LAN parties. It's written in Rust on Bevy, lightyear and avian3d.

- Design: `IDEA.md`, `docs/PRD-lan-party.md`
- Glossary: `CONTEXT.md`
- Decisions: `docs/adr/`

## Run

One binary does everything. Launched without flags it opens the main menu: HOST GAME opens the Host setup screen (below) and then drops you in, JOIN GAME opens the Server Browser, which lists the servers on your LAN (click one) and also joins by IP (`192.168.1.20` or `192.168.1.20:5000`). If a Join fails you land back in the browser with the reason. CREATE A CLASS edits your custom classes without joining a game (see Create a Class below).

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

Settings are saved to `<config dir>/frontline/settings.ron`: `~/Library/Application Support/` on macOS, `~/.config/` on Linux, `%APPDATA%` on Windows. A file from an older version loads, with the defaults for anything it lacks. The Profile (custom Loadouts and the chosen Loadout) is saved next to it as `profile.ron`. See Settings below for what the four tabs hold.

`--mode <key>` picks the Game Mode of a single-entry game: `tdm` (Team Deathmatch, the default), `dom` (Domination), `hq` (Headquarters) or `sd` (Search & Destroy). The Map must support it (`modes` in `data/maps.ron`; an unsupported pair stops at startup with the list). `--rotation <file.ron>` replaces `--map`, `--mode` and `--rules` with a Rotation: a RON list of entries that the server plays in order, one per Match, then repeats, e.g.

```ron
[
    (map: "arena", mode: tdm),
    (map: "crossroads", mode: dom, rules: "hardcore"),   // rules default to "default"
]
```

`--map <id>` picks the Map a server (Dedicated, or Host) starts on: `arena` unless given. It's also the Map behind the main menu. An unknown id stops it at startup with the list of Maps. Clients load whatever Map the server names, so they need the same Map files (a client without a Homage Map leaves with an explanation).

`--look <preset>` (dev/test) replaces the Map's own look with a preset from `data/looks.ron` for that run: `overcast_urban`, `desert_dust`, `dawn_cold`, `jungle_humid`, `green_dusk` or `night_ops`. It applies to the window of a Host; a Dedicated Server accepts it but has nothing to render, and it can't be combined with `--connect`. An unknown preset stops a windowed run at startup with the list.

A shipped build needs its `assets/` folder (maps, models, fonts) next to the executable; only the surface textures, faction emblems and rank insignia are compiled in. Under `cargo run` the assets come from `crates/frontline/assets/`.

## Game Modes

Every Match runs one Game Mode (`data/game_modes.ron`; every number below is data). After a Match the server moves on to the next Rotation entry (changing Map and mode if the entry says so) and the end screen shows `Next: <Map> - <Mode>`.

| Mode | Key | Rules |
|---|---|---|
| Team Deathmatch | `tdm` | Each kill scores a point. 50 points or 10 min. The Map's fixed **ammo crates** (the `ammo_crate` entity, 6 on each built-in Map) fully resupply a player who walks up to them (no key): both magazines and the whole Reserve. A used crate stays shut for 30 s (`ammo_crates.respawn_secs`), then opens again; open it is lit green with its lid up, shut it is dark. |
| Domination | `dom` | Three Flags (A, B, C). Stand in a Flag's zone to capture it: ~10 s alone, up to 3x faster with teammates; both teams on it freezes it, and an enemy Flag is neutralised first. Each held Flag scores a point every 5 s. 200 points or 10 min. Spawns lean toward your Flags. The HUD shows A/B/C icons under the score, a progress bar while you capture, and markers over the Flags. Each Flag has an **ammo crate** beside it: while you stand within 4 m of a Flag your Team holds you are slowly resupplied (every 2 s, 10% of each Weapon's maximum Reserve, `resupply` in `game_modes.ron`), the crate opens and lights up while a holder is there, and the HUD shows `RESUPPLYING`. An enemy or neutral Flag's crate stays shut. |
| Headquarters | `hq` | One HQ is active at a time, ~60 s each; when it falls or expires the next one appears at a random one of the Map's HQ sites that are far from the one it left (at least 60% of the farthest site's distance, `relocate_far_fraction` in `data/game_modes.ron`; never the same site twice in a row), so it moves between areas without a fixed order. Capture it by standing in it (~10 s); the holders score a point per second and **don't respawn** while they hold it, so the other team must destroy it (stand in it ~10 s) before they are all dead. 250 points or 10 min. |
| Search & Destroy | `sd` | Rounds without respawns, attackers against defenders; sides swap after round 3, first to 4 rounds wins. A random attacker starts with the bomb (dropped on death, picked up by walking over it). The carrier plants it at site A or B by **holding F** for ~5 s (frozen in place), defenders defuse by holding F ~7 s, the fuse is ~45 s and the explosion kills everyone within 20 m. Defenders are never told which site holds the planted bomb (both site markers look the same all round): they find it by its positional beep, which speeds up as the fuse runs down. A round ends on elimination, detonation, defusal or the 2:30 clock (defenders win on time unless it is planted). Dead players spectate a teammate (click to switch). |

### Host setup

HOST GAME opens a setup screen: server name, max players, the Rotation, per-mode score and time limits (in Search & Destroy: rounds to win and minutes per round), then START. The Rotation is a table under the column headers MAP / MODE / RULES: each entry is a row and each cell a picker that shows its current value between ◀ ▶ arrows (a Map that lacks the entry's Game Mode brings its first one); `^` `v` reorder a row and `X` removes it; ADD ENTRY copies the last entry (up to 8). MATCH RULES opens the rules editor and returns to the same setup. Score limits step by 5 points (Search & Destroy: whole rounds), time limits by a minute. Your choices are saved with Settings. `--host` skips the screen and plays the `--map`/`--mode`/`--rules` entry.

### Match Rules

The built-in presets (`default`, `hardcore`, from `data/match_rules.ron`) are read-only starting points. The rules editor (SETTINGS > CUSTOM RULE SETS, or MATCH RULES on the Host setup screen) lists them with your own sets: select one, type a name (or leave it empty for `<set> copy`) and COPY SELECTED makes a custom set, which you edit field by field: max health, health regen on/off with its delay and rate, respawn delay and the earliest early respawn, damage scale, friendly fire, where fall damage starts and where it is lethal, every HUD toggle (crosshair, hitmarkers, kill hitmarker, kill feed, damage direction, name tags) and the kill feed delay. DONE saves your sets to `<config dir>/frontline/rules.ron`; BACK (or Esc) discards the edits. Custom sets then appear in the Rotation's RULES column next to the presets. A Match's rule values travel to the clients in `MatchState`, so only the Host needs the file; a Dedicated Server reads its own `rules.ron`.

## Controls

On joining, pick a Team (and then a Loadout). The mouse is grabbed while you play and freed whenever a menu is open; left-click grabs it again if the window lost focus. The keys below are the defaults: SETTINGS > CONTROLS rebinds them.

| Action | Key |
|---|---|
| Move | WASD |
| Sprint | Left Shift (forward only, limited stamina) |
| Jump / stand up | Space |
| Mantle | Move into a ledge up to ~1.6 m (jumping helps); steps and ramps (stairs) up to 0.45 m per step are simply walked up at full speed |
| Vault | Jump into a thin wall or window sill (0.6–1.2 m high, up to 0.6 m thick) with floor behind it: you cross it crouched in ~0.5 s and land on the far side. Every generated window is vaultable |
| Crouch | C |
| Prone (while sprinting: dive) | Left Ctrl or Z |
| Fire (in the Weapon's fire mode; a bolt-action click racks first, see Aiming below) | Left mouse |
| Fire mode (cycles the Weapon's modes: Auto, Burst, Semi or Bolt; a click sounds, the HUD shows the mode next to the ammo and the Profile remembers it per Weapon) | X |
| Aim down sights | Right mouse (hold) |
| Hold breath (aimed through a 3x or stronger sight; Shift is sprint otherwise) | Left Shift while aiming |
| Reload | R (sprinting cancels it). A reload moves rounds from the Reserve into the magazine: a partial magazine takes only what it is missing and the unused rounds stay in the Reserve. With an empty Reserve a reload does nothing: pulling the trigger on the empty magazine only clicks (once per pull), even with Auto reload on, and the HUD flashes `NO AMMO`. An empty magazine with rounds in the Reserve does **not** reload on its own: the pull gives a dry-fire click (once per pull) and a flashing RELOAD hint shows, unless Settings > Gameplay > Auto reload is on (then the pull reloads). Takes longer from an empty magazine (AR 2.5 / 3.1 s, SMG 2.4 / 2.9, Sniper 3.5 / 4.0, Pistol 1.7 / 2.1; Sleight of Hand x0.7) |
| Throw the lethal / the tactical equipment | G / Q: hold to ready it (its HUD row lights up), let go to throw. A Frag, Flashbang or Smoke leaves your hand with a lob that follows where you look: at the horizon about 25-30 m before the first bounce, looking up higher and farther (about 40 m at 45 degrees up), looking down a short underhand toss (a few metres, straight down at your feet). Dying, opening a menu, switching Weapon or running out cancels the throw. The HUD lists what is left above the ammo, one row per piece with its key |
| Grenade launcher mode (M16A4 with the launcher fitted; 3 rounds, 2.5 s between) | V switches the M16A4 into the launcher (the HUD shows `40MM` where the fire mode is, with the rounds left, and a click sounds); the fire button then fires one 40mm round per click, and the rifle cannot fire; V again switches back. With no rounds left a click is dry. Dying, switching Weapon or opening a menu returns to the rifle |
| Scoreboard (every row shows the player's rank insignia and Level before the name) | Tab (hold) |
| Chat to everyone / to your Team | Y / U: a one-line input opens bottom left (gameplay input is blocked while you type); Enter sends, Esc cancels |
| Plant / defuse the bomb (Search & Destroy; hold) | F |
| Switch Team | T (switching kills you, no credit to anyone, and the death screen shows until you respawn on the new Team's spawns; in Search & Destroy you wait for the next round) |
| Change Loadout (applies on next spawn, or at once within 5 s of spawning) | B |
| Primary / Secondary (lowers one and raises the other, about 0.4 s) | 1 / 2, or the mouse wheel |
| Respawn early (dead, once `min_respawn_secs` have passed; the death screen says when) | Space or click |
| Pause menu (Resume, Change Team, Change Loadout, Respawn, Settings, Leave Game) | Esc; Esc again steps back. RESPAWN is for getting unstuck: it kills you with no credit (a death in your stats, "X respawned" in the kill feed) and respawns you as usual; not in Search & Destroy |

### HUD

Styled after the CoD4 / MW2 HUD, in the olive and amber of the menus. **Bottom right**: the carried equipment (a key plate, the name and the count per row), then the held Weapon's name, the fire mode in amber and the rounds in the stencil face (`30 / 120`: magazine and Reserve; the count is red at 5 or fewer rounds and the Reserve is red at one magazine or less; the 40mm launcher shows its own rounds with no Reserve). **Under the crosshair**: `RELOAD` (flashing, empty magazine and nothing reloads it), `NO AMMO` (flashing, magazine and Reserve both empty) or a steady amber `LOW AMMO` (a Reserve of one magazine or less). **Below it**: `RESUPPLYING` at a held Domination Flag's crate and `AMMO REFILLED` after an ammo pack or a Team Deathmatch crate. **Centre**: an outlined four-arm crosshair that opens with the hip spread and fades out aiming, and a hitmarker that pops in and shrinks (white on a hit, bigger and red on a kill). **Top left**: the kill feed, one dark plate per line with the killer's side colour on its edge (amber when you got the kill) and `HEADSHOT` tagged; lines fade after 5 s. **Top centre**: the score with Faction emblems and the clock, each box rule-lined in its side colour. **Damage**: the screen edges darken and a blood splatter grows as health drops, flashes on every hit and throbs below 40% health; the red direction bars point at the shooter. **Death screen**: `KILLED BY`, the killer's name, the Weapon and the respawn countdown between letterbox bars. **Tab Scoreboard**: faction sections with striped rows, your own row in amber. The Match Rules' `hud` block (crosshair, hitmarkers, red kill hitmarker, kill feed, damage direction, name tags) switches the matching part off, as in Hardcore.

### Aiming

Every sight has a **Magnification** and a **Sway Scale** in the data (`iron_magnification` / `iron_sway_scale` per Weapon in `weapons.ron` for its own sights, or the Sniper's built-in scope; `magnification` / `sway_scale` per Optic in `attachments.ron`). Magnification sets the ADS field of view (the view narrows with the square root of it: 1x keeps 80% of the Settings FOV, the ACOG 40%, the 8x scope 28%), and from 3x it allows hold breath and shows a glint:

| Sight | Magnification | Sway Scale |
|---|---|---|
| Iron sights | 1x | 0.8 |
| Red Dot | 1x | 0.9 |
| Holographic (rectangular window, ring-and-dot reticle) | 1.5x | 0.95 |
| Prism (short tube, chevron reticle) | 2x | 1.0 |
| ACOG | 4x | 1.15 |
| Sniper scope | 8x | 1.3 |

- **Sway**: while aimed the view drifts along a slow figure-eight (6 s a lap), by a peak angle of the Weapon's `sway_deg` (Sniper 1.0, AR 0.5, SMG 0.15, Pistol 0.1) times the sight's Sway Scale, x0.7 crouched and x0.4 prone, blended in with the aim. The drift is added to your aim itself, so shots go where the sight points.
- **Hold Breath**: Shift while aimed through a 3x or stronger sight. Sway x0.15 for up to 4 s; then you are out of breath for 2 s with sway x1.6 and can't hold. The 4 s tank only refills while you are not holding. A vignette darkens the screen edges while you hold (stronger as the tank drains) and throbs red while you are out of breath.
- **Flinch**: taking a hit kicks your view by the Weapon's `flinch_deg` (a 50-damage hit, scaled by the damage), x0.6 while aimed, in a random direction; it recovers like recoil.
- **Glint**: a player aimed through a 3x or stronger sight shows a bright star at their scope to anyone within about 25 degrees of where they aim (the pose carries `aim_magnification`).
- **Soldiers**: other players (and their bodies) are low-poly soldiers dressed as their Team's Faction: uniform, vest, headgear (helmet, beret, boonie, cap, beanie, shemagh or ushanka), mask and goggles come from the Faction's `look` in `data/factions.ron`, and they carry the first-person rifle model. Friend or foe shows as armbands and chest and back panels (friendly blue, enemy red) so it reads whatever the uniform. They follow the replicated pose: standing, crouched (squatting) or prone (on the belly, rifle forward), the rifle pitching with the aim, and a walk cycle driven by how fast they move (a swing of the legs, a lean when sprinting with the rifle lowered). Hit zones and colliders are unchanged: this is only what you see. The soldiers are procedural (`src/player/soldier.rs`, no model files).
- **Rack** (Sniper): a shot leaves you aimed and the bolt open; you can't fire. While aimed a click racks it (`rack_secs_scoped`, 1.4 s, still zoomed, with a small sway kick, and the HUD asks `CLICK TO RACK`); the next click fires. Letting go of the aim racks automatically at `rack_secs_unscoped` (0.9 s), and letting go mid-rack finishes the rest at that faster rate.
- **Bullet flight** (Sniper): the Sniper's bullet is not hitscan. The server flies it from your eye at 600 m/s with 12 m/s² of gravity (`projectile: Some((muzzle_speed, gravity))` in `weapons.ron`; every other Weapon has none and stays hitscan), so a target at 240 m is hit 0.4 s after the click and the bullet has dropped 0.96 m by then (0.1 m at 80 m, 2.7 m at the 400 m `range`): lead movers and hold over. It ends at the first World or hitbox it crosses, or at `range` metres of flight, and damage falloff counts the path flown. Everyone sees a bright streak fly the same path, your own too, and hears the impact where it lands; your own gunshot and flash stay instant. Hits use the hitboxes as they are when the bullet arrives (`docs/adr/0005`, no lag compensation on a LAN).

Fall damage (CoD style, from the peak of the fall): none below `min_height` (4.5 m by default, 3.5 m in hardcore), then linear up to a lethal fall at `max_height` (12 m, 8 m in hardcore). The kill feed says "X fell to their death". The Tab Scoreboard's header shows the win condition, e.g. `FIRST TO 50 · 10:00 LIMIT` (Search & Destroy: `FIRST TO 4 ROUNDS`), with the Host's limit overrides applied.

Tuning lives in `data/`: `weapons.ron` (including per-Weapon Hit Zone damage multipliers), `attachments.ron` (Optics, with their magnification and sway scale, Muzzles and Underbarrels, as stat modifiers and optional class point costs), `perks.ron` (the six Perks and the ammo pack), `class_points.ron` (the Pick 10 budget, point costs, slot limits and the Wildcards), `equipment.ron` (Grenade flight, fuse and effect tuning), `loadouts.ron` (Default Loadouts, the instant-swap window and the Weapon switch time), `movement.ron`, `match_rules.ron` (presets: health, regen, respawn delay and the earliest early respawn, damage scale, friendly fire, fall damage and the HUD block), `game_modes.ron` (score and time limits, capture, HQ and bomb tuning of every Game Mode), `factions.ron`, `maps.ron` (every built-in Map: file, look, Game Modes, and the two Factions its theater plays), `looks.ron` (per-Map look presets: grade, fog, sun, ambient, sky), `presentation.ron` (how long bodies stay and how many at once) and `progression.ron` (XP awards, the Level curve, unlocks and camo Challenges). The game reads them at startup from `./data/` (relative to where you launch it), so edit and restart to retune. Without that folder it uses the copies built into the binary. Both machines should use the same files: damage and score are decided on the server, but movement, recoil, spread and fire rate come from each client's own copy.

## Loadouts

A Loadout (a class) is a **Pick 10** build: every class has 10 points to spend (`data/class_points.ron`).

| Choice | Cost |
|---|---|
| Primary (AR, M16A4, SMG or Sniper; always there) | 1 |
| Secondary (the M9 pistol; optional) | 1 |
| Each Attachment, up to 2 on the Primary | 1 (an Optic of 3x or more, the ACOG, costs 2; so does the Grenade Launcher, which only the M16A4 offers) |
| Each Perk, one per Perk Slot | 1 |
| Each piece of equipment (lethal and tactical slots) | 1 |
| Each Wildcard, up to 3 | 1 |

**Fire modes.** Each Weapon lists its modes in `weapons.ron` (`fire_modes`) and X cycles them: the M4 Auto / Burst 3 / Semi, the M16A4 Burst 3 / Semi, the MP5 Auto / Burst 2 / Semi, the pistol Semi, the Sniper Bolt. A burst fires its rounds at `burst_rpm` (about 0.09 s apart, slower than the Weapon's auto rate) and, with the trigger held, the next burst starts after `burst_delay_secs` more; a started burst always finishes. Semi never fires faster than `semi_interval_secs` (M4 0.14 s, MP5 0.12 s, pistol 0.2 s), however fast the button is mashed; dry-fire clicks keep to it too. Every shot still reaches the server one by one.

**Ammo.** Besides the magazine in the gun every Weapon carries a **Reserve** of spare rounds (`reserve_mags` in `weapons.ron`, times the magazine): the M4, M16A4 and MP5 four spare magazines (`30 / 120`), the M9 three (`15 / 45`), the M40 four (`5 / 20`). The 40mm launcher keeps its own 3 rounds a life. A reload takes only the rounds the magazine is missing from the Reserve; with the Reserve empty the Weapon can't be reloaded. Each Weapon keeps its own magazine and Reserve across a Weapon switch, and a respawn hands out the whole Loadout's ammo again. No Perk or Attachment changes the magazine size today; the Reserve is `reserve_mags` times the effective magazine, so one that did would scale it. More ammo comes from **ammo packs**: a dead player drops one where they fell (45 s, `ammo_pack` in `perks.ron`) and anyone alive who walks within 1.5 m of it takes it for a quarter (`share`) of every Weapon's maximum Reserve (one spare magazine of the AR), while a **Scavenger** takes the full refill from the same pack: both Weapons' Reserves and magazines topped up. The server decides who picked a pack up; the counts live on each client (docs/adr/0002). The other sources are the ammo crates of Domination (at a held Flag) and Team Deathmatch (fixed on the Map), see Game Modes.

**Equipment** (`data/equipment.ron`, class entries in `class_points.ron`): the lethal Frag Grenade (150 damage at the centre falling off linearly to nothing at 8 m, only to players in a straight line from the blast, you included; the fuse is 3 s), the tactical Flashbang (2.4 s fuse; players within 16 m looking at it are whited out for up to 5 s, everyone near is deafened for up to 6 s with a ringing and quieter sound) and Smoke Grenade (a 3 s fuse, then a 5.5 m cloud for 25 s of dense grey puffs wide and opaque enough that you cannot see a target through it, greying the view of anyone inside; bullets still pass through it). Grenades are physical projectiles the server flies: they bounce off the World, lose speed and come to rest, and the fuse runs from the throw. A throw leaves the hand when you let go of G / Q, along the look angle (`throw` in `equipment.ron`: a lob of 16 degrees at the horizon, launch speed from 3 m/s straight down to 25 m/s at the horizon and 23 m/s from 45 degrees up; the `speed` of 25 m/s is the cap the server holds every throw to). The Frag unlocks at Level 1, the Flashbang at Level 3 and the Smoke at Level 4. The Grenade Launcher (M16A4 only, Underbarrel slot, 2 points, M16A4 level 2) is a mode of the M16A4: V selects it (the HUD shows `40MM` and its rounds), a click fires a 40mm round, V selects the rifle again; 3 rounds, 58 m/s with a game-tuned arc, bursting on the first surface after 5 m (130 damage, 5.5 m). The default classes Assault (Frag, Flashbang), Close Quarters (Smoke) and Grenadier (M16A4 with the launcher, Frag, Smoke) carry equipment; a kill by a blast is an Explosive Kill (it counts for no Weapon's level).

**Wildcards** break a slot rule: *Perk 1 / 2 / 3 Greed* adds a second Perk in that Perk Slot (both Perks' effects stack), *Gunfighter* allows a third Attachment on the Primary, and *Overkill* lets the second Weapon be a Primary (a rifle, SMG or Sniper) instead of the pistol; it is carried and switched to like the pistol (`1`, `2` or the wheel). Everything but the Primary is optional: no Secondary frees a point for another Perk, and a class of just a Primary is valid. The Primary has three Attachment Slots, Optic (Red Dot 1x, Holographic 1.5x, Prism 2x, ACOG 4x; none on the Sniper, which has its built-in 8x scope), Muzzle (Silencer: no muzzle flash, a quieter and duller shot, damage falls off at 0.75x the distance) and Underbarrel (Foregrip: 25% less recoil, a little slower to aim), but only two Attachments at once without Gunfighter. Each Weapon offers its own set (see Progression). Camos (per Weapon, see Progression) go on the Primary and are free.

A class applies on your next spawn, or at once within 5 s of spawning. The Loadout screen (B, or Change Loadout in the pause menu) lists three read-only Default Loadouts (valid Pick 10 classes, usable from the start) and the three custom ones in your Profile; CHOOSE picks one.

**Create a Class** is three screens deep, never a scrolling form, and opens from the main menu (CREATE A CLASS) as well as in game (the Create a Class button on the Loadout screen). Arrows move the highlight, Enter selects, Esc goes back one screen (out of the class list it returns to the title, or in game to the Loadout screen); the mouse hovers and clicks the same things.

1. **Class list**: the three read-only Default classes and your three custom ones on the left, a summary of the highlighted one (weapons with their Attachments and camo, Perks, equipment, Wildcards, points used) on the right. EDIT is greyed with a hint on a Default class; Enter or a click on a custom class opens its slot overview.
2. **Slot overview**: big tiles for PRIMARY (weapon, its Attachments and camo), SECONDARY, LETHAL, TACTICAL, PERK 1 to 3 (the Greed Perk sits inside the tile while that Wildcard is on, with its own click target) and WILDCARDS, each with its pick and point cost. A big points meter (`7 / 10` and ten pips) is on top, next to the class name (click the field or press Enter on it to type; Enter or Esc ends the typing) and SAVE / BACK. SAVE writes the class into the Profile and returns to the class list; BACK or Esc leaves, asking once to discard unsaved changes.
3. **Picker**: a tile opens a full-screen picker for that slot. The option list is on the left (seven rows a page; Left / Right or the arrow buttons turn the page): locked options are greyed with what unlocks them (`Unlocks at Level 8`, `Unlocks at M4 level 3`, `Locked: 34/50 kills`), options that don't fit the budget show their cost in red and say why, the equipped one is tagged. The right panel previews the highlighted option: name, description, stat bars for weapons and Attachments, its cost and the budget after picking. Enter or a click takes it and returns; Esc goes back unchanged. A locked or refused option can't be taken. Picking the Primary leads on to its own screen with an ATTACHMENT tile per Attachment it may carry (a third with Gunfighter) and a CAMO tile, each with its own picker and the weapon's stat bars beside them.

A class that breaks a rule can't be saved, whatever the order of picks: the pickers refuse it first (`Not enough points: 11 / 10`, `2 attachments at most without Gunfighter`). A Secondary that is a Primary weapon (Overkill) has no Attachments or camo of its own: they belong to the Primary.

The Profile is a client-local file, `<config dir>/frontline/profile.ron`, next to `settings.ron`: the custom classes, the chosen Loadout, your XP (the Level follows from it) and, per Weapon, kills, headshot kills and Weapon XP. A Profile from before Pick 10 is migrated when it loads: each class keeps what fits and, if it is over the rules or the budget, loses things from the end (Wildcards, equipment, Perks, the Secondary, then Attachments); the log says what was dropped. The first run starts with the Default Loadout your old Settings had chosen. It follows you to any server you join.

| Perk Slot | Perks |
|---|---|
| Perk 1 | Sleight of Hand (reload 30% faster), Scavenger (a dead player's ammo pack gives you the full refill, not a share) |
| Perk 2 | Stopping Power (+25% bullet damage), Lightweight (+10% move speed) |
| Perk 3 | Marathon (sprint 2.5x as long), Ninja (your footsteps make no sound) |

Footsteps are heard as concrete steps while a player walks or runs. A step falls due per distance walked (about 2.2 a second walking, rising to about 3 sprinting), so steps follow the movement exactly: none while standing or pushing against a wall, the next right after setting off. Other players' steps are heard from where they are; you hear your own, quieter, and none while airborne. Crouched and prone players, and Ninja, are silent to others. The eight step sounds are made by `tools/audio/footsteps.py` (Kenney's recordings, brightened and layered with a synthesized heel click and thump). Bullets leave a black hole with a gray rim where they hit the World; the newest 120 (Low), 300 (Medium) or 600 (High detail) stay until a new Map loads. Positional sounds (other players' shots, impacts and footsteps) play low-passed and about 3 dB quieter when they come from behind you. The server applies the Attachments' falloff and the Perks' damage (a Perk Greed pair both count); clients tell it their Attachments with every shot and their Perks when they spawn (clients are trusted on a LAN, docs/adr/0002). Fire Pings and the Minimap don't exist yet, so the Silencer only changes the flash, the sound and the damage range.

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

**Rank insignia.** Every Level has a small worn cloth patch: chevrons for Levels 1-10, bars for 11-20 and stars for 21-30, one more of them each Level inside a band. The Tab Scoreboard shows it, then the Level number, before each name (`[patch] 12  Bjeaurn`). Your client reports its Level to the server when you join and on every level-up (the server clamps it to 1 to 30 and replicates it as `PlayerLevel`; clients are trusted, docs/adr/0002), so the board updates live. The patches are 64x64 PNGs in `assets/icons/ranks/` (`chevrons_01.png` ... `stars_10.png`, compiled into the binary), made by `tools/ranks/ranks.py` on top of the emblem generator (`/tmp/emb/bin/python tools/ranks/ranks.py`, Pillow only; `--sheet out.png` writes a contact sheet); `progression/rank.rs` maps a Level to its patch.

**Chat.** Y opens a one-line input bottom left for everyone, U for your Team (Enter sends, Esc cancels; gameplay input is blocked while you type). The server trims messages, drops any over 120 characters and allows 3 per 3 seconds per player; team chat reaches only your Team. The chat box shows the last 6 lines for about 8 seconds (they fade), and the last 30 while you type. Lines read `[ALL] Name: text` and `[TEAM] Name: text` (team chat in the friendly blue); system lines in a muted grey announce `Name reached Level N`, joins, leaves and Team switches.

**Unlocks** come on two tracks. The **account Level** unlocks Weapons, Perks, equipment and Wildcards; it is a table in `progression.ron` (anything not listed is there from Level 1, and the Default Loadouts are always usable):

| Level | Unlocks |
|---|---|
| 1 | AR, SMG, Sniper, Pistol, Frag Grenade, Sleight of Hand, Stopping Power, Marathon |
| 3 / 4 | Flashbang / Smoke Grenade |
| 4 | M16A4 |
| 2 | Custom classes (editing and choosing the three custom slots) |
| 5 / 6 / 8 | Scavenger / Lightweight / Ninja |
| 6 | Gunfighter |
| 8 / 9 / 10 | Perk 1 / 2 / 3 Greed |
| 12 | Overkill |

Equipment unlocks are `Equipment(..)` entries in the same table. Each **Weapon Level** unlocks that Weapon's Attachments, then its camos. Every kill with a Weapon gives it 1 Weapon XP and a headshot kill 1 more, and its Weapon Level rises at 5, 15, 30, 50 and 75 Weapon XP (`weapon_levels` in `progression.ron`; the Profile stores Weapon XP per Weapon). `weapon_attachments` lists, per Weapon, which Attachments it offers and at which Weapon Level (an Attachment a Weapon doesn't list can't be fitted to it: the Sniper has no Optics, the pistol takes none, and only the M16A4 has the Grenade Launcher: Weapon level 2, on the Underbarrel slot it shares with the Foregrip):

| Weapon Level | M4 (AR) | M16A4 (AR) | MP5 (SMG) | M40 (Sniper) |
|---|---|---|---|---|
| 1 | Red Dot | Red Dot | Red Dot | Silencer |
| 2 | Silencer | Silencer, Grenade Launcher | Silencer | |
| 3 | Holographic | Holographic | Holographic | Foregrip |
| 4 | Foregrip, Prism | Foregrip | Foregrip | |
| 5 | ACOG | ACOG | Prism | |

Reaching a Weapon Level shows a toast such as `M4 LEVEL 3: HOLOGRAPHIC UNLOCKED`, and the end-of-Match Scoreboard lists the Weapons that gained XP (up to three: `M4  +7 XP   LEVEL 2 > 3  12 / 15 XP`). The editor shows the Primary's Weapon Level and what the next one opens, and a locked Attachment says `Unlocks at M4 level 3`.

**Camos** are material variants of the Primary's body in the first-person viewmodel (the gun model's parts re-skinned in gun space, keeping its normal map; trigger, magazine, scope, sights and bolt handle stay bare; the third-person gun comes later with remote players' guns). Six ship: Woodland, Desert, Urban (grey digital), Tiger (stripes), Arctic and a metallic Gold. Each Weapon unlocks them separately by a **Challenge** counted in your Profile on that Weapon: Woodland 25 kills, Desert 50 kills, Urban 25 headshot kills, Tiger 100 kills, Arctic 50 headshot kills, and Gold once the other five are unlocked on it. The Camo picker shows the progress of a locked one (`34/50 kills`). The textures are 128 px tiling PNGs under `crates/frontline/assets/camos/` (embedded in the binary), made by `tools/camos/camos.py`:

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

Textures come from `crates/frontline/assets/textures/<surface>/color.jpg` (concrete, plaster, metal, corrugated, wood, ground, brick, grass). A face's texture picks the in-game surface by its folder name, and the game draws it with that surface's full material (normal and roughness maps, palette tint). The texture scale is real-world: a scale of 0.0846 on concrete repeats it every 2.2 m, as `tools/mapgen` sets it.

Brushes in worldspawn or any brush entity (`func_group`, `func_generic`) become solid, bullet-stopping geometry, one convex collider per brush. Entities:

| Class | Properties | Purpose |
|---|---|---|
| `worldspawn` | `name`, `look` (from `data/looks.ron`) | Map settings; `data/maps.ron` may override the look |
| `spawn_point` | `team` (alpha, bravo, any), `modes` (Game Mode keys `tdm dom hq sd`, empty for all), `angle` | Where players spawn: origin at the feet, `angle` the facing |
| `dom_flag` | `label` (A, B, C) | Domination Flag |
| `hq_site` | | Headquarters site |
| `bomb_site` | `label` (A, B) | Search & Destroy bomb site |
| `ammo_crate` | | Team Deathmatch ammo crate: a player walking within 1.6 m is fully resupplied, then it stays shut for 30 s (`ammo_crates` in `data/game_modes.ron`). The origin is the centre of its base on the ground; the game draws the crate itself, so there is nothing to build around it. Only Team Deathmatch uses it: Headquarters, Search & Destroy and Domination ignore it (they have no fixed crates; Domination has one at each Flag). Place a few in the middle lanes, in pairs turned 180 degrees about the Map's centre, well away from the spawns (`tools/mapgen` wants at least 12 m) and with no brush within a metre |
| `prop` | `model` (glTF asset path), `collider` (none, box, convex) | A model, e.g. `models/downtown/Prop_Bollard.glb`; `box` collides as one box around it, `convex` as a hull per mesh. Homage Maps point at their own copy, `private://models/...` |
| `light_point`, `light_spot`, `light_directional` | Bevy light settings | Extra lights (the sun, sky and fog come from the look). A `light_point` with `targetname` `practical` is a lamp: its intensity is scaled by the look's `practical_lights` (night looks switch it on, day looks leave it dark) |

### Generated maps

`tools/mapgen/` is a small Python 3 library (no dependencies) that writes Valve 220 `.map` text from layouts in game space (metres, y up): boxes, turned boxes, ramps (`wedge`) and entities. `kit.py` builds on it with a `Level` (walls with door and window gaps, slabs with stairwell holes, ramps, vehicle and barrier blockouts, `prop` entities, optional 180-degree mirroring, and `shifted(dx, dz)` to move a part of a layout apart from the rest) and a walkability checker (`--check`) that flood-fills the floor on foot to find unreachable objectives, trapped spots and closed-off pockets without launching the game. The same check measures the playable extents and the longest spawn-to-spawn walk (shortest on-foot path of each Alpha-Bravo pair, longest pair; at 5.5 m/s a 22 s crossing is ~120 m), fails when that is outside the size a Map is meant to have, and flags floating geometry (brushes not attached to the ground, props with nothing under their base). Writing a Map also fails when objectives sit too close: HQ sites and bomb sites 30 m apart, Domination Flags 25 m. Maps are spread out rather than scaled up (players stay real size): buildings and props keep their size, the gaps and yards between them grow, and the new space gets cover. Walk lengths now: Arena 116 m, Crossroads 136 m, Backlot 180 m (enlarged again after the v0.1.0 playtest, with fences and a compound hub), Vacant 126 m (longest 148 m). `arena.py` describes the arena and `crossroads.py` the Remix Map Crossroads (an original urban layout: three lanes, a two-storey building in the middle, ~22 s to cross); regenerate them with:

```sh
python3 tools/mapgen/arena.py                # writes crates/frontline/assets/maps/arena.map
python3 tools/mapgen/crossroads.py --check   # writes crossroads.map, then checks it can be walked
```

Generated maps stay editable in TrenchBroom, but regenerating overwrites hand edits, so pick one source per map.

`Level.ammo_crate(x, z)` places a Team Deathmatch ammo crate (inside `mirrored()` its twin turned 180 degrees about the origin too); `write` refuses a layout whose crates are closer than 8 m to each other, closer than 12 m to a Team Deathmatch spawn, or touching a solid. The built-in Arena and Crossroads have six each (three pairs).

`Level.fence(axis, c, a0, a1, kind, gaps, lows)` (and `fence_rect`) builds a chain-link (2.4 m) or slat (1.9 m) fence as an open lattice of thin brushes: shots and sight pass through, bodies do not. Gates must be at least 3 m wide, `lows` are 1 m sections a player vaults. `Level.container(x, z, yaw)` is a shipping container. `--check` also runs `kit.placement_problems` (spawns and sites inside a brush; any Alpha spawn that sees a Bravo spawn, standing or crouched, fences excepted), `kit.climb_problems` (each trick-jump route's steps must stay within the 1.6 m mantle) and `kit.fence_conflicts` (nothing stands in a fence).

`python3 tools/mapgen/lint.py <map>...` is the flicker linter. Two surfaces flicker (z-fight) when they lie in one plane: two brush faces that are coplanar (within 0.01 units), face the same way and overlap, or two flat props (street tiles, decals) at the same height that overlap; a perimeter wall flush with a building model's facade is the same thing and hides the windows. It reads any Valve 220 `.map` (Homage Maps too), rebuilds each face as a polygon, ignores overlaps buried inside a third brush and undersides at or below the ground, prints the pairs grouped by plane with brush numbers (`// brush N` of the entity) and positions in game metres, and exits 1 if it found any. Every generated Map must lint clean; run it after regenerating. Fixes that work: stop a brush where the next one begins (or 0.07 m inside it), give crossing bars different thicknesses, keep decals 1 cm above their tile and tiles apart, stand a perimeter wall 0.5 m behind the buildings' front plane (`crossroads.py`).

### MapEdit

`mapedit` (`crates/mapedit`) is a second executable that shows a Map on the game's own rendering (materials, looks, props, lights, physics colliders: it is built on the same `frontline` library), with a free-fly camera and the Map's entities drawn over it, and edits it: you move, resize, paint, add and delete brushes and entities, create new Maps, and save. After regenerating a Map with `tools/mapgen` you see the result at once.

```sh
cargo run -p mapedit -- --map z_town                              # the viewer and editor
cargo run -p mapedit -- --map arena                                      # Esc: load, save, new Map, settings
cargo run -p mapedit -- --map z_town --top --shot /tmp/top.png    # top view, screenshot, exit
```

`--look`, `--mode tdm|dom|hq|sd` (show only what that Game Mode uses), `--cam X Y Z YAW PITCH` (metres, degrees; yaw 0 looks north, -z), `--top`, `--size 1600x900`, `--remote-port`, `--no-remote`. Like the game it reads `data/` from the working directory; run from the repository root.

| Control | Action |
|---|---|
| Hold right mouse button | Look |
| W A S D, Q / E | Move, down / up (Shift fast, Ctrl slow, wheel changes the speed) |
| T / Home | Top view (fog off) / default aerial view |
| `[` `]` | Previous / next Map (every Map in `data/maps.ron` and the private folder) |
| `,` `.` | Previous / next look preset (the first is the Map's own) |
| R | Read the Map's `.map` file again (asks first when there are unsaved edits; the file is also re-read by itself when it changes on disk and there are none) |
| 1 - 5 | Show everything, or only what Team Deathmatch, Domination, Headquarters or Search & Destroy uses |
| C / X / L / F | Collider wireframe / draw markers through walls / labels / fog |
| F12 | Screenshot to `screenshots/` (also `--shot`, and the remote method below) |
| H | Hide or show the help |
| **Esc** | The menu: Resume, Load Map, Save, Save As, New Map, Map settings, Quit. Esc steps back a screen, then closes it |
| **P** | The palette panel: Surfaces, Brushes, Entities, Props (click to paint, or to place in front of the camera) |
| Left click | Select the brush or entity under the cursor (yellow outline; white outline on hover); click empty space to deselect |
| Arrows, PgUp / PgDn | Move the selection by the grid step (0.25 m; Shift: 4 steps). The arrows follow the camera: right and forward snap to the nearest world axis |
| Alt + arrows, PgUp / PgDn | Grow the brush towards that direction by one grid step; with Shift, shrink it from that side |
| Tab / Shift+Tab | Next / previous surface of the selected brush |
| B / Shift+B | A box / a ramp (climbing away from you) at the cursor, in the current surface |
| Delete | Delete the selection |
| Ctrl+D (Cmd+D) | Duplicate the selection, four grid steps (1 m) to the right; the copy is selected |
| Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z | Undo, redo (200 steps) |
| Ctrl+S (Cmd+S) | Save |
| G | Grid step: 0.25, 0.5, 1, 0.05 m |
| J / K | Turn the selected entity 45 degrees (spawn facing) |

Drawn over the Map: spawn points (by Team, with their facing), Flags and HQ sites (their 3.5 m capture zone), bomb sites (2.5 m) and ammo crates (1.6 m), the sizes of `data/game_modes.ron`. The HUD counts them, shows the selection, the grid, the unsaved flag and the last message.

**How editing works.** Opening a Map parses its `.map` file (Valve 220, `crates/mapedit/src/mapfile.rs`) into a working copy; every edit changes that copy and is undoable. The viewer shows the copy through the game's own Map loader: after each edit MapEdit writes it to a git-ignored preview next to the Map's file (`maps/.mapedit-arena.map`, deleted when you leave the Map or quit) and reloads it, so what you see is exactly what the game would load (a few hundred milliseconds after an edit; large Maps take longer). Nothing touches the Map's own file before **Save**. Moving and resizing keep each face's texture alignment to the world, as `tools/mapgen` writes it, so neighbouring brushes stay seamless; a painted brush gets its surface's real-world texture scale. Resizing scales the brush's bounding box: exact for boxes and ramps, a turned box shears. Brush primitives are boxes, walls, slabs and ramps (`Brush::wedge`, the same as `mapgen.py`'s `wedge`); the new Map's floor and everything MapEdit adds lands in the worldspawn. Point entities are the classes of the table above: spawn points (team, modes, facing), Flags, HQ and bomb sites, ammo crates, point lights and props (the palette lists `models/*/*.glb` with their bounds).

**The menu.** *Load Map* lists every Map of `data/maps.ron` and the private folder, plus loose `.map` files in `maps/` that no entry uses (they are registered for this session and listed in `data/maps.ron` at the next Save). *Save* writes the working copy to the Map's file, and the Map's entry in `data/maps.ron` when its settings changed or it was loose. *Save As* writes a copy as a new Map (`maps/<id>.map`), registers it and opens it; the original stays as last saved. *New Map* takes an id (letters, digits, `_`) and a name and creates `maps/<id>.map`: a 60 x 60 m ground floor (top at y = 0), two spawns per Team, a worldspawn with a look; it is listed in `data/maps.ron` for Team Deathmatch with the first two Factions of `factions.ron` (set Game Modes and Factions in *Map settings* as the Map grows objectives). *Map settings* edit the name, look (also written to the worldspawn), Game Modes and Factions; **Apply** changes the open Map, **Save** writes `data/maps.ron`. `data/maps.ron` is edited as text: only that Map's entry is rewritten, comments and the other entries stay. Unsaved changes are marked `* UNSAVED` in the HUD, and loading another Map, creating one, reloading or quitting (also the window's close button) asks: save and continue, discard, or cancel. A Map the file parser cannot read is shown as the game loads it but marked view only.

**Generated maps.** Maps written by a `tools/mapgen` script (Arena, Crossroads, Z-Town: any `.map` that has a `tools/mapgen/<name>.py`) are overwritten by the next run of the script. MapEdit says so in the menu, in the HUD while there are unsaved edits, and in the answer to Save: edit those Maps in the script, or stop regenerating them. Saving a generated Map without edits leaves the bytes as the script wrote them (the writer reproduces `mapgen.py`'s layout exactly; a test checks every committed Map).

**Remote control.** MapEdit serves Bevy's remote protocol (JSON-RPC over HTTP, localhost only, port 15702) with its own methods, documented in `crates/mapedit/src/remote.rs` (the generic `world.*` methods work too). Viewing: `mapedit/status` (also the editor state: `dirty`, selection, `synced`, undo availability, the `generator` script), `load_map`, `reload`, `set_look`, `set_camera`, `set_overlay`, `entities`, `screenshot`, `ui` (show the menu or the palette) and `quit`. Editing: `new_map`, `save`, `save_as`, `map_settings` (name, look, Game Modes, Factions), `palette` (surfaces, primitives, entity classes with their properties, props with bounds, looks, Factions), `items` (the brushes and entities of the working copy, filterable and paged), `select` (by item, ray or window pixel), `move`, `resize`, `delete`, `duplicate`, `set_texture`, `set_property`, `add_brush` (box, wedge, wall, slab, ramp), `add_entity`, `undo` and `redo`. Positions are game space, metres, y up; an item is `{"entity": 0, "brush": 12}` or `{"entity": 5}`. Methods that would lose unsaved changes (`load_map`, `reload`, `new_map`, `quit`) refuse unless called with `discard: true`. `tools/mapedit-mcp/server.py` is a standard-library MCP server over all of them (`mapedit_launch`, `mapedit_new_map`, `mapedit_add_brush`, `mapedit_add_entity`, `mapedit_screenshot`, `mapedit_save` and so on), so an AI agent can create a Map, build it, look at it and save it; `mapedit_screenshot` waits until the view shows the latest edit. Register it with an MCP client as the command `python3 tools/mapedit-mcp/server.py`. It starts the viewer with `cargo run -p mapedit` (or the executable named in `MAPEDIT_BIN`).

MapEdit is a blockout tool, not TrenchBroom: there is no vertex or face editing, no rotation of brushes, no multi-selection, and no texture alignment beyond the surface's real-world scale. For those, open the same `.map` in TrenchBroom. It is not part of the release archive.

### Props

Props are glTF models from the CC0 Downtown City MegaKit (Quaternius), processed into one `.glb` each (textures at most 1024 px, shared ones in `models/downtown/textures/`) under `crates/frontline/assets/models/downtown/`; credits in `assets/models/CREDITS.md`. The raw pack stays in the git-ignored `content/assets/`. `tools/assets/import_props.py` picks the pieces out of the zip, writes each pack's `bounds.json` (the model bounds the generators place by: `Level.prop(..., y)` puts a model's *base* at `y`, wherever its origin is; `--bounds-only` rewrites it from the `.glb` files already there), and with `--from-maps` imports exactly the ones the maps use:

```sh
python3 tools/assets/import_props.py --zip "content/assets/Downtown City MegaKit[Standard].zip" \
    --pack downtown --from-maps crates/frontline/assets/maps/*.map --clean
```

The kit has no vehicles, barriers or lamp posts, so the generators build those as brushes. Homage Maps keep their own copy in `content/private/models/` (`--prefix private://models --out content/private/models`), so the public repo carries nothing selected for them.

### Original CoD4 textures (bring your own)

Homage Maps can use Call of Duty 4's own textures, read from **your** copy of the game. They are Activision's files, so Frontline never stores or ships them (`docs/adr/0004`): they are read and decoded in memory while the game runs, and nothing is written to disk, into either repo or into a build.

Set **Settings > Gameplay > CoD4 textures** to one of:

- your CoD4 install folder (its `main/iw_*.iwd` archives are used),
- a folder that holds `iw_*.iwd`,
- a zip that contains IWDs (such as a texture mod's `IW3_TEXTURES_MOD.zip`) or `images/*.iwi` files (`raw/images/`).

Press CHECK (or APPLY): the status line shows what was found, e.g. `Found 7 archives, 6,236 images`, or `Not found`. For one run, `--cod4 <path>` overrides the saved setting. Later archives override earlier ones, as in the game (`iw_06` over `iw_00`). The index is built at startup and whenever the path changes; a Map picks the change up the next time it loads.

It only affects **Homage Maps** (the private ones, `content/private/cod4_textures.ron` maps each Map's brush textures like `brick/color` to CoD4 image names; names only). Remix Maps, props, weapons and everything else stay on the CC0 textures, as does any surface whose image isn't in your files, and the whole game when no path is set; players without CoD4 see CC0 surfaces on the same geometry. Textures larger than 1024 px are downscaled, and a Dedicated Server never loads textures.

## Settings

SETTINGS (title menu, or the pause menu in game) has four tabs. Edits wait for APPLY, which saves them to `settings.ron` and goes back (after a display or resolution change it first asks to keep it, below); BACK or Esc discards them, the Video changes included.

- **Video:** display mode (windowed, borderless fullscreen, exclusive fullscreen), resolution (the sizes the monitor's video modes offer; borderless always uses the monitor's), vsync, an FPS cap (30 to 240 or unlimited, sleep-based; no effect above the refresh rate while vsync is on), a quality preset (Low, Medium, High, Custom) and its four settings: sun shadows (off, 1024 or 2048 px), ambient occlusion (off, low, medium, high), bloom and anti-aliasing (off, FXAA, SMAA), and brightness (50 to 150%, an exposure offset on the Map's look). Medium is what the game looked like before these settings existed. Changing a single quality setting switches the preset to Custom. Everything but the display mode and resolution applies live as you change it. Those two only change what the pickers show until you press APPLY; BACK or Esc drops them and leaves the window alone. When APPLY finds a different display mode or resolution it applies it and asks "Keep these settings?": KEEP confirms, saves everything and goes back; REVERT, Esc or 15 s without an answer put the previous display mode and resolution back (nothing is saved until KEEP). There is no render scale: Bevy 0.20 has no supported way to render the 3D view below the window's size.
- **Audio:** master volume. (There are no music or voice sounds yet, so no other sliders.)
- **Controls:** mouse sensitivity, an ADS sensitivity multiplier (0.25x to 1.5x; the view blends to it as you aim down sights), a Scope sensitivity multiplier (0.1x to 1.5x, 1.0x by default; it stacks on the ADS one and only applies through a magnified sight, 3x and up, such as the sniper's scope and the ACOG, so a narrow scoped view can turn slower), invert Y, and the key-rebinding list. Click an action, then press the new key (or mouse button for Fire and Aim); Esc cancels. A key another action already uses is taken from it, and that action shows UNBOUND. Rebinding replaces all of an action's keys (Prone has two by default). RESET ALL restores the defaults. Esc stays the Pause / back key, and clicking the game still grabs the mouse; neither is in the list. Keys outside the usual letters, digits, F-keys, arrows, numpad, modifier and punctuation set can't be bound.
- **Gameplay:** your name, the field of view, Auto reload (off by default: an empty magazine only clicks and R reloads; on, pulling the trigger on it reloads), Handedness (Right by default; Left mirrors your view, below), the CoD4 texture path (CHECK applies it and the status line says what was found) and the Custom Match Rules editor.

### First-person arms and left-handed play

The arms are only something to look at: they have no effect on gameplay or on a hit. The muzzle, the sights and where a shot goes come from the eye and the gun's own data, whichever hand holds the gun. Each arm is two bones (upper arm and forearm) between a shoulder behind and below the eye and the glove; the elbow is solved every frame (a two-bone solve towards a pole that points down and outward, so the elbows hang). A sleeve tube per bone and a joint at the elbow bend with it, and the glove (a palm, four finger blocks, a thumb and a cuff) closes its fingers round what it holds: the pistol grip, the handguard, the magazine while it is pulled, a Foregrip or the Grenade Launcher, the Sniper's bolt knob (the hand leaves the grip to work the bolt) and a Grenade. A hand out of the arm's reach gets a straight arm, never a gap. The arms stay on the viewmodel layer, so they never clip into a wall.

**Settings > Gameplay > Handedness** (`handedness` in `settings.ron`, Right when missing) set to Left mirrors the whole viewmodel: the gun sits on the left of the screen at the hip, the left hand is the strong one (grip, trigger, Grenade throw), the right hand holds the handguard, and the gun model itself is mirrored (its mirrored triangles are drawn with the other face culled; camo, optics and the muzzle flash follow). Aiming stays in the centre of the screen: the sight point is the aim line in both modes. The Sniper's bolt handle stays on the right of the screen and the right hand works it, so it sits on the opposite side of the gun in gun space. It takes effect when you APPLY, in the middle of a game too. Only your own view changes: the replicated pose carries no handedness, so other players' soldiers (and yours, to them) still hold the gun in the right hand.

## CI and releases

GitHub Actions (`.github/workflows/`) run on every push to `main` and every pull request:

1. **Format:** `cargo fmt --all --check`.
2. **Clippy and tests:** `cargo clippy --locked --workspace --all-targets -- -D warnings`, then `cargo test --locked --workspace`, on Linux (the whole workspace, MapEdit included; plain `cargo run`, `build` and the release build are the game alone). The Homage Maps (`content/private/`) are never there: nothing may depend on them.

**Releasing:** bump the version with `tools/ci/bump.sh minor --commit` (or `patch`, `major`, or an exact `0.3.0` / `0.3.0-rc1`; it sets `crates/frontline` and `crates/mapedit` together and refreshes `Cargo.lock`), push `main`, start the Mac runner (below), then:

```sh
tools/ci/release.sh v0.2.0
```

It checks that the tag matches the crate version, that `main` is clean and pushed, that the Mac runner is online and that there is about 10 GB free for the macOS builds (`--check` only checks), and then tags and pushes. It refuses rather than letting a release hang: the macOS jobs wait for the runner, `timeout-minutes` (120 for a build) only counts once a job has started, and GitHub cancels a job that has waited 24 hours. A run that includes macOS also prints a "Start the Mac runner" warning on its summary page.

The Release workflow runs the checks above on the tagged commit, builds Frontline in release mode for Linux (x86_64), Windows (x86_64) and macOS (Apple silicon and Intel), packs each into `frontline-<tag>-<target>.tar.gz` (`.zip` on Windows) and publishes them in a GitHub release with generated notes. A tag with a suffix (`v0.2.0-rc1`) is marked a pre-release; a tag that doesn't match the crate version fails before building. Run the workflow by hand (Actions > Release > Run workflow) to get the archives as run artifacts without releasing anything; it can build just `macos` or `linux-windows`, and skip the checks. Tick `test_release` to also publish them as a pre-release tagged `test-<sha>-run<n>` (never "Latest"), for a playtest build without a version tag: `gh workflow run release.yml -f test_release=true`.

**The macOS builds run on the maintainer's own Mac**, not on GitHub's runners, which bill macOS minutes at 10x (a cold build is about 40 minutes each). Linux and Windows build on GitHub's. The Mac is a self-hosted runner labelled `frontline-release` in `~/actions-runner-frontline`; the macOS jobs wait until it is online, so start it before tagging and stop it after:

```sh
~/actions-runner-frontline/start.sh      # Ctrl-C to stop
```

It is deliberately not a login service, and only the Release workflow uses it (never pull requests: code from a pull request would run on the Mac). It keeps its own Rust toolchain (`rust/`, so it needs no rustup of yours), builds in `~/.cache/frontline-runner-target` and deletes that folder after every job, so it needs about 10 GB free while a release builds. To set one up on another Mac: download the runner for your architecture from github.com/actions/runner/releases, `./config.sh --url https://github.com/Bjeaurn/frontline --token <token from Settings > Actions > Runners> --labels frontline-release`, install rustup into it (`RUSTUP_HOME=<runner>/rust/rustup CARGO_HOME=<runner>/rust/cargo`, put `<runner>/rust/cargo/bin` first in the runner's `.path` and set both variables in its `.env`), and have Xcode's command line tools installed.

Each archive holds the executable, `assets/` (maps, models, fonts: it must stay next to the executable), `data/` (optional tuning overrides; the same files are built in) and this README. The builds are unsigned: macOS asks you to allow the app in System Settings > Privacy & Security (or `xattr -d com.apple.quarantine frontline`), and Windows SmartScreen shows a warning. The toolchain is the `rust-version` in the workspace `Cargo.toml`, so it is bumped in one place.

## Layout

- `crates/frontline/src/flow.rs`: screen states (main menu, in game, overlays), flow commands, Settings file
- `crates/frontline/src/menus/`: title, Host setup, Server Browser, Connecting, Pause, Settings (`settings/`: one file per tab) and Match Rules editor screens
- `crates/frontline/src/ui/`: shared menu look (olive/amber CoD palette, sharp rule lines, hover bars): fonts (Rajdhani for text, Saira Stencil One for titles and the big ammo count; both OFL, `assets/fonts/CREDITS.md`), buttons, panels, sliders, text fields, the result banner and `hud_text` (HUD text with a drop shadow)
- `crates/frontline/src/chat/`: the Y / U chat input and the fading chat box (client); the server side (validation, rate limit, system lines) is `net/chat.rs`
- `crates/frontline/src/match_hud/`: team select (Faction cards), top-centre score with Faction emblems, the objective HUD, Tab Scoreboard and end screen, friendly name tags
- `crates/frontline/src/combat/`: server-side shots, per-Hit-Zone hitboxes, damage, health/regen, death and respawn, spawn selection, the Match flow (Rotation, scores, end) and the Bevy systems of the objective modes (`mode_systems.rs`)
- `crates/frontline/src/modes/`: pure, unit-tested rules of Domination, Headquarters and Search & Destroy (capture, scoring, rounds), the replicated `ModeView`, and the client visuals (zone rings, markers, explosion, spectating)
- `crates/frontline/src/modes/crates.rs`, `dom.rs`: the ammo crates as pure, unit-tested rules (Team Deathmatch's fixed crates and their respawn timer; Domination's crate gating by Flag owner, radius and pulse rate); `crate_visuals.rs` draws them (a lidded crate that opens and lights up while usable); `combat/packs.rs` is the server side of the dead players' ammo packs
- `crates/frontline/src/rotation.rs`: Rotation entries, the server's `ServerConfig` and the Host setup that Settings remembers
- `crates/frontline/src/rules.rs`: Custom Match Rules: the `rules.ron` file, validation, naming, merging the custom sets into `GameData` (unit-tested)
- `crates/frontline/src/data.rs`: loads the `data/*.ron` tuning files
- `crates/frontline/src/net/`: lightyear protocol, server, client, Host/Join/Leave sessions, LAN discovery
- `crates/frontline/src/player/`: first-person controller, input bindings (rebindable, saved in Settings; `input.rs`), damage/kill-feed/death HUD, remote players drawn as Faction soldiers (`remote.rs`, `soldier.rs`) and their bodies (`corpse.rs`)
- `crates/frontline/src/weapon/`: local Weapon (firing, ammo, aiming, recoil, sway, hold breath, flinch, the bolt rack), per-weapon viewmodels (glTF gun models: the Styloo pack's for the SMG, Sniper and Pistol, and the M4 and M16A4 built from primitives by `tools/assets/build_rifles.py`; procedural gloved arms (two bones with an elbow solved each frame, blocky gloves that wrap the grip, the handguard, the magazine or the bolt knob) that follow every animation, a Grenade-in-hand throw, the left-handed mirror; `rig.rs` places each model, its hands and moving parts, and is what remote players' held guns use), shot effects, crosshair/hitmarker/ammo/scope/breath HUD
- `crates/frontline/src/weapon/trigger.rs`: the trigger as a pure state machine (Auto, Burst, Semi and Bolt timing, the semi interval, dry fire and Auto reload; unit-tested at any frame rate) and the underslung launcher mode (V toggles, click fires, rifle blocked while on)
- `crates/frontline/src/weapon/ammo.rs`: the magazine and the Reserve as pure, unit-tested rules (what a reload moves, what a refill adds, the LOW AMMO / NO AMMO hint)
- `crates/frontline/src/equipment/`: Grenades: the data (`data/equipment.ron`) and the pure, unit-tested rules (body stepping with bounces and the fuse, the throw arc by look angle, the hold-to-ready, release-to-throw state, blast falloff, flash and smoke exposure, the carried pouches); `client.rs` draws them, the throw keys, the HUD and the flash/smoke screen effects
- `crates/frontline/src/combat/grenades.rs`: the server flying every Grenade (sphere casts against the World), blast damage through the damage path, flashes and smoke
- `crates/frontline/src/gear.rs`: Attachments, Perks and the Loadout, and the pure rules that turn them into effective Weapon stats and Perk effects (unit-tested)
- `crates/frontline/src/class.rs`: Pick 10: the point budget and costs, Wildcards, equipment slots, class validation (budget, slot rules, Wildcard rules, unlocks) and trimming a class that doesn't fit (unit-tested)
- `crates/frontline/src/profile.rs`: the Profile file (`profile.ron`): custom Loadouts, the chosen one, XP and per-Weapon kill counters and Weapon XP, migrating old Profiles, and applying an award to them
- `crates/frontline/src/progression/`: XP values, the Level curve, the unlock table, Weapon Levels and camo Challenges (pure, unit-tested); client-side award handling, and the XP popups and toasts
- `crates/frontline/src/combat/xp.rs`: server XP awarding, the per-life damage ledger (Assists) and the Multikill chain
- `crates/frontline/src/ballistics.rs`: pure bullet flight for projectile Weapons (the exact gravity step, the closed-form path the clients draw tracers along, time of flight and drop), unit-tested; `combat/projectiles.rs` flies the server's bullets with it (`docs/adr/0005`)
- `crates/frontline/src/loadout/`: the Loadout screen, the Create a Class screens (`editor/`: rules in `model.rs`, drawing in `view.rs`), stat bars, the CoD swap rule
- `crates/frontline/src/maps/`: Map registry (`data/maps.ron`, private Maps), TrenchBroom loading (units, textures, colliders, entity classes), loading the Match's Map and swapping it (`ChangeMap`), spawn points
- `crates/frontline/src/world.rs`: physics
- `crates/frontline/src/cod4/`: bring-your-own CoD4 textures for Homage Maps: the archive index (`source.rs`), the IWI image decoder (`iwi.rs`, unit-tested) and the per-Map decode/override plumbing
- `crates/frontline/src/materials.rs`: material library (one tinted material per `SurfaceKind` texture set)
- `crates/frontline/src/atmosphere.rs` (+ `atmosphere/sky.rs`): camera look (tonemapping, plus bloom, SSAO, anti-aliasing and sun shadows from the Video quality settings) and per-Map look presets (`data/looks.ron`: grade, fog, sun, hemispherical ambient, vignette, cloud cover); every world camera draws the look's procedural sky (a generated cubemap with a gradient to the fog colour, the sun and noise clouds); brightness offsets the look's exposure
- `crates/frontline/src/particles.rs`: cheap particles (soft camera-facing puffs, sparks, bullet-hole decals) with procedural textures, used by `weapon/effects.rs` (muzzle flash, tracers, bullet impacts) and `equipment/client.rs` (frag fireball, flash pop, smoke cloud); how many there are follows the Video quality (bloom, SSAO and anti-aliasing together make Low, Medium or High detail)
- `crates/frontline/src/video.rs`: the Video settings (display mode, resolution, vsync, FPS cap, quality, brightness), applying them to the window, the "Keep these settings?" revert timer and the FPS limiter
- `crates/frontline/src/assets.rs`: files embedded in the binary (faction emblems, rank insignia, camo textures, surface textures; CC0 credits in `assets/textures/CREDITS.md`)
- `tools/mapgen/`: Python generator for `.map` files (the arena, Crossroads), its building kit and `lint.py`, the flicker linter
- `crates/frontline/src/lib.rs`: the game's modules as a library; `main.rs` is the game executable around it
- `crates/mapedit/`: MapEdit, the Map viewer and editor executable on the library's rendering (see "MapEdit"): `mapfile.rs` (the `.map` model: parse, write, brush geometry, primitives), `mapsron.rs` (text edits of `data/maps.ron`), `edit.rs` (working copy, undo, live preview, file commands, cursor and keys), `menu.rs` (the Esc menu), `palette.rs` (what can be placed, and the `P` panel), `remote.rs`, `session.rs`, `camera.rs`, `overlay.rs`; `tools/mapedit-mcp/` drives it from AI tooling
- `tools/camos/`: generates the weapon camo textures (`camos.py`, Pillow only)
- `tools/assets/`: imports prop models from raw asset packs (`import_props.py`, also their `bounds.json`)
- `tools/ci/package.sh`: packs a release build (executable, `assets/`, `data/`, README) into the archive the release workflow publishes
- `tools/ci/bump.sh`: bumps the version of both crates together (`patch`, `minor`, `major` or `X.Y.Z[-suffix]`), refreshes `Cargo.lock`, and with `--commit` commits it
- `crates/frontline/src/shared.rs`: pose and stance types shared by netcode and gameplay

Private content such as Homage Maps lives in the separate private repo `Bjeaurn/frontline-private`, cloned into the git-ignored `content/private/` (see `docs/adr/0003`):

```sh
git clone https://github.com/Bjeaurn/frontline-private.git content/private
```
