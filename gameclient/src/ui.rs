//! The interface layer: crowd badges, the roster they open, and telemetry.
//!
//! This is the part of the client that answers the question the simulation
//! creates. Once enough people stand in the same place, drawing all of them
//! tells the player nothing — the sprites are simply on top of each other. So
//! the client stops trying: a dense cell collapses to a badge carrying its
//! count, and opening the badge lists who is standing there.
//!
//! egui draws at the display's own resolution rather than the world's
//! magnification, which is why the roster stays legible over pixel art.

use egui_macroquad::egui;
use umwelt::EntityId;

use crate::render::Camera;
use crate::world::{Cluster, World};

/// Where the pack's interface art and font live.
pub const UI_ROOT: &str = "assets/cute-fantasy-ui";

/// Body text size, in egui's own typeface.
const TEXT: f32 = 17.0;

/// Heading size, in the pack's typeface.
///
/// Bigger than the body for two reasons beyond hierarchy: the pack's capitals
/// are 411 units on an 800 unit em, a ratio of 0.51 against the 0.70 an
/// ordinary text face uses, so it draws about a quarter small for its nominal
/// size; and it needs the room to stay readable at all.
const DISPLAY: f32 = 28.0;

/// The pack's typeface is used for headings only, and deliberately.
///
/// It is a display face wearing pixel-art clothes. Despite the `5x9` on the
/// file it is not pixel art: fewer than one percent of its outline coordinates
/// land on a regular grid at any spacing, so the letters were drawn to
/// resemble pixels rather than snapped to them and every size is antialiased.
/// It also carries 86 glyphs and no space character.
///
/// Set against a panel of numbers being scanned, that is illegible at any size
/// — enlarging it to seven notches of zoom made it big and no easier to read.
/// So it labels things and counts things, and egui's own faces carry the text:
/// a characterful display face over a legible body face, which is the ordinary
/// arrangement and the one that works.
const DISPLAY_FAMILY: &str = "pack-display";

/// Which badge, if any, the player has open.
pub struct UiState {
    /// The cell whose roster is open, named by cell rather than by position in
    /// the list: the list is sorted by size and reorders as people walk.
    pub selected: Option<(i32, i32)>,
    pub show_telemetry: bool,
    /// The pack's typeface, if it was installed. Applied on the first frame,
    /// because the egui context only exists inside the draw closure.
    font: Option<Vec<u8>>,
    /// Lines shown briefly at startup, then dropped.
    notices: Vec<(String, egui::Color32)>,
    /// Scales the whole interface. [`TEXT`] is chosen by eye and I cannot see
    /// the screen it lands on, so the size is adjustable without a rebuild.
    pub zoom: f32,
    /// Cells currently carrying a badge. Held across frames so one can stay up
    /// while its crowd hovers around the threshold instead of blinking.
    showing: std::collections::HashSet<(i32, i32)>,
}

impl Default for UiState {
    fn default() -> UiState {
        UiState {
            selected: None,
            show_telemetry: true,
            font: None,
            notices: Vec::new(),
            zoom: 1.0,
            showing: std::collections::HashSet::new(),
        }
    }
}

impl UiState {
    /// Reads the pack's typeface and works out what to say on startup.
    ///
    /// A missing typeface is fine — egui's own is legible, and a clone without
    /// the art should still be usable.
    pub async fn load(art: bool) -> UiState {
        let font = macroquad::file::load_file(&format!("{UI_ROOT}/Fonts/CuteFantasy-5x9.ttf"))
            .await
            .ok();
        let mut notices = Vec::new();
        if art {
            notices.push((
                "art by Kenmi: kenmi-art.itch.io/cute-fantasy-rpg".to_owned(),
                egui::Color32::from_rgb(0xc8, 0xb8, 0x90),
            ));
        } else {
            notices.push((
                "drawing stand-in shapes: see gameclient/ASSETS.md".to_owned(),
                egui::Color32::from_rgb(0xf2, 0xb5, 0x44),
            ));
        }
        UiState { font, notices, ..UiState::default() }
    }
}

/// Applies the palette and typeface. Call once, before the first frame.
///
/// This has to happen outside a frame. [`egui::Context::set_fonts`] does not
/// take effect until the next `begin_frame`, so dressing from inside the frame
/// closure would leave the heading style pointing at a font family that is not
/// bound yet, and laying out a heading in that frame aborts the process.
pub fn install(state: &UiState) {
    egui_macroquad::cfg(|ctx| dress(ctx, state.font.as_ref()));
}

/// Applies the game's own palette and typeface to egui.
///
/// The panels are deliberately not the world's magnification: text is drawn at
/// the display's resolution so a roster stays readable over pixel art. Only the
/// colors and the typeface are borrowed, so the interface belongs to the game
/// without inheriting its blockiness.
fn dress(ctx: &egui::Context, font: Option<&Vec<u8>>) {
    let display = if let Some(bytes) = font {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("pack".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes.clone())));
        // A family of its own rather than the front of Proportional, so the
        // pack face reaches headings and nothing else. egui's own font sits
        // behind it to supply the space character and anything outside the 86
        // glyphs it draws.
        let mut behind = fonts
            .families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        behind.insert(0, "pack".into());
        fonts.families.insert(egui::FontFamily::Name(DISPLAY_FAMILY.into()), behind);
        ctx.set_fonts(fonts);
        egui::FontFamily::Name(DISPLAY_FAMILY.into())
    } else {
        egui::FontFamily::Proportional
    };

    let ink = egui::Color32::from_rgb(0xf2, 0xec, 0xdc);
    let dim = egui::Color32::from_rgb(0x9d, 0x92, 0xac);
    let panel = egui::Color32::from_rgb(0x22, 0x1d, 0x2c);
    let raised = egui::Color32::from_rgb(0x2d, 0x27, 0x39);
    let accent = egui::Color32::from_rgb(0x7b, 0xc9, 0x6f);

    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(ink);
    v.panel_fill = panel;
    v.window_fill = panel;
    v.extreme_bg_color = egui::Color32::from_rgb(0x18, 0x14, 0x20);
    v.window_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x3d, 0x35, 0x4c));
    v.widgets.noninteractive.bg_fill = panel;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, dim);
    v.widgets.inactive.bg_fill = raised;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, ink);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x39, 0x31, 0x47);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, accent);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(0x2f, 0x40, 0x2c);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, accent);
    v.selection.bg_fill = egui::Color32::from_rgb(0x2f, 0x40, 0x2c);
    v.selection.stroke = egui::Stroke::new(1.0_f32, accent);
    style.text_styles = [
        (egui::TextStyle::Small, egui::FontId::new(TEXT * 0.85, egui::FontFamily::Proportional)),
        (egui::TextStyle::Body, egui::FontId::new(TEXT, egui::FontFamily::Proportional)),
        (egui::TextStyle::Button, egui::FontId::new(TEXT, egui::FontFamily::Proportional)),
        // Numbers are read in columns, so they get the monospace face.
        (egui::TextStyle::Monospace, egui::FontId::new(TEXT, egui::FontFamily::Monospace)),
        (egui::TextStyle::Heading, egui::FontId::new(DISPLAY, display)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(10);
    ctx.set_style(style);
}

/// Queues every panel for this frame. Call once per frame, then
/// `egui_macroquad::draw()` after the world has been drawn, so the interface
/// lands on top of it.
pub fn draw(
    world: &World,
    clusters: &[Cluster],
    camera: &Camera,
    state: &mut UiState,
    drawn: usize,
    elapsed: f32,
) {
    // A crowd the player had open can disperse or walk out of view. Forget it
    // rather than leave a panel describing nobody.
    if state.selected.is_some_and(|cell| !clusters.iter().any(|c| c.cell == cell)) {
        state.selected = None;
    }

    // Decide what carries a badge before drawing, so the answer is the same
    // for the badge and for the panel it opens.
    state.showing.retain(|cell| {
        clusters.iter().any(|c| c.cell == *cell && c.members.len() >= crate::world::CROWD_KEEP)
    });
    for c in clusters.iter().filter(|c| c.members.len() >= crate::world::CROWD_MIN) {
        state.showing.insert(c.cell);
    }

    egui_macroquad::ui(|ctx| {
        if ctx.zoom_factor() != state.zoom {
            ctx.set_zoom_factor(state.zoom);
        }
        badges(ctx, clusters, camera, &state.showing, &mut state.selected);
        if let Some(cluster) =
            state.selected.and_then(|cell| clusters.iter().find(|c| c.cell == cell))
        {
            roster(ctx, world, camera, cluster);
        }
        if state.show_telemetry {
            telemetry(ctx, world, clusters, drawn);
        }
        notices(ctx, state, elapsed);
    });
}

/// How long a startup notice stays up, and how much of that is spent fading.
const NOTICE_SECS: f32 = 7.0;
const NOTICE_FADE: f32 = 1.5;

/// Startup notices, shown once and then out of the way.
///
/// The pack's license asks for a credit; it is owed to the player, not to the
/// margin of every frame, so it says its piece and leaves.
fn notices(ctx: &egui::Context, state: &UiState, elapsed: f32) {
    if elapsed > NOTICE_SECS {
        return;
    }
    let fade = ((NOTICE_SECS - elapsed) / NOTICE_FADE).clamp(0.0, 1.0);
    let alpha = |c: egui::Color32| {
        egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * fade) as u8)
    };

    egui::Area::new(egui::Id::new("startup-notice"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -18.0))
        .interactable(false)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                for (text, color) in &state.notices {
                    ui.label(egui::RichText::new(text).color(alpha(*color)));
                }
            });
        });
}

/// Where a world point sits in egui's coordinates.
///
/// The two disagree. macroquad reports the window in logical points — 1280 by
/// 720 on a display that is really 2560 by 1440 — and [`Camera::to_screen`]
/// answers in that space, which is what drawing a sprite wants. egui lays out
/// in the physical pixels behind it. Handing one to the other puts the whole
/// interface in a quarter of the screen at half scale.
///
/// The factor is taken from the two widths each frame rather than from the
/// display's scale, so it stays right when the interface is zoomed too.
fn to_ui(ctx: &egui::Context, camera: &Camera, world: (f32, f32)) -> egui::Pos2 {
    let (sx, sy) = camera.to_screen(world);
    let scale = ui_scale(ctx.screen_rect().width(), macroquad::window::screen_width());
    egui::pos2(sx * scale, sy * scale)
}

fn ui_scale(egui_width: f32, macroquad_width: f32) -> f32 {
    if macroquad_width > 0.0 { egui_width / macroquad_width } else { 1.0 }
}

/// One badge per crowded cell, floating above it.
fn badges(
    ctx: &egui::Context,
    clusters: &[Cluster],
    camera: &Camera,
    showing: &std::collections::HashSet<(i32, i32)>,
    selected: &mut Option<(i32, i32)>,
) {
    for cluster in clusters.iter().filter(|c| showing.contains(&c.cell)) {
        // Culling is a question about the window, so it is asked in
        // macroquad's space; placement is a question for egui, so it is
        // answered in egui's.
        if !camera.on_screen(camera.to_screen(cluster.center), 80.0) {
            continue;
        }
        let at = to_ui(ctx, camera, cluster.center);
        let open = *selected == Some(cluster.cell);

        egui::Area::new(egui::Id::new(("crowd-badge", cluster.cell)))
            .fixed_pos(egui::pos2(at.x - 34.0, at.y - 84.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let label = egui::RichText::new(format!("{} here", cluster.members.len()))
                        .strong()
                        .color(egui::Color32::from_rgb(0xf2, 0xb5, 0x44));
                    if ui.selectable_label(open, label).clicked() {
                        *selected = if open { None } else { Some(cluster.cell) };
                    }
                });
            });
    }
}

/// Everyone standing in one cell, nearest the player first.
fn roster(ctx: &egui::Context, world: &World, camera: &Camera, cluster: &Cluster) {
    let at = to_ui(ctx, camera, cluster.center);
    // Put the panel on whichever side of the crowd has room, so it never
    // covers the thing it is describing.
    let middle = ctx.screen_rect().center().x;
    let x = if at.x > middle { at.x - 400.0 } else { at.x + 44.0 };

    let mut ranked: Vec<(f32, EntityId)> = cluster
        .members
        .iter()
        .map(|(id, pos)| (distance(world.player, *pos), *id))
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));

    egui::Window::new("crowd-roster")
        .title_bar(false)
        .resizable(false)
        .fixed_pos(egui::pos2(x.max(8.0), (at.y - 80.0).max(8.0)))
        .fixed_size(egui::vec2(360.0, 430.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(format!("{}", cluster.members.len()));
                ui.label("in this cell");
            });
            ui.label(
                egui::RichText::new(format!(
                    "around {:.0}, {:.0} m",
                    cluster.center.0, cluster.center.1
                ))
                .weak(),
            );
            ui.separator();

            egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                for (dist, id) in &ranked {
                    ui.horizontal(|ui| {
                        // The client knows ids and positions. Names and what
                        // each person is doing would arrive as game messages,
                        // which is what `message_received` is the seam for.
                        ui.monospace(format!("{id}"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.monospace(format!("{dist:.1} m"));
                        });
                    });
                }
            });
        });
}

/// What the engine is doing, which for an exemplar is half the point.
fn telemetry(ctx: &egui::Context, world: &World, clusters: &[Cluster], drawn: usize) {
    egui::Window::new(egui::RichText::new("umwelt").heading())
        .resizable(false)
        .default_pos(egui::pos2(12.0, 12.0))
        .show(ctx, |ui| {
            let state = if world.connected { "connected" } else { "offline" };
            ui.horizontal(|ui| {
                ui.label("link");
                ui.monospace(state);
            });
            row(ui, "tracked", &format!("{}", world.entities.len()));
            row(ui, "people", &format!("{}", world.people()));
            row(ui, "drawn", &format!("{drawn}"));
            row(ui, "crowds", &format!("{}", clusters.len()));
            ui.separator();
            row(ui, "packets", &format!("{}", world.stats.packets));
            row(ui, "updates", &format!("{}", world.stats.updates));
            row(ui, "despawns", &format!("{}", world.stats.despawns));
            let drift = world
                .drift()
                .map(|m| format!("{m:.2} m"))
                .unwrap_or_else(|| "-".to_owned());
            row(ui, "drift", &drift);
            ui.separator();
            ui.label(egui::RichText::new("wasd walk / -= ui size").weak());
        });
}

fn row(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.monospace(value);
        });
    });
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (a.0 - b.0, a.1 - b.1);
    (dx * dx + dy * dy).sqrt()
}


#[cfg(test)]
mod tests {
    use super::*;

    /// egui's layout is pure CPU, so the styling can be checked without a
    /// window. This is here because the sizes silently not applying looks
    /// exactly like not having rebuilt.
    #[test]
    fn dressing_sets_every_text_size() {
        let ctx = egui::Context::default();
        let before = ctx.style().text_styles[&egui::TextStyle::Body].size;
        dress(&ctx, None);
        let after = ctx.style().text_styles.clone();

        assert_ne!(before, TEXT, "the test would prove nothing if these matched");
        assert_eq!(after[&egui::TextStyle::Body].size, TEXT);
        assert_eq!(after[&egui::TextStyle::Monospace].size, TEXT);
        assert_eq!(after[&egui::TextStyle::Button].size, TEXT);
        assert_eq!(after[&egui::TextStyle::Heading].size, DISPLAY);
        assert_eq!(after[&egui::TextStyle::Small].size, TEXT * 0.85);
    }

    /// The status panel is built from `label` and `monospace`, so those two
    /// styles are the ones that have to grow for it to read differently.
    #[test]
    fn a_laid_out_row_is_taller_after_dressing() {
        let plain = egui::Context::default();
        let plain_h = row_height(&plain);

        let dressed = egui::Context::default();
        dress(&dressed, None);
        let dressed_h = row_height(&dressed);

        assert!(
            dressed_h > plain_h,
            "a telemetry row measured {dressed_h} dressed against {plain_h} plain"
        );
    }

    /// The two coordinate spaces really do differ, and by the display scale.
    /// Getting this backwards put every badge in a quarter of the screen.
    #[test]
    fn the_ui_scale_bridges_macroquad_and_egui() {
        // What the instrumented client actually reported: a 1280 point window
        // on a 2560 pixel display.
        assert_eq!(ui_scale(2560.0, 1280.0), 2.0);
        // No scaling on a plain display.
        assert_eq!(ui_scale(1280.0, 1280.0), 1.0);
        // Zoomed interface: egui reports fewer points, so the factor drops.
        assert!((ui_scale(1706.0, 1280.0) - 1.333).abs() < 0.01);
        // A window reporting nothing must not produce infinity.
        assert_eq!(ui_scale(2560.0, 0.0), 1.0);
    }

    /// Where the pack typeface sits, from the crate root.
    fn pack_font() -> Option<Vec<u8>> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/cute-fantasy-ui/Fonts/CuteFantasy-5x9.ttf"
        ))
        .ok()
    }

    /// Why [`install`] exists, pinned as a test.
    ///
    /// Fonts set during a frame are not bound until the next one, so a heading
    /// style naming the new family cannot be laid out in the same frame. This
    /// aborted the client on its first frame. If egui ever makes this work,
    /// this test fails and `install` can be simplified.
    #[test]
    fn dressing_inside_a_frame_leaves_the_heading_unbound() {
        let Some(bytes) = pack_font() else { return };

        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let ctx = egui::Context::default();
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                dress(ctx, Some(&bytes));
                egui::Area::new(egui::Id::new("probe")).show(ctx, |ui| {
                    ui.label(egui::RichText::new("umwelt").heading());
                });
            });
        }));
        std::panic::set_hook(quiet);

        let err = outcome.expect_err("dressing mid-frame is supposed to leave the family unbound");
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(msg.contains("not bound to any fonts"), "panicked for another reason: {msg}");
    }

    /// The other half: dressed the way [`install`] does it, a heading lays out.
    #[test]
    fn dressing_before_a_frame_binds_the_heading() {
        let Some(bytes) = pack_font() else { return };
        let ctx = egui::Context::default();
        dress(&ctx, Some(&bytes));
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::Area::new(egui::Id::new("probe")).show(ctx, |ui| {
                ui.label(egui::RichText::new("umwelt").heading());
            });
        });
    }

    /// The pack face belongs on headings and nowhere else. Body text must
    /// measure the same with or without it installed, or it has leaked into
    /// the places that have to stay legible.
    #[test]
    fn the_pack_typeface_is_confined_to_headings() {
        let Some(bytes) = pack_font() else { return };

        let plain = egui::Context::default();
        dress(&plain, None);
        let with_pack = egui::Context::default();
        dress(&with_pack, Some(&bytes));

        // No spaces: the pack has no space glyph, so one would be measured
        // from the fallback in both and dilute the comparison.
        let width = |ctx: &egui::Context, style: egui::TextStyle| -> f32 {
            let mut w = 0.0;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::Area::new(egui::Id::new("probe")).show(ctx, |ui| {
                    w = ui.label(egui::RichText::new("packets1234").text_style(style.clone())).rect.width();
                });
            });
            w
        };

        let (a, b) = (width(&plain, egui::TextStyle::Body), width(&with_pack, egui::TextStyle::Body));
        assert!(a > 0.0 && b > 0.0, "nothing was laid out: {a} / {b}");
        assert_eq!(a, b, "the pack face leaked into body text: {a} vs {b}");

        let (a, b) = (
            width(&plain, egui::TextStyle::Heading),
            width(&with_pack, egui::TextStyle::Heading),
        );
        assert_ne!(a, b, "headings measured {a} either way, so the pack face is unused");
    }

    /// Lays out one telemetry row and returns how tall it came out.
    fn row_height(ctx: &egui::Context) -> f32 {
        let mut height = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::Area::new(egui::Id::new("probe")).show(ctx, |ui| {
                row(ui, "packets", "1234");
                height = ui.min_rect().height();
            });
        });
        height
    }
}
