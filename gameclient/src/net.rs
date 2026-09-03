//! The edge connection, and the callbacks that keep [`World`] current.
//!
//! Everything here runs on the runtime's threads, not the render loop's. The
//! only thing the two share is the mutex around [`World`].

use std::sync::{Arc, Mutex};
use std::time::Instant;

use mildew_common::net;
use umwelt::{ClientGame, EntityHandle, EntityId, RegionId, TickObservation};

use crate::world::{Track, World};

/// Opens a QUIC connection to an edge.
///
/// Blocks the calling thread until the handshake finishes, which is fine
/// before the window opens and would not be once it has.
pub fn dial(edge: &str, runtime: &tokio::runtime::Runtime) -> Result<quinn::Connection, String> {
    let endpoint = net::game_endpoint(runtime.handle());
    let target: std::net::SocketAddr =
        edge.parse().map_err(|e| format!("--edge {edge:?}: {e}"))?;

    runtime
        .block_on(async {
            endpoint
                .connect(target, "localhost")
                .map_err(|e| e.to_string())?
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(|e| format!("connecting to {edge}: {e}"))
}

/// Writes what the edge reports into the world the renderer reads.
pub struct Link {
    world: Arc<Mutex<World>>,
    /// Echo every callback to stdout, the way `mildew-probe` does.
    verbose: bool,
}

impl Link {
    pub fn new(world: Arc<Mutex<World>>, verbose: bool) -> Link {
        Link { world, verbose }
    }
}

impl ClientGame for Link {
    fn spawned(&mut self, _handle: EntityHandle, region: RegionId, entity: EntityId) {
        let mut world = self.world.lock().expect("the world lock");
        world.player_entity = Some(entity);
        world.connected = true;
        if self.verbose {
            println!("spawned: region={region} entity={entity}");
        }
    }

    fn removed(&mut self, _handle: EntityHandle) {
        let mut world = self.world.lock().expect("the world lock");
        world.player_entity = None;
        if self.verbose {
            println!("removed");
        }
    }

    fn observed(
        &mut self,
        _handle: EntityHandle,
        _region: RegionId,
        observation: &TickObservation<'_>,
    ) {
        let now = Instant::now();
        let mut world = self.world.lock().expect("the world lock");

        world.tick = observation.tick();
        world.stats.packets += 1;
        world.stats.last_packet = Some(now);

        for id in observation.despawns() {
            world.entities.remove(&id);
            world.stats.despawns += 1;
        }
        for (id, pos, tag) in observation.updates() {
            let at = (pos.x.to_f32(), pos.y.to_f32());
            world
                .entities
                .entry(id)
                .and_modify(|track| track.push(at, tag, now))
                .or_insert_with(|| Track::new(at, tag, now));
            world.stats.updates += 1;
        }
    }

    fn message_received(&mut self, body: &[u8]) {
        if self.verbose {
            println!("message: {} bytes", body.len());
        }
    }

    fn disconnected(&mut self) {
        let mut world = self.world.lock().expect("the world lock");
        world.connected = false;
        world.entities.clear();
        if self.verbose {
            println!("disconnected");
        }
    }
}
