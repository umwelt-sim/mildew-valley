use serde::{Deserialize, Serialize};

/// A game client's request to the simulation, encoded as the body of a
/// game message. The simulation decides whether to honor it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameCommand {
    PlantLettuce { x: i32, y: i32 },
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
}
