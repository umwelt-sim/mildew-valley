//! The mildew-valley edge server.
//!
//! Relays between game clients and regions. Entity spawns, moves, despawns,
//! and `entity_send` messages are all handled by the library — this binary
//! just stands up the QUIC and NATS endpoints.
//!
//! ```text
//! cargo run --release -p mv-edge
//! cargo run --release -p mv-edge -- --edge 0.0.0.0:7777
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mildew_common::net;
use umwelt::{EdgeGame, EdgeServer};

struct Game;
impl EdgeGame for Game {}

fn main() {
    let nats_url: String = net::arg_or("nats", net::DEFAULT_NATS.to_string());
    let listen: String = net::arg_or("edge", net::DEFAULT_EDGE.to_string());

    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let nats = runtime
        .block_on(net::connect(&nats_url, net::arg("creds")))
        .unwrap_or_else(|e| {
            eprintln!("nats {nats_url}: {e}");
            std::process::exit(1);
        });
    let quic = net::edge_endpoint(&listen, runtime.handle());

    let server = EdgeServer::new(nats, runtime.handle().clone(), quic, |_handle| Game)
        .unwrap_or_else(|e| {
            eprintln!("starting the edge: {e}");
            std::process::exit(1);
        });
    println!("mv-edge: {} listening on {listen}", server.name());

    let stop = AtomicBool::new(false);
    let mut reported = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
        if reported.elapsed() < Duration::from_secs(1) {
            continue;
        }
        let stats = server.stats();
        println!(
            "mv-edge: {} clients | {} entities ({} observing) | \
             relayed {} undeliverable {} | commands {} refused {}",
            stats.clients,
            stats.entities,
            stats.observers,
            stats.relayed,
            stats.undeliverable,
            stats.commands,
            stats.refused,
        );
        reported = Instant::now();
    }
}
