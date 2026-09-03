//! Loading the sprite pack and packing it into a single texture.
//!
//! Everything drawn in the depth-sorted pass lives in one atlas. Depth sorting
//! fixes the draw order, so it cannot be reordered to group by texture; if
//! appearances were spread across separate textures the renderer would have to
//! rebind once per entity. In one atlas a whole crowd is a single batch no
//! matter what order it is drawn in.
//!
//! Farmers are composited, not tinted. The pack draws its character as a bare
//! base plus separate, pixel-aligned layers for shoes, trousers, shirt and
//! hair, in eight or so colors each; stacking those at load gives real
//! authored variation rather than a palette swap, and costs nothing per frame.
//! Only skin is recolored, because the pack ships one tone.
//!
//! The art is not in this repository. If it is missing, [`load_or_placeholder`]
//! falls back to flat shapes so a fresh clone still builds and runs — the
//! engine is the thing being demonstrated, and it should not take a purchase to
//! see it work. See `gameclient/ASSETS.md`.

use macroquad::prelude::*;

/// Where the pack is expected to sit, relative to the working directory.
pub const ROOT: &str = "assets/cute-fantasy";

/// Side of one terrain tile, in source pixels.
pub const TILE: u16 = 16;

/// Side of one character frame in the pack's sheets. The figure occupies
/// roughly the middle third; the rest is room for tool swings and mounts.
pub const FRAME: u16 = 64;

/// Frames per animation row that the poses below use.
pub const STEPS: usize = 6;

/// Distinct farmers baked into the atlas.
pub const LOOKS: usize = 24;

/// Rows of the character sheets. The pack orders them the same way in every
/// layer, so one row index addresses all of them.
#[derive(Clone, Copy)]
pub enum Pose {
    IdleDown = 0,
    IdleSide = 1,
    IdleUp = 2,
    WalkDown = 3,
    WalkSide = 4,
    WalkUp = 5,
}

/// Lettuce is row 15 of the crop sheet, whose seven columns are a field sign,
/// a seed jar, four growth stages, then the harvested item.
const LETTUCE_ROW: u16 = 15;
const CROP_STAGE_COLS: [u16; 4] = [2, 3, 4, 5];

/// The base sheet's skin, which is the only part of a farmer not supplied as
/// an authored layer.
const SKIN_SRC: [u32; 2] = [0xf6ca9f, 0xd29f70];
const SKINS: [[u32; 2]; 6] = [
    [0xf6ca9f, 0xd29f70], [0xe8b483, 0xc08e5c], [0xd19a6e, 0xa8744c],
    [0xa9714a, 0x825334], [0x7a4f33, 0x5a3823], [0xfbdcbb, 0xdcb489],
];

const SHIRT_COLORS: [&str; 8] = [
    "Black", "Blue", "Green", "Orange", "Pink", "Purple", "Red", "White_and_Brown",
];
const PANTS_COLORS: [&str; 8] = SHIRT_COLORS;
const SHOE_COLORS: [&str; 8] = [
    "Black", "Blue", "Brown", "Green", "Orange", "Pink", "Purple", "Red",
];
const HAIR_STYLES: [u8; 6] = [1, 2, 3, 4, 5, 6];
const HAIR_COLORS: [&str; 5] = ["Black", "Blonde", "Brown", "Ginger", "Grey"];

/// Every sprite the renderer can name, and the texture they share.
pub struct Atlas {
    pub texture: Texture2D,
    pub grass: Rect,
    /// Nine-slice sets, indexed `row * 3 + col`.
    pub farmland: [Rect; 9],
    pub path: [Rect; 9],
    pub water: [Rect; 9],
    /// Lettuce, youngest first.
    pub crops: [Rect; 4],
    pub tree_big: Rect,
    pub tree_medium: Rect,
    pub fence: Rect,
    pub chicken: Rect,
    pub cow: Rect,
    /// Scatter props for grass, picked by tile hash.
    pub decor: Vec<Rect>,
    /// Top-left of each farmer's block of frames.
    looks: Vec<Rect>,
    /// Size of one cropped character frame.
    cell: (f32, f32),
    /// Where the feet sit inside that frame, so a sprite can be planted on a
    /// world position rather than hung from its corner.
    pub foot: (f32, f32),
    /// Whether this is the real pack or the stand-in.
    pub real: bool,
}

impl Atlas {
    /// One frame of one farmer. `step` advances the cycle; the pack animates
    /// its idle poses too, so it is never ignored.
    pub fn player(&self, look: usize, pose: Pose, step: usize) -> Rect {
        frame_rect(self.looks[look % self.looks.len()], self.cell, pose as usize, step)
    }

    pub fn look_count(&self) -> usize {
        self.looks.len()
    }
}

/// Addresses one frame inside a farmer's block. Kept out of [`Atlas`] so the
/// arithmetic can be tested without a graphics context to hang a texture on.
fn frame_rect(block: Rect, cell: (f32, f32), pose: usize, step: usize) -> Rect {
    Rect::new(
        block.x + (step % STEPS) as f32 * cell.0,
        block.y + pose as f32 * cell.1,
        cell.0,
        cell.1,
    )
}

/// Loads the pack, or falls back to flat shapes if it is not installed.
pub async fn load_or_placeholder() -> Atlas {
    match load().await {
        Ok(atlas) => atlas,
        Err(e) => {
            eprintln!("mildew: {e}");
            eprintln!("mildew: drawing with stand-in shapes; see gameclient/ASSETS.md");
            placeholder()
        }
    }
}

/// Loads the pack and packs it. Fails with the path it could not read, so a
/// missing checkout says which file to go and get.
pub async fn load() -> Result<Atlas, String> {
    let grass_img = image("Tiles/Grass/Grass_1_Middle.png").await?;
    let farm_sheet = image("Tiles/FarmLand/FarmLand_Tile.png").await?;
    let water_sheet = image("Tiles/Water/Water_Tile_1.png").await?;
    let path_sheet = image("Tiles/Grass/Grass_Tiles_1.png").await?;
    let crops_sheet = image("Crops/Crops.png").await?;
    let big_tree = image("Trees/Big_Oak_Tree.png").await?;
    let med_tree = image("Trees/Medium_Oak_Tree.png").await?;
    let fence_sheet = image("Outdoor decoration/Fences.png").await?;
    let flowers = image("Outdoor decoration/Flowers.png").await?;
    let chicken_sheet = image("Animals/Chicken/Chicken_01.png").await?;
    let cow_sheet = image("Animals/Cow/Cow_01.png").await?;

    let farmers = composite_farmers().await?;
    // Every layer aligns, so one box taken across all of them crops every
    // frame identically and keeps the sprites registered with each other.
    let box_ = union_box(&farmers);
    let (bw, bh) = (box_.2 - box_.0, box_.3 - box_.1);

    let mut packer = Packer::new(2048, 2048);

    let grass = packer.place(&grass_img);
    // The premium sheets are full blob sets; these are the nine-slice each one
    // keeps, located by matching the free pack's own three-by-three.
    let farmland = packer.place_grid(&farm_sheet, 1, 0);
    let water = packer.place_grid(&water_sheet, 0, 0);
    let path = packer.place_grid(&path_sheet, 0, 5);

    let crops = CROP_STAGE_COLS.map(|c| packer.place(&cell(&crops_sheet, c, LETTUCE_ROW, TILE)));

    let tree_big = packer.place(&trim(&sub(&big_tree, 0, 0, 64, 80)));
    let tree_medium = packer.place(&trim(&sub(&med_tree, 0, 0, 32, 48)));
    let fence = packer.place(&cell(&fence_sheet, 2, 0, TILE));
    let chicken = packer.place(&first_sprite(&chicken_sheet, 32));
    let cow = packer.place(&first_sprite(&cow_sheet, 32));

    // Take whatever the flower sheet actually holds rather than naming cells,
    // so a pack update that shuffles it cannot silently draw blanks.
    let mut decor = Vec::new();
    for r in 0..(flowers.height / TILE) {
        for c in 0..(flowers.width / TILE) {
            let one = cell(&flowers, c, r, TILE);
            if one.get_image_data().iter().any(|p| p[3] > 32) {
                decor.push(packer.place(&one));
            }
        }
    }

    let mut looks = Vec::with_capacity(farmers.len());
    for sheet in &farmers {
        looks.push(packer.place_frames(sheet, box_, STEPS, 6));
    }

    let texture = Texture2D::from_image(&packer.atlas);
    texture.set_filter(FilterMode::Nearest);

    Ok(Atlas {
        texture,
        grass,
        farmland,
        path,
        water,
        crops,
        tree_big,
        tree_medium,
        fence,
        chicken,
        cow,
        decor,
        looks,
        cell: (bw as f32, bh as f32),
        // Feet sit on the bottom of the figure, centered across the frame.
        foot: ((FRAME / 2 - box_.0) as f32, (41 - box_.1) as f32),
        real: true,
    })
}

/// Builds every farmer by stacking the pack's authored layers.
async fn composite_farmers() -> Result<Vec<Image>, String> {
    let base = image("Player/Player_Base/Player_Base_animations.png").await?;
    let mut out = Vec::with_capacity(LOOKS);

    for n in 0..LOOKS {
        let shirt = image(&format!(
            "Player/Chest/Farmer_Shirt/Farmer_Shirt_1_{}.png",
            SHIRT_COLORS[n % SHIRT_COLORS.len()]
        ))
        .await?;
        let pants = image(&format!(
            "Player/Legs/Farmer_Pants/Farmer_Pants_1_{}.png",
            PANTS_COLORS[(n / 2) % PANTS_COLORS.len()]
        ))
        .await?;
        let shoes = image(&format!(
            "Player/Feet/Shoes_1_{}.png",
            SHOE_COLORS[(n / 3) % SHOE_COLORS.len()]
        ))
        .await?;
        let style = HAIR_STYLES[(n / 4) % HAIR_STYLES.len()];
        let hair = image(&format!(
            "Player/Head/Hair_{style}/Hair_{style}_{}.png",
            HAIR_COLORS[n % HAIR_COLORS.len()]
        ))
        .await?;

        // Only the rows the renderer draws: idle and walk, six frames each.
        let keep = (0, 0, STEPS as u16 * FRAME, 6 * FRAME);
        let mut person = recolor_skin(&sub(&base, keep.0, keep.1, keep.2, keep.3), n);
        for layer in [&shoes, &pants, &shirt, &hair] {
            over(&mut person, &sub(layer, keep.0, keep.1, keep.2, keep.3));
        }
        out.push(person);
    }
    Ok(out)
}

/// The tightest box holding every frame of every farmer, so all of them crop
/// to the same size and stay aligned with one another.
fn union_box(sheets: &[Image]) -> (u16, u16, u16, u16) {
    let (mut x0, mut y0, mut x1, mut y1) = (FRAME, FRAME, 0u16, 0u16);
    for sheet in sheets {
        for r in 0..6u16 {
            for c in 0..STEPS as u16 {
                if let Some(b) = bbox(&cell(sheet, c, r, FRAME)) {
                    x0 = x0.min(b.0);
                    y0 = y0.min(b.1);
                    x1 = x1.max(b.2);
                    y1 = y1.max(b.3);
                }
            }
        }
    }
    // A pixel of margin keeps a hat or a swinging arm off the crop edge.
    (x0.saturating_sub(1), y0.saturating_sub(1), (x1 + 1).min(FRAME), (y1 + 1).min(FRAME))
}

fn bbox(img: &Image) -> Option<(u16, u16, u16, u16)> {
    let (w, h) = (img.width, img.height);
    let px = img.get_image_data();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u16, 0u16);
    let mut any = false;
    for y in 0..h {
        for x in 0..w {
            if px[(y as usize) * (w as usize) + x as usize][3] > 16 {
                any = true;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    any.then_some((x0, y0, x1, y1))
}

fn trim(img: &Image) -> Image {
    match bbox(img) {
        Some((x0, y0, x1, y1)) => sub(img, x0, y0, x1 - x0, y1 - y0),
        None => img.clone(),
    }
}

/// The first cell of a sheet holding anything, trimmed to what it holds.
fn first_sprite(sheet: &Image, size: u16) -> Image {
    for r in 0..(sheet.height / size) {
        for c in 0..(sheet.width / size) {
            let one = cell(sheet, c, r, size);
            if bbox(&one).is_some() {
                return trim(&one);
            }
        }
    }
    sheet.clone()
}

/// Alpha-over composite of two images of the same size.
fn over(dst: &mut Image, src: &Image) {
    let top = src.get_image_data().to_vec();
    for (under, above) in dst.get_image_data_mut().iter_mut().zip(top) {
        match above[3] {
            0 => {}
            255 => *under = above,
            a => {
                let (a, inv) = (a as u32, 255 - a as u32);
                for k in 0..3 {
                    under[k] = ((above[k] as u32 * a + under[k] as u32 * inv) / 255) as u8;
                }
                under[3] = under[3].max(above[3]);
            }
        }
    }
}

fn recolor_skin(src: &Image, n: usize) -> Image {
    let tone = SKINS[(n / 5) % SKINS.len()];
    let mut out = src.clone();
    for px in out.get_image_data_mut() {
        if px[3] == 0 {
            continue;
        }
        let rgb = (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32;
        if let Some(k) = SKIN_SRC.iter().position(|s| *s == rgb) {
            let to = tone[k];
            px[0] = (to >> 16) as u8;
            px[1] = (to >> 8) as u8;
            px[2] = to as u8;
        }
    }
    out
}

async fn image(path: &str) -> Result<Image, String> {
    load_image(&format!("{ROOT}/{path}"))
        .await
        .map_err(|e| format!("{ROOT}/{path}: {e}"))
}

fn cell(sheet: &Image, col: u16, row: u16, size: u16) -> Image {
    sub(sheet, col * size, row * size, size, size)
}

fn sub(sheet: &Image, x: u16, y: u16, w: u16, h: u16) -> Image {
    sheet.sub_image(Rect::new(x as f32, y as f32, w as f32, h as f32))
}

// ── the stand-in ────────────────────────────────────────────────

/// Flat shapes, so the client runs without the pack installed. Deliberately
/// plain: it should be obvious that the art is missing rather than broken.
pub fn placeholder() -> Atlas {
    const CELL: u16 = 24;
    let mut packer = Packer::new(1024, 512);

    let flat = |c: Color| Image::gen_image_color(TILE, TILE, c);
    let grass = packer.place(&flat(Color::new(0.243, 0.529, 0.278, 1.0)));
    let farm = packer.place(&flat(Color::new(0.470, 0.325, 0.220, 1.0)));
    let path = packer.place(&flat(Color::new(0.784, 0.659, 0.439, 1.0)));
    let water = packer.place(&flat(Color::new(0.157, 0.404, 0.667, 1.0)));

    let mut crops = [Rect::default(); 4];
    for (k, slot) in crops.iter_mut().enumerate() {
        let mut img = Image::gen_image_color(TILE, TILE, Color::new(0.0, 0.0, 0.0, 0.0));
        let r = 2 + k as i32 * 2;
        disc(&mut img, 8, 9, r, Color::new(0.22, 0.62, 0.30, 1.0));
        *slot = packer.place(&img);
    }

    let mut tree = Image::gen_image_color(48, 56, Color::new(0.0, 0.0, 0.0, 0.0));
    disc(&mut tree, 24, 22, 20, Color::new(0.18, 0.44, 0.22, 1.0));
    for y in 40..56 {
        for x in 21..27 {
            tree.set_pixel(x, y, Color::new(0.35, 0.24, 0.15, 1.0));
        }
    }
    let tree_big = packer.place(&tree);
    let tree_medium = packer.place(&sub(&tree, 8, 12, 32, 44));

    let mut fence = Image::gen_image_color(TILE, TILE, Color::new(0.0, 0.0, 0.0, 0.0));
    for x in 0..TILE as u32 {
        fence.set_pixel(x, 7, Color::new(0.55, 0.40, 0.25, 1.0));
        fence.set_pixel(x, 8, Color::new(0.42, 0.30, 0.18, 1.0));
    }
    let fence = packer.place(&fence);

    let bird = |c: Color| {
        let mut i = Image::gen_image_color(12, 12, Color::new(0.0, 0.0, 0.0, 0.0));
        disc(&mut i, 6, 7, 4, c);
        i
    };
    let chicken = packer.place(&bird(Color::new(0.93, 0.93, 0.90, 1.0)));
    let cow = packer.place(&bird(Color::new(0.85, 0.85, 0.88, 1.0)));

    let mut decor = Vec::new();
    for c in [Color::new(0.95, 0.88, 0.35, 1.0), Color::new(0.90, 0.45, 0.60, 1.0)] {
        let mut i = Image::gen_image_color(TILE, TILE, Color::new(0.0, 0.0, 0.0, 0.0));
        disc(&mut i, 8, 9, 2, c);
        decor.push(packer.place(&i));
    }

    // One block per look: six poses down, six steps across, all identical.
    let mut looks = Vec::with_capacity(8);
    for n in 0..8 {
        let hue = n as f32 / 8.0;
        let body = Color::new(0.35 + hue * 0.5, 0.45, 0.85 - hue * 0.4, 1.0);
        let mut block =
            Image::gen_image_color(CELL * STEPS as u16, CELL * 6, Color::new(0.0, 0.0, 0.0, 0.0));
        for row in 0..6u32 {
            for step in 0..STEPS as u32 {
                let (ox, oy) = (step * CELL as u32, row * CELL as u32);
                for y in 8..21u32 {
                    for x in 8..16u32 {
                        block.set_pixel(ox + x, oy + y, body);
                    }
                }
                disc_at(&mut block, ox + 12, oy + 7, 5, Color::new(0.96, 0.80, 0.62, 1.0));
            }
        }
        looks.push(packer.place(&block));
    }
    // Blocks were packed whole; address frames inside them.
    let looks = looks
        .into_iter()
        .map(|b| Rect::new(b.x, b.y, CELL as f32, CELL as f32))
        .collect();

    let texture = Texture2D::from_image(&packer.atlas);
    texture.set_filter(FilterMode::Nearest);
    Atlas {
        texture,
        grass,
        farmland: [farm; 9],
        path: [path; 9],
        water: [water; 9],
        crops,
        tree_big,
        tree_medium,
        fence,
        chicken,
        cow,
        decor,
        looks,
        cell: (CELL as f32, CELL as f32),
        foot: (12.0, 21.0),
        real: false,
    }
}

fn disc(img: &mut Image, cx: i32, cy: i32, r: i32, c: Color) {
    disc_at(img, cx as u32, cy as u32, r, c)
}

fn disc_at(img: &mut Image, cx: u32, cy: u32, r: i32, c: Color) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let (x, y) = (cx as i32 + dx, cy as i32 + dy);
            if x >= 0 && y >= 0 && (x as u16) < img.width && (y as u16) < img.height {
                img.set_pixel(x as u32, y as u32, c);
            }
        }
    }
}

// ── packing ─────────────────────────────────────────────────────

/// A shelf packer: fills a row left to right, drops to a new row when the
/// next sprite will not fit. Good enough for a fixed set loaded once.
struct Packer {
    atlas: Image,
    x: u16,
    y: u16,
    shelf: u16,
}

impl Packer {
    fn new(w: u16, h: u16) -> Packer {
        Packer {
            atlas: Image::gen_image_color(w, h, Color::new(0.0, 0.0, 0.0, 0.0)),
            x: 0,
            y: 0,
            shelf: 0,
        }
    }

    fn place(&mut self, img: &Image) -> Rect {
        let (w, h) = (img.width, img.height);
        // One pixel of gutter, so filtering never samples a neighbor.
        if self.x + w > self.atlas.width {
            self.x = 0;
            self.y += self.shelf + 1;
            self.shelf = 0;
        }
        assert!(
            self.y + h <= self.atlas.height,
            "the sprite atlas is too small for the pack"
        );
        blit(&mut self.atlas, img, self.x, self.y);
        let placed = Rect::new(self.x as f32, self.y as f32, w as f32, h as f32);
        self.x += w + 1;
        self.shelf = self.shelf.max(h);
        placed
    }

    /// A three-by-three nine-slice starting at a cell of a larger blob sheet.
    fn place_grid(&mut self, sheet: &Image, col: u16, row: u16) -> [Rect; 9] {
        let mut out = [Rect::default(); 9];
        for r in 0..3u16 {
            for c in 0..3u16 {
                out[(r * 3 + c) as usize] = self.place(&cell(sheet, col + c, row + r, TILE));
            }
        }
        out
    }

    /// Packs a character sheet's frames as one contiguous block, each cropped
    /// to `box_`, so a frame can be addressed by arithmetic rather than a
    /// rectangle per frame.
    fn place_frames(
        &mut self,
        sheet: &Image,
        box_: (u16, u16, u16, u16),
        cols: usize,
        rows: usize,
    ) -> Rect {
        let (bw, bh) = (box_.2 - box_.0, box_.3 - box_.1);
        let mut block =
            Image::gen_image_color(bw * cols as u16, bh * rows as u16, Color::new(0.0, 0.0, 0.0, 0.0));
        for r in 0..rows as u16 {
            for c in 0..cols as u16 {
                let frame = sub(sheet, c * FRAME + box_.0, r * FRAME + box_.1, bw, bh);
                blit(&mut block, &frame, c * bw, r * bh);
            }
        }
        let placed = self.place(&block);
        Rect::new(placed.x, placed.y, bw as f32, bh as f32)
    }
}

fn blit(dst: &mut Image, src: &Image, dx: u16, dy: u16) {
    let (sw, sh) = (src.width as usize, src.height as usize);
    let dw = dst.width as usize;
    let pixels = src.get_image_data().to_vec();
    let out = dst.get_image_data_mut();
    for y in 0..sh {
        let to = (dy as usize + y) * dw + dx as usize;
        out[to..to + sw].copy_from_slice(&pixels[y * sw..y * sw + sw]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(w: u16, h: u16) -> Image {
        Image::gen_image_color(w, h, Color::new(0.0, 0.0, 0.0, 0.0))
    }

    fn solid(w: u16, h: u16, c: Color) -> Image {
        Image::gen_image_color(w, h, c)
    }

    /// Packs the shapes and order [`load`] uses. The packer asserts on
    /// overflow, so without this the first thing a fresh checkout with the
    /// pack installed would learn is a panic on startup.
    #[test]
    fn the_atlas_has_room_for_the_whole_pack() {
        let mut packer = Packer::new(2048, 2048);
        packer.place(&blank(TILE, TILE));
        for _ in 0..27 {
            packer.place(&blank(TILE, TILE)); // three nine-slices
        }
        for _ in 0..4 {
            packer.place(&blank(TILE, TILE)); // crop stages
        }
        packer.place(&blank(64, 80)); // big oak
        packer.place(&blank(32, 48)); // medium oak
        for _ in 0..3 {
            packer.place(&blank(TILE, TILE)); // fence, chicken, cow
        }
        for _ in 0..100 {
            packer.place(&blank(TILE, TILE)); // flowers, generously
        }
        // Farmer blocks, at the widest crop the union box could produce.
        for _ in 0..LOOKS {
            packer.place(&blank(FRAME * STEPS as u16, FRAME * 6));
        }
        let used = packer.y + packer.shelf;
        assert!(used <= 2048, "the pack needs {used} rows, the atlas has 2048");
    }

    #[test]
    fn nothing_is_placed_outside_the_atlas() {
        let mut packer = Packer::new(256, 256);
        for _ in 0..40 {
            let r = packer.place(&blank(48, 24));
            assert!(r.x >= 0.0 && r.y >= 0.0);
            assert!(r.x + r.w <= 256.0, "ran off the right edge: {r:?}");
            assert!(r.y + r.h <= 256.0, "ran off the bottom: {r:?}");
        }
    }

    #[test]
    fn compositing_puts_the_upper_layer_on_top() {
        // Exact channel values, not macroquad's named colors: `BLUE` there is
        // a designed shade rather than pure blue.
        let mut under = solid(2, 1, Color::from_rgba(255, 0, 0, 255));
        let mut top = blank(2, 1);
        top.set_pixel(0, 0, Color::from_rgba(0, 0, 255, 255));
        over(&mut under, &top);
        let px = under.get_image_data();
        assert_eq!(px[0][..3], [0, 0, 255], "the layer on top must win");
        assert_eq!(px[1][..3], [255, 0, 0], "a clear pixel must not erase what is under it");
    }

    #[test]
    fn a_half_clear_pixel_blends_rather_than_replaces() {
        let mut under = solid(1, 1, Color::from_rgba(0, 0, 0, 255));
        let mut top = blank(1, 1);
        top.set_pixel(0, 0, Color::from_rgba(255, 255, 255, 128));
        over(&mut under, &top);
        let mid = under.get_image_data()[0][0];
        assert!((120..=136).contains(&mid), "expected roughly half, got {mid}");
    }

    #[test]
    fn the_union_box_covers_every_frame() {
        // One sheet with a mark in the top-left frame and another in the
        // bottom-right; the box has to reach both.
        let mut sheet = blank(FRAME * STEPS as u16, FRAME * 6);
        sheet.set_pixel(10, 12, RED);
        sheet.set_pixel((FRAME * 2 - 5) as u32, (FRAME * 2 - 6) as u32, RED);
        let b = union_box(std::slice::from_ref(&sheet));
        assert!(b.0 <= 10 && b.1 <= 12, "box {b:?} misses the first mark");
        assert!(b.2 >= FRAME - 5 && b.3 >= FRAME - 6, "box {b:?} misses the second");
    }

    #[test]
    fn skin_is_recolored_and_the_outline_is_not() {
        let mut src = blank(2, 1);
        src.set_pixel(0, 0, Color::from_rgba(0xf6, 0xca, 0x9f, 255));
        src.set_pixel(1, 0, Color::from_rgba(0x0e, 0x07, 0x1b, 255));
        // Look 5 is the first that lands on a tone other than the pack's own.
        let out = recolor_skin(&src, 5);
        let px = out.get_image_data();
        assert_ne!(px[0][..3], [0xf6, 0xca, 0x9f], "skin was not recolored");
        assert_eq!(px[1][..3], [0x0e, 0x07, 0x1b], "the outline must be left alone");
    }

    /// Every frame has to land inside the block it was packed from. Building a
    /// real [`Atlas`] would need a graphics context, so this checks the
    /// arithmetic that addresses the frames instead.
    #[test]
    fn every_frame_stays_inside_its_block() {
        let cell = (24.0, 32.0);
        let block = Rect::new(100.0, 200.0, cell.0 * STEPS as f32, cell.1 * 6.0);
        for pose in 0..6 {
            for step in 0..STEPS * 3 {
                let f = frame_rect(block, cell, pose, step);
                assert!(f.x >= block.x && f.x + f.w <= block.x + block.w,
                    "pose {pose} step {step} ran off the side: {f:?}");
                assert!(f.y >= block.y && f.y + f.h <= block.y + block.h,
                    "pose {pose} step {step} ran off the bottom: {f:?}");
            }
        }
    }

    /// The walk cycle has to wrap rather than run past the last frame.
    #[test]
    fn the_step_wraps_around_the_cycle() {
        let cell = (10.0, 10.0);
        let block = Rect::new(0.0, 0.0, cell.0 * STEPS as f32, cell.1 * 6.0);
        assert_eq!(frame_rect(block, cell, 0, 0), frame_rect(block, cell, 0, STEPS));
        assert_eq!(frame_rect(block, cell, 3, 1), frame_rect(block, cell, 3, STEPS + 1));
    }
}
