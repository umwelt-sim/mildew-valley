use umwelt::{EntityId, Game, Pos3, Step};
use mildew_common::command::GameCommand;
use crate::prefab::{Prefabs, phased::Phased};

pub struct MildewValleyGame {
    prefabs: Prefabs,
    phased: Phased,
    pending: Vec<(EntityId, GameCommand)>,
}

impl MildewValleyGame {
    pub fn new() -> Self {
        MildewValleyGame {
            prefabs: Prefabs::new(),
            phased: Phased::new(),
            pending: Vec::new(),
        }
    }

}

impl Game for MildewValleyGame {
    fn message_received(&mut self, from: EntityId, body: &[u8]) {
        if let Some(cmd) = GameCommand::decode(body) {
            self.pending.push((from, cmd));
        }
    }

    fn step(&mut self, world: &mut Step<'_>) {
        for (_, cmd) in self.pending.drain(..) {
            match cmd {
                GameCommand::PlantLettuce { x, y } => {
                    self.phased.spawn(
                        world,
                        Pos3::from_meters(x, y, 0),
                        &self.prefabs.crops.lettuce,
                    );
                }
            }
        }
        self.phased.advance_all(world);
    }
}
