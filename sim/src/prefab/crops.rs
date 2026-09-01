use std::time::Duration;
use mildew_common::tags;
use super::phased::PhasedDef;

pub struct CropPrefabs {
    pub lettuce: PhasedDef,
}

impl CropPrefabs {
    pub fn new() -> Self {
        CropPrefabs {
            lettuce: PhasedDef::new(
                tags::lettuce::SEED,
                vec![
                    Duration::from_secs(2),
                    Duration::from_secs(3),
                    Duration::from_secs(10),
                ],
            ),
        }
    }
}
