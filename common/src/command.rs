use serde::{Deserialize, Serialize};

use crate::pace::{WALK_STEP_DIAGONAL_MM, WALK_STEP_MM};

/// Which way a farmer is walking.
///
/// Eight directions, which is what a keyboard produces. A heading names a
/// direction and nothing else. The step it stands for is fixed here, so a
/// sender cannot ask to travel further in one tick than a walk covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Heading {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl Heading {
    /// How far one tick of walking moves, in millimeters on each axis.
    pub const fn step_mm(self) -> (i32, i32) {
        const S: i32 = WALK_STEP_MM;
        const D: i32 = WALK_STEP_DIAGONAL_MM;
        match self {
            Heading::North => (0, S),
            Heading::NorthEast => (D, D),
            Heading::East => (S, 0),
            Heading::SouthEast => (D, -D),
            Heading::South => (0, -S),
            Heading::SouthWest => (-D, -D),
            Heading::West => (-S, 0),
            Heading::NorthWest => (-D, D),
        }
    }

    /// The heading a pair of axis inputs names, or `None` when neither axis is
    /// pressed. What a keyboard produces.
    ///
    /// Only the sign of each input is read, so a caller holding a longer vector
    /// gets the same heading and the same step.
    pub fn from_axes(dx: f32, dy: f32) -> Option<Heading> {
        of_signs(sign(dx), sign(dy))
    }

    /// The heading closest to a direction, or `None` if there is no direction.
    ///
    /// For anything aiming at a point rather than pressing keys. Each heading
    /// covers 45 degrees, so an axis shorter than `tan(22.5 degrees)` of the
    /// other drops out and leaves a straight line rather than a diagonal.
    pub fn toward(dx: f32, dy: f32) -> Option<Heading> {
        const OCTANT: f32 = 0.414_213_57;
        let (ax, ay) = (dx.abs(), dy.abs());
        of_signs(
            if ax >= ay * OCTANT { sign(dx) } else { 0 },
            if ay >= ax * OCTANT { sign(dy) } else { 0 },
        )
    }
}

/// Which side of zero a value falls on, treating a hair either way as zero.
fn sign(v: f32) -> i32 {
    const DEAD_ZONE: f32 = 1e-3;
    if v > DEAD_ZONE {
        1
    } else if v < -DEAD_ZONE {
        -1
    } else {
        0
    }
}

fn of_signs(sx: i32, sy: i32) -> Option<Heading> {
    Some(match (sx, sy) {
        (0, 1) => Heading::North,
        (1, 1) => Heading::NorthEast,
        (1, 0) => Heading::East,
        (1, -1) => Heading::SouthEast,
        (0, -1) => Heading::South,
        (-1, -1) => Heading::SouthWest,
        (-1, 0) => Heading::West,
        (-1, 1) => Heading::NorthWest,
        _ => return None,
    })
}

/// A game client's request to the simulation, encoded as the body of a
/// game message. The simulation decides whether to honor it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameCommand {
    PlantLettuce { x: i32, y: i32 },
    /// Walk this way until told otherwise. `None` stands still.
    ///
    /// The simulation owns the position and moves the entity itself. A sender
    /// chooses the direction and the moment, never the distance.
    Walk { heading: Option<Heading> },
}

impl GameCommand {
    /// Serializes to a compact binary representation (postcard).
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("GameCommand is always encodable")
    }

    /// Deserializes from bytes produced by [`encode`](Self::encode).
    /// Returns `None` if the bytes are malformed or truncated.
    pub fn decode(bytes: &[u8]) -> Option<GameCommand> {
        postcard::from_bytes(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let cmd = GameCommand::PlantLettuce { x: 12, y: -34 };
        let bytes = cmd.encode();
        let back = GameCommand::decode(&bytes).expect("decodes");
        assert_eq!(back, cmd);
    }

    #[test]
    fn garbage_returns_none() {
        assert_eq!(GameCommand::decode(&[0xFF, 0xFF, 0xFF]), None);
    }

    #[test]
    fn empty_returns_none() {
        assert_eq!(GameCommand::decode(&[]), None);
    }

    #[test]
    fn a_walk_round_trips() {
        for heading in [None, Some(Heading::North), Some(Heading::SouthWest)] {
            let cmd = GameCommand::Walk { heading };
            assert_eq!(GameCommand::decode(&cmd.encode()), Some(cmd));
        }
    }

    #[test]
    fn keys_name_the_eight_directions() {
        assert_eq!(Heading::from_axes(0.0, 1.0), Some(Heading::North));
        assert_eq!(Heading::from_axes(1.0, 1.0), Some(Heading::NorthEast));
        assert_eq!(Heading::from_axes(-1.0, 0.0), Some(Heading::West));
        assert_eq!(Heading::from_axes(0.0, 0.0), None, "no key is no heading");
    }

    /// A longer vector is the same heading, so no sender can ask for a longer
    /// step by pushing harder.
    #[test]
    fn magnitude_does_not_change_a_heading() {
        assert_eq!(Heading::from_axes(1.0, 0.0), Heading::from_axes(1000.0, 0.0));
        assert_eq!(Heading::from_axes(1.0, 1.0), Heading::from_axes(50.0, 50.0));
    }

    /// A target far off one axis and barely off the other reads as a straight
    /// line, not a diagonal.
    #[test]
    fn a_direction_picks_the_nearest_of_the_eight() {
        assert_eq!(Heading::toward(100.0, 1.0), Some(Heading::East));
        assert_eq!(Heading::toward(100.0, 100.0), Some(Heading::NorthEast));
        assert_eq!(Heading::toward(-1.0, -100.0), Some(Heading::South));
        assert_eq!(Heading::toward(0.0, 0.0), None);
    }

    /// Every heading is reachable from some direction, so nothing in the table
    /// is unusable.
    #[test]
    fn every_heading_is_reachable() {
        let mut seen = std::collections::HashSet::new();
        for step in 0..360 {
            let a = (step as f32).to_radians();
            if let Some(h) = Heading::toward(a.cos(), a.sin()) {
                seen.insert(h);
            }
        }
        assert_eq!(seen.len(), 8, "saw {seen:?}");
    }
}
