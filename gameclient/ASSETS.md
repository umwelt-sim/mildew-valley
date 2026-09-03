# Sprites

The client draws with the **Cute Fantasy** pack by Kenmi.

<https://kenmi-art.itch.io/cute-fantasy-rpg>

## License, and what it does and does not stop you doing

The premium tier's readme:

> - You can use these assets in any commercial or non-commercial projects.
> - You can modify the assets.
> - You can not redistribute or resale, even if modified

"No redistribution" means you may not hand anyone the assets *as assets*. It
does not stop you shipping a game built with them — that is the licensed use.

| | |
|---|---|
| Ship the art inside a game build, or as a folder beside the binary | fine |
| Commit the sheets to this public repository | **not** fine |
| Publish them anywhere as an asset pack, modified or not | **not** fine |

So `assets/` is in `.gitignore` and each checkout fetches its own copy, while a
release can carry the art with it like any other game.

Both tiers require credit. The client shows "art by Kenmi" as a startup notice
that fades after a few seconds, which discharges it without spending permanent
screen real estate.

## Setup

Buy the premium tier (pay at least \$3.99 — the free tier is non-commercial and
has no crops), then extract so the tree looks like this:

```
mildew-valley/
  assets/
    cute-fantasy/       <- the Cute_Fantasy archive
    cute-fantasy-ui/    <- the Cute_Fantasy_UI archive, optional
```

Run from the workspace root; the paths are relative to the working directory.

**Nothing here is required to build or run.** Without the pack the client draws
flat stand-in shapes and says so on screen. That is deliberate: this repository
is public so people can try the engine, and evaluating the networking should not
cost \$3.99. The UI pack is likewise optional — it only supplies the typeface.

## Layout notes

The numbers that are not what you would guess, all verified against the files:

| Sheet | Cell | Grid | Notes |
|---|---|---|---|
| `Player/**` | **64x64** | 9 x 56 | Not 32x32 and not 16x32. The figure sits in the middle, around x 25-38, y 21-41; the rest is room for tool swings and mounts. |
| Player rows | | | 0 idle down, 1 idle side, 2 idle up, 3 walk down, 4 walk side, 5 walk up, 6 frames each. Later rows are actions. |
| `Crops/Crops.png` | 16x16 | 7 x 44 | 22 crops on **odd rows**; the even row above each is overflow for tall plants. Columns: field sign, seed jar, **4 growth stages**, harvested item. |
| `Tiles/FarmLand/FarmLand_Tile.png` | 16x16 | 7 x 8 | Full blob set. The nine-slice starts at **column 1, row 0**. |
| `Tiles/Water/Water_Tile_1.png` | 16x16 | 3 x 5 | Nine-slice at **0,0**. |
| `Tiles/Grass/Grass_Tiles_1.png` | 16x16 | 16 x 10 | Path-in-grass nine-slice at **column 0, row 5**. |
| `Outdoor decoration/Fences.png` | 16x16 | 4 x 4 | Column 2 of row 0 is the run that tiles seamlessly. Column 1 leaves gaps. |

Those nine-slice offsets were found by matching the free tier's own 3x3 sheets
against the premium blob sets, not by eye.

## Crops, and which one is lettuce

The 22 crops are: wheat, tomato, carrot, eggplant, corn, pumpkin, turnip,
**lettuce**, cucumber, chilli, and red, orange and green peppers, broccoli,
sunflower, garlic, potato, strawberry, radish, onion, leek, grapes.

The pack does not name them — they are addressed by row. **Lettuce is row 15**,
a round pale-green head that grows from a sprout through a rosette. Four growth
stages, which is what `mildew_common::tags::lettuce` already has and what
`PhasedDef` already models, so nothing in the simulation had to change.

## How a farmer is built

`assets.rs` composites rather than tints. The pack ships a bare base plus
pixel-aligned layers, all 576x3584 and registered with each other:

```
Player_Base_animations.png      skin, and the only layer that is recolored
  + Feet/Shoes_1_*.png          8 colors
  + Legs/Farmer_Pants/*.png     8 colors
  + Chest/Farmer_Shirt/*.png    8 colors
  + Head/Hair_{1..6}/*.png      6 styles x 5 colors
```

Twenty-four farmers are stacked at load, cropped to a shared box computed
across all of them, and packed into one atlas so a crowd of any size is one
batch. Only skin is palette-swapped, because the pack ships a single tone.

A farmer's appearance derives from their entity id, so every client draws the
same person the same way and someone stays recognizable after walking out of
view and back.

## Not yet used

Plenty is installed and unwired: cliffs, beaches, bridges, caves, waterfalls,
animated water, buildings, premade NPCs, item icons, weather effects, the other
seasonal packs, and the UI frames and buttons. Most of it wants collision and a
richer map before it would help.
