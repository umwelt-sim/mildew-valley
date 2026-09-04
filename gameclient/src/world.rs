//! What the client believes the world looks like right now.
//!
//! The network thread writes here as observation packets land; the render loop
//! reads it every frame. Positions are kept in meters as `f32` because that is
//! what drawing wants, and converted back to fixed point only when the local
//! player moves.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use umwelt::EntityId;

pub use mildew_common::pace::TICK;

/// How far behind the newest sample remote entities are drawn.
///
/// Drawing in the past is what buys smooth motion: with samples on both sides
/// of the draw time there is always a pair to interpolate between, so an
/// entity slides instead of stepping. The cost is that everyone else is shown
/// this far out of date. Two ticks leaves room for one late or dropped packet
/// before the client runs out of future and has to hold still.
pub const INTERP_DELAY: f32 = 2.0 * TICK;

/// Samples kept per entity. Enough to cover [`INTERP_DELAY`] with slack; past
/// that the oldest are worthless because the draw time has moved on.
const SAMPLES: usize = 8;

/// The tag an entity carries when it is a person rather than a crop. Clients
/// spawn as observers, which the simulation tags 0.
pub const PLAYER_TAG: u16 = 0;

/// Whether a tag marks a person.
pub fn is_player(tag: u16) -> bool {
    tag == PLAYER_TAG
}

/// Whether a tag marks lettuce at any point in its growth.
pub fn is_lettuce(tag: u16) -> bool {
    mildew_common::tags::lettuce::RANGE.contains(&tag)
}

/// Below this much movement between two samples, an entity counts as standing
/// still. Interpolation jitter alone shifts a position by a hair, and without
/// a floor that would flip a sprite between poses every frame.
pub const STILL: f32 = 0.01;

/// Which way a figure is turned.
///
/// Facing is not on the wire — the simulation carries a position and a tag,
/// and nothing about how anyone is oriented. The client infers it from where
/// an entity was last seen heading, and remembers it once they stop, so
/// someone who walks west and halts keeps looking west.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Facing {
    Down,
    Up,
    Left,
    Right,
}

impl Facing {
    /// The way a step points, or `None` if it is too small to count as one.
    pub fn of(motion: (f32, f32)) -> Option<Facing> {
        if motion.0.abs() + motion.1.abs() <= STILL {
            return None;
        }
        Some(if motion.0.abs() > motion.1.abs() {
            if motion.0 < 0.0 { Facing::Left } else { Facing::Right }
        } else if motion.1 > 0.0 {
            Facing::Up
        } else {
            Facing::Down
        })
    }
}

/// One remote entity and the recent history the client has for it.
pub struct Track {
    samples: VecDeque<(Instant, (f32, f32))>,
    /// The game's own meaning for this entity. See `mildew_common::tags`.
    pub tag: u16,
    /// Where the entity was last seen heading, held after it stops.
    pub facing: Facing,
}

impl Track {
    /// A track holding a single sample.
    pub fn new(pos: (f32, f32), tag: u16, at: Instant) -> Track {
        let mut samples = VecDeque::with_capacity(SAMPLES);
        samples.push_back((at, pos));
        Track { samples, tag, facing: Facing::Down }
    }

    /// Records where the entity was as of `at`, dropping the oldest sample
    /// once the buffer is full.
    pub fn push(&mut self, pos: (f32, f32), tag: u16, at: Instant) {
        let last = self.latest();
        if let Some(turned) = Facing::of((pos.0 - last.0, pos.1 - last.1)) {
            self.facing = turned;
        }
        if self.samples.len() == SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((at, pos));
        self.tag = tag;
    }

    /// Whether the entity is on the move as of the last two samples.
    pub fn walking(&self) -> bool {
        Facing::of(self.motion()).is_some()
    }

    /// The newest position the client was told about, with no interpolation.
    pub fn latest(&self) -> (f32, f32) {
        self.samples.back().expect("a track always holds a sample").1
    }

    /// How far the entity moved between the last two samples.
    ///
    /// Facing is not on the wire — the simulation carries position and a tag,
    /// and nothing about which way anyone is turned. So the client infers it:
    /// a sprite faces where it was last seen heading, and stands still when
    /// that is nowhere.
    pub fn motion(&self) -> (f32, f32) {
        if self.samples.len() < 2 {
            return (0.0, 0.0);
        }
        let n = self.samples.len();
        let (from, to) = (self.samples[n - 2].1, self.samples[n - 1].1);
        (to.0 - from.0, to.1 - from.1)
    }

    /// Where to draw the entity now: its position [`INTERP_DELAY`] ago,
    /// interpolated between the two samples that straddle that moment.
    ///
    /// Outside the samples it holds at the nearest one rather than
    /// extrapolating. A guess that overshoots has to be walked back on the
    /// next packet, which reads as a stutter — worse than briefly standing
    /// still.
    pub fn at(&self, now: Instant) -> (f32, f32) {
        let newest = self.samples.back().expect("a track always holds a sample");
        let Some(draw_at) = now.checked_sub(Duration::from_secs_f32(INTERP_DELAY)) else {
            return newest.1;
        };

        let mut prev = self.samples.front().expect("a track always holds a sample");
        for sample in &self.samples {
            if sample.0 <= draw_at {
                prev = sample;
                continue;
            }
            let span = sample.0.duration_since(prev.0).as_secs_f32();
            if span <= f32::EPSILON {
                return sample.1;
            }
            let t = (draw_at.duration_since(prev.0).as_secs_f32() / span).clamp(0.0, 1.0);
            return (
                prev.1.0 + (sample.1.0 - prev.1.0) * t,
                prev.1.1 + (sample.1.1 - prev.1.1) * t,
            );
        }
        newest.1
    }

}

/// How far the region's copy of the player may sit from the client's own
/// reckoning before the client gives up and takes the region's word.
///
/// The two disagree by a round trip of walking even when everything is
/// working, because the region starts walking after the key is pressed and
/// stops after it is released. That is not an error to correct, and correcting
/// it would drag the figure about while a player holds a key. This is set well
/// clear of it, so it fires only when something real happened: a refused move,
/// a region boundary, a client that was not listening.
const SNAP_M: f32 = 3.0;

/// Where to draw the local player.
///
/// Its own reckoning, which answers the key the same frame it is pressed, until
/// that has drifted far enough from the region's copy to be wrong rather than
/// merely ahead.
///
/// The reckoning is what the player sees, never what the world runs on. The
/// region owns the position and takes every step itself, so a client that
/// reckons wrongly misleads nobody but itself.
pub fn reconcile(reckoned: (f32, f32), region: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (region.0 - reckoned.0, region.1 - reckoned.1);
    if dx * dx + dy * dy > SNAP_M * SNAP_M { region } else { reckoned }
}

/// Counters behind the telemetry readout.
#[derive(Default)]
pub struct Stats {
    pub packets: u64,
    pub updates: u64,
    pub despawns: u64,
    pub last_packet: Option<Instant>,
}

/// Everything the client knows.
pub struct World {
    /// Every entity a region has told this client about, itself included.
    pub entities: HashMap<EntityId, Track>,
    /// Where the local player is standing, in meters. Read from the region's
    /// own copy of the player's entity: input goes up, position comes down.
    pub player: (f32, f32),
    /// The id the edge assigned this client's entity, once it has one.
    pub player_entity: Option<EntityId>,
    /// Which way the local player is turned, held after they stop.
    pub player_facing: Facing,
    /// The newest tick any packet carried.
    pub tick: u32,
    pub connected: bool,
    pub stats: Stats,
}

impl World {
    pub fn new(player: (f32, f32)) -> World {
        World {
            entities: HashMap::new(),
            player,
            player_entity: None,
            player_facing: Facing::Down,
            tick: 0,
            connected: false,
            stats: Stats::default(),
        }
    }

    /// How many people the client is currently tracking.
    pub fn people(&self) -> usize {
        self.entities.values().filter(|t| is_player(t.tag)).count()
    }

    /// How far the simulation's idea of where this client stands has drifted
    /// from the client's own, in meters.
    ///
    /// The client moves first and tells the edge afterwards, so this is never
    /// quite zero while walking. It should hover around one tick of travel and
    /// fall back toward zero on stopping. A figure that keeps climbing means
    /// moves are not landing.
    pub fn drift(&self) -> Option<f32> {
        let id = self.player_entity?;
        let server = self.entities.get(&id)?.latest();
        let (dx, dy) = (self.player.0 - server.0, self.player.1 - server.1);
        Some((dx * dx + dy * dy).sqrt())
    }
}

/// One person in a crowd and where they are being drawn, in meters.
pub type Member = (EntityId, (f32, f32));

/// How many people have to share a cell before it is worth collapsing.
pub const CROWD_MIN: usize = 6;

/// How far it has to thin out before the badge goes away again.
///
/// A crowd sitting exactly on [`CROWD_MIN`] would otherwise flicker a badge in
/// and out as one person wanders across the boundary.
pub const CROWD_KEEP: usize = 4;

/// A knot of people standing close enough together that drawing them all
/// individually tells the player nothing.
pub struct Cluster {
    /// Which cell this is, in cell units. Fixed in the world and independent
    /// of who is standing in it, so it works as a stable identity. A badge
    /// whose widget id changes between frames drops clicks, because egui
    /// matches a press to a release by id.
    pub cell: (i32, i32),
    /// The middle of the cell, not the centroid of its members. A centroid
    /// recomputed each frame from moving entities drifts continuously, which
    /// makes a badge pinned to it hard to click.
    pub center: (f32, f32),
    /// Members and where each is being drawn, so the roster can sort by
    /// distance without re-interpolating.
    pub members: Vec<Member>,
}

/// Buckets people into `cell`-meter squares and returns every square holding
/// at least [`CROWD_KEEP`] of them, largest first.
///
/// A fixed grid rather than true clustering: it is one pass over the entities
/// with no distance comparisons, and the artifact it produces — two knots
/// either side of a cell boundary counting separately — costs the player
/// nothing at the sizes this is used for.
///
/// The floor is the lower one so the interface can hold a badge steady while
/// the crowd under it hovers around [`CROWD_MIN`].
pub fn clusters(world: &World, now: Instant, cell: f32) -> Vec<Cluster> {
    let mut buckets: HashMap<(i32, i32), Vec<Member>> = HashMap::new();
    for (id, track) in &world.entities {
        if !is_player(track.tag) {
            continue;
        }
        let pos = track.at(now);
        let key = ((pos.0 / cell).floor() as i32, (pos.1 / cell).floor() as i32);
        buckets.entry(key).or_default().push((*id, pos));
    }

    let mut found: Vec<Cluster> = buckets
        .into_iter()
        .filter(|(_, members)| members.len() >= CROWD_KEEP)
        .map(|(key, members)| Cluster {
            cell: key,
            center: ((key.0 as f32 + 0.5) * cell, (key.1 as f32 + 0.5) * cell),
            members,
        })
        .collect();
    // Largest first, then by cell, so equal-sized crowds keep a stable order
    // instead of swapping places between frames.
    found.sort_by_key(|c| (std::cmp::Reverse(c.members.len()), c.cell));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, secs: f32) -> Instant {
        base + Duration::from_secs_f32(secs)
    }

    #[test]
    fn a_single_sample_draws_where_it_is() {
        let base = Instant::now();
        let t = Track::new((5.0, 7.0), PLAYER_TAG, base);
        assert_eq!(t.at(at(base, INTERP_DELAY)), (5.0, 7.0));
    }

    /// A step of walking is nothing like far enough to be wrong, so the
    /// player keeps its own reckoning and answers the key at once.
    #[test]
    fn a_step_of_drift_leaves_the_reckoning_alone() {
        let reckoned = (10.0, 5.0);
        let region = (10.0 - 0.225, 5.0);
        assert_eq!(reconcile(reckoned, region), reckoned);
    }

    /// Half a second of walking is still only a metre, which a round trip
    /// accounts for on its own.
    #[test]
    fn a_round_trip_of_walking_is_not_a_disagreement() {
        let reckoned = (10.0, 5.0);
        let region = (10.0 - 4.5 * 0.25, 5.0);
        assert_eq!(reconcile(reckoned, region), reckoned);
    }

    /// Somewhere else entirely is the region saying no, and the region is what
    /// the world runs on.
    #[test]
    fn a_real_disagreement_takes_the_regions_word() {
        let reckoned = (10.0, 5.0);
        let region = (40.0, 5.0);
        assert_eq!(reconcile(reckoned, region), region);
    }

    #[test]
    fn a_draw_between_two_samples_interpolates() {
        let base = Instant::now();
        let mut t = Track::new((0.0, 0.0), PLAYER_TAG, base);
        t.push((10.0, 0.0), PLAYER_TAG, at(base, TICK));

        // Ask for the moment halfway between the two samples.
        let now = at(base, INTERP_DELAY + TICK / 2.0);
        let (x, _) = t.at(now);
        assert!((x - 5.0).abs() < 0.01, "expected the midpoint, got {x}");
    }

    #[test]
    fn a_draw_past_the_newest_sample_holds_still() {
        let base = Instant::now();
        let mut t = Track::new((0.0, 0.0), PLAYER_TAG, base);
        t.push((10.0, 0.0), PLAYER_TAG, at(base, TICK));
        // Far beyond anything the client has been told about.
        assert_eq!(t.at(at(base, 30.0)), (10.0, 0.0), "must not extrapolate");
    }

    #[test]
    fn the_buffer_keeps_a_bounded_number_of_samples() {
        let base = Instant::now();
        let mut t = Track::new((0.0, 0.0), PLAYER_TAG, base);
        for k in 1..50 {
            t.push((k as f32, 0.0), PLAYER_TAG, at(base, k as f32 * TICK));
        }
        assert_eq!(t.samples.len(), SAMPLES);
        assert_eq!(t.latest(), (49.0, 0.0));
    }

    /// A badge has to name the same widget from one frame to the next, or a
    /// click is dropped between press and release.
    #[test]
    fn a_cluster_keeps_its_identity_while_people_move_inside_it() {
        let base = Instant::now();
        let mut w = World::new((0.0, 0.0));
        for k in 0..8 {
            w.entities.insert(
                EntityId::from_raw(k),
                Track::new((2.0 + k as f32 * 0.3, 2.0), PLAYER_TAG, base),
            );
        }
        let before = clusters(&w, base, 8.0);
        assert_eq!(before.len(), 1);

        // Everyone shuffles about, staying inside the same cell.
        for k in 0..8 {
            w.entities.get_mut(&EntityId::from_raw(k)).unwrap().push(
                (1.0 + (7 - k) as f32 * 0.4, 3.0),
                PLAYER_TAG,
                base,
            );
        }
        let after = clusters(&w, base, 8.0);
        assert_eq!(after.len(), 1);
        assert_eq!(before[0].cell, after[0].cell, "the cell must not move");
        assert_eq!(before[0].center, after[0].center, "the badge must not drift");
    }

    /// Equal-sized crowds must not trade places, or the selected one changes
    /// under the player.
    #[test]
    fn equal_sized_crowds_keep_a_stable_order() {
        let base = Instant::now();
        let mut w = World::new((0.0, 0.0));
        for k in 0..12 {
            let at = if k < 6 { (2.0, 2.0) } else { (40.0, 40.0) };
            w.entities.insert(EntityId::from_raw(k), Track::new(at, PLAYER_TAG, base));
        }
        let order: Vec<_> = clusters(&w, base, 8.0).iter().map(|c| c.cell).collect();
        for _ in 0..8 {
            let again: Vec<_> = clusters(&w, base, 8.0).iter().map(|c| c.cell).collect();
            assert_eq!(order, again, "the order changed with nothing moving");
        }
    }

    #[test]
    fn only_crowded_cells_become_clusters() {
        let base = Instant::now();
        let mut w = World::new((0.0, 0.0));
        // Six people in one 8 m square, one person well away from them.
        for k in 0..6 {
            w.entities.insert(
                EntityId::from_raw(k),
                Track::new((1.0 + k as f32 * 0.5, 1.0), PLAYER_TAG, base),
            );
        }
        w.entities.insert(EntityId::from_raw(99), Track::new((80.0, 80.0), PLAYER_TAG, base));

        let found = clusters(&w, base, 8.0);
        assert_eq!(found.len(), 1, "the lone walker must not form a cluster");
        assert_eq!(found[0].members.len(), 6);
    }

    #[test]
    fn crops_are_not_counted_as_a_crowd() {
        let base = Instant::now();
        let mut w = World::new((0.0, 0.0));
        for k in 0..20 {
            w.entities.insert(
                EntityId::from_raw(k),
                Track::new((1.0, 1.0), mildew_common::tags::lettuce::RIPE, base),
            );
        }
        assert!(clusters(&w, base, 8.0).is_empty(), "a full field is not a crowd");
    }
}
