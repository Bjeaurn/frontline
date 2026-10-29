# Sniper bullets are server-simulated projectiles, everything else stays hitscan

ADR 0002 has the server resolve a shot with one raycast the moment it arrives. For a sniper that makes range free: a 300 m shot lands the instant the trigger is pulled and the only skill is aim. We want long shots to need lead and hold-over, so a Weapon can now carry an optional projectile model in `weapons.ron` (`projectile: Some((muzzle_speed: 600.0, gravity: 12.0))`, about 550-650 m/s, game-tuned rather than physical). Only the sniper rifle has one; every other Weapon keeps the ADR 0002 hitscan path unchanged.

A client still reports one `FireShot` (origin, direction with spread and recoil already applied). For a projectile Weapon the server doesn't raycast. It spawns a bullet with the muzzle velocity and advances it every fixed tick (64 Hz) with the exact constant-acceleration step (`position += v*dt + g*dt²/2`, `v += g*dt`, gravity pointing down). Each tick's segment is cast against the World and the hitboxes as they are posed that tick, excluding the shooter's own. The first hit ends the bullet, or it ends at the Weapon's `range` as path length. A player hit becomes the same `PlayerHit` as a hitscan hit, with `distance` the path length flown, so damage falloff, Hit Zone multipliers, headshots, Perks, Attachments, friendly fire and XP are all the existing code. The stepping lives in pure functions (`ballistics.rs`) that the server and the clients share.

Clients see the same path. The server sends `ProjectileFired` (id, shooter, Weapon, origin, muzzle velocity) to every client, including the shooter, when the bullet spawns and `ProjectileLanded` (id, point, whether it struck something) when it ends, on the reliable event channel. In between each client draws a tracer streak flying along `position_at(origin, velocity, gravity, t)`, the same closed form the server steps through, so it needs no per-tick updates. Impact sounds play at the landing point. The shooter's own client no longer draws an instant tracer or plays an instant impact for a projectile Weapon, since the bullet's real arrival is what they see; the muzzle flash and gunshot stay immediate.

## Considered Options

- **Keep hitscan, fake the delay:** wait `distance / speed` after a raycast made at fire time. Cheap, but the ray is cast at the target's old position, so a mover can't be led and dodging the shot can't work, which is the entire point.
- **Simulate on the shooter's client and report hits:** no flight-time awareness on the server and it breaks ADR 0002's rule that damage and score live in one place.
- **Physics-engine bodies (avian rigid bodies with continuous collision):** heavier, non-deterministic across ticks and hard to test. A bullet is a point moving in a known arc.

## Consequences

- Lag behaviour stays simple on purpose. The server hits against the hitboxes of the tick the bullet is in, never rewound ("no lag compensation" per ADR 0002). On a LAN (about 1 ms) that is invisible next to 0.5 s of flight; over a real network a shooter would lead by their latency. Internet play would need the lag-compensation work ADR 0002 already calls for, now with rewound hitboxes per tick of flight.
- Clients are trusted: the muzzle velocity and gravity come from the shared data file, not the message; `ProjectileFired` carries the velocity only so every client draws the very path the server flies.
- A bullet fired just before the Match ends, or before its shooter dies, still lands if the Match is live; once the Match ends all bullets in flight are dropped. A tracer whose `ProjectileLanded` never arrives (the shooter's own client lost it) expires by itself after `range / muzzle_speed` plus a margin.
- The server's per-tick cost is one raycast per bullet in flight; a sniper fires about once every 2 s, so a full lobby stays well inside the budget.
- Future work fits in the same place: wind is another acceleration in `ballistics::step`, and other long-range Weapons opt in by adding a `projectile` model, with no netcode change.
