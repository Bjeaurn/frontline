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
```

Servers listen on UDP 5000 (`--port <n>` changes it; it's also the port HOST GAME uses). Every server answers LAN discovery on UDP 5001; a second server on the same machine can't take that port, so it logs a warning and only shows up when joined by IP. `--name` sets your player name for this run (overriding Settings) or a Dedicated Server's name in the browser. `--rules <preset>` picks the Match Rules preset a server (Dedicated, or Host) runs, from `data/match_rules.ron`: `default` unless given, or `hardcore` (30 HP, no regen, friendly fire, no crosshair, hitmarkers or damage direction, kill feed 6 s late). An unknown name stops it at startup with the list of presets. Clients take the HUD settings of that preset from their own copy of the file. For faster rebuilds during development, add `--features dev` (dynamic linking).

Settings (name, mouse sensitivity, field of view, master volume) are saved to `<config dir>/frontline/settings.ron`: `~/Library/Application Support/` on macOS, `~/.config/` on Linux, `%APPDATA%` on Windows.

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

Tuning lives in `data/`: `weapons.ron` (including per-Weapon Hit Zone damage multipliers), `loadouts.ron` (Default Loadouts and the instant-swap window), `movement.ron`, `match_rules.ron` (presets, each with its HUD block), `game_modes.ron` (Team Deathmatch: 50 kills or 10 min) and `presentation.ron` (how long bodies stay and how many at once). The game reads them at startup from `./data/` (relative to where you launch it), so edit and restart to retune. Without that folder it uses the copies built into the binary. Both machines should use the same files: damage and score are decided on the server, but movement, recoil, spread and fire rate come from each client's own copy.

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
- `crates/frontline/src/world.rs`: greybox arena and spawn points
- `crates/frontline/src/shared.rs`: pose and stance types shared by netcode and gameplay

Private content such as Homage Maps goes in `content/private/`, which git ignores (see `docs/adr/0003`).
