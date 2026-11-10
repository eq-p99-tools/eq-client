//! Doors as the client shows them: the server's definitions and actions, plus
//! the motion and returns that clients do on their own.
use crate::{GameDialect, WorldPosition};
pub use eq_network_game::doors::titanium::{Slide, Solidity, Vertical};
pub use eq_network_game::doors::{Door, DoorUpdate, Doors, decode, eqmac, titanium};
use std::collections::BTreeMap;
use std::time::Instant;

/// Server door actions; inverted doors swap them.
const OPEN: u8 = 2;
const CLOSE: u8 = 3;

/// Server door definitions/actions with generation-selected local motion and
/// return timing: Titanium's own door rules ([`titanium`]), `EQMac`'s hinge.
#[derive(Clone, Debug)]
pub struct DoorTable {
    doors: Doors,
    closing: BTreeMap<u8, Instant>,
    generation: GameDialect,
    hinges: BTreeMap<u8, eqmac::Hinge>,
    lifts: titanium::Lifts,
    zone_id: u16,
}

impl Default for DoorTable {
    fn default() -> Self {
        Self {
            doors: Doors::default(),
            closing: BTreeMap::new(),
            generation: GameDialect::Titanium,
            hinges: BTreeMap::new(),
            lifts: titanium::Lifts::default(),
            zone_id: 0,
        }
    }
}

impl DoorTable {
    /// Current definitions in stable door-ID order.
    #[must_use]
    pub fn entries(&self) -> &BTreeMap<u8, Door> {
        self.doors.entries()
    }

    /// The zone's number, which a few of Titanium's door rules name.
    pub const fn set_zone(&mut self, zone_id: u16) {
        self.zone_id = zone_id;
    }

    /// Applies a Titanium server update with its own motion and returns.
    pub fn apply(&mut self, update: &DoorUpdate, now: Instant) {
        self.apply_for(update, now, GameDialect::Titanium);
    }

    /// Applies the selected generation's definition/action and local motion rules.
    /// Unknown generations keep definitions without offering interaction.
    pub fn apply_for(&mut self, update: &DoorUpdate, now: Instant, generation: GameDialect) {
        if self.generation != generation {
            self.closing.clear();
            self.hinges.clear();
            self.lifts.clear();
            self.generation = generation;
        }
        self.doors.apply(update);
        if generation == GameDialect::EqMac {
            self.apply_hinges(update, now);
            return;
        }
        if generation != GameDialect::Titanium {
            return;
        }
        match update {
            DoorUpdate::RemoveAll => {
                self.closing.clear();
                self.lifts.clear();
            }
            DoorUpdate::Spawn(doors) => {
                for door in doors {
                    self.lifts.spawned(door, now);
                    self.schedule(door.id, now);
                }
            }
            DoorUpdate::Move { id, .. } => {
                if let Some(door) = self.doors.entries().get(id) {
                    self.lifts.moved(door, now);
                }
                self.schedule(*id, now);
            }
        }
    }

    fn apply_hinges(&mut self, update: &DoorUpdate, now: Instant) {
        match update {
            DoorUpdate::RemoveAll => self.hinges.clear(),
            DoorUpdate::Spawn(doors) => {
                for door in doors {
                    if let Some(hinge) = eqmac::Hinge::new(door, now) {
                        self.hinges.insert(door.id, hinge);
                    } else {
                        self.hinges.remove(&door.id);
                    }
                }
            }
            DoorUpdate::Move { id, action } => {
                if let Some(door) = self
                    .doors
                    .entries()
                    .get(id)
                    .filter(|door| eqmac::ordinary(door))
                {
                    if let Some(hinge) = self.hinges.get_mut(id) {
                        hinge.action(*action, now);
                    } else if let Some(hinge) = eqmac::Hinge::new(door, now) {
                        self.hinges.insert(*id, hinge);
                    }
                } else {
                    self.hinges.remove(id);
                }
            }
        }
    }

    /// Whether the definition belongs to this generation's supported use path.
    /// This is not evidence that the server will unlock or accept a click.
    #[must_use]
    pub fn supports_use(&self, id: u8) -> bool {
        self.doors
            .entries()
            .get(&id)
            .is_some_and(|door| match self.generation {
                GameDialect::Titanium => id != u8::MAX,
                GameDialect::EqMac => eqmac::ordinary(door),
                _ => false,
            })
    }

    /// Distance to an ordinary usable door, under this generation's reach and
    /// local movement guard. No command is queued and no door state is predicted.
    #[must_use]
    pub fn use_distance(&self, id: u8, position: WorldPosition, now: Instant) -> Option<f32> {
        if !self.supports_use(id) {
            return None;
        }
        let reach = match self.generation {
            GameDialect::Titanium => eq_network_game::reach::TITANIUM.doors,
            GameDialect::EqMac => {
                let mut hinge = self.hinges.get(&id)?.clone();
                hinge.tick(now);
                if !hinge.settled() {
                    return None;
                }
                eq_network_game::reach::EQMAC.doors
            }
            _ => return None,
        };
        let door = self.doors.entries().get(&id)?;
        if !reach.admits(position, door.position) {
            return None;
        }
        let flat = (door.position.x - position.x).hypot(door.position.y - position.y);
        Some(if reach.flat {
            flat
        } else {
            flat.hypot(door.position.z - position.z)
        })
    }

    /// How far a Titanium vertical door stands from its placed height at `now`.
    #[must_use]
    pub fn lift_offset(&self, id: u8, now: Instant) -> Option<f32> {
        self.lifts.offset(id, now)
    }

    /// Titanium's vertical doors as this table steps them.
    #[must_use]
    pub const fn lifts(&self) -> &titanium::Lifts {
        &self.lifts
    }

    /// Whether door `id` holds the player up and stops them walking: the
    /// program's rule on Titanium, and both on a generation whose rule is not
    /// read (recording checklist 6.425).
    #[must_use]
    pub fn solidity(&self, id: u8) -> Solidity {
        match (self.generation, self.doors.entries().get(&id)) {
            (GameDialect::Titanium, Some(door)) => Solidity::of(door.open_type, self.zone_id),
            _ => Solidity::BOTH,
        }
    }

    /// The ordinary `EQMac` hinge's local pose; unsupported mechanisms have none.
    #[must_use]
    pub fn hinge_fraction(&self, id: u8) -> Option<f32> {
        self.hinges.get(&id).map(eqmac::Hinge::fraction)
    }

    /// Whether any door is due to close by `now`.
    #[must_use]
    pub fn closes_due(&self, now: Instant) -> bool {
        self.closing.values().any(|at| *at <= now)
            || self.hinges.values().any(|hinge| hinge.return_due(now))
    }

    /// Advances local motion and returns doors whose local delay has expired.
    /// Any resulting table action is presentation state, not server evidence.
    pub fn close_due(&mut self, now: Instant) {
        for (&id, hinge) in &mut self.hinges {
            let before = hinge.endpoint();
            hinge.tick(now);
            if before != hinge.endpoint() {
                // Local return animation, as for Titanium's closing below.
                // This is presentation state, not a received server action.
                let action = if hinge.endpoint() { OPEN } else { CLOSE };
                self.doors.apply(&DoorUpdate::Move { id, action });
            }
        }
        let due: Vec<u8> = self
            .closing
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.closing.remove(&id);
            if let Some(door) = self.doors.entries().get(&id) {
                // Local presentation of the program's own return, not a
                // received server action. The vertical motion already turns
                // back at this time inside the lift's own steps.
                let action = titanium::return_action(door);
                self.doors.apply(&DoorUpdate::Move { id, action });
                if let Some(door) = self.doors.entries().get(&id) {
                    self.lifts.moved(door, now);
                }
            }
        }
    }

    /// Times Titanium's own return of a door away from its rest (0x48faa5).
    /// Types 40 and 45 return by a click the session sends, and wait for the
    /// server's action (read R1c).
    fn schedule(&mut self, id: u8, now: Instant) {
        match self
            .doors
            .entries()
            .get(&id)
            .filter(|door| !titanium::returns_by_click(door))
            .and_then(|door| titanium::returns_at(door, now))
        {
            Some(at) => self.closing.insert(id, at),
            None => self.closing.remove(&id),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A type 0 door's own return (0x48faa5, read R1b).
    const CLOSE_DELAY: Duration = Duration::from_secs(10);

    fn ordinary() -> Door {
        Door {
            id: 7,
            model: "DOOR".into(),
            position: WorldPosition::default(),
            incline: 0,
            size: 100,
            open_type: 0,
            state_at_spawn: 0,
            invert_state: 0,
            parameter: 0,
            action: None,
        }
    }

    #[test]
    fn eqmac_use_obeys_flat_reach_and_rejects_unknown_or_special_definitions() {
        let now = Instant::now();
        let mut table = DoorTable::default();
        let mut definition = ordinary();
        definition.position = WorldPosition {
            x: 20.0,
            z: 100.0,
            ..WorldPosition::default()
        };
        table.apply_for(
            &DoorUpdate::Spawn(vec![definition.clone()]),
            now,
            GameDialect::EqMac,
        );
        assert_eq!(
            table.use_distance(7, WorldPosition::default(), now),
            Some(20.0)
        );
        let mut titanium = DoorTable::default();
        titanium.apply(&DoorUpdate::Spawn(vec![definition.clone()]), now);
        assert_eq!(
            titanium.use_distance(7, WorldPosition::default(), now),
            None
        );
        for open_type in [54, 55, 56, 57, 58, 120, 255] {
            definition.open_type = open_type;
            table.apply_for(
                &DoorUpdate::Spawn(vec![definition.clone()]),
                now,
                GameDialect::EqMac,
            );
            assert!(!table.supports_use(7));
            assert_eq!(table.use_distance(7, WorldPosition::default(), now), None);
        }
        table.apply_for(&DoorUpdate::RemoveAll, now, GameDialect::EqMac);
        assert!(!table.supports_use(7));
        assert_eq!(table.hinge_fraction(7), None);
    }

    #[test]
    fn eqmac_received_motion_and_local_return_gate_use_without_a_five_second_guess() {
        let start = Instant::now();
        let mut table = DoorTable::default();
        table.apply_for(
            &DoorUpdate::Spawn(vec![ordinary()]),
            start,
            GameDialect::EqMac,
        );
        assert_eq!(table.hinge_fraction(7), Some(0.0));
        assert!(
            table
                .use_distance(7, WorldPosition::default(), start)
                .is_some()
        );
        table.apply_for(
            &DoorUpdate::Move { id: 7, action: 2 },
            start,
            GameDialect::EqMac,
        );
        table.close_due(start + Duration::from_millis(400));
        assert_eq!(table.hinge_fraction(7), Some(0.5));
        assert_eq!(
            table.use_distance(
                7,
                WorldPosition::default(),
                start + Duration::from_millis(400)
            ),
            None
        );
        table.close_due(start + Duration::from_millis(800));
        assert_eq!(table.hinge_fraction(7), Some(1.0));
        table.close_due(start + CLOSE_DELAY);
        assert_eq!(table.entries()[&7].action, Some(2));
        let due = start + Duration::from_millis(800) + Duration::from_secs(10);
        table.close_due(due);
        assert!(!table.closes_due(due));
        table.close_due(due + Duration::from_millis(1));
        assert_eq!(table.entries()[&7].action, Some(3));
        assert_eq!(
            table.use_distance(7, WorldPosition::default(), due + Duration::from_millis(1)),
            None
        );
        table.close_due(due + Duration::from_millis(801));
        assert_eq!(table.hinge_fraction(7), Some(0.0));
        assert!(
            table
                .use_distance(
                    7,
                    WorldPosition::default(),
                    due + Duration::from_millis(801)
                )
                .is_some()
        );
    }

    #[test]
    fn an_unknown_action_or_replacement_cannot_reuse_an_old_usable_hinge() {
        let now = Instant::now();
        let mut table = DoorTable::default();
        table.apply_for(
            &DoorUpdate::Spawn(vec![ordinary()]),
            now,
            GameDialect::EqMac,
        );
        table.apply_for(
            &DoorUpdate::Move { id: 7, action: 251 },
            now,
            GameDialect::EqMac,
        );
        assert_eq!(table.entries()[&7].action, Some(251));
        assert_eq!(table.hinge_fraction(7), None);
        assert!(!table.supports_use(7));
        table.apply_for(
            &DoorUpdate::Spawn(vec![ordinary()]),
            now,
            GameDialect::EqMac,
        );
        assert_eq!(table.hinge_fraction(7), Some(0.0));
        assert!(table.supports_use(7));
        table.apply_for(
            &DoorUpdate::Move { id: 8, action: 2 },
            now,
            GameDialect::EqMac,
        );
        assert!(!table.entries().contains_key(&8));
        assert_eq!(table.hinge_fraction(8), None);
    }

    fn door(id: u8, state_at_spawn: u8, invert_state: u8) -> Door {
        let mut body = [0u8; 80];
        body[..5].copy_from_slice(b"DOOR1");
        body[60] = id;
        body[62] = state_at_spawn;
        body[63] = invert_state;
        let Some(DoorUpdate::Spawn(mut doors)) = decode(0x4c24, &body).unwrap() else {
            panic!("spawn");
        };
        doors.remove(0)
    }

    fn action(table: &DoorTable, id: u8) -> Option<u8> {
        table.entries()[&id].action
    }

    #[test]
    fn an_opened_door_closes_after_the_delay_unless_the_server_closes_it_first() {
        let start = Instant::now();
        let mut table = DoorTable::default();
        table.apply(
            &DoorUpdate::Spawn(vec![door(4, 0, 0), door(5, 0, 0)]),
            start,
        );
        assert!(!table.closes_due(start + CLOSE_DELAY));
        table.apply(
            &DoorUpdate::Move {
                id: 4,
                action: OPEN,
            },
            start,
        );
        table.apply(
            &DoorUpdate::Move {
                id: 5,
                action: OPEN,
            },
            start,
        );
        table.apply(
            &DoorUpdate::Move {
                id: 5,
                action: CLOSE,
            },
            start,
        );
        let early = start + CLOSE_DELAY.saturating_sub(Duration::from_millis(1));
        assert!(!table.closes_due(early));
        table.close_due(early);
        assert_eq!(action(&table, 4), Some(OPEN));
        assert!(table.closes_due(start + CLOSE_DELAY));
        table.close_due(start + CLOSE_DELAY);
        assert_eq!(action(&table, 4), Some(CLOSE));
        assert_eq!(action(&table, 5), Some(CLOSE));
        assert!(!table.closes_due(start + CLOSE_DELAY * 10));
    }

    #[test]
    fn opening_again_restarts_the_delay() {
        let start = Instant::now();
        let mut table = DoorTable::default();
        table.apply(&DoorUpdate::Spawn(vec![door(4, 0, 0)]), start);
        table.apply(
            &DoorUpdate::Move {
                id: 4,
                action: OPEN,
            },
            start,
        );
        let later = start + Duration::from_secs(3);
        table.apply(
            &DoorUpdate::Move {
                id: 4,
                action: OPEN,
            },
            later,
        );
        assert!(!table.closes_due(start + CLOSE_DELAY));
        assert!(table.closes_due(later + CLOSE_DELAY));
    }

    #[test]
    fn doors_that_spawn_open_close_too_and_inverted_doors_close_their_own_way() {
        let start = Instant::now();
        let mut table = DoorTable::default();
        // Spawned open; inverted and closed (spawn state 1); inverted and open.
        let doors = vec![door(1, 1, 0), door(2, 1, 1), door(3, 0, 1)];
        table.apply(&DoorUpdate::Spawn(doors), start);
        table.close_due(start + CLOSE_DELAY);
        assert_eq!(action(&table, 1), Some(CLOSE));
        assert_eq!(action(&table, 2), None);
        // An inverted door's close is the plain open action.
        assert_eq!(action(&table, 3), Some(OPEN));
        assert_eq!(table.entries()[&3].active_endpoint(), Some(true));
        assert_eq!(
            table.entries()[&2].active_endpoint(),
            table.entries()[&3].active_endpoint()
        );
    }

    #[test]
    fn removing_all_doors_cancels_their_closing() {
        let start = Instant::now();
        let mut table = DoorTable::default();
        table.apply(&DoorUpdate::Spawn(vec![door(4, 1, 0)]), start);
        table.apply(&DoorUpdate::RemoveAll, start);
        assert!(!table.closes_due(start + CLOSE_DELAY));
        table.apply(&DoorUpdate::Spawn(vec![door(4, 0, 0)]), start);
        assert!(!table.closes_due(start + CLOSE_DELAY));
    }

    fn lift(id: u8, parameter: u32) -> Door {
        Door {
            open_type: 59,
            parameter,
            ..door(id, 0, 0)
        }
    }

    #[test]
    fn a_titanium_lift_rises_waits_from_arrival_and_comes_back_down_on_its_own() {
        let start = Instant::now();
        let mut table = DoorTable::default();
        table.apply(&DoorUpdate::Spawn(vec![lift(4, 90)]), start);
        assert_eq!(table.lift_offset(4, start), Some(0.0));
        table.apply(
            &DoorUpdate::Move {
                id: 4,
                action: OPEN,
            },
            start,
        );
        // 90 units at 0.45 x 20 u/s is 9 a second (eqgame.exe, read R1).
        let offset = |table: &DoorTable, at: Duration| table.lift_offset(4, start + at).unwrap();
        assert!((offset(&table, Duration::from_secs(5)) - 45.0).abs() < 0.01);
        assert!((offset(&table, Duration::from_secs(10)) - 90.0).abs() < 0.01);
        // Type 59 at 170 or less waits 20 s from its arrival at 10 s.
        assert!(!table.closes_due(start + Duration::from_secs(29)));
        assert!(table.closes_due(start + Duration::from_secs(30)));
        table.close_due(start + Duration::from_secs(30));
        assert_eq!(action(&table, 4), Some(CLOSE));
        assert!((offset(&table, Duration::from_secs(35)) - 45.0).abs() < 0.01);
        assert!(offset(&table, Duration::from_secs(40)).abs() < 0.01);
        // Over 170, it never goes back on its own.
        table.apply(&DoorUpdate::Spawn(vec![lift(4, 300)]), start);
        table.apply(
            &DoorUpdate::Move {
                id: 4,
                action: OPEN,
            },
            start,
        );
        assert!(!table.closes_due(start + Duration::from_secs(3600)));
    }

    #[test]
    fn titanium_names_each_doors_solidity_and_eqmac_keeps_both() {
        let start = Instant::now();
        let mut table = DoorTable::default();
        let mut sinking = lift(6, 10);
        sinking.open_type = 120;
        table.apply(
            &DoorUpdate::Spawn(vec![lift(4, 90), door(5, 0, 0), sinking]),
            start,
        );
        // A type 59 lift is both; a hinged door and a type 120 platform are
        // walls nobody stands on; types 60-77 are neither (eqgame.exe
        // 0x490370 and the wall contact).
        assert_eq!(table.solidity(4), Solidity::BOTH);
        assert!(!table.solidity(5).floor && table.solidity(5).wall);
        assert!(!table.solidity(6).floor && table.solidity(6).wall);
        let mut passage = lift(8, 0);
        passage.open_type = 60;
        table.apply(&DoorUpdate::Spawn(vec![passage]), start);
        assert!(!table.solidity(8).floor && !table.solidity(8).wall);
        // Type 27 is a floor only in zone 154 (eqgame.exe 0x490370).
        let mut floor = door(7, 0, 0);
        floor.open_type = 27;
        table.apply(&DoorUpdate::Spawn(vec![floor.clone()]), start);
        assert!(!table.solidity(7).floor);
        table.set_zone(154);
        assert!(table.solidity(7).floor);
        // EQMac's rule is not read: every door stays both (checklist 6.425).
        table.apply_for(
            &DoorUpdate::Spawn(vec![lift(4, 90)]),
            start,
            GameDialect::EqMac,
        );
        assert_eq!(table.solidity(4), Solidity::BOTH);
        assert_eq!(table.lift_offset(4, start), None);
    }
}
