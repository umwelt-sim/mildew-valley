pub mod crops {
    pub mod lettuce {
        use std::ops::RangeInclusive;

        pub const SEED: u16 = 1;
        pub const SPROUT: u16 = 2;
        pub const GROWING: u16 = 3;
        pub const RIPE: u16 = 4;

        pub const RANGE: RangeInclusive<u16> = SEED..=RIPE;
    }
}