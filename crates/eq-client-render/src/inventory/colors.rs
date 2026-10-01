//! Per-storage-slot bag tints; presentation choices never send inventory commands.
use super::{InventoryState, label};
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_core::inventory::InventorySlot;
use std::collections::BTreeMap;

const PALETTE: [[f32; 3]; 8] = [
    [0.065, 0.080, 0.095],
    [0.105, 0.120, 0.135],
    [0.075, 0.13, 0.20],
    [0.075, 0.16, 0.12],
    [0.18, 0.13, 0.065],
    [0.17, 0.08, 0.09],
    [0.135, 0.09, 0.18],
    [0.065, 0.16, 0.17],
];

#[derive(Default)]
pub(super) struct Colors {
    slots: BTreeMap<InventorySlot, usize>,
    picker: Option<InventorySlot>,
}

impl Colors {
    pub fn tint(&self, slot: InventorySlot) -> Color {
        let default = usize::from(slot.0.rem_euclid(2) != 0);
        let index = self.slots.get(&slot).copied().unwrap_or(default);
        let [r, g, b] = PALETTE[index];
        Color::srgb(r, g, b)
    }
}

#[derive(Component)]
pub(crate) enum Action {
    Toggle(InventorySlot),
    Set(InventorySlot, usize),
}

/// Adds a swatch button and its compact palette beside the bag icon.
pub(super) fn controls(parent: &mut ChildSpawnerCommands, colors: &Colors, slot: InventorySlot) {
    parent
        .spawn((
            Button,
            Action::Toggle(slot),
            Node {
                width: px(22),
                height: px(22),
                border: UiRect::all(px(1)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(colors.tint(slot)),
            BorderColor::all(Color::srgb(0.5, 0.55, 0.6)),
        ))
        .with_children(|button| {
            label(button, "+", 12.0);
            if colors.picker != Some(slot) {
                return;
            }
            button
                .spawn((
                    GlobalZIndex(50),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(26),
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::px(4, 24.0),
                        padding: UiRect::all(px(5)),
                        column_gap: px(4),
                        row_gap: px(4),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.025, 0.032, 0.04)),
                ))
                .with_children(|palette| {
                    for (index, [r, g, b]) in PALETTE.iter().copied().enumerate() {
                        palette.spawn((
                            Button,
                            Action::Set(slot, index),
                            Node {
                                width: px(24),
                                height: px(24),
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(r, g, b)),
                            BorderColor::all(Color::srgb(0.5, 0.55, 0.6)),
                        ));
                    }
                });
        });
}

/// Changes only the selected bag group's visual tint and invalidates its layout.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn input(
    mut state: ResMut<InventoryState>,
    shown: Res<crate::windows::Shown>,
    actions: Query<(Ref<Interaction>, &Action)>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if !shown.is_open(crate::windows::WindowId::Inventory)
        || !windows.single().is_ok_and(|window| window.focused)
    {
        return;
    }
    // Resolve a swatch before its parent toggle if both report a press.
    let pressed: Vec<_> = actions
        .iter()
        .filter(|(interaction, _)| {
            interaction.is_changed() && **interaction == Interaction::Pressed
        })
        .map(|(_, action)| action)
        .collect();
    let action = pressed
        .iter()
        .find(|action| matches!(action, Action::Set(..)))
        .copied()
        .or_else(|| pressed.first().copied());
    if let Some(action) = action {
        match *action {
            Action::Toggle(slot) => {
                state.colors.picker = (state.colors.picker != Some(slot)).then_some(slot);
            }
            Action::Set(slot, index) if index < PALETTE.len() => {
                state.colors.slots.insert(slot, index);
                state.colors.picker = None;
            }
            Action::Set(..) => return,
        }
        state.revision = state.revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_press_changes_only_its_group_and_closes_popup() {
        let mut app = App::new();
        app.init_resource::<InventoryState>()
            .init_resource::<crate::windows::Shown>()
            .add_systems(Update, input);
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .open(crate::windows::WindowId::Inventory);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        let slot = InventorySlot(22);
        app.world_mut()
            .spawn((Interaction::Pressed, Action::Toggle(slot)));
        app.update();
        let original_other = app
            .world()
            .resource::<InventoryState>()
            .colors
            .tint(InventorySlot(23));
        assert_eq!(
            app.world().resource::<InventoryState>().colors.picker,
            Some(slot)
        );
        app.world_mut()
            .spawn((Interaction::Pressed, Action::Set(slot, 3)));
        app.update();
        let state = app.world().resource::<InventoryState>();
        assert_eq!(state.colors.picker, None);
        assert_eq!(state.colors.tint(slot), Color::srgb(0.075, 0.16, 0.12));
        assert_eq!(state.colors.tint(InventorySlot(23)), original_other);
    }
}
