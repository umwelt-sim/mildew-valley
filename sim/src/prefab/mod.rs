use crate::prefab::{crops::crops::CropPrefabs, phased::PhasedEntities};

pub mod crops;
pub mod phased;

pub struct Prefabs {
    pub crops: CropPrefabs,
}

impl Prefabs {
    pub fn new(tick_hz: u32) -> Self {
        let defs = PhasedEntities::new(tick_hz);
        Prefabs {
            crops: CropPrefabs::new(&defs),
        }
    }
}
