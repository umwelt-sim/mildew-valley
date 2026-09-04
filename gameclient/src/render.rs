//! Drawing the world.
//!
//! The simulation works in meters. The art is 16-pixel tiles drawn at a
//! whole-number magnification so pixels stay square. [`Camera`] converts
//! between the two and every draw goes through it. The egui layer does not:
//! interface text is drawn at the display's own resolution.
//!
//! Every sprite comes out of one atlas, so the ground, the scenery, and a
//! crowd of any size are a handful of batches rather than one per entity.

use std::time::Instant;

use macroquad::prelude::*;

use crate::assets::{Atlas, Pose, TILE};
use crate::terrain::{self, Tile};
use crate::world::{self, Facing, World};

/// Magnification. Whole numbers only — fractional zoom makes pixel art shimmer.
pub const ZOOM: f32 = 3.0;

/// Screen pixels per world meter. One meter is one tile.
pub const PPM: f32 = TILE as f32 * ZOOM;

/// How far off screen a sprite can be rooted and still reach into view. A tree
/// is the tallest thing drawn, at a few tiles.
const REACH: f32 = PPM * 5.0;

const SKY: Color = Color::new(0.239, 0.529, 0.278, 1.0);

/// Converts world meters to screen pixels, centered on a point.
pub struct Camera {
    pub center: (f32, f32),
}

impl Camera {
    /// Where a world point lands on screen.
    ///
    /// World `y` counts north and screen `y` counts down, so it is negated.
    pub fn to_screen(&self, world: (f32, f32)) -> (f32, f32) {
        (
            (world.0 - self.center.0) * PPM + screen_width() * 0.5,
            (self.center.1 - world.1) * PPM + screen_height() * 0.5,
        )
    }

    /// Whether a screen point is close enough to matter, with `pad` pixels of
    /// slack so sprites straddling the edge still draw.
    pub fn on_screen(&self, screen: (f32, f32), pad: f32) -> bool {
        screen.0 >= -pad
            && screen.0 <= screen_width() + pad
            && screen.1 >= -pad
            && screen.1 <= screen_height() + pad
    }

    /// The tile range covering the window, with a margin for sprites that are
    /// taller than their tile and so reach in from off screen.
    fn tile_bounds(&self, margin: f32) -> (i32, i32, i32, i32) {
        let half_w = screen_width() / PPM * 0.5 + margin;
        let half_h = screen_height() / PPM * 0.5 + margin;
        (
            (self.center.0 - half_w).floor() as i32,
            (self.center.0 + half_w).ceil() as i32,
            (self.center.1 - half_h).floor() as i32,
            (self.center.1 + half_h).ceil() as i32,
        )
    }
}

/// Stamps one atlas sprite with its bottom-center at a screen point.
fn stamp(atlas: &Atlas, src: Rect, at: (f32, f32), flip: bool) {
    draw_texture_ex(
        &atlas.texture,
        at.0 - src.w * ZOOM * 0.5,
        at.1 - src.h * ZOOM,
        WHITE,
        DrawTextureParams {
            source: Some(src),
            dest_size: Some(vec2(src.w * ZOOM, src.h * ZOOM)),
            flip_x: flip,
            ..Default::default()
        },
    );
}

/// Lays down the ground, then the scenery rooted in it.
pub fn draw_ground(atlas: &Atlas, camera: &Camera) {
    clear_background(SKY);
    let (x0, x1, y0, y1) = camera.tile_bounds(1.0);

    for ty in y0..=y1 {
        for tx in x0..=x1 {
            // Tiles are addressed by their southwest corner, so the top of the
            // tile on screen is one meter north of it.
            let (sx, sy) = camera.to_screen((tx as f32, ty as f32 + 1.0));
            let kind = terrain::tile_at(tx, ty);

            // Grass sits under everything, so an edge tile of any other
            // material has something to be an edge against.
            draw_tile(atlas, atlas.grass, sx, sy);
            let set = match kind {
                Tile::Grass => continue,
                Tile::Farm => &atlas.farmland,
                Tile::Path => &atlas.path,
                Tile::Water => &atlas.water,
            };
            draw_tile(atlas, set[terrain::slice9(tx, ty, kind)], sx, sy);
        }
    }

    for ty in y0..=y1 {
        for tx in x0..=x1 {
            let (sx, sy) = camera.to_screen((tx as f32 + 0.5, ty as f32));
            if terrain::fence_at(tx, ty) {
                stamp(atlas, atlas.fence, (sx, sy), false);
            } else if let Some(prop) = terrain::decor_at(tx, ty, atlas.decor.len()) {
                stamp(atlas, atlas.decor[prop], (sx, sy), false);
            }
        }
    }
}

fn draw_tile(atlas: &Atlas, src: Rect, sx: f32, sy: f32) {
    draw_texture_ex(
        &atlas.texture,
        sx,
        sy,
        WHITE,
        DrawTextureParams {
            source: Some(src),
            dest_size: Some(vec2(PPM, PPM)),
            ..Default::default()
        },
    );
}

/// One thing to draw, and how far north it stands.
struct Standing {
    north: f32,
    screen: (f32, f32),
    what: What,
}

enum What {
    Tree(bool),
    Crop(usize),
    Animal(bool),
    Person { look: usize, pose: Pose, step: usize, flip: bool },
}

/// Draws the scenery and everyone in it, back to front.
///
/// Returns how many entities were drawn. The gap between that and the tracked
/// count is what culling saved.
pub fn draw_scene(
    atlas: &Atlas,
    world: &World,
    camera: &Camera,
    now: Instant,
    elapsed: f32,
) -> usize {
    let mut queue: Vec<Standing> = Vec::with_capacity(world.entities.len() + 64);

    // Trees are rooted in the ground and sort against people like anything
    // else, so a farmer can stand behind one.
    let (x0, x1, y0, y1) = camera.tile_bounds(4.0);
    for ty in y0..=y1 {
        for tx in x0..=x1 {
            let north = ty as f32;
            let screen = camera.to_screen((tx as f32 + 0.5, north));
            if let Some(large) = terrain::tree_at(tx, ty) {
                queue.push(Standing { north, screen, what: What::Tree(large) });
            } else if let Some(cow) = terrain::animal_at(tx, ty) {
                queue.push(Standing { north, screen, what: What::Animal(cow) });
            }
        }
    }

    let mut entities = 0;
    for (id, track) in &world.entities {
        if world.player_entity == Some(*id) {
            continue; // drawn separately, always at the center
        }
        let pos = track.at(now);
        let screen = camera.to_screen(pos);
        if !camera.on_screen(screen, REACH) {
            continue;
        }
        entities += 1;

        let what = if world::is_lettuce(track.tag) {
            What::Crop(stage_of(track.tag))
        } else {
            let (look, pose, step, flip) = figure(*id, track, elapsed, atlas.look_count());
            What::Person { look, pose, step, flip }
        };
        queue.push(Standing { north: pos.1, screen, what });
    }

    // Further north draws first, so nearer things overlap it.
    queue.sort_by(|a, b| b.north.total_cmp(&a.north));
    for item in &queue {
        match item.what {
            What::Tree(true) => stamp(atlas, atlas.tree_big, item.screen, false),
            What::Tree(false) => stamp(atlas, atlas.tree_medium, item.screen, false),
            What::Crop(stage) => stamp(atlas, atlas.crops[stage], item.screen, false),
            What::Animal(true) => stamp(atlas, atlas.cow, item.screen, false),
            What::Animal(false) => stamp(atlas, atlas.chicken, item.screen, false),
            What::Person { look, pose, step, flip } => {
                person(atlas, atlas.player(look, pose, step), item.screen, flip)
            }
        }
    }
    entities
}

/// The local player, always at the center of the view.
pub fn draw_player(atlas: &Atlas, world: &World, camera: &Camera, moving: bool, elapsed: f32) {
    let screen = camera.to_screen(world.player);
    let (pose, flip) = pose_for(world.player_facing, moving);
    let step = (elapsed * 9.0) as usize;
    let look = world
        .player_entity
        .map(|id| look_of(id, atlas.look_count()))
        .unwrap_or(0);
    person(atlas, atlas.player(look, pose, step), screen, flip);
}

/// A character frame, offset so the figure's feet land on the given point
/// rather than the corner of its frame.
fn person(atlas: &Atlas, src: Rect, at: (f32, f32), flip: bool) {
    // Mirroring moves the feet to the other side of the frame.
    let foot_x = if flip { src.w - atlas.foot.0 } else { atlas.foot.0 };
    draw_texture_ex(
        &atlas.texture,
        at.0 - foot_x * ZOOM,
        at.1 - atlas.foot.1 * ZOOM,
        WHITE,
        DrawTextureParams {
            source: Some(src),
            dest_size: Some(vec2(src.w * ZOOM, src.h * ZOOM)),
            flip_x: flip,
            ..Default::default()
        },
    );
}

/// Picks the appearance, pose, and animation phase for one remote person.
fn figure(
    id: umwelt::EntityId,
    track: &world::Track,
    elapsed: f32,
    looks: usize,
) -> (usize, Pose, usize, bool) {
    let (pose, flip) = pose_for(track.facing, track.walking());
    // Offsetting the phase by identity keeps a crowd from marching in step.
    let step = (elapsed * 9.0) as usize + terrain::hash2(id.index() as i32, 7) as usize % 6;
    (look_of(id, looks), pose, step, flip)
}

/// The frame row for a figure, and whether it needs mirroring.
///
/// The pack draws its character facing right, so west is the same art flipped.
fn pose_for(facing: Facing, moving: bool) -> (Pose, bool) {
    match (facing, moving) {
        (Facing::Up, true) => (Pose::WalkUp, false),
        (Facing::Up, false) => (Pose::IdleUp, false),
        (Facing::Down, true) => (Pose::WalkDown, false),
        (Facing::Down, false) => (Pose::IdleDown, false),
        (Facing::Left, true) => (Pose::WalkSide, true),
        (Facing::Left, false) => (Pose::IdleSide, true),
        (Facing::Right, true) => (Pose::WalkSide, false),
        (Facing::Right, false) => (Pose::IdleSide, false),
    }
}

/// The appearance an entity wears.
///
/// Derived from its id rather than assigned as it arrives, so every client
/// draws the same person the same way and someone stays recognizable after
/// walking out of view and back.
fn look_of(id: umwelt::EntityId, count: usize) -> usize {
    terrain::hash2(id.index() as i32, 0x51ed) as usize % count.max(1)
}

fn stage_of(tag: u16) -> usize {
    use mildew_common::tags::lettuce;
    match tag {
        lettuce::SEED => 0,
        lettuce::SPROUT => 1,
        lettuce::GROWING => 2,
        _ => 3,
    }
}
