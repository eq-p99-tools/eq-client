//! The Combat Abilities window: the disciplines the player knows, each named
//! with the time left on its timer, dimmed while it waits. A click uses one
//! through the same path as `/discipline` and a hotkey (0x4f8aab to
//! 0x4bb39d, read R48 c). A hold or Ctrl and a number puts one on the
//! action bar, as the Actions window's buttons do.
//!
//! This client draws the window itself. The skin's own window, the order it
//! lists disciplines in and what its saved `[CombatAbilities]` entries mean
//! are not read, so it lists the known disciplines in the known list's order
//! (recording checklist 6.488).
use crate::{
    chat::ChatState,
    hud::hotbar,
    online::OnlineState,
    spellbook::SpellNames,
    theme::{self, Size},
    windows::{self, Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{hotbar::Binding, world::ClientWorld};
use std::time::Instant;

/// The window's size, in logical pixels.
const SIZE: Vec2 = Vec2::new(220.0, 260.0);
/// One row's height.
const ROW_HEIGHT: f32 = 20.0;

/// The window's frame, made while it is open.
#[derive(Component)]
pub(crate) struct AbilitiesFrame;

/// The list of rows, and the disciplines it was last drawn with.
#[derive(Component, Default)]
pub(crate) struct Rows {
    drawn: Option<Vec<u32>>,
}

/// A row: the discipline it uses, by its spell.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry(pub(crate) u32);

/// A row's words.
#[derive(Component, Clone, Copy)]
pub(crate) struct EntryLabel(u32);

/// The known disciplines, in the known list's order.
pub(crate) fn listed(world: &ClientWorld) -> Vec<u32> {
    world
        .player()
        .and_then(|player| player.disciplines.as_deref())
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|spell| *spell != 0)
        .collect()
}

/// What a row says: the discipline's name, and the time left while its
/// timer runs.
pub(crate) fn caption(world: &ClientWorld, names: &SpellNames, spell: u32, now: Instant) -> String {
    let name = crate::disciplines::name(names, spell);
    match crate::disciplines::time_left_of(world, names, spell, now) {
        Some(left) => format!("{name} ({:.0}s)", left.as_secs_f32().ceil()),
        None => name,
    }
}

/// Makes the window's frame while it is open and the player is in the
/// world, and takes it away when it closes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut shown: ResMut<Shown>,
    frames: Query<Entity, With<AbilitiesFrame>>,
) {
    if !online.world().connected() || online.world().player().is_none() {
        shown.close(WindowId::CombatAbilities);
    }
    if !shown.displayed(WindowId::CombatAbilities) {
        for frame in &frames {
            commands.entity(frame).despawn();
        }
        return;
    }
    if !frames.is_empty() {
        return;
    }
    let frame = windows::frame(
        &mut commands,
        WindowId::CombatAbilities,
        Node {
            width: px(SIZE.x),
            height: px(SIZE.y),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            ..default()
        },
    );
    commands
        .entity(frame)
        .insert(AbilitiesFrame)
        .with_children(|window| {
            window.spawn((
                Rows::default(),
                windows::KeepsPress,
                windows::pointer::TakesWheel,
                ScrollPosition::default(),
                Node {
                    overflow: Overflow::scroll_y(),
                    flex_direction: FlexDirection::Column,
                    ..crate::skinned::at(6.0, 26.0, SIZE.x - 12.0, SIZE.y - 32.0)
                },
                BackgroundColor(theme::INSET),
            ));
        });
}

/// Draws the rows again when the known disciplines change.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut lists: Query<(Entity, &mut Rows, Option<&Children>)>,
) {
    let wanted = listed(online.world());
    for (entity, mut rows, children) in &mut lists {
        if rows.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        for child in children.into_iter().flatten() {
            commands.entity(*child).despawn();
        }
        for spell in &wanted {
            let binding = Binding::Discipline(*spell);
            commands
                .spawn((
                    Button,
                    Entry(*spell),
                    hotbar::Pickable(hotbar::Source::Fixed(binding)),
                    hotbar::need(Some(binding)),
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        padding: UiRect::horizontal(px(4)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    ChildOf(entity),
                ))
                .with_child((
                    EntryLabel(*spell),
                    theme::text("", Size::Small, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                ));
        }
        rows.drawn = Some(wanted.clone());
    }
}

/// Names each row with its time left, dimmed while it waits.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn present(
    online: Res<OnlineState>,
    names: Res<SpellNames>,
    mut labels: Query<(&EntryLabel, &mut Text, &mut TextColor)>,
) {
    let now = Instant::now();
    let world = online.world();
    for (EntryLabel(spell), mut text, mut color) in &mut labels {
        let wanted = caption(world, &names, *spell, now);
        if text.0 != wanted {
            text.0 = wanted;
        }
        let ink = if crate::disciplines::waits(world, &names, *spell, now) {
            theme::INK_DIM
        } else {
            theme::INK_BRIGHT
        };
        if color.0 != ink {
            color.0 = ink;
        }
    }
}

/// A clicked row uses its discipline.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn input(mut chat: ResMut<ChatState>, rows: crate::clicks::Clicks<Entry>) {
    for Entry(spell) in rows.iter() {
        chat.disciplines
            .push(crate::disciplines::Asked::Use(*spell));
    }
}

#[cfg(test)]
mod tests;
