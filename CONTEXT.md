# Untitled FPS

A fast-paced, CoD4/MW2-inspired multiplayer FPS, first aimed at being fun at a LAN party.

## Language

### Servers

**Host**:
A player whose game also runs the server that other players join.
_Avoid_: Owner, admin, listen server (as a player-facing term)

**Dedicated Server**:
A server run without a local player, started headless from the same game binary.
_Avoid_: Headless client, standalone server

**Server Browser**:
The in-game list of **Hosts** and **Dedicated Servers** discovered on the local network.
_Avoid_: Matchmaking, lobby list

### Play

**Lobby**:
The pre-game gathering on a server where players join before the first **Match** starts.
_Avoid_: Room, party, pre-game

**Match**:
One round of one **Game Mode** on one **Map**, ending at its score or time limit.
_Avoid_: Game, round, session

**Rotation**:
The ordered list of entries, each a **Map**, a **Game Mode** and a **Match Rules** preset, that the server cycles through after each **Match**.
_Avoid_: Playlist, map cycle

**Join In Progress**:
Entering a **Match** after it has started, choosing a team with the smaller team pre-selected.
_Avoid_: Hot join, late join

### Game Modes

**Game Mode**:
The objective of a **Match**: how teams form, how points are scored, and how it ends.
_Avoid_: Playlist, mode type, ruleset

**Match Rules**:
The tunable survival and HUD settings of a **Match**, such as max health, health regen or health packs, damage scale, respawn delay and HUD visibility.
_Avoid_: Ruleset, game settings, mutators

**Hardcore**:
A named **Match Rules** preset with low health, no regen and a minimal HUD, applicable to any **Game Mode**.
_Avoid_: Hardcore mode

**Team Deathmatch**:
A two-team **Game Mode** where each kill scores a point for the killer's team.
_Avoid_: TDM (in docs), deathmatch

**Domination**:
A two-team **Game Mode** where teams score over time for each **Flag** they hold.
_Avoid_: Control, conquest

**Flag**:
A fixed capturable area in **Domination**, labelled A, B or C.
_Avoid_: Zone, point, objective, capture point

**Team**:
One of the two sides in a team **Game Mode**; each player sees their own as friendly and the other as enemy.
_Avoid_: Side, squad

**Faction**:
The identity a **Team** plays as on a **Map**, set by the Map's theater: name, emblem and colours (e.g. Task Force Ranger, Red Brigade).
_Avoid_: Team (for the identity), army, clan

**Scoreboard**:
The view, held open with Tab and forced open at **Match** end, listing every player by **Team** with their score, kills and deaths.
_Avoid_: Leaderboard, stats screen

### Maps

**Map**:
A playable level that one or more **Game Modes** run on.
_Avoid_: Level, arena, stage

**Homage Map**:
A faithful recreation of an existing Call of Duty map, for private play only.
_Avoid_: Remake, port, copy

**Remix Map**:
An original **Map** that reuses the structure of classic Call of Duty maps (lanes, sightlines, central contested area) with its own layout and name.
_Avoid_: Inspired map, clone

### Movement

**Mantle**:
Climbing onto a ledge up to about chest height by moving into it.
_Avoid_: Vault, climb, ledge grab

### Weapons

**Weapon**:
A gun a player carries, either a **Primary** or a **Secondary**.
_Avoid_: Gun (in docs), item

**Primary**:
The main **Weapon** in a **Loadout**: assault rifle, SMG, sniper rifle or shotgun.
_Avoid_: Main weapon

**Secondary**:
The backup **Weapon** in a **Loadout**, currently a pistol.
_Avoid_: Sidearm

**Attachment**:
A modification fitted to one **Attachment Slot** on a **Weapon**.
_Avoid_: Mod, upgrade

**Attachment Slot**:
A position on a **Weapon** that holds at most one **Attachment**: Optic (e.g. Red Dot, ACOG) or Muzzle (e.g. Silencer).
_Avoid_: Rail, socket

**Hit Zone**:
The part of a body a bullet strikes: head, upper torso, lower torso or limbs (arms and legs). Each **Weapon** sets its own damage multiplier per zone.
_Avoid_: Hit location, body part (for the damage category)

### Loadouts & progression

**Loadout**:
A player's chosen **Primary**, **Secondary**, **Attachments** and **Perks**, applied at spawn.
_Avoid_: Class, kit, build

**Perk**:
A passive ability that occupies one **Perk Slot** in a **Loadout**.
_Avoid_: Skill, trait, buff

**Perk Slot**:
One of three positions in a **Loadout**, each holding exactly one **Perk** from that slot's own set.
_Avoid_: Perk tier

**Killstreak**:
A reward a player earns for consecutive kills without dying; the count resets on death.
_Avoid_: Scorestreak, streak reward

**UAV**:
A **Killstreak** that reveals enemy positions on the **Minimap** to the earner's team for a limited time.
_Avoid_: Radar, scan

**Airstrike**:
A **Killstreak** that bombards a point the earner picks on a map overview, after a short delay.
_Avoid_: Precision strike, bombing run

**Minimap**:
The in-HUD overhead view showing teammates and any enemies revealed by a **UAV** or a **Fire Ping**.
_Avoid_: Radar

**Fire Ping**:
A brief mark on enemies' **Minimap** where a player fired a **Weapon** without a Silencer.
_Avoid_: Red dot, shot reveal

**Level**:
A player's progression rank, raised by earning XP in **Matches**, which unlocks **Weapons**, **Attachments** and **Perks**.
_Avoid_: Rank, prestige

**Profile**:
A player's name and **Level** progress, owned by the player and carried to whichever server they join.
_Avoid_: Account, save, user

**Assist**:
Credit to a player who damaged an enemy that someone else killed, worth more the more damage they dealt.
_Avoid_: Helper kill

**Multikill**:
Two or more kills by one player in quick succession, rewarded with bonus XP.
_Avoid_: Combo, double/triple (as the general term)

## Relationships

- A server is run by exactly one **Host** or one **Dedicated Server**
- A server holds one **Lobby**, which leads into a continuous sequence of **Matches** driven by its **Rotation**
- The **Host** chooses the **Rotation**
- The **Server Browser** shows only servers on the local network; joining by address is the fallback
- A **Match** runs exactly one **Game Mode** on exactly one **Map** under one **Match Rules** preset
- **Domination** has exactly three **Flags** (A, B, C) per **Map**
- A **Loadout** has one **Primary**, one **Secondary**, and one **Perk** per **Perk Slot**
- A **Weapon** has an Optic and a Muzzle **Attachment Slot**, each optionally filled
- A **Profile** belongs to one player and is independent of any **Host**
- A **Profile** holds three custom and three default **Loadouts**

## Flagged ambiguities

- "Online lobby management" (IDEA.md MVP) is deferred: the first playable target covers local network play only.
- "Session" was used loosely for both a server's lifetime and a single round — resolved: use **Match** for a round; a server's lifetime needs no term.
- "New teams" meant new identities, not more than two sides — resolved: a **Match** always has two **Teams**; each plays as a **Faction**, and new Factions are added as data.
- "Level" could mean a playable area or a progression rank — resolved: **Level** is the progression rank; playable areas are **Maps**.
- "Hardcore mode" suggested a separate **Game Mode** — resolved: **Hardcore** is a **Match Rules** preset combinable with any **Game Mode**.
- **Join In Progress** first meant automatic placement on the smaller team — resolved: joiners choose a team, with the smaller one pre-selected.
