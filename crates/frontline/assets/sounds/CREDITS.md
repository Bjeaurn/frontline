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
| `footstep_concrete_000.ogg` to `007` | Kenney, Impact Sounds 1.0 (CC0), processed by `tools/audio/footsteps.py` | Built from `Audio/footstep_concrete_000.ogg` to `004` (read from `content/assets/Kenney Impact Sounds.zip`): pitch-shifted, tilted towards the highs, layered with a synthesized heel click (high-passed noise, a few ms), a softer toe click and a low thump, 0.3 s, mono, 48 kHz, peak about -1.5 dB. Each variant uses its own pitch, click and thump |

free-guns-sounds-audio is royalty-free for commercial and non-commercial use with no attribution
required (its README); the files may be committed as part of a project but not redistributed on
their own, which the embedded game assets are not.

Which Weapon plays which shot is `shot_sound` in `data/weapons.ron`.

The M9 Pistol reuses `shot_smg.ogg`. A Silencer plays a shot quieter and at a lower speed (`data/attachments.ron`). Footsteps are a random one of the eight `footstep_concrete` files at ±6% speed (`player/footsteps.rs`). Impact Sounds is CC0, so it needs no credit and may be redistributed.

`ring.ogg` is synthesized (a 3.15 kHz sine with a long fade, `ffmpeg -f lavfi -i sine=f=3150:d=6`): the ringing after a flashbang. Grenade bangs reuse `impact.ogg` and `shot_sniper.ogg` at low speed. `shot_ar.ogg` and `shot_smg.ogg` were trimmed to 0.5 s and 0.4 s with a fade-out so fast bursts don't smear.
