//! The mildew-valley game client.
//!
//! Draws the region around the player, and where a crowd is packed too tightly
//! to read, collapses it into a badge that opens the roster standing there.
//!
//! ```text
//! cargo run --release -p mildew-gameclient --bin mildew
//! cargo run --release -p mildew-gameclient --bin mildew -- --edge 127.0.0.1:7778 --region 2
//! cargo run --release -p mildew-gameclient --bin mildew -- --verbose
//! ```
//!
//! Needs `mv-edge` and `mv-sim` running behind it, and the sprite pack in
//! place — see `gameclient/ASSETS.md`. For a look at the wire without a window,
//! use the `mildew-probe` binary in this crate.
//!
//! Three threads meet here. The runtime's threads receive packets and write
//! into the world; this thread reads the world and draws it; the two share one
//! mutex. The render loop holds that mutex for the length of a frame, which is
//! long enough to matter only if drawing gets slow — at which point the fix is
//! to copy what the frame needs and let go, not to lock more finely.

mod assets;
mod net;
mod render;
mod terrain;
mod ui;
mod world;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use macroquad::prelude::*;
use mildew_common::net as mvnet;
use umwelt::{EdgeClient, EntityKind, Fixed, Pos3, RegionId};

use render::Camera;
use world::World;

use mildew_common::command::{GameCommand, Heading};
use mildew_common::pace::WALK_M_PER_SEC as WALK;

/// Meters on a side of the squares crowds are counted in. A little wider than
/// a person needs, so a knot standing shoulder to shoulder lands in one cell.
const CLUSTER_CELL: f32 = 6.0;

/// Where a client that was given no other instruction starts standing.
const SPAWN: (f32, f32) = (200.0, 200.0);

fn window_conf() -> Conf {
    Conf {
        window_title: "Mildew Valley".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    // Pixel art has to be point sampled. The default is linear, which would
    // blur every sprite the moment it is magnified.
    macroquad::texture::set_default_filter_mode(FilterMode::Nearest);

    let edge_addr: String = mvnet::arg_or("edge", mvnet::DEFAULT_EDGE.to_string());
    let region: u32 = mvnet::arg_or("region", 1);
    let verbose = std::env::args().any(|a| a == "--verbose");

    // The art is not in the repository, so a fresh clone draws stand-ins
    // rather than refusing to start. See gameclient/ASSETS.md.
    let atlas = assets::load_or_placeholder().await;

    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let conn = match net::dial(&edge_addr, &runtime) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };

    let shared = Arc::new(Mutex::new(World::new(SPAWN)));
    let for_link = Arc::clone(&shared);

    let client = match EdgeClient::new(conn, runtime.handle().clone(), move |_handle| {
        net::Link::new(for_link, verbose)
    }) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("opening a stream: {e}");
            return;
        }
    };

    let sending = client.handle();
    let me = match sending.spawn(RegionId::from_raw(region), pos3(SPAWN), EntityKind::observer(0)) {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("spawning: {e}");
            return;
        }
    };
    println!("mildew: connected to {edge_addr}, spawned in region {region}");

    let mut camera = Camera { center: SPAWN };
    let mut ui_state = ui::UiState::load(atlas.real).await;
    // Before the first frame: egui binds new fonts at the start of a frame, so
    // a style naming them cannot be installed from inside one.
    ui::install(&ui_state);
    // The last heading the edge was told. A held key is one message, not one
    // per frame, so only a change is worth sending.
    let mut sent_heading: Option<Heading> = None;
    // Where this client reckons it is standing. Answers the key on the frame it
    // is pressed, which is the whole reason it exists.
    let mut reckoned = SPAWN;

    loop {
        let dt = get_frame_time();
        let elapsed = get_time() as f32;
        let now = Instant::now();

        // Walk. Only the direction leaves here: the simulation owns the
        // position and decides how far a tick of walking covers.
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            dy += 1.0;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            dy -= 1.0;
        }
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            dx -= 1.0;
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            dx += 1.0;
        }
        let heading = Heading::from_axes(dx, dy);
        // Diagonals are normalized so a corner is not faster than a side, the
        // same trade the region makes with its diagonal step.
        let len = (dx * dx + dy * dy).sqrt();
        if len > 0.0 {
            dx /= len;
            dy /= len;
        }
        if is_key_pressed(KeyCode::Tab) {
            ui_state.show_telemetry = !ui_state.show_telemetry;
        }
        // The interface size is a guess until someone sees it on a real
        // screen, so it is adjustable without a rebuild.
        if is_key_pressed(KeyCode::Equal) {
            ui_state.zoom = (ui_state.zoom + 0.1).min(3.0);
        }
        if is_key_pressed(KeyCode::Minus) {
            ui_state.zoom = (ui_state.zoom - 0.1).max(0.5);
        }

        if heading != sent_heading {
            sent_heading = heading;
            let cmd = GameCommand::Walk { heading };
            if let Err(e) = sending.entity_send(me, &cmd.encode()) {
                eprintln!("walk: {e}");
            }
        }

        // Walk on the frame the key is pressed. The region is told a heading and
        // takes its own steps, so this is a reckoning of where those steps have
        // got to, not a claim about where anyone is: the region owns the
        // position and nothing here can move it.
        //
        // Continuous rather than in the region's whole ticks. A tick is 225 mm,
        // which at this magnification is eleven pixels, and stepping it would
        // jump every third frame at sixty.
        reckoned.0 += dx * WALK * dt;
        reckoned.1 += dy * WALK * dt;

        let moving = heading.is_some();
        let player = {
            let mut world = shared.lock().expect("the world lock");
            // The region's own copy, newest rather than interpolated: this is a
            // check on the reckoning, not something to draw.
            if let Some(at) = world.player_entity.and_then(|id| world.entities.get(&id))
            {
                reckoned = world::reconcile(reckoned, at.latest());
            }
            world.player = reckoned;
            // Facing follows the keys rather than the reply, so turning on the
            // spot shows up at once.
            if let Some(turned) = world::Facing::of((dx, dy)) {
                world.player_facing = turned;
            }
            world.player
        };

        camera.center = player;

        {
            let world = shared.lock().expect("the world lock");
            render::draw_ground(&atlas, &camera);
            let drawn = render::draw_scene(&atlas, &world, &camera, now, elapsed);
            render::draw_player(&atlas, &world, &camera, moving, elapsed);
            let clusters = world::clusters(&world, now, CLUSTER_CELL);
            ui::draw(&world, &clusters, &camera, &mut ui_state, drawn, elapsed);
        }

        egui_macroquad::draw();
        next_frame().await;
    }
}

/// Meters to the simulation's fixed-point position.
///
/// Read the scale off [`Fixed::ONE`] rather than hard-coding it, so this keeps
/// working if the library ever changes how many raw units a meter is worth.
fn pos3(meters: (f32, f32)) -> Pos3 {
    Pos3::new(fixed(meters.0), fixed(meters.1), Fixed::ZERO)
}

fn fixed(meters: f32) -> Fixed {
    Fixed::from_raw((meters * Fixed::ONE.raw() as f32).round() as i32)
}
