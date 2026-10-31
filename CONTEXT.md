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
The identity a **Team** plays as on a **Map**, set by the Map's theater: name, emblem, colours and the soldier look (uniform, headgear) other players see (e.g. Task Force Ranger, Harmattan Legion).
_Avoid_: Team (for the identity), army, clan

**Scoreboard**:
The view, held open with Tab and forced open at **Match** end, listing every player by **Team** with their **Rank insignia**, **Level**, name, kills and deaths.
_Avoid_: Leaderboard, stats screen

**Chat**:
Short text messages between players, typed with Y (to everyone) or U (to the player's **Team**) and shown in a small fading box at the bottom left, together with system lines: level-ups ("X reached Level N"), joins, leaves and **Team** switches. The server limits messages to 120 characters and about 3 per 3 seconds per player. A level-up line carries the **Level** a client reported (trusted, ADR 0002).
_Avoid_: Messages, comms, text box

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
_Avoid_: Climb, ledge grab

**Vault**:
Crossing a thin wall or window sill (sill 0.6 to 1.2 m high, wall up to 0.6 m thick) in one move by jumping into it, with floor to land on behind. A thin wall is vaulted, anything else is Mantled. Every generated window has the standard sill (0.9 m) and opening (1.4 m tall, 1 m wide at least), so any of them can be vaulted.
_Avoid_: Window climb

**Dive**:
Pressing prone while sprinting: a short forward lunge into the prone stance, then a brief recovery with no moving, firing or standing.
_Avoid_: Slide, roll

### Weapons

**Weapon**:
A gun a player carries, either a **Primary** or a **Secondary**.
_Avoid_: Gun (in docs), item

**Projectile Weapon**:
A **Weapon** whose bullet the server simulates with travel time and drop instead of landing instantly (hitscan): today only the sniper rifle (ADR 0005). Targets at range must be led and held over.
_Avoid_: Ballistic weapon

**Primary**:
The main **Weapon** in a **Loadout**: assault rifle, SMG, sniper rifle or shotgun.
_Avoid_: Main weapon

**Secondary**:
The backup **Weapon** in a **Loadout**, a pistol, or a **Primary** with the Overkill **Wildcard**.
_Avoid_: Sidearm

**Attachment**:
A modification fitted to one **Attachment Slot** on a **Weapon**.
_Avoid_: Mod, upgrade

**Attachment Slot**:
A position on a **Weapon** that holds at most one **Attachment**: Optic (e.g. Red Dot, Holographic, Prism, ACOG), Muzzle (e.g. Silencer) or Underbarrel (e.g. Foregrip).
_Avoid_: Rail, socket

**Magnification**:
The zoom number of a sight (iron sights and Red Dot 1x, Holographic 1.5x, Prism 2x, ACOG 4x, the Sniper's scope 8x). It sets the aimed field of view, and from 3x it allows **Hold Breath** and shows a **Glint**. Each sight also has a **Sway Scale**.
_Avoid_: Zoom level, scope power

**Sway**:
The slow figure-eight drift of the view while aimed, by a per-**Weapon** angle times the sight's **Sway Scale**, less while crouched or prone or holding breath. It moves the aim itself, so shots follow it.
_Avoid_: Wobble, bob (bob is the viewmodel's walking motion)

**Handedness**:
Which hand a player holds the gun in, in their own first-person view (Settings > Gameplay: Right or Left). Left mirrors the viewmodel and nothing else: aim, shots and the replicated pose are the same, so other players still see a right-hander. The arms in the view are only a visual prop.
_Avoid_: Left-handed mode (it is a Setting, not a mode)

**Flinch**:
The kick a hit gives the victim's view, by a per-**Weapon** angle scaled by the damage and reduced while aimed; it recovers like recoil.
_Avoid_: Hit reaction, screen shake

**Hold Breath**:
Shift while aimed through a sight of 3x or more: **Sway** drops to 0.15 for up to 4 s, then 2 s out of breath with **Sway** at 1.6; the tank only refills while not holding.
_Avoid_: Steady aim, breath control

**Glint**:
The bright star other players see at the scope of someone aimed through a sight of 3x or more, when they are within about 25 degrees of where that player aims.
_Avoid_: Lens flare, scope flare

**Rack**:
Working the bolt of a bolt-action **Weapon** after a shot; it can't fire until the bolt is closed. A click while aimed racks slowly and keeps the sights up (the next click fires); letting go of the sights racks faster by itself.
_Avoid_: Cycle, chamber

**Hit Zone**:
The part of a body a bullet strikes: head, upper torso, lower torso or limbs (arms and legs). Each **Weapon** sets its own damage multiplier per zone.
_Avoid_: Hit location, body part (for the damage category)

**Fire Mode**:
How a **Weapon**'s trigger fires: Auto (rounds while held), Burst (n rounds per pull at the burst rate, then a longer delay before the next burst), Semi (one round per pull, never faster than the Weapon's semi interval) or Bolt (one round, then a **Rack**). Each Weapon lists the modes it has in weapons.ron; X cycles them and the last one used on each Weapon is kept in the **Profile**.
_Avoid_: Firing mode, trigger mode

**Dry Fire**:
Pulling the trigger on an empty magazine: a click per pull and a flashing RELOAD hint, but no reload; R reloads. The Gameplay setting Auto reload turns the pull into a reload.
_Avoid_: Empty click

**Grenade**:
A physical projectile the server flies with gravity, bounces and a fuse: the lethal Frag (blast damage that falls off with distance), the tactical Flashbang (blinds a viewer facing it, deafens everyone near) and Smoke (a cloud that blocks the view for a while), and the Grenade Launcher's 40mm round. A thrown one is readied by holding its key and thrown when the key is let go, along an arc that depends on the look angle (far at the horizon, farther looking up, a short toss looking down).
_Avoid_: Nade, bomb (a **Bomb** is the Search & Destroy objective)

**Grenade Launcher**:
The underslung "noob tube" Attachment (Underbarrel slot, 2 **Class Points**) only the M16A4 offers. It is a mode of the M16A4 (V switches it in and out, the HUD shows `40MM` where the **Fire Mode** goes, and the rifle cannot fire meanwhile); a click fires its 40mm round, which arcs and bursts on impact, 3 rounds per life. Dying, switching **Weapon** or opening a menu returns to the rifle.
_Avoid_: Noob tube (in docs), GL

### Loadouts & progression

**Loadout**:
A player's chosen **Primary**, optional **Secondary**, **Attachments**, **Perks**, equipment, **Wildcards** and **Camo**, within a budget of **Class Points**, applied at spawn. In the editor and the menus it is called a class.
_Avoid_: Kit, build

**Class Points** ("Pick 10"):
The budget every **Loadout** has (10) and what each choice costs: a **Primary** 1, a **Secondary** 1, each **Attachment** 1 (an Optic of 3× or more, and the grenade launcher, 2), each **Perk** 1, each piece of equipment 1 and each **Wildcard** 1. Everything but the **Primary** is optional, so dropping something frees points. The costs live in `data/class_points.ron`.
_Avoid_: Cost, credits, slots (for the budget)

**Wildcard**:
A **Loadout** choice, costing a point, that breaks one slot rule: Perk 1, 2 or 3 Greed (a second **Perk** in that **Perk Slot**), Gunfighter (a third **Attachment** on the **Primary**) or Overkill (a **Primary** as the second **Weapon**). At most three per **Loadout**, each unlocked at an account **Level**.
_Avoid_: Perk (a Wildcard isn't one), modifier

**Perk**:
A passive ability that occupies one **Perk Slot** in a **Loadout**.
_Avoid_: Skill, trait, buff

**Perk Slot**:
One of three positions in a **Loadout**, each holding one **Perk** from that slot's own set, or two with that slot's Perk Greed **Wildcard**.
_Avoid_: Perk tier

**Create a Class**:
The three-level editor, opened from the main menu (CREATE A CLASS) or from the Loadout screen in game, where a player fills one of the **Profile**'s custom **Loadouts** within its **Class Points**: a class list, a slot overview (big tiles for **Primary** with its **Attachments**, **Secondary**, equipment, **Perks**, **Wildcards**, plus a name) and a full-screen picker per slot.
_Avoid_: Class builder, loadout editor (as the in-game name)

**Reserve**:
The spare rounds a **Weapon** carries besides the magazine in the gun: `reserve_mags` full magazines (the HUD shows `30 / 120`). A reload moves rounds from it into the magazine; with it empty a reload does nothing. Each **Weapon** keeps its own, and a respawn restores it. The launcher's 3 rounds are not part of it.
_Avoid_: Infinite ammo, spare ammo, reserve ammo

**Ammo Pack**:
A pickup a player drops on dying; any living player walking over it gets a share of every **Weapon**'s maximum **Reserve** (about one spare magazine), a player with the Scavenger **Perk** the full refill (**Reserve** and magazines).
_Avoid_: Ammo crate, drop

**Ammo Crate**:
A place that resupplies players from the map rather than from a death: at each captured **Flag** in Domination (slowly, while a player of the holding **Team** stands within its radius) and, fixed on the **Map** (the `ammo_crate` entity), in Team Deathmatch (a full resupply on walking up, then closed for a respawn timer). Headquarters and Search & Destroy have none.
_Avoid_: Ammo pack, supply drop

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
A player's account progression rank, derived from their XP (level n+1 needs n×1500 XP in all), up to a maximum of 30; reaching it unlocks **Weapons**, **Perks**, equipment, **Wildcards** and custom **Loadouts**. Not to be confused with a **Weapon Level**.
_Avoid_: Rank, prestige

**Rank insignia**:
The small worn cloth patch that shows a player's **Level** on the **Scoreboard**: chevrons for Levels 1-10, bars for 11-20 and stars for 21-30, with one more of them each Level inside a band. Drawn by `tools/ranks/ranks.py` in the style of the **Faction** emblems. Each client reports its **Level** to the server, which replicates it so everyone sees it.
_Avoid_: Rank icon, badge, prestige

**Weapon Level**:
A **Weapon**'s own progression rank (0 to 5), derived from its **Weapon XP**: each kill with it gives 1 **Weapon XP** (+1 for a headshot) and the levels come at 5, 15, 30, 50 and 75. Each level unlocks that **Weapon**'s next **Attachments** (e.g. M4 level 3: Holographic); a **Weapon Level** is separate from the account **Level**.
_Avoid_: Mastery, weapon rank

**Unlock**:
An item (a **Weapon**, **Perk**, piece of equipment, **Wildcard** or the custom **Loadouts**) that becomes usable at a given **Level**, or an **Attachment** at a given **Weapon Level**; anything not in the unlock table is there from Level 1, and Default **Loadouts** are always usable. The **Create a Class** pickers show what is locked and which track and level unlocks it (`Level 8`, `M4 level 3`).
_Avoid_: Reward, perk (for a Level reward)

**Camo**:
A skin on a **Primary**'s body in the viewmodel, chosen per **Loadout** and unlocked per **Weapon** by a **Challenge**; Woodland, Desert, Urban, Tiger, Arctic and Gold ship.
_Avoid_: Skin, paint, finish

**Challenge**:
What a **Weapon** must do to unlock a **Camo**: a number of kills or headshot kills with it, or (Gold) every other **Camo** unlocked on it. The **Profile** counts kills and headshot kills per **Weapon**; camos are listed after the **Attachments** in a **Weapon**'s track.
_Avoid_: Mastery, achievement

**Profile**:
A player's name, **Loadouts**, **XP** and per-**Weapon** kill counts and **Weapon XP**, owned by the player and carried to whichever server they join; the server awards **XP** and the client stores it.
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
- A **Loadout** costs at most 10 **Class Points**; it has one **Primary**, optionally one **Secondary** (a **Primary** with Overkill), a **Perk** per **Perk Slot** (two with that slot's Perk Greed), optionally a lethal and a tactical piece of equipment, and up to three **Wildcards**
- A **Weapon** fires through exactly one sight at a time (its own, or the fitted Optic); that sight's **Magnification** and Sway Scale drive aiming
- A **Weapon** has at least one **Fire Mode** and starts in the first it lists (or the one last used on it, from the **Profile**); a **Dry Fire** pull never reloads unless Auto reload is on
- Only the M16A4 offers the **Grenade Launcher**; its rounds, like a thrown **Grenade**, are simulated by the server
- A **Primary** has an Optic, a Muzzle and an Underbarrel **Attachment Slot**, each optionally filled, but carries two **Attachments** at most (three with Gunfighter); a scoped Primary (the sniper rifle) has no Optic slot, and a **Secondary** has no Attachment Slots
- Each **Weapon** offers its own **Attachments**, each unlocked at a **Weapon Level**; the sniper rifle offers no Optics
- A **Profile** belongs to one player and is independent of any **Host**
- A **Profile** holds three custom and three default **Loadouts**; the default ones are read-only and the custom ones are edited through **Create a Class**
- A player's **Level** follows from the **XP** in their **Profile**; custom **Loadouts** and each **Unlock** wait for their **Level**
- A **Weapon Level** follows from the **Weapon XP** in the **Profile** and unlocks that **Weapon**'s **Attachments**; its **Camos** are unlocked separately by their **Challenges**, counted in kills
- A **Camo** is unlocked on one **Weapon** at a time by its **Challenge**, and a **Loadout**'s Camo goes on its **Primary**
- A dead player drops an **Ammo Pack** that any living player picks up by walking over it, for a share of their **Reserve** (a player with the Scavenger **Perk**: the full refill)
- An **Ammo Crate** resupplies only the **Team** holding its **Flag** (Domination) or whoever walks up to it (Team Deathmatch); only those two **Game Modes** have them

## Flagged ambiguities

- "Online lobby management" (IDEA.md MVP) is deferred: the first playable target covers local network play only.
- "Session" was used loosely for both a server's lifetime and a single play — resolved: use **Match** for a play; a server's lifetime needs no term.
- "Round" could mean a whole **Match** or one fight of **Search & Destroy** — resolved: **Round** is only the Search & Destroy fight; a **Match** is the whole play.
- "New teams" meant new identities, not more than two sides — resolved: a **Match** always has two **Teams**; each plays as a **Faction**, and new Factions are added as data.
- "Level" could mean a playable area or a progression rank — resolved: **Level** is the progression rank; playable areas are **Maps**.
- "Hardcore mode" suggested a separate **Game Mode** — resolved: **Hardcore** is a **Match Rules** preset combinable with any **Game Mode**.
- **Join In Progress** first meant automatic placement on the smaller team — resolved: joiners choose a team, with the smaller one pre-selected.
