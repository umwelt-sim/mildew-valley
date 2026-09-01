use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhasedEntity {
    pub base_tag: u16,
    pub thresholds: Vec<u32>,
}

impl PhasedEntity {
    pub fn new(base_tag: u16, transitions: &[Duration], tick_hz: u32) -> PhasedEntity {
        let mut thresholds = Vec::with_capacity(transitions.len());
        let mut acc: u32 = 0;
        for dur in transitions {
            let ticks = (dur.as_millis()) as u32 * tick_hz / 1000;
            assert!(ticks > 0, "transition cannot be shorter than 1 tick");
            acc += ticks;
            thresholds.push(acc);
        }
        PhasedEntity { base_tag, thresholds }
    }

    pub fn phases(&self) -> u8 {
        (self.thresholds.len() + 1) as u8
    }

    pub fn tag_for(&self, phase: u8) -> u16 {
        self.base_tag + phase as u16
    }

    pub fn phase_at(&self, age: u32) -> u8 {
        let mut phase = 0u8;
        for &t in &self.thresholds {
            if age >= t {
                phase += 1;
            } else {
                break;
            }
        }
        phase
    }
}
