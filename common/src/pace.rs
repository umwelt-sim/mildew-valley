//! How fast things happen.
//!
//! Shared so that a bot loads the server the way a person does. If the load
//! generator walked at a different speed or reported at a different cadence
//! than the game client, a run would measure something nobody will ever do.

/// The rate the simulation ticks at.
///
/// Must match the `tick_hz` the sim is built with. Reporting faster puts the
/// client's frame rate on the wire; reporting slower leaves gaps.
pub const TICK_HZ: f32 = 20.0;

/// One server tick, in seconds.
pub const TICK: f32 = 1.0 / TICK_HZ;

/// The fastest the simulation will accept, from its `WorldConfig`.
pub const SPEED_LIMIT_M_PER_SEC: f32 = 40.0;

/// How fast a farmer walks, in meters per second.
///
/// Well under the limit. The headroom is for mounts and vehicles later.
pub const WALK_M_PER_SEC: f32 = 4.5;

/// How far one tick of walking covers, in millimeters.
///
/// The simulation moves farmers itself and works in fixed point, so the step is
/// a whole number of millimeters rather than a rate to multiply by a frame
/// time.
pub const WALK_STEP_MM: i32 = 225;

/// One axis of a diagonal step, `WALK_STEP_MM / sqrt(2)`, so walking into a
/// corner is not faster than walking along a side.
pub const WALK_STEP_DIAGONAL_MM: i32 = 159;

// Compile-time rather than a test: a walk over the limit would have every
// move refused, and a test only reports that once someone runs it.
const _: () = assert!(WALK_M_PER_SEC < SPEED_LIMIT_M_PER_SEC);
const _: () = assert!(TICK_HZ > 0.0);
// The millimeter step is the rate expressed for one tick.
const _: () = assert!({
    let mm = WALK_M_PER_SEC * TICK * 1000.0;
    mm > WALK_STEP_MM as f32 - 0.5 && mm < WALK_STEP_MM as f32 + 0.5
});
const _: () = assert!((WALK_STEP_MM as f32) < SPEED_LIMIT_M_PER_SEC * TICK * 1000.0);

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

    /// A corner covers the same ground as a side, within the millimeter the
    /// step is rounded to.
    #[test]
    fn a_diagonal_step_is_the_same_length_as_a_straight_one() {
        let d = WALK_STEP_DIAGONAL_MM as f32;
        let diagonal = (d * d * 2.0).sqrt();
        assert!(
            (diagonal - WALK_STEP_MM as f32).abs() < 1.0,
            "a diagonal covers {diagonal} mm against {WALK_STEP_MM}"
        );
    }
}
