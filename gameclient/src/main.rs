//! A minimal game client for mildew-valley.
//!
//! Connects to an edge, spawns one entity, plants a single seed of lettuce,
//! and prints every update the server sends back.
//!
//! ```text
//! cargo run --release -p mildew-gameclient
//! cargo run --release -p mildew-gameclient -- --edge 10.0.0.5:7777
//! ```
//!
//! Needs `mv-edge` and `mv-sim` running behind it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mildew_common::command::GameCommand;
use mildew_common::net;
use umwelt::{
    ClientGame, ClientHandle, EdgeClient, EntityHandle, EntityId, EntityKind, Pos3, RegionId,
    TickObservation,
};

/// Prints every callback the edge delivers.
struct Printer {
    planted: Arc<AtomicBool>,
    handle: ClientHandle,
}

impl ClientGame for Printer {
    fn spawned(&mut self, handle: EntityHandle, region: RegionId, entity: EntityId) {
        println!("spawned: handle={handle:?} region={region} entity={entity}");

        // Plant lettuce once the entity exists in a region.
        if !self.planted.swap(true, Ordering::Relaxed) {
            let cmd = GameCommand::PlantLettuce { x: 200, y: 200 };
            match self.handle.entity_send(handle, &cmd.encode()) {
                Ok(()) => println!("sent: PlantLettuce at (200, 200)"),
                Err(e) => eprintln!("failed to send PlantLettuce: {e}"),
            }
        }
    }

    fn removed(&mut self, handle: EntityHandle) {
        println!("removed: handle={handle:?}");
    }

    fn observed(
        &mut self,
        _handle: EntityHandle,
        _region: RegionId,
        observation: &TickObservation<'_>,
    ) {
        let tick = observation.tick();
        for id in observation.despawns() {
            println!("  tick {tick}: despawn {id}");
        }
        for (id, pos, tag) in observation.updates() {
            println!(
                "  tick {tick}: {id} at ({}, {}, {}) tag={tag}",
                pos.x, pos.y, pos.z,
            );
        }
    }

    fn message_received(&mut self, body: &[u8]) {
        println!("message: {} bytes: {body:?}", body.len());
    }

    fn disconnected(&mut self) {
        println!("disconnected");
    }
}

fn main() {
    let edge_addr: String = net::arg_or("edge", net::DEFAULT_EDGE.to_string());
    let region: u32 = net::arg_or("region", 1);

    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let endpoint = net::game_endpoint(runtime.handle());

    let target: std::net::SocketAddr = edge_addr.parse().unwrap_or_else(|e| {
        eprintln!("--edge {edge_addr:?}: {e}");
        std::process::exit(1);
    });

    let conn = runtime
        .block_on(async {
            endpoint
                .connect(target, "localhost")
                .expect("configured")
                .await
        })
        .unwrap_or_else(|e| {
            eprintln!("connecting to {edge_addr}: {e}");
            std::process::exit(1);
        });

    let planted = Arc::new(AtomicBool::new(false));
    let planted_for_game = Arc::clone(&planted);
    let client = EdgeClient::new(conn, runtime.handle().clone(), |handle| Printer {
        planted: planted_for_game,
        handle,
    })
    .unwrap_or_else(|e| {
        eprintln!("opening a stream: {e}");
        std::process::exit(1);
    });

    let sending = client.handle();
    let _entity = sending
        .spawn(
            RegionId::from_raw(region),
            Pos3::from_meters(200, 200, 0),
            EntityKind::observer(0),
        )
        .expect("asks for an entity");

    println!("mildew: connected to {edge_addr}, spawned in region {region}");

    // Stay alive and let the callbacks print. Ctrl-C to quit.
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
