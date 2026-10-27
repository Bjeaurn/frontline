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
One play of one **Game Mode** on one **Map**, ending at its score or time limit.
_Avoid_: Game, session

**Rotation**:
The ordered list of **Rotation entries** that the server cycles through after each **Match**; a single entry repeats.
_Avoid_: Playlist, map cycle

**Rotation entry**:
One item of a **Rotation**: a **Map**, a **Game Mode** the Map supports and a **Match Rules** preset. The **Host** builds the Rotation on the Host setup screen.
_Avoid_: Playlist item, map slot

**Join In Progress**:
Entering a **Match** after it has started, choosing a team with the smaller team pre-selected.
_Avoid_: Hot join, late join

### Game Modes

**Game Mode**:
The objective of a **Match**: how teams form, how points are scored, and how it ends.
_Avoid_: Playlist, mode type, ruleset

**Match Rules**:
The tunable survival and HUD settings of a **Match**, such as max health, health regen or health packs, damage scale, respawn delay, fall damage and HUD visibility.
_Avoid_: Ruleset, game settings, mutators

**Hardcore**:
A named **Match Rules** preset with low health, no regen and a minimal HUD, applicable to any **Game Mode**.
_Avoid_: Hardcore mode

**Custom rule set**:
A named **Match Rules** set a **Host** makes by copying a preset in the rules editor and changing its values. It is saved on the Host's machine, selectable per **Rotation entry**, and its values reach the clients with the Match, so they need no copy of it. The built-in presets stay read-only.
_Avoid_: Mod, mutator, custom mode

**Team Deathmatch**:
A two-team **Game Mode** where each kill scores a point for the killer's team.
_Avoid_: TDM (in docs), deathmatch

**Domination**:
A two-team **Game Mode** where teams score over time for each **Flag** they hold.
_Avoid_: Control, conquest

**Flag**:
A fixed capturable area in **Domination**, labelled A, B or C.
_Avoid_: Zone, point, objective, capture point

**Headquarters**:
A **Game Mode** where one **HQ** at a time is captured and defended: the team holding it scores every second and its players don't respawn until the other team destroys it or it expires and relocates.
_Avoid_: HQ mode, radio, hardpoint

**HQ**:
The capturable site of **Headquarters**, active at one of the **Map**'s HQ sites at a time for about a minute.
_Avoid_: Radio, hill

**Search & Destroy**:
A round-based **Game Mode** without respawns: attackers plant the **Bomb** at a **Bomb Site** and defenders stop them, over several **Rounds**.
_Avoid_: S&D (in docs), bomb mode

**Round**:
One short fight of **Search & Destroy**, won by elimination, detonation, defusal or the clock; a **Match** is several Rounds, and attackers and defenders swap sides at half.
_Avoid_: Match, game

**Bomb**:
The single explosive of **Search & Destroy**, carried by one attacker, dropped on their death, planted at a **Bomb Site** and defused by a defender.
_Avoid_: C4, explosive, charge

**Bomb Site**:
A fixed area, labelled A or B, where the **Bomb** can be planted.
_Avoid_: Plant zone, target

**Team**:
One of the two sides in a team **Game Mode**; each player sees their own as friendly and the other as enemy.
_Avoid_: Side, squad

**Faction**:
The identity a **Team** plays as on a **Map**, set by the Map's theater: name, emblem and colours (e.g. Task Force Ranger, Harmattan Legion).
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
Climbing onto a ledge up to about chest height by moving into it. Steps and ramps up to 0.45 m (the stairs) are not Mantles: they are simply walked.
_Avoid_: Vault, climb, ledge grab

**Dive**:
Pressing prone while sprinting: a short forward lunge into the prone stance, then a brief recovery with no moving, firing or standing.
_Avoid_: Slide, roll

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
A player's chosen **Primary**, **Secondary**, **Attachments**, **Perks** and **Camo**, applied at spawn.
_Avoid_: Class, kit, build

**Perk**:
A passive ability that occupies one **Perk Slot** in a **Loadout**.
_Avoid_: Skill, trait, buff

**Perk Slot**:
One of three positions in a **Loadout**, each holding exactly one **Perk** from that slot's own set.
_Avoid_: Perk tier

**Create a Class**:
The editor, opened from the Loadout screen, where a player fills one of the **Profile**'s custom **Loadouts**: **Primary** with its **Attachments**, **Secondary**, one **Perk** per **Perk Slot** and a name.
_Avoid_: Class builder, loadout editor (as the in-game name)

**Ammo Pack**:
A pickup a player drops on dying; walking over it refills the ammo of a player with the Scavenger **Perk**.
_Avoid_: Ammo crate, drop

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

**XP**:
The experience points the server awards a player for kills, **Assists**, **Multikills**, objectives and finishing **Matches**. Each award names its reason; the player's **Profile** stores the total.
_Avoid_: Score (that is the **Team** points), points, exp

**Level**:
A player's progression rank, derived from their XP (level n+1 needs n×1500 XP in all), up to a maximum of 30; reaching it unlocks **Weapons**, **Attachments**, **Perks** and custom **Loadouts**.
_Avoid_: Rank, prestige

**Unlock**:
An item (a **Weapon**, **Attachment**, **Perk** or the custom **Loadouts**) that becomes usable at a given **Level**; anything not in the unlock table is there from Level 1, and Default **Loadouts** are always usable. The **Create a Class** editor shows what is locked and at which Level.
_Avoid_: Reward, perk (for a Level reward)

**Camo**:
A skin on a **Primary**'s body in the viewmodel, chosen per **Loadout** and unlocked per **Weapon** by a **Challenge**; Woodland, Desert, Urban, Tiger, Arctic and Gold ship.
_Avoid_: Skin, paint, finish

**Challenge**:
What a **Weapon** must do to unlock a **Camo**: a number of kills or headshot kills with it, or (Gold) every other **Camo** unlocked on it. The **Profile** counts kills and headshot kills per **Weapon**.
_Avoid_: Mastery, achievement

**Profile**:
A player's name, **Loadouts**, **XP** and per-**Weapon** kill counts, owned by the player and carried to whichever server they join; the server awards **XP** and the client stores it.
_Avoid_: Account, save, user

**Assist**:
Credit to a player who damaged an enemy that someone else killed during that enemy's life, paid **XP** in proportion to the damage dealt.
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
- **Headquarters** needs at least one HQ site on the **Map**; **Search & Destroy** needs two **Bomb Sites** (A, B)
- A **Rotation entry** only names a **Game Mode** its **Map** supports
- A **Loadout** has one **Primary**, one **Secondary**, and one **Perk** per **Perk Slot**
- A **Primary** has an Optic and a Muzzle **Attachment Slot**, each optionally filled; a scoped Primary (the sniper rifle) has no Optic slot, and a **Secondary** has no Attachment Slots
- A **Profile** belongs to one player and is independent of any **Host**
- A **Profile** holds three custom and three default **Loadouts**; the default ones are read-only and the custom ones are edited through **Create a Class**
- A player's **Level** follows from the **XP** in their **Profile**; custom **Loadouts** and each **Unlock** wait for their **Level**
- A **Camo** is unlocked on one **Weapon** at a time by its **Challenge**, and a **Loadout**'s Camo goes on its **Primary**
- A dead player drops an **Ammo Pack** that a player with the Scavenger **Perk** picks up by walking over it

## Flagged ambiguities

- "Online lobby management" (IDEA.md MVP) is deferred: the first playable target covers local network play only.
- "Session" was used loosely for both a server's lifetime and a single play — resolved: use **Match** for a play; a server's lifetime needs no term.
- "Round" could mean a whole **Match** or one fight of **Search & Destroy** — resolved: **Round** is only the Search & Destroy fight; a **Match** is the whole play.
- "New teams" meant new identities, not more than two sides — resolved: a **Match** always has two **Teams**; each plays as a **Faction**, and new Factions are added as data.
- "Level" could mean a playable area or a progression rank — resolved: **Level** is the progression rank; playable areas are **Maps**.
- "Hardcore mode" suggested a separate **Game Mode** — resolved: **Hardcore** is a **Match Rules** preset combinable with any **Game Mode**.
- **Join In Progress** first meant automatic placement on the smaller team — resolved: joiners choose a team, with the smaller one pre-selected.
