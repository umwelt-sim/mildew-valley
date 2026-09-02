//! The mildew-valley region server.
//!
//! Owns a [`WorldSimulation`] with the game, serves a region over NATS, and
//! drives the tick loop. Messages from clients arrive through the edge and
//! are drained into the game each tick.
//!
//! ```text
//! cargo run --release -p mv-sim
//! cargo run --release -p mv-sim -- --region 1 --nats nats://10.0.0.5:4222
//! ```

use std::sync::Arc;
use std::time::Duration;

use mildew_common::net;
use umwelt::net::{EdgeSink, Edges, Inbound, RegionServer};
use umwelt::{
    ClientLimits, Flow, Handoff, Overrun, Pacing, RegionId, Wait, WorldConfig, WorldSimulation,
};

use mv_sim::game::MildewValleyGame;

fn config() -> WorldConfig {
    WorldConfig::builder()
        .region_size_m(4096)
        .vertical_extent_m(1024)
        .horizontal_view_radius_m(256)
        .max_horizontal_speed_m_per_sec(40)
        .tick_hz(20)
        .build()
        .expect("config is valid")
}

fn main() {
    let nats_url: String = net::arg_or("nats", net::DEFAULT_NATS.to_string());
    let region_raw: u32 = net::arg_or("region", 1);
    let edge_timeout: u64 = net::arg_or("edge-timeout", 5);

    let cfg = config();
    let region = RegionId::from_raw(region_raw);
    let edges = Arc::new(Edges::new());
    let inbound = Arc::new(Inbound::new(Arc::clone(&edges)));

    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let nats = runtime
        .block_on(net::connect(&nats_url, net::arg("creds")))
        .unwrap_or_else(|e| {
            eprintln!("nats {nats_url}: {e}");
            std::process::exit(1);
        });

    let _server = RegionServer::new(
        nats.clone(),
        runtime.handle().clone(),
        region,
        cfg,
        Arc::clone(&inbound),
        Duration::from_secs(edge_timeout),
    )
    .unwrap_or_else(|e| {
        eprintln!("serving {region}: {e}");
        std::process::exit(1);
    });

    let sink = EdgeSink::new(region, nats, runtime.handle().clone(), Arc::clone(&edges));
    let mut sim = WorldSimulation::new(cfg, MildewValleyGame::new(Arc::clone(&inbound)))
        .with_sink(Handoff::new(sink.clone()));

    println!("mv-sim: serving {region} over {nats_url}");

    sim.run(
        Pacing { wait: Wait::Sleep, overrun: Overrun::Dilate, ticks: None },
        |_, sim| {
            for (from, body) in inbound.drain_messages() {
                sim.deliver_message(from, &body);
            }
            inbound.settle(sim, &sink, ClientLimits::default());
            Flow::Continue
        },
    );
}
