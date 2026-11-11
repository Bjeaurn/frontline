# Characters

`soldier.glb` is original work, built from scratch by `tools/assets/build_soldier.py` (which runs `tools/assets/blender_soldier.py` in headless Blender 5): the body, vest, backpack, pouches, knee pads, boots and every headgear are generated from code as lofted cross-sections and rounded boxes, then skinned with explicit bone weights, given per-vertex ambient occlusion and box-projected UVs. The 64 x 64 greyscale fabric texture is a seeded woven noise the same script writes. No third-party model, texture or scan went in, and it is not traced from any game's character. Like the repo's other own assets it is released under CC0 1.0.

- Triangles: 5,996 in all: body 4,204; headgear helmet 380, beret 116, boonie 192, cap 176, beanie 140, shemagh 220, ushanka 296; balaclava 92; goggles 180. A soldier draws the body and one headgear (plus the mask and goggles when its Faction's look asks), about 4,800 triangles at most, and the rifle.
- Height 1.82 m standing, facing -Z, feet at the origin.
- Bones (18, parents first): `hips`, `spine`, `chest`, `neck`, `head`, `shoulders` (the aim pivot the arms and the rifle hang from), `upper_arm_r`, `forearm_r`, `hand_r`, `upper_arm_l`, `forearm_l`, `hand_l`, `thigh_r`, `shin_r`, `foot_r`, `thigh_l`, `shin_l`, `foot_l`. The rest pose holds the rifle at the shoulder.
- Other nodes: `gun_mount` (an empty on `shoulders`: where the rifle's origin goes), `headgear_<helmet|beret|boonie|cap|beanie|shemagh|ushanka>`, `mask`, `goggles`, `soldier_body`.
- Materials: `uniform`, `gear`, `skin`, `headgear`, `mask` (tinted per Faction in `src/player/soldier.rs`), `fixed` (boots, gloves, straps, goggles) and `team` (armbands and patches, coloured friendly or enemy).

Rebuild: `python3 tools/assets/build_soldier.py` (add `--preview /tmp/prefix` for renders from several sides). `crates/frontline/src/player/soldier.rs` tests check the file against the bones and nodes the code names.
