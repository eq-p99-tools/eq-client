//! Doors as the client shows them: the server's definitions and actions, plus
//! the closing that clients do on their own.
pub use eq_network_game::doors::{Door, DoorUpdate, Doors, decode};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How long an opened door stays open before the client closes it. Servers
/// close ordinary doors on their own timers without telling clients (`EQEmu`
/// sends a close only for a few special kinds), so clients close them locally.
/// Provisional: `EQEmu`'s default server timer, not measured on the official
/// client.
pub const CLOSE_DELAY: Duration = Duration::from_secs(5);

/// Server door actions; inverted doors swap them.
const OPEN: u8 = 2;
const CLOSE: u8 = 3;

/// The server's doors, with opened doors closing after [`CLOSE_DELAY`].
#[derive(Clone, Debug, Default)]
pub struct DoorTable {
    doors: Doors,
    closing: BTreeMap<u8, Instant>,
}

impl DoorTable {
    /// Current definitions in stable door-ID order.
    #[must_use]
    pub fn entries(&self) -> &BTreeMap<u8, Door> {
        self.doors.entries()
    }

    /// Applies a server update; a door the server leaves open starts closing.
    pub fn apply(&mut self, update: &DoorUpdate, now: Instant) {
        self.doors.apply(update);
        match update {
            DoorUpdate::RemoveAll => self.closing.clear(),
            DoorUpdate::Spawn(doors) => {
                for door in doors {
                    self.schedule(door.id, now);
                }
            }
            DoorUpdate::Move { id, .. } => self.schedule(*id, now),
        }
    }

    /// Whether any door is due to close by `now`.
    #[must_use]
    pub fn closes_due(&self, now: Instant) -> bool {
        self.closing.values().any(|at| *at <= now)
    }

    /// Closes the doors whose time is up, as the server's close action would.
    pub fn close_due(&mut self, now: Instant) {
        let due: Vec<u8> = self
            .closing
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.closing.remove(&id);
            if let Some(door) = self.doors.entries().get(&id) {
                let action = if door.invert_state == 0 { CLOSE } else { OPEN };
                self.doors.apply(&DoorUpdate::Move { id, action });
            }
        }
    }

    fn schedule(&mut self, id: u8, now: Instant) {
        if self.doors.entries().get(&id).is_some_and(is_open) {
            self.closing.insert(id, now + CLOSE_DELAY);
        } else {
            self.closing.remove(&id);
        }
    }
}

/// Whether the server has a door open: its latest action opened it, or it
/// spawned open. The server applies a door's inversion to both.
fn is_open(door: &Door) -> bool {
    let inverted = door.invert_state != 0;
    match door.action {
        Some(action) => action == if inverted { CLOSE } else { OPEN },
        None => (door.state_at_spawn != 0) != inverted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
