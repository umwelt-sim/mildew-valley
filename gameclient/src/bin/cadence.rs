//! Measures how often a client is told where its own entity is.
//!
//! Walks in one direction for a while and records every update the region sends
//! about the walker itself, then reports two things.
//!
//! The first is whether the region ever skipped or doubled a step. A walk is a
//! fixed distance per tick, so the ground covered between two updates has to be
//! the gap between them times that step. Positions are quantized before they go
//! on the wire, so the check allows for one quantum, which the run measures
//! rather than assumes.
//!
//! The second is the gap between updates. The client draws two ticks behind and
//! holds its last known position rather than guessing past it, so a gap wider
//! than that is a stall followed by a jump. The share of gaps over that width
//! is what a person would see.
//!
//! ```text
//! cargo run --release -p mildew-gameclient --bin mildew-cadence
//! cargo run --release -p mildew-gameclient --bin mildew-cadence -- --seconds 30
//! ```
//!
//! Needs `mv-edge` and `mv-sim` running behind it. Run a crowd alongside it to
//! see what competing for a packet does to the cadence.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mildew_common::command::{GameCommand, Heading};
use mildew_common::net;
use umwelt::{
    ClientGame, ClientHandle, EdgeClient, EntityHandle, EntityId, EntityKind, Fixed, Pos3,
    RegionId, TickObservation,
};

/// Ticks the client draws behind the newest sample. A gap wider than this
/// leaves it holding a stale position.
const INTERP_DELAY_TICKS: u32 = 2;

/// One sighting of the walker.
#[derive(Clone, Copy)]
struct Seen {
    tick: u32,
    x: i32,
    y: i32,
}

#[derive(Default)]
struct Log {
    /// The walker's own id, once the region has assigned one.
    me: Option<EntityId>,
    seen: Vec<Seen>,
    /// Updates that named some other entity, for context on how busy the
    /// packets were.
    others: u64,
    packets: u64,
}

struct Walker {
    log: Arc<Mutex<Log>>,
    handle: ClientHandle,
    heading: Heading,
}

impl ClientGame for Walker {
    fn spawned(&mut self, handle: EntityHandle, _region: RegionId, entity: EntityId) {
        self.log.lock().expect("not poisoned").me = Some(entity);
        let cmd = GameCommand::Walk { heading: Some(self.heading) };
        match self.handle.entity_send(handle, &cmd.encode()) {
            Ok(()) => println!("walking {:?} as {entity}", self.heading),
            Err(e) => eprintln!("failed to start walking: {e}"),
        }
    }

    fn observed(
        &mut self,
        _handle: EntityHandle,
        _region: RegionId,
        observation: &TickObservation<'_>,
    ) {
        let tick = observation.tick();
        let mut log = self.log.lock().expect("not poisoned");
        log.packets += 1;
        let me = log.me;
        for (id, pos, _tag) in observation.updates() {
            if Some(id) == me {
                log.seen.push(Seen { tick, x: pos.x.raw(), y: pos.y.raw() });
            } else {
                log.others += 1;
            }
        }
    }

    fn disconnected(&mut self) {
        eprintln!("disconnected");
    }
}

fn main() {
    let edge_addr: String = net::arg_or("edge", net::DEFAULT_EDGE.to_string());
    let region: u32 = net::arg_or("region", 1);
    let x: i32 = net::arg_or("x", 2100);
    let y: i32 = net::arg_or("y", 2100);
    let seconds: u64 = net::arg_or("seconds", 20);
    let heading = Heading::East;

    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let endpoint = net::game_endpoint(runtime.handle());
    let target: std::net::SocketAddr = edge_addr.parse().unwrap_or_else(|e| {
        eprintln!("--edge {edge_addr:?}: {e}");
        std::process::exit(1);
    });
    let conn = runtime
        .block_on(async { endpoint.connect(target, "localhost").expect("configured").await })
        .unwrap_or_else(|e| {
            eprintln!("connecting to {edge_addr}: {e}");
            std::process::exit(1);
        });

    let log = Arc::new(Mutex::new(Log::default()));
    let for_game = Arc::clone(&log);
    let client = EdgeClient::new(conn, runtime.handle().clone(), move |handle| Walker {
        log: for_game,
        handle,
        heading,
    })
    .unwrap_or_else(|e| {
        eprintln!("opening a stream: {e}");
        std::process::exit(1);
    });

    let sending = client.handle();
    sending
        .spawn(RegionId::from_raw(region), Pos3::from_meters(x, y, 0), EntityKind::observer(0))
        .expect("asks for an entity");
    println!("mildew-cadence: {edge_addr}, region {region}, from {x},{y} for {seconds}s");

    std::thread::sleep(Duration::from_secs(seconds));
    let log = log.lock().expect("not poisoned");
    report(&log, heading);
}

fn report(log: &Log, heading: Heading) {
    if log.seen.len() < 2 {
        println!("only {} sightings, nothing to measure", log.seen.len());
        return;
    }

    // The step the region takes, in the same raw units the wire reports.
    let (sx, sy) = heading.step_mm();
    let step = (raw(sx), raw(sy));

    let mut gaps: Vec<u32> = Vec::new();
    let mut worst_error = 0i32;
    let mut total_error = 0i64;
    let mut quantum = i32::MAX;

    for pair in log.seen.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let gap = b.tick.saturating_sub(a.tick);
        if gap == 0 {
            continue;
        }
        gaps.push(gap);

        // Ground covered has to be the gap times one step.
        let want = (step.0 * gap as i32, step.1 * gap as i32);
        let got = (b.x - a.x, b.y - a.y);
        let error = (got.0 - want.0).abs().max((got.1 - want.1).abs());
        worst_error = worst_error.max(error);
        total_error += error as i64;

        for d in [(b.x - a.x).abs(), (b.y - a.y).abs()] {
            if d > 0 {
                quantum = quantum.min(d);
            }
        }
    }

    if gaps.is_empty() {
        println!("no two sightings a tick apart, nothing to measure");
        return;
    }

    gaps.sort_unstable();
    let spanned = log.seen.last().expect("checked").tick - log.seen[0].tick;
    let stalled = gaps.iter().filter(|&&g| g > INTERP_DELAY_TICKS).count();
    let mean_gap = gaps.iter().map(|&g| g as f64).sum::<f64>() / gaps.len() as f64;

    println!();
    println!("{} sightings over {spanned} ticks, {} packets", log.seen.len(), log.packets);
    println!(
        "  neighbours per packet: {:.0}",
        log.others as f64 / log.packets.max(1) as f64
    );
    println!();
    println!("gap between sightings, in ticks");
    println!("  mean {mean_gap:.2}   median {}   worst {}", gaps[gaps.len() / 2], gaps[gaps.len() - 1]);
    println!(
        "  over {INTERP_DELAY_TICKS} ticks: {stalled} of {} ({:.1}%), which is what stalls",
        gaps.len(),
        100.0 * stalled as f64 / gaps.len() as f64
    );
    println!();
    println!("ground covered against gap x step");
    println!("  worst error {worst_error} raw units, mean {:.2}", total_error as f64 / gaps.len() as f64);
    println!("  one step is {} raw units, the wire's quantum is {quantum}", step.0.abs().max(step.1.abs()));
    if worst_error <= quantum {
        println!("  never skipped or doubled a step");
    } else {
        println!("  OFF by more than one quantum, so a step went missing or double counted");
    }
}

/// Millimeters in the raw units positions are held in.
fn raw(mm: i32) -> i32 {
    Fixed::from_millimeters(0, mm).raw()
}
