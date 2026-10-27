# Sound credits

Mono Ogg Vorbis (`oggenc -q 4`), trimmed, faded out and peak-normalised to about -1.5 dB from the
original WAVs. Both packs are free to use in a game; the sources stay in `content/assets/` (git-ignored).

| File | Source | Original |
|---|---|---|
| `shot_ar.ogg` | Snake's Authentic Gun Sounds And More | `Isolated/5.56/556 Single Isolated WAV.wav`, first 0.85 s |
| `shot_smg.ogg` | Snake's Authentic Gun Sounds And More | `Isolated/.22LR/22LR Single Isolated WAV.wav`, 0.135 s to 0.735 s (the lightest round in the pack, for the SMG) |
| `shot_sniper.ogg` | Snake's Authentic Gun Sounds And More | `Isolated/7.62x54R/762x54r Single Isolated WAV.wav` |
| `dry_fire.ogg` | free-guns-sounds-audio (moneywithjjcom, itch.io) | `pistol-dry-fire.wav` |
| `mag_insert.ogg` | free-guns-sounds-audio | `rifle-mag-insert.wav` |
| `impact.ogg` | free-guns-sounds-audio | `impact-concrete-1.wav`, first 0.6 s |
| `ricochet.ogg` | free-guns-sounds-audio | `ricochet-1.wav` |

free-guns-sounds-audio is royalty-free for commercial and non-commercial use with no attribution
required (its README); the files may be committed as part of a project but not redistributed on
their own, which the embedded game assets are not.

Which Weapon plays which shot is `shot_sound` in `data/weapons.ron`.

The M9 Pistol reuses `shot_smg.ogg`. A Silencer plays a shot quieter and at a lower speed (`data/attachments.ron`). Remote players' footsteps are `impact.ogg` at a low pitch and volume (there is no footstep recording yet).
