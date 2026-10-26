# PRD: LAN Party Build

Terms follow `CONTEXT.md`. Decisions respect `docs/adr/0001`–`0003`.

## Problem Statement

A LAN party is less than a month away. The group wants a fast-paced, CoD4/MW2-style multiplayer FPS they can install on mixed Linux/macOS/Windows machines, start without internet or accounts, and enjoy for a full evening. Nothing exists yet: no engine project, no netcode, no Maps, no Weapons. The developer works solo and full-time, and hasn't used the chosen engine before. The real question to answer is: is this fun enough to keep building?

## Solution

A single game binary. Any player can start a **Host** game or run it as a **Dedicated Server**. Others find it in the **Server Browser** on the local network, or join by address. Players gather in a **Lobby**, then play a continuous **Rotation** of **Matches** of **Team Deathmatch** and **Domination** on two **Homage Maps**, Vacant and Backlot. Each player builds **Loadouts** from 5 **Weapons** with Optic/Muzzle **Attachments** and 3 **Perk Slots**. They earn the **UAV** and **Airstrike** **Killstreaks**, and climb about 10 **Levels** across the evening to unlock gear. Their **Profile** travels with them between servers. Combat follows CoD core feel (fast time-to-kill, regenerating health), and every value is tunable through **Match Rules** and data files without recompiling.

## User Stories

### Starting and joining

1. As a player, I want to click "Host Game" and be playing within seconds, so that setting up a server at the party takes no technical effort.
2. As a technical player, I want to run the same binary headless with a dedicated flag, so that a spare machine can serve Matches without anyone losing their seat.
3. As a player, I want a **Server Browser** that lists every **Host** and **Dedicated Server** on the local network, so that I can join without typing addresses.
4. As a player, I want to join by IP address, so that I can still connect when broadcast discovery is blocked by the network.
5. As a player on Linux, macOS or Windows, I want to play with players on any other of these systems, so that the OS mix at the party doesn't matter.
6. As a player, I want to see server name, current **Map**, **Game Mode** and player count in the **Server Browser**, so that I can pick where to join.

### Lobby, Rotation and Matches

7. As a **Host**, I want to set the **Rotation** (Map + Game Mode + Match Rules preset per entry), so that the evening flows without me restarting things.
8. As a player, I want to wait in a **Lobby** until the Host starts the first **Match**, so that everyone can gather first.
9. As a player, I want the next Match to start automatically after a ~10s scoreboard, so that downtime between Matches stays short.
10. As a latecomer, I want to **Join In Progress**, so that I can drop into a Match already running.
11. As a joining player, I want to pick my team with the smaller team pre-selected, so that I can play with friends but am nudged toward balance.
12. As a player, I want to switch teams between Matches, so that we can reshuffle sides.
13. As a player, I want a scoreboard (kills, deaths, Assists, score) during and after a Match, so that I can see how I'm doing and trash-talk accordingly.

### Movement and combat

14. As a player, I want to walk, sprint (limited), crouch, go prone and jump, so that movement feels like CoD4/MW2.
15. As a player, I want aiming down sights to slow me and tighten accuracy, so that there's a mobility versus precision trade-off.
16. As a player, I want my movement to respond instantly, so that the game feels snappy.
17. As a player, I want hits decided in one place, so that we don't argue about who shot first.
18. As a player, I want headshots to deal ~1.4× damage and damage to fall off with range, so that accuracy and positioning matter.
19. As a player, I want 100 HP that regenerates after ~5s without taking damage, so that fights are fast and forgiving, the CoD way.
20. As a player, I want to respawn after ~3s at a spawn away from enemy sightlines, so that I get back into action without being spawn-killed.
21. As a player, I want procedural recoil, sway and ADS animation, so that guns feel weighty despite simple art.
22. As a player, I want distinct gun sounds and hit feedback (hitmarkers, damage direction), so that combat reads clearly.
23. As a player, I want team members tinted in a team colour, so that I can tell friend from foe instantly.

### Weapons and Loadouts

24. As a player, I want an assault rifle, SMG, sniper rifle and shotgun as **Primaries** and a pistol as **Secondary**, so that every range on both Maps has a good choice.
25. As a player, I want to fit an Optic (Red Dot or ACOG) and a Muzzle (Silencer) **Attachment**, so that I can customise how each Weapon plays.
26. As a player, I want the Silencer to hide my **Fire Pings** at the cost of range, so that stealth is a real trade-off.
27. As a player, I want 3 default **Loadouts** (AR, SMG, Sniper) available from Level 1, so that I can play right away.
28. As a player, I want 3 custom Loadouts I edit in the menu or mid-Match, so that I can experiment.
29. As a player, I want a Loadout change to apply on my next spawn, or instantly if I'm within ~5s of spawning, so that I can fix a bad pick right away.
30. As a player, I want to choose one **Perk** in each of 3 **Perk Slots** (Sleight of Hand/Scavenger, Stopping Power/Lightweight, Marathon/Ninja), so that I can shape my playstyle.

### Game Modes

31. As a player, I want **Team Deathmatch**, where each kill scores for my team, so that we have a simple default mode.
32. As a player, I want **Domination** with **Flags** A, B and C, so that we have an objective mode.
33. As a Domination player, I want capture to take ~10s alone and go faster with teammates (capped at 3×), so that grouping up pays off.
34. As a Domination player, I want capture progress to freeze when both teams are on a Flag, so that contesting matters.
35. As a Domination player, I want enemy Flags to neutralize before I can capture them, so that flips take commitment.
36. As a Domination player, I want +1 point per held Flag every 5s, a win at 200 points or 10 minutes, and spawns biased toward our Flags, so that holding ground pays off.

### Minimap and Killstreaks

37. As a player, I want a **Minimap** showing teammates, so that I know where support is.
38. As a player, I want enemies firing unsuppressed Weapons to appear on my Minimap for ~1.5s, so that noise gives away positions.
39. As a player, I want a **UAV** at 3 kills that reveals enemies on my team's Minimap, so that streaks help my team.
40. As a player, I want an **Airstrike** at 5 kills, where I pick a point on a map overview and it lands after a short delay, so that I get a big payoff moment.
41. As a player, I want Killstreak kills to count toward my next Killstreak (MW2 chaining), so that hot streaks feel huge.
42. As a player, I want my Killstreak count to reset on death, so that streaks are earned.

### Progression

43. As a player, I want XP for kills (100), headshots (+50), Flag captures (150), Flag-defend kills (+50), Match wins (500) and Matches played (200), so that both fragging and playing the objective pay.
44. As a player, I want **Assist** XP scaled by the damage I dealt, so that nearly killing someone still counts.
45. As a player, I want **Multikill** bonuses (double, triple, ...), so that clutch moments are rewarded.
46. As a player, I want to reach about Level 10 over one party evening, so that I unlock everything by the end.
47. As a player, I want Levels to unlock custom Loadouts (L2), Shotgun (L3), ACOG (L4), Silencer (L5) and second Perks (L6–8), so that I always have something to chase.
48. As a player, I want my **Profile** (name, XP, Level, Loadouts) saved on my own machine, so that it follows me when someone else hosts.
49. As a player, I want an end-of-Match XP summary and level-up notice, so that progress feels visible.

### Settings and tuning

50. As a player, I want rebindable keys and a mouse sensitivity setting, so that controls feel right on my setup.
51. As a **Host**, I want to choose a **Match Rules** preset per Rotation entry (health, regen, respawn delay, damage scale, friendly fire, HUD), so that we can try variants later, such as **Hardcore**.
52. As the developer, I want Weapon stats, Match Rules, XP values and Domination values in data files that reload without recompiling, so that I can tune balance during the party.
53. As the developer, I want friendly fire off by default and configurable per Match Rules, with Airstrike team damage following the same setting, so that the rules stay consistent.

## Implementation Decisions

- **Stack (ADR 0001):** Rust with Bevy, lightyear (networking), avian3d (physics) and bevy_trenchbroom (Maps authored in TrenchBroom). All versions are pinned as one compatible set on day 1, with no upgrades before the party. Targets: Linux, macOS, Windows. A 3-OS CI build matrix is set up in week 1.
- **Topology:** one binary with three roles: client, Host (listen server plus local player, using lightyear's host-server mode), and Dedicated Server (headless flag). LAN discovery uses UDP broadcast, with join-by-address as fallback. No online lobby or relay.
- **Trust model (ADR 0002):** clients own their own movement and replicate their transforms and stance. A client reports each shot as tick, origin and direction. The server raycasts against current hitboxes, applies damage, and owns all score, Killstreak and XP awards. No client prediction or lag compensation. Profiles and Loadouts come from the client and are only loosely checked against unlocks.
- **Pure logic modules.** These have no engine or network dependency, take simple inputs, and are deterministic:
  - **Damage Model:** weapon stats + fitted Attachments + Perks + distance + hit zone + Match Rules → damage dealt. Owns falloff, the headshot multiplier, Silencer range penalty, Stopping Power and damage scale.
  - **Game Mode Rules:** tick-driven state for Team Deathmatch and Domination. Takes kill events, presence on Flags and elapsed time. Produces team scores, Flag states (owner, progress, contested, neutral) and the Match end with its winner. Capture speed, scoring interval and limits come from data.
  - **Killstreak Tracker:** consumes kill and death events per player, including kills made by Killstreaks (chaining). Emits earned Killstreaks at the configured thresholds (UAV 3, Airstrike 5). Resets on death.
  - **Progression:** consumes Match events (kill, headshot, damage share for Assists, Multikill windows, Flag capture/defend, win, played). Produces XP awards, Level, and newly unlocked Weapons, Attachments, Perks and Loadout slots, using a data-driven curve (~1500×n) and unlock table.
  - **Spawn Selector:** candidate spawn points, enemy positions and line-of-sight queries (passed in as a function), and the Game Mode bias (held Flags in Domination) → the chosen spawn point.
  - **Profile Store:** a versioned, client-local file holding name, XP, Level and 3 custom Loadouts, with load, save and migration. Corrupt or missing files fall back to a fresh Profile.
- **Engine-bound modules:** Server Hit Resolution (avian raycasts and hit zones per stance, including prone), Netcode Protocol (lightyear messages and replicated components), LAN Discovery, Server Session (Lobby, Rotation, Join In Progress, team pick, end-of-Match scoreboard and transition), Player Controller (walk, sprint, crouch, prone, jump, ADS slow), FP Presentation (procedural recoil, sway, ADS; team tint), HUD and menus (Server Browser, Loadout editor, scoreboard, Minimap with Fire Pings and UAV reveal, Airstrike target overview, XP summary, settings), Map Loader (TrenchBroom entities for spawns per team/mode and Flags A/B/C), Tunable Data (hot-reloaded data files).
- **Match Rules** is a data-defined preset (max health, regen on/off and delay, health packs, damage scale, respawn delay, friendly fire, HUD visibility). Each Game Mode names a default preset, and Rotation entries can override it. Hardcore is a preset, not a Game Mode.
- **Maps (ADR 0003):** Vacant and Backlot are **Homage Maps**. They live in a private content location separate from anything that might become public.
- **Art:** CC0 low-poly characters and weapons, CC0 textures, CC0 or cheap audio packs. No hand-keyed first-person animation.
- **Schedule gate:** by the end of week 1, two machines on LAN are moving and shooting each other in a greybox. If that slips more than 3 days, apply cuts 1–3 below immediately.
- **Cut order**, first cut first: (1) Levels, with everything unlocked; (2) Airstrike; (3) Domination; (4) Backlot; (5) Perks. Never cut: movement and gunfeel, LAN join, Team Deathmatch, Vacant, the 5 Weapons.

## Testing Decisions

- Good tests drive a module only through its public interface with plain inputs, and assert on observable outputs: damage numbers, scores, Flag states, awards, Levels, the chosen spawn, round-tripped Profiles. They don't assert on internal fields, call order, or exact data-file values that are expected to be tuned.
- Unit-tested modules: Damage Model, Game Mode Rules, Killstreak Tracker, Progression, Spawn Selector, Profile Store.
- Examples of high-value cases:
  - Damage Model: falloff boundaries, headshot versus body, Silencer range penalty, Stopping Power, Match Rules damage scale.
  - Game Mode Rules: contested Flag freezes, neutralize-before-capture, capture speed cap at 3×, score limit versus time limit ending, ties.
  - Killstreak Tracker: thresholds, chaining via Killstreak kills, reset on death.
  - Progression: Assist scaling by damage share, Multikill windows, level-up across several thresholds in one Match, unlock table.
  - Spawn Selector: avoids enemy line of sight, prefers held Flags, falls back when every spawn is exposed.
  - Profile Store: round-trip, version migration, corrupt-file fallback.
- There's no prior art yet; this is a greenfield repo. Tests use the standard Rust test harness, colocated per module.
- Netcode, the Server Browser, rendering, UI and Map flow are verified by manual multi-machine LAN playtests, starting at the week-1 gate. There's no automated multi-client test.

## Out of Scope

- Online play: internet lobbies, relays, NAT punch-through, matchmaking, accounts.
- Client prediction, lag compensation, anti-cheat, server-side Profile authority.
- Free-for-all, Search & Destroy, Headquarters, and custom Game Modes.
- **Remix Maps**, and any public release or distribution.
- Pro Perks, choosable Killstreaks, Killstreaks beyond UAV and Airstrike, Care Packages.
- Slide, mantle, lean; controller support; aim assist.
- Bullet drop and projectile ballistics (all Weapons are hitscan).
- Prestige, challenges, and cosmetics.
- Supporting 24–32 players. Noted as a later target that will need per-size tuning of Maps, Game Modes and Lobby sizes.
- Hardcore and health-pack presets beyond the Match Rules structure existing.

## Further Notes

- Codename `frontline` (crate and binary). The `mw2` folder name and any CoD naming are Activision trademarks and must go before anything is public.
- Windows, Linux and macOS are all confirmed at the party; CI builds all three.
- Airstrike team damage follows the friendly-fire **Match Rules** setting.
- Airstrike targeting: a full-screen top-down Map overview where the player clicks a point; the strike lands ~3s later, and the player stays vulnerable meanwhile.
- The deadline was relaxed on 2026-10-26: build as far as we get. The schedule gate and cut order now just set priority.
