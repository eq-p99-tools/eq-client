//! Loot cells retain which button chose the destination until submission.
use super::Action;
use bevy::{ecs::system::SystemParam, prelude::*};
use eq_client_core::{inventory::InventorySlot, world::ClientWorld};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Target {
    entity: Entity,
    session: u64,
    corpse: u16,
    place: u16,
    item: u32,
}

/// Right release belongs to this original control, admission, corpse and item.
#[derive(Default)]
pub(super) struct Press(Option<Target>);

impl Press {
    pub(super) fn cancel_target(&mut self, entity: Entity) {
        if self.0.is_some_and(|target| target.entity == entity) {
            self.0 = None;
        }
    }
}

pub(super) fn cursor_empty(world: &ClientWorld) -> bool {
    !world
        .inventory()
        .items()
        .contains_key(&InventorySlot::CURSOR)
}

/// Reads the same hit-tested controls for ordinary and skinned loot windows.
#[derive(SystemParam)]
pub(crate) struct Buttons<'w, 's> {
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    controls: Query<
        'w,
        's,
        (
            Entity,
            &'static Action,
            &'static Interaction,
            Has<crate::tooltip::ScriptPressed>,
        ),
    >,
}

impl Buttons<'_, '_> {
    pub(super) fn actions(&self, world: &ClientWorld, pressed: &mut Press) -> Vec<Action> {
        let Some(mouse) = self.mouse.as_deref() else {
            pressed.0 = None;
            return Vec::new();
        };
        if mouse.pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left) {
            pressed.0 = None;
            return if mouse.just_pressed(MouseButton::Left) {
                self.controls
                    .iter()
                    .filter_map(|(_, action, interaction, _)| match action {
                        Action::Take { place, .. } if *interaction == Interaction::Pressed => {
                            Some(Action::Take {
                                place: *place,
                                auto: false,
                            })
                        }
                        _ => None,
                    })
                    .collect()
            } else {
                Vec::new()
            };
        }
        let press = mouse.just_pressed(MouseButton::Right);
        let under_pointer = || {
            let session = world.session_id()?;
            let loot = world.loot()?;
            let (entity, action, ..) =
                self.controls
                    .iter()
                    .find(|(_, action, interaction, scripted)| {
                        matches!(action, Action::Take { .. })
                            && (**interaction == Interaction::Hovered
                                || (press && *scripted && **interaction == Interaction::Pressed))
                    })?;
            let Action::Take { place, .. } = action else {
                return None;
            };
            Some(Target {
                entity,
                session,
                corpse: loot.corpse_id,
                place: *place,
                item: loot.items.get(place)?.details.id,
            })
        };
        if press {
            pressed.0 = under_pointer();
        }
        if mouse.just_released(MouseButton::Right) {
            return pressed
                .0
                .take()
                .filter(|start| under_pointer().as_ref() == Some(start))
                .map(|start| {
                    vec![Action::Take {
                        place: start.place,
                        auto: true,
                    }]
                })
                .unwrap_or_default();
        }
        if !mouse.pressed(MouseButton::Right) {
            pressed.0 = None;
        }
        Vec::new()
    }
}
