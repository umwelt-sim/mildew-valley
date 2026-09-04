use std::collections::HashMap;
use std::sync::Arc;

use umwelt::net::Inbound;
use umwelt::{EntityId, Fixed, Game, Pos3, Step};
use mildew_common::command::{GameCommand, Heading};
use crate::prefab::{Prefabs, phased::Phased};

pub struct MildewValleyGame {
    inbound: Arc<Inbound>,
    prefabs: Prefabs,
    phased: Phased,
    pending: Vec<(EntityId, GameCommand)>,
    /// Which way each farmer is walking, held until they say otherwise. A
    /// farmer standing still is absent rather than present with a zero.
    walking: HashMap<EntityId, Heading>,
}

impl MildewValleyGame {
    pub fn new(inbound: Arc<Inbound>) -> Self {
        MildewValleyGame {
            inbound,
            prefabs: Prefabs::new(),
            phased: Phased::new(),
            pending: Vec::new(),
            walking: HashMap::new(),
        }
    }

    /// Moves everyone who is walking one step along their heading.
    ///
    /// The step comes from the heading rather than from the sender, so the
    /// distance is the simulation's to decide. A step that would leave the
    /// region stops at the edge, which keeps a farmer walking into a boundary
    /// against it rather than through it.
    fn walk_everyone(&mut self, world: &mut Step<'_>) {
        let bound = world.config().region_size();
        self.walking.retain(|&id, &mut heading| {
            let Some(at) = world.position(id) else {
                // The farmer left, so the heading goes with them.
                return false;
            };
            let (dx, dy) = heading.step_mm();
            world.move_to(
                id,
                Pos3::new(
                    clamp_axis(at.x, mm(dx), bound),
                    clamp_axis(at.y, mm(dy), bound),
                    at.z,
                ),
            );
            true
        });
    }
}

/// A whole number of millimeters as a position offset.
fn mm(v: i32) -> Fixed {
    Fixed::from_millimeters(0, v)
}

/// One axis of a step, stopped at the region's edge.
///
/// A region holds `0 .. bound`, so the last position inside it is one raw unit
/// short of `bound`.
fn clamp_axis(at: Fixed, step: Fixed, bound: Fixed) -> Fixed {
    let moved = at.raw().saturating_add(step.raw());
    Fixed::from_raw(moved.clamp(0, bound.raw() - 1))
}

impl Game for MildewValleyGame {
    fn message_received(&mut self, from: EntityId, body: &[u8]) {
        if let Some(cmd) = GameCommand::decode(body) {
            self.pending.push((from, cmd));
        }
    }

    fn step(&mut self, world: &mut Step<'_>) {
        self.inbound.apply(world);

        for (from, cmd) in self.pending.drain(..) {
            match cmd {
                GameCommand::PlantLettuce { x, y } => {
                    self.phased.spawn(
                        world,
                        Pos3::from_meters(x, y, 0),
                        &self.prefabs.crops.lettuce,
                    );
                }
                GameCommand::Walk { heading } => match heading {
                    Some(h) => {
                        self.walking.insert(from, h);
                    }
                    None => {
                        self.walking.remove(&from);
                    }
                },
            }
        }

        self.walk_everyone(world);
        self.phased.advance_all(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mildew_common::pace::WALK_STEP_MM;
    use umwelt::net::{Edges, Inbound};
    use umwelt::{NullSink, WorldConfig, WorldSimulation};

    type Sim = WorldSimulation<MildewValleyGame, NullSink>;

    fn sim() -> Sim {
        let inbound = Arc::new(Inbound::new(Arc::new(Edges::new())));
        WorldSimulation::new(WorldConfig::default(), MildewValleyGame::new(inbound))
    }

    /// Nothing here spawns farmers, so a planted crop stands in for one. The
    /// walk applies to whatever entity a heading names.
    fn plant(sim: &mut Sim, x: i32, y: i32, nth: u32) -> EntityId {
        sim.deliver_message(
            EntityId::from_raw(0),
            &GameCommand::PlantLettuce { x, y }.encode(),
        );
        sim.tick();
        EntityId::from_raw(nth)
    }

    fn walk(sim: &mut Sim, who: EntityId, heading: Option<Heading>) {
        sim.deliver_message(who, &GameCommand::Walk { heading }.encode());
    }

    fn step_raw() -> i32 {
        Fixed::from_millimeters(0, WALK_STEP_MM).raw()
    }

    #[test]
    fn a_farmer_walks_the_way_it_was_told() {
        let mut s = sim();
        let who = plant(&mut s, 100, 100, 0);
        let before = s.position(who).expect("planted");

        walk(&mut s, who, Some(Heading::East));
        s.tick();

        let after = s.position(who).expect("still here");
        assert_eq!(after.y, before.y, "east does not change y");
        assert_eq!(after.x.raw() - before.x.raw(), step_raw(), "one step east");
    }

    /// The heading holds, so one command moves the entity on every tick after
    /// it. This is what lets a held key send a single message.
    #[test]
    fn one_command_keeps_a_farmer_walking() {
        let mut s = sim();
        let who = plant(&mut s, 100, 100, 0);
        let before = s.position(who).expect("planted");

        walk(&mut s, who, Some(Heading::North));
        for _ in 0..10 {
            s.tick();
        }

        let after = s.position(who).expect("still here");
        assert_eq!(after.y.raw() - before.y.raw(), step_raw() * 10, "ten steps");
    }

    #[test]
    fn a_halt_stops_a_farmer() {
        let mut s = sim();
        let who = plant(&mut s, 100, 100, 0);

        walk(&mut s, who, Some(Heading::North));
        s.tick();
        let stopped_at = s.position(who).expect("still here");

        walk(&mut s, who, None);
        for _ in 0..5 {
            s.tick();
        }

        assert_eq!(s.position(who), Some(stopped_at), "a halted farmer stays put");
    }

    /// A sender chooses the direction and the moment. The distance belongs to
    /// the heading, so no message covers more ground than a walk.
    #[test]
    fn every_heading_covers_one_walk_step() {
        let want = step_raw() as f32;
        for heading in [
            Heading::North,
            Heading::NorthEast,
            Heading::East,
            Heading::SouthEast,
            Heading::South,
            Heading::SouthWest,
            Heading::West,
            Heading::NorthWest,
        ] {
            let (dx, dy) = heading.step_mm();
            let (x, y) = (mm(dx).raw() as f32, mm(dy).raw() as f32);
            let covered = (x * x + y * y).sqrt();
            assert!(
                (covered - want).abs() <= 2.0,
                "{heading:?} covers {covered} against {want}"
            );
        }
    }

    /// The region refuses a position outside itself, so a farmer who could step
    /// past the edge would have every move after that rejected.
    #[test]
    fn a_farmer_stops_at_the_region_edge() {
        let mut s = sim();
        let who = plant(&mut s, 1, 1, 0);

        walk(&mut s, who, Some(Heading::SouthWest));
        for _ in 0..100 {
            s.tick();
        }

        let at = s.position(who).expect("still here");
        assert!(at.x.raw() >= 0 && at.y.raw() >= 0, "inside the region at {at:?}");
    }

    /// A heading naming an entity the world does not have is dropped, so the
    /// table tracks the living rather than everyone who ever walked.
    #[test]
    fn a_heading_for_nobody_is_forgotten() {
        let mut s = sim();
        walk(&mut s, EntityId::from_raw(9_999), Some(Heading::North));
        s.tick();
        assert!(s.game().walking.is_empty(), "no heading survives its entity");
    }
}
