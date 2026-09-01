use std::collections::HashMap;
use std::time::Duration;
use umwelt::{EntityId, Pos3, Step};

/// A template for an entity that advances through contiguous tags on a timer.
///
/// Stores human-readable durations. The conversion to tick thresholds happens
/// at spawn time, when the world's tick rate is available.
#[derive(Clone, Debug)]
pub struct PhasedDef {
    base_tag: u16,
    transitions: Vec<Duration>,
}

impl PhasedDef {
    pub fn new(base_tag: u16, transitions: Vec<Duration>) -> PhasedDef {
        PhasedDef { base_tag, transitions }
    }

    pub fn tag_for(&self, phase: u8) -> u16 {
        self.base_tag + phase as u16
    }

    /// Cumulative tick thresholds for each phase transition.
    fn thresholds(&self, tick_hz: u32) -> Vec<u32> {
        let mut acc = 0u32;
        self.transitions
            .iter()
            .map(|dur| {
                let ticks = dur.as_millis() as u32 * tick_hz / 1000;
                assert!(ticks > 0, "transition shorter than one tick");
                acc += ticks;
                acc
            })
            .collect()
    }
}

/// Dense storage for entities that advance through phases.
pub struct Phased {
    ids: Vec<EntityId>,
    base_tags: Vec<u16>,
    thresholds: Vec<Vec<u32>>,
    started_at: Vec<u32>,
    index: HashMap<EntityId, usize>,
}

impl Phased {
    pub fn new() -> Self {
        Phased {
            ids: Vec::new(),
            base_tags: Vec::new(),
            thresholds: Vec::new(),
            started_at: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn spawn(
        &mut self,
        w: &mut Step<'_>,
        pos: Pos3,
        def: &PhasedDef,
    ) -> EntityId {
        let id = w.spawn(pos, def.tag_for(0));
        self.insert(id, def, w);
        id
    }

    /// Attach a phase progression to an entity that already exists
    /// (e.g. a mob that just died and is now a decaying corpse).
    pub fn spawn_at_existing(
        &mut self,
        w: &mut Step<'_>,
        id: EntityId,
        def: &PhasedDef,
    ) {
        w.set_tag(id, def.tag_for(0));
        self.insert(id, def, w);
    }

    pub fn advance_all(&self, w: &mut Step<'_>) {
        let tick = w.tick();
        for i in 0..self.ids.len() {
            let age = tick.wrapping_sub(self.started_at[i]);
            let phase = phase_at(&self.thresholds[i], age);
            w.set_tag(self.ids[i], self.base_tags[i] + phase as u16);
        }
    }

    pub fn remove(&mut self, id: EntityId) {
        let Some(slot) = self.index.remove(&id) else { return };
        let last = self.ids.len() - 1;
        if slot != last {
            let moved_id = self.ids[last];
            self.ids.swap(slot, last);
            self.base_tags.swap(slot, last);
            self.thresholds.swap(slot, last);
            self.started_at.swap(slot, last);
            self.index.insert(moved_id, slot);
        }
        self.ids.pop();
        self.base_tags.pop();
        self.thresholds.pop();
        self.started_at.pop();
    }

    fn insert(&mut self, id: EntityId, def: &PhasedDef, w: &Step<'_>) {
        let slot = self.ids.len();
        self.ids.push(id);
        self.base_tags.push(def.base_tag);
        self.thresholds.push(def.thresholds(w.config().tick_hz()));
        self.started_at.push(w.tick());
        self.index.insert(id, slot);
    }
}

fn phase_at(thresholds: &[u32], age: u32) -> u8 {
    let mut phase = 0u8;
    for &t in thresholds {
        if age >= t {
            phase += 1;
        } else {
            break;
        }
    }
    phase
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefab::Prefabs;
    use mildew_common::tags;
    use umwelt::{Game, Pos3, WorldConfig, WorldSimulation};

    struct PhaseHarness {
        phased: Phased,
        prefabs: Prefabs,
        planted: bool,
        seen: Vec<u16>,
    }

    impl Game for PhaseHarness {
        fn step(&mut self, world: &mut Step<'_>) {
            if !self.planted {
                self.planted = true;
                self.phased.spawn(world, Pos3::ZERO, &self.prefabs.crops.lettuce);
            }
            self.phased.advance_all(world);

            let tag = world.tag(self.phased.ids[0]).unwrap();
            if self.seen.last() != Some(&tag) {
                self.seen.push(tag);
            }
        }
    }

    #[test]
    fn lettuce_grows_through_all_phases() {
        let mut sim = WorldSimulation::new(
            WorldConfig::default(),
            PhaseHarness {
                phased: Phased::new(),
                prefabs: Prefabs::new(),
                planted: false,
                seen: Vec::new(),
            },
        );

        for _ in 0..1000 {
            sim.tick();
        }

        assert_eq!(
            sim.game().seen,
            tags::lettuce::RANGE.collect::<Vec<u16>>(),
        );
    }
}
