use std::{collections::HashMap, hash::Hash, time::Duration};
use mildew_common::phased::PhasedEntity;
use umwelt::{EntityId, Pos3, Step};

pub struct PhasedEntities {
    tick_hz: u32,
}

impl PhasedEntities {
    pub fn new(tick_hz: u32) -> Self {
        Self { tick_hz }
    }

    pub fn define(&self, base_tag: u16, transitions: &[Duration]) -> PhasedEntity {
        PhasedEntity::new(base_tag, transitions, self.tick_hz)
    }
}

pub struct Phased {
    ids: Vec<EntityId>,
    defs: Vec<PhasedEntity>,
    started_at: Vec<u32>,
    index: HashMap<EntityId, usize>,    
}

impl Phased {
    pub fn new() -> Self {
        Phased {
            ids: Vec::new(),
            defs: Vec::new(),
            started_at: Vec::new(),
            index: HashMap::new(),
        }
    }
    pub fn spawn(
        &mut self,
        w: &mut Step<'_>,
        pos: Pos3,
        def: &PhasedEntity,
    ) -> EntityId {
        let id = w.spawn(pos, def.tag_for(0));
        self.insert(id, def, w.tick());
        id
    }

    pub fn spawn_at_existing(
        &mut self,
        w: &mut Step<'_>,
        id: EntityId,
        def: &PhasedEntity,
    ) {
        w.set_tag(id, def.tag_for(0));
        self.insert(id, def, w.tick());
    }

    pub fn advance_all(&self, w: &mut Step<'_>) {
        let tick = w.tick();
        for i in 0..self.ids.len() {
            let d = &self.defs[i];
            let age = tick.wrapping_sub(self.started_at[i]);
            let phase = d.phase_at(age);
            w.set_tag(self.ids[i], d.tag_for(phase));
        }
    }

    pub fn remove(&mut self, id: EntityId) {
        let Some(slot) = self.index.remove(&id) else {return };
        let last = self.ids.len() - 1;
        if slot != last {
            let moved_id = self.ids[last];
            self.ids.swap(slot, last);
            self.defs.swap(slot, last);
            self.started_at.swap(slot, last);
            self.index.insert(moved_id, slot);
        }
        self.ids.pop();
        self.defs.pop();
        self.started_at.pop();
    }

    fn insert(&mut self, id: EntityId, def: &PhasedEntity, tick: u32) {
        let slot = self.ids.len();
        self.ids.push(id);
        self.defs.push(def.clone());
        self.started_at.push(tick);
        self.index.insert(id, slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefab::Prefabs;    
    use umwelt::{Game, Pos3, WorldConfig, WorldSimulation};

    struct PhaseHarness {
        phased :Phased,
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
        let tick_hz = 20;        
        let mut sim = WorldSimulation::new(
            WorldConfig::default(),
            PhaseHarness {
                phased: Phased::new(),
                prefabs: Prefabs::new(tick_hz),
                planted: false,
                seen: Vec::new()
            }
        );

        for _ in 0..1000 {
            sim.tick();
        }

        assert_eq!(sim.game().seen, mildew_common::tags::crops::lettuce::RANGE.collect::<Vec<u16>>());
        
    }
}