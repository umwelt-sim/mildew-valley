//! What the client believes the world looks like right now.
//!
//! The network thread writes here as observation packets land; the render loop
//! reads it every frame. Positions are kept in meters as `f32` because that is
//! what drawing wants, and converted back to fixed point only when the local
//! player moves.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use umwelt::EntityId;

/// The rate the simulation ticks at. Must match the `tick_hz` the sim is
/// built with, or interpolation will consistently over- or under-shoot.
pub const TICK_HZ: f32 = 20.0;

/// One server tick, in seconds.
pub const TICK: f32 = 1.0 / TICK_HZ;

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
/// spawn themselves as observers, which is what tag 0 means here.
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
    /// Where the local player is standing, in meters. Owned by the render
    /// loop, not the network: the client moves first and tells the edge after.
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

/// A knot of people standing close enough together that drawing them all
/// individually tells the player nothing.
pub struct Cluster {
    pub center: (f32, f32),
    /// Members and where each is being drawn, so the roster can sort by
    /// distance without re-interpolating.
    pub members: Vec<Member>,
}

/// Buckets people into `cell`-meter squares and returns every square holding
/// at least `min` of them, largest first.
///
/// A fixed grid rather than true clustering: it is one pass over the entities
/// with no distance comparisons, and the artifact it produces — two knots
/// either side of a cell boundary counting separately — costs the player
/// nothing at the sizes this is used for.
pub fn clusters(world: &World, now: Instant, cell: f32, min: usize) -> Vec<Cluster> {
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
        .into_values()
        .filter(|members| members.len() >= min)
        .map(|members| {
            let n = members.len() as f32;
            let cx = members.iter().map(|m| m.1.0).sum::<f32>() / n;
            let cy = members.iter().map(|m| m.1.1).sum::<f32>() / n;
            Cluster { center: (cx, cy), members }
        })
        .collect();
    found.sort_by_key(|c| std::cmp::Reverse(c.members.len()));
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

        let found = clusters(&w, base, 8.0, 4);
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
        assert!(clusters(&w, base, 8.0, 4).is_empty(), "a full field is not a crowd");
    }
}
