use umwelt::{Game, Step};
use crate::prefab::{Prefabs, phased::Phased};

pub struct MildewValleyGame {
    prefabs: Prefabs,
    phased: Phased,
}

impl MildewValleyGame {
    pub fn new() -> Self {
        MildewValleyGame {
            prefabs: Prefabs::new(),
            phased: Phased::new(),
        }
    }
}

impl Game for MildewValleyGame {
    fn step(&mut self, world: &mut Step<'_>) {
        self.phased.advance_all(world);
    }
}
