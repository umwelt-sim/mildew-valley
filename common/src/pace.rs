//! How fast things happen.
//!
//! Shared so that a bot loads the server the way a person does. If the load
//! generator walked at a different speed or reported at a different cadence
//! than the game client, a run would measure something nobody will ever do.

/// The rate the simulation ticks at.
///
/// Must match the `tick_hz` the sim is built with. A client that reports
/// faster is putting its own frame rate on the wire; one that reports slower
/// leaves the region interpolating across gaps.
pub const TICK_HZ: f32 = 20.0;

/// One server tick, in seconds.
pub const TICK: f32 = 1.0 / TICK_HZ;

/// The fastest the simulation will accept, from its `WorldConfig`.
pub const SPEED_LIMIT_M_PER_SEC: f32 = 40.0;

/// How fast a farmer walks, in meters per second.
///
/// Well under the limit, because this is a walk. The headroom is there for
/// whatever a cart or a horse turns out to be.
pub const WALK_M_PER_SEC: f32 = 4.5;

// Checked where it cannot be ignored: a walk that outran the limit would have
// every move refused, and a test would only say so once someone ran it.
const _: () = assert!(WALK_M_PER_SEC < SPEED_LIMIT_M_PER_SEC);
const _: () = assert!(TICK_HZ > 0.0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tick_is_the_reciprocal_of_the_rate() {
        assert!((TICK * TICK_HZ - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_tick_of_walking_is_a_sane_step() {
        // Far enough to be worth sending, short enough that a dropped packet
        // does not teleport anyone.
        let step = WALK_M_PER_SEC * TICK;
        assert!((0.05..1.0).contains(&step), "a tick covers {step} m");
    }
}
