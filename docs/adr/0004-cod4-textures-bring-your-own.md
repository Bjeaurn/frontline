# Original CoD4 textures are bring-your-own, never stored

Homage Maps look most like the originals with CoD4's own textures, but those are Activision's copyrighted files, and the October 2026 DMCA takedowns targeted exactly assets exported from the original games. We don't import, convert into, or commit them anywhere: not the main repo, not `frontline-private`, not a build we hand out. Instead, the game reads them at runtime from each player's own CoD4 installation (the `iw_*.iwd` archives in its `main/` folder, or a local copy the player points to), decodes them in memory, and applies them only to Homage Maps. Every other surface, and every player without CoD4, gets the CC0 textures.

## Considered Options

- **Reference only:** match the look with CC0 textures, never touching the originals. Safest, but Homage Maps look less like CoD4.
- **Commit converted textures to the private repo:** simplest technically, but it stores and redistributes Activision files.

## Consequences

- Each LAN player who wants the original look needs CoD4 installed and points the game at it (Settings). Mixed setups are fine: textures are cosmetic, so players without CoD4 see CC0 surfaces on the same geometry.
- The private repo holds only a name mapping from our map texture names to CoD4 image names, never image data.
- Raw downloads stay in the git-ignored `content/assets/`.
