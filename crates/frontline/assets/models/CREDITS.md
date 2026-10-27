# Prop models

## `downtown/`

Pieces of the **Downtown City MegaKit [Standard]** by [Quaternius](https://quaternius.com),
licensed [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/) (public domain; no
attribution required, credited anyway): facades, complete buildings, street tiles, road decals and
street furniture.

Only the pieces the built-in Maps use are here. `tools/assets/import_props.py` made each `.glb`
from the kit's glTF files: one file per piece with its geometry in the binary chunk, every texture
downscaled to at most 1024 px and re-encoded as JPEG (PNG where it has transparency). The kit's few
textures are shared by all pieces and live in `downtown/textures/`.
