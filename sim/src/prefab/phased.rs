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
    use std::sync::Arc;
    use crate::game::MildewValleyGame;
    use mildew_common::command::GameCommand;
    use mildew_common::tags;
    use umwelt::net::{Edges, Inbound};
    use umwelt::{EntityId, WorldConfig, WorldSimulation};

    #[test]
    fn a_plant_lettuce_command_produces_ripe_lettuce() {
        let inbound = Arc::new(Inbound::new(Arc::new(Edges::new())));
        let mut sim = WorldSimulation::new(
            WorldConfig::default(),
            MildewValleyGame::new(inbound),
        );

        let cmd = GameCommand::PlantLettuce { x: 0, y: 0 };
        sim.deliver_message(EntityId::from_raw(0), &cmd.encode());

        // One tick to process the pending command.
        sim.tick();
        let lettuce = EntityId::from_raw(0);
        assert_eq!(sim.tag(lettuce), Some(tags::lettuce::SEED));

        // Enough ticks for it to ripen (default 20 Hz, transitions at 2s + 3s + 10s = 15s).
        for _ in 0..400 {
            sim.tick();
        }
        assert_eq!(sim.tag(lettuce), Some(tags::lettuce::RIPE));
    }
}
