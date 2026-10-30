# Surface textures

All textures come from [ambientCG](https://ambientcg.com) and are licensed under
[CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/) (public domain; no
attribution required, credited anyway).

Concrete, wood and ground were swapped for grittier sets (weathered grey concrete, grey-brown
planks, dry stony earth) and resampled to the size of the set they replaced (`concrete` 1024x512,
the others 1024x1024), so the tiling that `tools/mapgen` bakes into the Maps stays right.

Each folder keeps the colour, OpenGL normal (`NormalGL`) and roughness maps of the 1K-JPG
download. Colour and normal stay at the source resolution (1024 px on the long side), roughness is
downscaled to 512 px; all were re-encoded as JPEG with ImageMagick.

| Folder        | ambientCG asset                                                        |
|---------------|------------------------------------------------------------------------|
| `concrete/`   | [Concrete042A](https://ambientcg.com/view?id=Concrete042A)             |
| `plaster/`    | [PaintedPlaster017](https://ambientcg.com/view?id=PaintedPlaster017)   |
| `metal/`      | [PaintedMetal012](https://ambientcg.com/view?id=PaintedMetal012)       |
| `corrugated/` | [CorrugatedSteel005](https://ambientcg.com/view?id=CorrugatedSteel005) |
| `wood/`       | [Planks023A](https://ambientcg.com/view?id=Planks023A)                 |
| `ground/`     | [Ground109](https://ambientcg.com/view?id=Ground109)                   |
| `brick/`      | [Bricks105](https://ambientcg.com/view?id=Bricks105)                   |
