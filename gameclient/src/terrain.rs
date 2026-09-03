//! The ground under everyone's feet.
//!
//! The simulation owns entities, not scenery, so the client has to decide for
//! itself what the world looks like. Rather than ship a map, every tile is a
//! pure function of its coordinates: the same seed on every client puts the
//! same field in the same place, so two players describing a landmark are
//! talking about the same one, and nothing has to be sent to agree on it.
//!
//! The layout repeats on a fixed block so the world stays walkable in any
//! direction without anyone deciding where its edges are.

/// Side of one repeating block, in meters. One meter is one tile.
const BLOCK: i32 = 32;

/// What is underfoot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Grass,
    Farm,
    Path,
    Water,
}

/// Scrambles a pair of coordinates into something that looks unpatterned.
pub fn hash2(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x1656_67b1) ^ (y as u32).wrapping_mul(0x27d4_eb2d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_f491);
    h ^ (h >> 13)
}

/// What sits at a tile. Total, deterministic, and cheap enough to call for
/// every tile on screen and each of their neighbors, every frame.
pub fn tile_at(tx: i32, ty: i32) -> Tile {
    let bx = tx.rem_euclid(BLOCK);
    let by = ty.rem_euclid(BLOCK);

    // Two-meter lanes along the north and west edges of every block, which
    // meet to form a continuous grid of roads.
    if bx < 2 || by < 2 {
        return Tile::Path;
    }

    let block = hash2(tx.div_euclid(BLOCK), ty.div_euclid(BLOCK));
    let field = |a: i32, b: i32| (4..=14).contains(&a) && (4..=14).contains(&b);

    // Three fields to a block, with the fourth quarter left open as a commons
    // — which is where crowds collect — or given over to a pond.
    if field(bx, by) || field(bx - 13, by) || field(bx, by - 13) {
        return Tile::Farm;
    }
    if block % 3 == 0 && (19..=27).contains(&(bx - 13)) && (19..=27).contains(&(by - 13)) {
        return Tile::Water;
    }
    Tile::Grass
}

/// Which of a nine-slice's cells a tile needs, as `row * 3 + col`.
///
/// Edges are chosen by whether the neighbor on each side is the same
/// material, so a field's border resolves without anyone authoring corners.
/// North is `ty + 1`, which is the cell drawn above this one.
pub fn slice9(tx: i32, ty: i32, kind: Tile) -> usize {
    let same = |x: i32, y: i32| tile_at(x, y) == kind;
    let col = if !same(tx - 1, ty) {
        0
    } else if !same(tx + 1, ty) {
        2
    } else {
        1
    };
    let row = if !same(tx, ty + 1) {
        0
    } else if !same(tx, ty - 1) {
        2
    } else {
        1
    };
    row * 3 + col
}

/// Whether a fence rail runs across this tile.
///
/// Fields are fenced where they meet open ground, which is what makes a block
/// read as somebody's farm rather than a patch of dirt. Roads are left clear,
/// so a lane is never walled off.
pub fn fence_at(tx: i32, ty: i32) -> bool {
    tile_at(tx, ty) == Tile::Grass
        && (tile_at(tx, ty - 1) == Tile::Farm || tile_at(tx, ty + 1) == Tile::Farm)
}

/// Livestock on open ground: `true` for a cow, `false` for a chicken.
pub fn animal_at(tx: i32, ty: i32) -> Option<bool> {
    if tile_at(tx, ty) != Tile::Grass || tree_at(tx, ty).is_some() {
        return None;
    }
    match hash2(tx ^ 0x6b1f, ty ^ 0x33a7) % 61 {
        0 => Some(true),
        1 | 2 => Some(false),
        _ => None,
    }
}

/// A prop to scatter on a grass tile, as an index into the atlas's decor list,
/// or nothing. Deterministic, so scenery does not crawl as the camera moves.
pub fn decor_at(tx: i32, ty: i32, kinds: usize) -> Option<usize> {
    if tile_at(tx, ty) != Tile::Grass || kinds == 0 || fence_at(tx, ty) {
        return None;
    }
    let h = hash2(tx ^ 0x5f37, ty ^ 0x1d9b);
    if h % 100 < 34 {
        Some((h >> 8) as usize % kinds)
    } else {
        None
    }
}

/// Whether a tree stands on this tile, and whether it is the large one.
///
/// Trees are kept off farmland and roads so they never cover a crop or block
/// a lane, and thinned so a walk between blocks is not a forest.
pub fn tree_at(tx: i32, ty: i32) -> Option<bool> {
    if tile_at(tx, ty) != Tile::Grass {
        return None;
    }
    let h = hash2(tx ^ 0x2ac1, ty ^ 0x7e15);
    match h % 23 {
        0 => Some(true),
        1 | 2 => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_is_the_same_everywhere_it_repeats() {
        // Two clients looking at the same coordinate must agree, and the
        // pattern must survive negative coordinates.
        for (x, y) in [(0, 0), (7, 19), (-40, 3), (1_000, -1_000), (-7, -19)] {
            assert!(tile_at(x, y) == tile_at(x, y));
            assert!(tile_at(x, y) == tile_at(x + BLOCK, y + BLOCK));
        }
    }

    #[test]
    fn roads_run_unbroken_along_every_block_edge() {
        for k in 0..BLOCK * 3 {
            assert!(matches!(tile_at(k, 0), Tile::Path), "a lane broke at x={k}");
            assert!(matches!(tile_at(0, k), Tile::Path), "a lane broke at y={k}");
        }
    }

    #[test]
    fn a_block_holds_farmland_and_open_ground() {
        let mut farm = 0;
        let mut open = 0;
        for y in 0..BLOCK {
            for x in 0..BLOCK {
                match tile_at(x, y) {
                    Tile::Farm => farm += 1,
                    Tile::Grass | Tile::Water => open += 1,
                    Tile::Path => {}
                }
            }
        }
        assert!(farm > 200, "too little farmland: {farm}");
        assert!(open > 100, "nowhere to stand: {open}");
    }

    #[test]
    fn nothing_is_planted_on_a_road_or_in_a_pond() {
        for y in -60..60 {
            for x in -60..60 {
                if tree_at(x, y).is_some() || decor_at(x, y, 8).is_some() {
                    assert!(matches!(tile_at(x, y), Tile::Grass));
                }
            }
        }
    }

    #[test]
    fn a_solid_interior_tile_uses_the_center_of_the_slice() {
        // Deep inside the first field, every neighbor matches.
        assert!(matches!(tile_at(9, 9), Tile::Farm));
        assert_eq!(slice9(9, 9, Tile::Farm), 4);
    }
}
