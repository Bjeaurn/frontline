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
| `grass/`      | [Grass001](https://ambientcg.com/view?id=Grass001)                     |
| `plaster_yellow/` | [PaintedPlaster018](https://ambientcg.com/view?id=PaintedPlaster018) |
| `plaster_ochre/` | [PaintedPlaster007](https://ambientcg.com/view?id=PaintedPlaster007) |
| `plaster_blue/` | [PaintedPlaster008](https://ambientcg.com/view?id=PaintedPlaster008) |
| `plaster_white/` | [PaintedPlaster016](https://ambientcg.com/view?id=PaintedPlaster016) |
| `plaster_green/` | [PaintedPlaster009](https://ambientcg.com/view?id=PaintedPlaster009) |
| `brick_old/` | [Bricks097](https://ambientcg.com/view?id=Bricks097) |
| `stone/` | [Bricks109A](https://ambientcg.com/view?id=Bricks109A) |
| `brick_hollow/` | [Bricks088](https://ambientcg.com/view?id=Bricks088) |
| `concrete_damaged/` | [Concrete044C](https://ambientcg.com/view?id=Concrete044C) |
| `wood_painted/` | [PaintedWood007B](https://ambientcg.com/view?id=PaintedWood007B) |
| `wood_siding/` | [WoodSiding011](https://ambientcg.com/view?id=WoodSiding011) |
| `asphalt/` | [Road012A](https://ambientcg.com/view?id=Road012A) |
| `sand/` | [Ground054](https://ambientcg.com/view?id=Ground054) |
| `gravel/` | [Gravel022](https://ambientcg.com/view?id=Gravel022) |
| `paving/` | [PavingStones128](https://ambientcg.com/view?id=PavingStones128) |
| `paving_brick/` | [PavingStones152](https://ambientcg.com/view?id=PavingStones152) |
| `tiles/` | [Tiles141](https://ambientcg.com/view?id=Tiles141) |
| `wood_floor/` | [WoodFloor041](https://ambientcg.com/view?id=WoodFloor041) |
| `roof_tiles/` | [RoofingTiles012B](https://ambientcg.com/view?id=RoofingTiles012B) |
| `roof_slate/` | [RoofingTiles013B](https://ambientcg.com/view?id=RoofingTiles013B) |
| `metal_rust/` | [MetalPlates011](https://ambientcg.com/view?id=MetalPlates011) |
| `metal_green/` | [PaintedMetal006](https://ambientcg.com/view?id=PaintedMetal006) |
| `metal_white/` | [PaintedMetal011](https://ambientcg.com/view?id=PaintedMetal011) |
| `corrugated_rust/` | [CorrugatedSteel007C](https://ambientcg.com/view?id=CorrugatedSteel007C) |
| `corrugated_blue/` | [CorrugatedSteel007B](https://ambientcg.com/view?id=CorrugatedSteel007B) |

The sets from `plaster_yellow/` on are imported by `tools/assets/import_textures.py` (which lists the asset behind each folder): square, 1024 px colour and normal, 512 px roughness.
