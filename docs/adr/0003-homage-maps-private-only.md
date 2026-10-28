# Homage Maps are private-only; public builds use Remix Maps

The first Maps (Backlot and Vacant) are Homage Maps: faithful recreations of Call of Duty 4 layouts, chosen because they're proven fun at 6–10 players and need no level-design iteration. In October 2026 Activision issued DMCA takedowns against fan ports of CoD maps. So Homage Maps must never ship in a public build or sit in a public repository. Any public release uses Remix Maps: original layouts that reuse the classic structure.

## Consequences

- Homage Map files live in a separate private repo, `frontline-private`, cloned into `content/private/` (git-ignored by the main repo). The game loads Maps from there when present, so the main repo can go public without scrubbing history.
- Original CoD4 textures for Homage Maps are bring-your-own from the player's own install, never stored (see `0004-cod4-textures-bring-your-own.md`).
