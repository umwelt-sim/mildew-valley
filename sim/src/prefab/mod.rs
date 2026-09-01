use crops::CropPrefabs;

pub mod crops;
pub mod phased;

pub struct Prefabs {
    pub crops: CropPrefabs,
}

impl Prefabs {
    pub fn new() -> Self {
        Prefabs {
            crops: CropPrefabs::new(),
        }
    }
}
