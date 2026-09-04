//! The mildew-valley load generator.
//!
//! Fills a region with farmers who walk about and plant lettuce, so the engine
//! has something to carry while a person walks among them. It drives the same
//! calls the game client does — [`ClientHandle::move_entities`] and the same
//! [`GameCommand`] — at the same pace, because a run that exercised a different
//! path would measure something nobody will ever do.
//!
//! ```text
//! cargo run --release -p mv-load
//! cargo run --release -p mv-load -- --bots 500 --spread 6   # a real crush
//! cargo run --release -p mv-load -- --bots 2000 --per-connection 100
//! ```
//!
//! Needs `mv-edge` and `mv-sim` running behind it.
//!
//! Bots share connections rather than taking one apiece. A real player is one
//! connection with one entity, but at a few hundred connections the socket
//! cost dominates. `--per-connection 1` loads the edge with connections; a
//! large value loads the region with entities.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mildew_common::command::GameCommand;
use mildew_common::net;
use mildew_common::pace::{TICK, WALK_M_PER_SEC};
use umwelt::{
    ClientGame, ClientHandle, EdgeClient, EntityHandle, EntityId, EntityKind, Fixed, Pos3,
    RegionId, TickObservation,
};

/// How close a bot has to get before it picks somewhere else to be.
const ARRIVED_M: f32 = 0.75;

/// How long a bot will keep walking at one target before giving up on it. Stops
/// a bot wedged against the edge of its patch from walking on the spot forever.
const PATIENCE: f32 = 12.0;

fn main() {
    let edge_addr: String = net::arg_or("edge", net::DEFAULT_EDGE.to_string());
    let region: u32 = net::arg_or("region", 1);
    let bots: usize = net::arg_or("bots", 200usize);
    let per_conn: usize = net::arg_or("per-connection", 50usize);
    let centre = (net::arg_or("x", 200.0f32), net::arg_or("y", 200.0f32));
    // Tight enough that the client's crowd badges actually trigger: they want
    // six people in a six metre cell, and a wider default leaves about four.
    let spread: f32 = net::arg_or("spread", 12.0f32);
    let plant_every: f32 = net::arg_or("plant-every", 20.0f32);
    let seed: u64 = net::arg_or("seed", 1u64);

    if bots == 0 || per_conn == 0 {
        eprintln!("--bots and --per-connection must both be at least 1");
        std::process::exit(1);
    }
    let conns = bots.div_ceil(per_conn);

    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let counters = Arc::new(Counters::default());
    let mut rng = Rng::new(seed);

    println!(
        "mv-load: {bots} farmers over {conns} connections to {edge_addr}, \
         region {region}, within {spread:.0} m of {:.0},{:.0}",
        centre.0, centre.1
    );

    let mut swarms = Vec::with_capacity(conns);
    let mut placed = 0;
    for n in 0..conns {
        let here = (bots - placed).min(per_conn);
        match Swarm::connect(&edge_addr, region, &runtime, &counters, here, centre, spread, &mut rng)
        {
            Ok(swarm) => swarms.push(swarm),
            Err(e) => {
                eprintln!("mv-load: connection {n}: {e}");
                std::process::exit(1);
            }
        }
        placed += here;
    }
    println!("mv-load: {placed} farmers are in");

    // One loop for every swarm, paced to the simulation. Sending faster would
    // put this process's scheduling on the wire instead of a walk.
    let step = Duration::from_secs_f32(TICK);
    let mut moves: Vec<(EntityHandle, Pos3)> = Vec::with_capacity(per_conn);
    let mut next = Instant::now() + step;
    let mut report = Report::new(bots, conns);

    loop {
        for swarm in &mut swarms {
            moves.clear();
            for bot in &mut swarm.bots {
                bot.advance(TICK, centre, spread, &mut rng);
                moves.push((bot.entity, pos3(bot.pos)));

                if plant_every > 0.0 {
                    bot.plant_in -= TICK;
                    if bot.plant_in <= 0.0 {
                        bot.plant_in = plant_every * (0.5 + rng.unit());
                        let cmd = GameCommand::PlantLettuce {
                            x: bot.pos.0 as i32,
                            y: bot.pos.1 as i32,
                        };
                        if swarm.handle.entity_send(bot.entity, &cmd.encode()).is_ok() {
                            counters.planted.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
            match swarm.handle.move_entities(&moves) {
                Ok(()) => {
                    counters.moved.fetch_add(moves.len() as u64, Ordering::Relaxed);
                }
                Err(_) => {
                    counters.refused.fetch_add(moves.len() as u64, Ordering::Relaxed);
                }
            }
        }

        report.maybe_print(&counters);

        // Sleep to the next tick rather than for a tick, so a slow pass costs
        // this run its slack and not its cadence.
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
        next += step;
        if next < now {
            next = now + step;
        }
    }
}

/// One connection and the farmers riding on it.
struct Swarm {
    /// Dropping this closes the connection, so it is held even though the
    /// handle is what gets used.
    _client: EdgeClient,
    handle: ClientHandle,
    bots: Vec<Bot>,
}

impl Swarm {
    #[allow(clippy::too_many_arguments)]
    fn connect(
        edge: &str,
        region: u32,
        runtime: &tokio::runtime::Runtime,
        counters: &Arc<Counters>,
        count: usize,
        centre: (f32, f32),
        spread: f32,
        rng: &mut Rng,
    ) -> Result<Swarm, String> {
        let endpoint = net::game_endpoint(runtime.handle());
        let target: std::net::SocketAddr =
            edge.parse().map_err(|e| format!("--edge {edge:?}: {e}"))?;
        let conn = runtime
            .block_on(async {
                endpoint
                    .connect(target, "localhost")
                    .map_err(|e| e.to_string())?
                    .await
                    .map_err(|e| e.to_string())
            })
            .map_err(|e| format!("connecting to {edge}: {e}"))?;

        let watching = Arc::clone(counters);
        let client = EdgeClient::new(conn, runtime.handle().clone(), move |_| Watcher {
            counters: watching,
            last: std::collections::HashMap::new(),
        })
        .map_err(|e| format!("opening a stream: {e}"))?;

        let handle = client.handle();
        let mut bots = Vec::with_capacity(count);
        for _ in 0..count {
            let pos = scatter(centre, spread, rng);
            let entity = handle
                .spawn(RegionId::from_raw(region), pos3(pos), EntityKind::observer(0))
                .map_err(|e| format!("spawning: {e}"))?;
            bots.push(Bot {
                entity,
                pos,
                target: scatter(centre, spread, rng),
                walking_for: 0.0,
                plant_in: rng.unit() * 8.0,
            });
        }
        Ok(Swarm { _client: client, handle, bots })
    }
}

/// One farmer.
struct Bot {
    entity: EntityHandle,
    pos: (f32, f32),
    target: (f32, f32),
    walking_for: f32,
    plant_in: f32,
}

impl Bot {
    /// Walks one tick's worth toward wherever it is going, and picks somewhere
    /// new on arrival.
    fn advance(&mut self, dt: f32, centre: (f32, f32), spread: f32, rng: &mut Rng) {
        let (dx, dy) = (self.target.0 - self.pos.0, self.target.1 - self.pos.1);
        let dist = (dx * dx + dy * dy).sqrt();
        self.walking_for += dt;

        if dist < ARRIVED_M || self.walking_for > PATIENCE {
            self.target = scatter(centre, spread, rng);
            self.walking_for = 0.0;
            return;
        }
        let step = WALK_M_PER_SEC * dt;
        self.pos.0 += dx / dist * step;
        self.pos.1 += dy / dist * step;
    }
}

/// Somewhere inside the patch, biased toward the middle so a crowd has a centre
/// to be dense at rather than a ring.
fn scatter(centre: (f32, f32), spread: f32, rng: &mut Rng) -> (f32, f32) {
    let r = spread * rng.unit().sqrt() * rng.unit().max(0.35);
    let a = rng.unit() * std::f32::consts::TAU;
    (centre.0 + r * a.cos(), centre.1 + r * a.sin())
}

/// What the edge tells us back. Nothing here steers a bot; a load generator
/// that reacted to what it saw would measure its own feedback loop.
///
/// It does time the arrivals. A region keeping real time sends one
/// observation per entity per tick, so the gap should sit at the tick period.
/// A longer gap means the region is running slow, which is what
/// `Overrun::Dilate` does under load instead of reporting an error.
struct Watcher {
    counters: Arc<Counters>,
    /// Last arrival per entity. Owned rather than shared: the callback for one
    /// client is not run concurrently with itself.
    last: std::collections::HashMap<u32, Instant>,
}

impl ClientGame for Watcher {
    fn spawned(&mut self, _handle: EntityHandle, _region: RegionId, _entity: EntityId) {
        self.counters.spawned.fetch_add(1, Ordering::Relaxed);
    }

    fn observed(
        &mut self,
        handle: EntityHandle,
        _region: RegionId,
        observation: &TickObservation<'_>,
    ) {
        let now = Instant::now();
        self.counters.packets.fetch_add(1, Ordering::Relaxed);
        let updates = observation.updates().count() as u64;
        self.counters.updates.fetch_add(updates, Ordering::Relaxed);

        if let Some(before) = self.last.insert(handle.raw(), now) {
            let gap = now.duration_since(before).as_micros() as u64;
            self.counters.gap_sum_us.fetch_add(gap, Ordering::Relaxed);
            self.counters.gap_count.fetch_add(1, Ordering::Relaxed);
            self.counters.gap_max_us.fetch_max(gap, Ordering::Relaxed);
            // Half a tick of slack before a gap counts as late.
            if gap as f32 > TICK * 1.5 * 1e6 {
                self.counters.gap_late.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn disconnected(&mut self) {
        self.counters.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Counters {
    spawned: AtomicU64,
    moved: AtomicU64,
    refused: AtomicU64,
    planted: AtomicU64,
    packets: AtomicU64,
    updates: AtomicU64,
    dropped: AtomicU64,
    /// Gaps between consecutive observations of the same entity.
    gap_sum_us: AtomicU64,
    gap_count: AtomicU64,
    gap_max_us: AtomicU64,
    gap_late: AtomicU64,
}

/// Prints a line a second, with rates worked out against the wall clock rather
/// than the tick count, so a run that falls behind says so.
struct Report {
    bots: usize,
    conns: usize,
    last: Instant,
    marks: (u64, u64, u64),
}

impl Report {
    fn new(bots: usize, conns: usize) -> Report {
        Report { bots, conns, last: Instant::now(), marks: (0, 0, 0) }
    }

    fn maybe_print(&mut self, c: &Counters) {
        let elapsed = self.last.elapsed().as_secs_f64();
        if elapsed < 1.0 {
            return;
        }
        let moved = c.moved.load(Ordering::Relaxed);
        let packets = c.packets.load(Ordering::Relaxed);
        let updates = c.updates.load(Ordering::Relaxed);
        let rate = |now: u64, then: u64| (now.saturating_sub(then)) as f64 / elapsed;

        let gaps = c.gap_count.load(Ordering::Relaxed);
        let mean_gap = if gaps > 0 {
            c.gap_sum_us.load(Ordering::Relaxed) as f64 / gaps as f64 / 1000.0
        } else {
            0.0
        };
        let per_observer = if packets > self.marks.1 {
            (updates - self.marks.2) as f64 / (packets - self.marks.1) as f64
        } else {
            0.0
        };

        println!(
            "mv-load: {} farmers on {} conns | spawned {} | moves {:.0}/s | \
             in {:.0} packets/s {:.0} updates/s ({:.0}/observer) | \
             gap mean {mean_gap:.1}ms max {:.1}ms late {} | \
             planted {} | refused {} | dropped {}",
            self.bots,
            self.conns,
            c.spawned.load(Ordering::Relaxed),
            rate(moved, self.marks.0),
            rate(packets, self.marks.1),
            rate(updates, self.marks.2),
            per_observer,
            c.gap_max_us.load(Ordering::Relaxed) as f64 / 1000.0,
            c.gap_late.load(Ordering::Relaxed),
            c.planted.load(Ordering::Relaxed),
            c.refused.load(Ordering::Relaxed),
            c.dropped.load(Ordering::Relaxed),
        );
        // Max is per window, so a spike does not follow the run around.
        c.gap_max_us.store(0, Ordering::Relaxed);
        self.marks = (moved, packets, updates);
        self.last = Instant::now();
    }
}

/// Meters to the simulation's fixed-point position.
fn pos3(meters: (f32, f32)) -> Pos3 {
    Pos3::new(fixed(meters.0), fixed(meters.1), Fixed::ZERO)
}

fn fixed(meters: f32) -> Fixed {
    Fixed::from_raw((meters * Fixed::ONE.raw() as f32).round() as i32)
}

/// A small deterministic generator, so `--seed` reproduces a run exactly.
/// Nothing here needs to resist anything; it needs to be the same twice.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }

    /// Uniform on `[0, 1)`.
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // Top 24 bits: every value an f32 can hold exactly in this range.
        ((self.0 >> 40) as f32) / (1u32 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_walks_the_same_route() {
        let route = |seed| {
            let mut rng = Rng::new(seed);
            (0..8).map(|_| scatter((200.0, 200.0), 20.0, &mut rng)).collect::<Vec<_>>()
        };
        assert_eq!(route(7), route(7), "a seeded run has to repeat");
        assert_ne!(route(7), route(8), "different seeds must not coincide");
    }

    #[test]
    fn scatter_stays_inside_the_patch() {
        let mut rng = Rng::new(3);
        let centre = (200.0, 200.0);
        for _ in 0..2000 {
            let (x, y) = scatter(centre, 20.0, &mut rng);
            let d = ((x - centre.0).powi(2) + (y - centre.1).powi(2)).sqrt();
            assert!(d <= 20.0 + 1e-3, "wandered {d} m from the middle");
        }
    }

    #[test]
    fn a_bot_walks_toward_its_target_and_then_picks_another() {
        let mut rng = Rng::new(11);
        let mut bot = Bot {
            entity: EntityHandle::from_raw(1),
            pos: (0.0, 0.0),
            target: (10.0, 0.0),
            walking_for: 0.0,
            plant_in: 0.0,
        };
        let start = bot.pos.0;
        bot.advance(TICK, (0.0, 0.0), 20.0, &mut rng);
        let stepped = bot.pos.0 - start;
        assert!(
            (stepped - WALK_M_PER_SEC * TICK).abs() < 1e-4,
            "a tick should cover one tick of walking, covered {stepped}"
        );

        // Walk until it arrives; it must then be aiming somewhere else.
        for _ in 0..400 {
            bot.advance(TICK, (0.0, 0.0), 20.0, &mut rng);
        }
        assert_ne!(bot.target, (10.0, 0.0), "a bot that arrived must move on");
    }

    #[test]
    fn a_stuck_bot_gives_up_on_an_unreachable_target() {
        let mut rng = Rng::new(5);
        let mut bot = Bot {
            entity: EntityHandle::from_raw(1),
            pos: (0.0, 0.0),
            // Far enough that PATIENCE runs out long before arrival.
            target: (10_000.0, 0.0),
            walking_for: 0.0,
            plant_in: 0.0,
        };
        for _ in 0..((PATIENCE / TICK) as usize + 2) {
            bot.advance(TICK, (0.0, 0.0), 20.0, &mut rng);
        }
        assert_ne!(bot.target, (10_000.0, 0.0), "patience must run out");
    }

    #[test]
    fn the_generator_covers_its_range() {
        let mut rng = Rng::new(99);
        let (mut lo, mut hi) = (1.0f32, 0.0f32);
        for _ in 0..10_000 {
            let v = rng.unit();
            assert!((0.0..1.0).contains(&v), "out of range: {v}");
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(lo < 0.01 && hi > 0.99, "poor spread: {lo} to {hi}");
    }
}
