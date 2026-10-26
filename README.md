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

Left-click grabs the mouse and Esc releases it. Once grabbed, left-click fires.

| Action | Key |
|---|---|
| Move | WASD |
| Sprint | Left Shift (forward only, limited stamina) |
| Jump / stand up | Space |
| Crouch | C |
| Prone | Left Ctrl or Z |
| Fire (full auto) | Left mouse |
| Aim down sights | Right mouse (hold) |
| Reload | R (sprinting cancels it) |

Weapon and Match Rules tuning lives in `data/weapons.ron` and `data/match_rules.ron`. The game reads them at startup from `./data/` (relative to where you launch it), so edit and restart to retune. Without that folder it uses the copies built into the binary. Both machines should use the same files: damage is decided on the server, but recoil, spread and fire rate come from each client's own copy.

## Layout

- `crates/frontline/src/combat/`: server-side shots, hitboxes, damage, health/regen, death and respawn, spawn selection
- `crates/frontline/src/data.rs`: loads the `data/*.ron` tuning files
- `crates/frontline/src/net/`: lightyear protocol, server, client, Host wiring
- `crates/frontline/src/player/`: first-person controller, input bindings, damage/kill-feed/death HUD, remote player visuals
- `crates/frontline/src/weapon/`: local Weapon (firing, ammo, aiming, recoil), viewmodel, shot effects, crosshair/hitmarker/ammo HUD
- `crates/frontline/src/world.rs`: greybox arena and spawn points
- `crates/frontline/src/shared.rs`: pose and stance types shared by netcode and gameplay

Private content such as Homage Maps goes in `content/private/`, which git ignores (see `docs/adr/0003`).
