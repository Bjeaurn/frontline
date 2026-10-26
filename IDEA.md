# Untitled

A fast paced, easy to pickup and learn, FPS shooter. Decent realism but more focus on gameplay and action, fun gunplay and intricate depth to customization.

Inspired by Modern Warfare 2 and the original COD4. Focus on multiplayer initially, LAN party compatible (so easy to setup a local server). For maps I want them to be inspired by the original COD4, MW and MW2 maps. Guns similarly. Killstreaks and customizability in your loadouts with perks, weapon customization etc that can be unlocekd through a fun mechanism.

Gamemodes from Team vs. Team deathmatch, FFA, bomb planting and defusal, headquarters, zone control and let's come up with some creative gamemodes ourselves too. 

The definition for a MVP would be Team deathmatch, 3-5 weapons with a different sight and silencer for example, 2 maps. LAN server and possibilities for lobby management (online and local).

## Decisions so far

- **Goal:** prove it's fun at an upcoming LAN party, less than 1 month away. Solo dev. A public release is undecided, so don't build for it yet.
- **Stack:** Rust, Bevy + lightyear (netcode) + avian3d (physics) + bevy_trenchbroom (TrenchBroom brush maps). Pin all versions on day 1 and don't upgrade before the party. Targets: Linux/macOS/Windows.
- **Topology:** one binary. "Host Game" runs a listen server; a `--dedicated` flag runs it headless. LAN UDP-broadcast **Server Browser** plus join-by-IP. Online lobby/relay is deferred.
- **Authority:** clients own their own movement, and the server decides hits (it raycasts each shot and applies damage). No prediction or lag compensation until we play over the internet. Anti-cheat is the honor system.
- **Lifecycle:** CoD-style. Players gather in a **Lobby**, then **Matches** cycle through the **Rotation**. **Join In Progress** auto-balances to the smaller team.
- **Modes:** Team Deathmatch + Domination. Domination gets cut first if gunfeel isn't fun by the end of week 2.
- **Player count:** design for 6–10 (3v3–5v5). Later target is 24–32, so Game Modes, Maps and Lobby sizes will need per-size tuning (e.g. max players per Map/Mode). Noted, not built yet.
- **Maps:** the party gets 2 **Homage Maps**, Backlot and Vacant (CoD4). They stay private: never in a public build or repo, because Activision DMCA'd CoD map ports in Oct 2026. Any public release uses **Remix Maps** instead.
- **Art:** CC0 low-poly (Quaternius/Kenney models, ambientCG/Poly Haven textures). First-person weapon animation is procedural (recoil, sway, ADS). Team colour comes from a tint. Audio is CC0 or cheap packs.
- **Weapons:** AR, SMG, Sniper, Shotgun as **Primaries**, pistol as **Secondary**. All hitscan. Stats are data-driven and can be tuned without recompiling.
- **Attachments:** two **Attachment Slots**, Optic (Red Dot, ACOG) and Muzzle (Silencer).
- **Progression:** party-compressed XP **Levels** (~10 in an evening) unlock **Weapons**, **Attachments** and **Perks**.
- **Perks:** 3 **Perk Slots** × 2 simple stat modifiers.
- **Killstreaks:** fixed **UAV** (3 kills) + **Airstrike** (5 kills). This needs a **Minimap** and a map-overview target picker.
- **Cut order** if behind schedule, first cut first: Levels (everything unlocked) → Airstrike → Domination → Backlot → Perks. Never cut: movement/gunfeel, LAN join, Team Deathmatch, Vacant, the 5 Weapons.
- **Time:** full-time (~35+ h/week). Gate: by the end of week 1, two machines on LAN are moving and shooting each other in a greybox. If that slips more than 3 days, apply cuts 1–3 immediately.
- **Fire Pings:** unsuppressed shots show on enemies' **Minimap** for ~1.5s. The Silencer removes the ping in exchange for less range.
- **Profile:** client-local file (name, XP, Level) sent on join. The server awards XP and the client saves it. Survives a change of Host.
- **Health/TTK default:** CoD core. 100 HP, regen after ~5s, AR kills in 3–4 body shots up close, ~1.4× headshots, damage falls off with range, 3s respawn, spawns avoid enemy line of sight. All of these are **Match Rules** values, not hardcoded. Each Game Mode has default Match Rules. **Hardcore** and health-pack variants are presets added later.
- **Movement:** walk, sprint (limited; Marathon extends it), crouch, prone, jump. ADS slows you. No slide, mantle or lean. Prone adds ~1–2 days for hitbox, camera and anim work.
- **Input:** keyboard + mouse only, with rebindable keys and a sensitivity setting. No controller. Set up a 3-OS CI build matrix (Linux/macOS/Windows) in week 1.
- **Teams:** free pick, including on **Join In Progress** (the smaller team is pre-selected). Stacking is allowed. Friendly fire is a **Match Rules** option, off by default.
- **Loadouts:** the **Profile** holds 3 custom and 3 default (AR/SMG/Sniper preset) **Loadouts**. They're editable in the menu or mid-Match. A change applies on the next spawn, or instantly within ~5s of spawning. The server trusts the Loadout and checks it against Level unlocks only loosely.
- **Domination:** CoD4 defaults. Solo capture takes ~10s; more teammates capture faster (capped at 3×). When both teams are on a Flag it's contested and progress freezes. An enemy Flag must be neutralized before you can capture it. +1 point per held Flag every 5s. Win at 200 points or 10 min. Spawns are biased toward held Flags. All values are data-driven.
- **XP (starting values, all configurable):** kill 100, headshot +50, Flag capture 150, Flag-defend kill +50, Match win 500, Match played 200. An **Assist** scales with damage dealt (e.g. ~1 XP per HP, so 99 HP ≈ 99). **Multikill** bonuses: double +50, triple +100, and so on. Level n needs ~1500×n. Unlocks: L1 AR/SMG/Sniper + Red Dot + 1 Perk per slot; L2 custom Loadouts; L3 Shotgun; L4 ACOG; L5 Silencer; L6–8 second Perks.
- **Killstreak chaining:** MW2 rule. Kills from a **Killstreak** count toward the next one. Airstrike team damage follows the friendly-fire **Match Rules** setting.
- **Airstrike targeting:** a full-screen top-down Map overview, click a point, and the strike lands ~3s later. The player stays vulnerable while the overview is open.
- **Platforms:** Windows, Linux, macOS are all confirmed at the party. CI builds all three.
- **Codename:** `frontline` (crate/binary name). Replaces the `mw2` folder name before anything goes public.
- **Deadline relaxed (2026-10-26):** build as far as we get; the party isn't a hard scope gate. The cut order now just sets priority.
