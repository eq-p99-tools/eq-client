//! Door activation is a pair of completed left clicks on the same visible door.
use bevy::prelude::*;
use std::time::{Duration, Instant};

const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const CLICK_SLOP: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Door {
    pub(super) entity: Entity,
    pub(super) id: u8,
}

struct Click {
    door: Door,
    cursor: Vec2,
    at: Instant,
}

#[derive(Default)]
pub(crate) struct DoorClicks {
    admission: Option<u64>,
    pressed: Option<Click>,
    previous: Option<Click>,
}

impl DoorClicks {
    /// Clears any sequence when another input owner takes over. A drag is
    /// cancelled as soon as it leaves the click area, even if it comes back.
    pub(super) fn track(
        &mut self,
        admission: Option<u64>,
        cursor: Option<Vec2>,
        mouse: &ButtonInput<MouseButton>,
        now: Instant,
    ) -> bool {
        if self.admission != admission {
            *self = Self {
                admission,
                ..default()
            };
        }
        let Some(cursor) = cursor.filter(|_| admission.is_some()) else {
            self.cancel();
            return false;
        };
        if self.pressed.as_ref().is_some_and(|click| {
            !near(click.cursor, cursor)
                || now.duration_since(click.at) > DOUBLE_CLICK
                || !mouse.pressed(MouseButton::Left) && !mouse.just_released(MouseButton::Left)
        }) {
            self.cancel();
        }
        if self
            .previous
            .as_ref()
            .is_some_and(|click| now.duration_since(click.at) > DOUBLE_CLICK)
        {
            self.previous = None;
        }
        self.pressed.is_some() && mouse.just_released(MouseButton::Left)
    }

    pub(super) fn cancel(&mut self) {
        self.pressed = None;
        self.previous = None;
    }

    pub(super) fn press(&mut self, door: Option<Door>, cursor: Vec2, at: Instant) {
        self.pressed = door.map(|door| Click { door, cursor, at });
        if door.is_none() {
            self.previous = None;
        }
    }

    pub(super) fn release(&mut self, door: Option<Door>, cursor: Vec2, at: Instant) -> Option<u8> {
        let previous = self.previous.take();
        let pressed = self.pressed.take()?;
        if door != Some(pressed.door) {
            return None;
        }
        if previous.is_some_and(|click| click.door == pressed.door && near(click.cursor, cursor)) {
            return Some(pressed.door.id);
        }
        self.previous = Some(Click {
            door: pressed.door,
            cursor,
            at,
        });
        None
    }
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.distance_squared(b) <= CLICK_SLOP * CLICK_SLOP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_completed_click_expires_and_cannot_pair_across_admissions_or_door_replacements() {
        let mut clicks = DoorClicks::default();
        let mut mouse = ButtonInput::default();
        let mut now = Instant::now();
        let door = Door {
            entity: Entity::PLACEHOLDER,
            id: 1,
        };
        let click =
            |clicks: &mut DoorClicks, mouse: &mut ButtonInput<MouseButton>, session, door, now| {
                mouse.clear();
                mouse.press(MouseButton::Left);
                assert!(!clicks.track(Some(session), Some(Vec2::ZERO), mouse, now));
                clicks.press(Some(door), Vec2::ZERO, now);
                mouse.clear();
                mouse.release(MouseButton::Left);
                assert!(clicks.track(Some(session), Some(Vec2::ZERO), mouse, now));
                clicks.release(Some(door), Vec2::ZERO, now)
            };
        assert_eq!(click(&mut clicks, &mut mouse, 1, door, now), None);
        now += DOUBLE_CLICK + Duration::from_millis(1);
        assert_eq!(click(&mut clicks, &mut mouse, 1, door, now), None);
        assert_eq!(click(&mut clicks, &mut mouse, 2, door, now), None);
        let replacement = Door {
            entity: World::new().spawn_empty().id(),
            ..door
        };
        assert_eq!(click(&mut clicks, &mut mouse, 2, replacement, now), None);
        assert_eq!(
            click(&mut clicks, &mut mouse, 2, replacement, now),
            Some(door.id)
        );
    }
}
