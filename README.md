# Frontline

A CoD4/MW2-inspired multiplayer FPS made for LAN parties. It's written in Rust on Bevy, lightyear and avian3d.

- Design: `IDEA.md`, `docs/PRD-lan-party.md`
- Glossary: `CONTEXT.md`
- Decisions: `docs/adr/`

## Run

One binary does everything. The default port is UDP 5000.

```sh
cargo run -- --host --name Alice                       # Host: server + play in this window
cargo run -- --connect 192.168.1.20:5000 --name Bob    # join a server
cargo run -- --dedicated                               # headless Dedicated Server
```

`--port <n>` changes the listen port. `--connect` defaults to `127.0.0.1:5000`. For faster rebuilds during development, add `--features dev` (dynamic linking).

## Controls

On joining, pick a Team with 1 / 2 and Enter (the smaller team is pre-selected). Left-click grabs the mouse and Esc releases it. Once grabbed, left-click fires.

| Action | Key |
|---|---|
| Move | WASD |
| Sprint | Left Shift (forward only, limited stamina) |
| Jump / stand up | Space |
| Mantle | Move into a ledge up to ~1.6 m (jumping helps) |
| Crouch | C |
| Prone | Left Ctrl or Z |
| Fire (full auto) | Left mouse |
| Aim down sights | Right mouse (hold) |
| Reload | R (sprinting cancels it) |
| Scoreboard | Tab (hold) |
| Switch Team | T (dying counts as a switch, no credit to anyone) |

Tuning lives in `data/`: `weapons.ron`, `movement.ron`, `match_rules.ron` and `game_modes.ron` (Team Deathmatch: 50 kills or 10 min). The game reads them at startup from `./data/` (relative to where you launch it), so edit and restart to retune. Without that folder it uses the copies built into the binary. Both machines should use the same files: damage and score are decided on the server, but movement, recoil, spread and fire rate come from each client's own copy.

## Layout

- `crates/frontline/src/match_hud/`: Team picker, top-centre score, Tab Scoreboard, friendly name tags
- `crates/frontline/src/combat/`: server-side shots, hitboxes, damage, health/regen, death and respawn, spawn selection
- `crates/frontline/src/data.rs`: loads the `data/*.ron` tuning files
- `crates/frontline/src/net/`: lightyear protocol, server, client, Host wiring
- `crates/frontline/src/player/`: first-person controller, input bindings, damage/kill-feed/death HUD, remote player visuals
- `crates/frontline/src/weapon/`: local Weapon (firing, ammo, aiming, recoil), viewmodel, shot effects, crosshair/hitmarker/ammo HUD
- `crates/frontline/src/world.rs`: greybox arena and spawn points
- `crates/frontline/src/shared.rs`: pose and stance types shared by netcode and gameplay

Private content such as Homage Maps goes in `content/private/`, which git ignores (see `docs/adr/0003`).
