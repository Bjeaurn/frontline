# Bevy-based Rust stack instead of a full editor engine

We build on Bevy with lightyear (networking), avian3d (physics) and bevy_trenchbroom (Maps authored in TrenchBroom). Godot, Unreal and Unity would reach a first playable sooner for a dev new to all of them. We chose Rust anyway: it's the developer's preferred language, it targets Linux/macOS/Windows natively, and TrenchBroom brushwork suits boxy CoD-style Maps. Fyrox was rejected because its networking is TCP-only and self-described as basic.

## Consequences

- No visual scene editor. Maps live in TrenchBroom; everything else is code and data files.
- Bevy and its plugins break APIs roughly every quarter, so all versions are pinned as a compatible set and not upgraded before a deadline.
