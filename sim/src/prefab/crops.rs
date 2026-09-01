pub mod crops {
    use std::time::Duration;

use mildew_common::{phased::PhasedEntity, tags};

use crate::prefab::phased::PhasedEntities;

    pub struct CropPrefabs {
        pub lettuce: PhasedEntity,        
    }

    impl CropPrefabs {
        pub fn new(defs: &PhasedEntities) -> Self {
            CropPrefabs { lettuce:  
                defs.define(tags::crops::lettuce::SEED, 
                    &[
                        Duration::from_secs(2),
                        Duration::from_secs(3),
                        Duration::from_secs(10)
                    ]
                )
            }                 
        }
    }
}
