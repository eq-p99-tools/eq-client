//! The skills picker, as the Titanium program has it (read R17): a left
//! click on an empty Actions window ability button, or a right click on any,
//! opens it for that button; it lists what the button can hold, and a click
//! on a row puts that on the button, saves the choice and closes it. The
//! skin's `SkillsSelectWindow` draws it where the skin has one; elsewhere
//! the client's own panel shows the same list. Closing it without a choice
//! only hides it and forgets the button, leaving the button as it was, as
//! the program's does (read R17b, 0x4d39b0 and 0x4d3c33). It also closes
//! at a zone change, a new session, and when the Actions window closes.
use super::{
    AbilityButton,
    choices::{Choices, Offer, Slot, offered},
};
use crate::{
    online::OnlineState,
    theme::{self, Size},
    windows::{self, Shown, WindowId},
};
use bevy::prelude::*;

/// The button the picker chooses for.
#[derive(Resource, Default, Debug)]
pub(crate) struct Picker {
    /// The button it chooses for, while it is open.
    button: Option<AbilityButton>,
    /// A button that asked for it this frame.
    asked: Option<AbilityButton>,
    /// The session it opened in.
    session: Option<u64>,
}

impl Picker {
    /// Opens the picker for a button, from the next run of [`window`].
    pub(crate) fn ask(&mut self, button: AbilityButton) {
        self.asked = Some(button);
    }

    /// The button the picker chooses for, while it is open.
    pub(crate) const fn button(&self) -> Option<AbilityButton> {
        self.button
    }
}

/// A row of the picker's list: its place from the top and what it puts on
/// the button.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PickRow {
    pub(crate) index: usize,
    pub(crate) slot: Slot,
}

/// The picker's list, which the client fills.
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct PickRows {
    /// Each column's width, from the left.
    columns: Vec<f32>,
    /// The rows last drawn, to redraw only when one changes.
    drawn: Option<Vec<Offer>>,
}

impl PickRows {
    pub(crate) fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
        }
    }
}

/// A right click on any ability button opens the picker for it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn right_click(
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &AbilityButton)>,
    mut picker: ResMut<Picker>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    if let Some((_, button)) = buttons
        .iter()
        .find(|(interaction, _)| **interaction != Interaction::None)
    {
        picker.ask(*button);
    }
}

/// Opens the picker when a button asks, in front of the other windows, and
/// keeps it with what it chooses for: closed by its close box or Escape, a
/// zone change or a new session, or the Actions window closing.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(
    online: Res<OnlineState>,
    mut picker: ResMut<Picker>,
    (mut shown, mut stack): (ResMut<Shown>, ResMut<windows::Stack>),
) {
    let session = online.world().session_id();
    if let Some(button) = picker.asked.take() {
        picker.button = Some(button);
        picker.session = session;
        shown.open(WindowId::SkillSelect);
        stack.raise(WindowId::SkillSelect);
        return;
    }
    let open = shown.is_open(WindowId::SkillSelect);
    let gone = picker.button.is_none()
        || !open
        || picker.session != session
        || !shown.displayed(WindowId::ActionsWindow);
    if gone {
        if picker.button.is_some() {
            picker.button = None;
        }
        if open {
            shown.close(WindowId::SkillSelect);
        }
    }
}

/// A click on a row puts it on the button, which saves it, and closes the
/// picker.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn choose(
    online: Res<OnlineState>,
    (mut choices, mut picker): (ResMut<Choices>, ResMut<Picker>),
    mut shown: ResMut<Shown>,
    rows: Query<(&Interaction, &PickRow), Changed<Interaction>>,
) {
    let Some(button) = picker.button else {
        return;
    };
    if let Some((_, row)) = rows
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
    {
        choices.choose(online.world(), button, row.slot);
        picker.button = None;
        shown.close(WindowId::SkillSelect);
    }
}

/// The height of a row of the list.
const ROW_HEIGHT: f32 = 16.0;

/// Fills the picker's list again whenever a row changes, as the player's
/// skills do while it is open.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    online: Res<OnlineState>,
    picker: Res<Picker>,
    mut lists: Query<(Entity, &mut PickRows, Option<&Children>)>,
) {
    if lists.is_empty() {
        return;
    }
    let wanted = picker
        .button
        .map(|button| offered(online.world(), button.page))
        .unwrap_or_default();
    for (entity, mut list, children) in &mut lists {
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        if let Some(children) = children {
            for child in children {
                commands.entity(*child).despawn();
            }
        }
        let name_width = list.columns.first().copied().unwrap_or(140.0);
        let value_width = list.columns.get(1).copied().unwrap_or(40.0);
        for (index, offer) in wanted.iter().enumerate() {
            let line = commands
                .spawn((
                    Button,
                    PickRow {
                        index,
                        slot: offer.slot,
                    },
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    ChildOf(entity),
                ))
                .id();
            for (words, width) in [
                (offer.name.to_owned(), name_width),
                (
                    offer
                        .value
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    value_width,
                ),
            ] {
                commands.spawn((
                    theme::text(words, Size::Small, theme::INK),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    Node {
                        width: px(width),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ChildOf(line),
                ));
            }
        }
        list.drawn = Some(wanted.clone());
    }
}

/// The client's own picker's width.
const OWN_WIDTH: f32 = 200.0;

/// Gives the open picker its frame, with the client's own list in it; where
/// the skin has a skills picker, the skin draws over that panel
/// ([`crate::skinned::apply`]).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn frame(
    mut commands: Commands,
    (shown, mut stack): (Res<Shown>, ResMut<windows::Stack>),
    frames: Query<(Entity, &WindowId), With<windows::Frame>>,
) {
    let displayed = shown.displayed(WindowId::SkillSelect);
    let mut exists = false;
    for (entity, id) in &frames {
        if *id != WindowId::SkillSelect {
            continue;
        }
        if displayed {
            exists = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if !displayed || exists {
        return;
    }
    // Each opening comes to the front, so Escape closes it first.
    stack.raise(WindowId::SkillSelect);
    let frame = windows::frame(
        &mut commands,
        WindowId::SkillSelect,
        Node {
            width: px(OWN_WIDTH),
            padding: UiRect::all(px(6)),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    commands.entity(frame).with_children(|panel| {
        panel.spawn((
            PickRows::new(vec![OWN_WIDTH - 52.0, 40.0]),
            windows::pointer::TakesWheel,
            Node {
                flex_direction: FlexDirection::Column,
                min_height: px(ROW_HEIGHT * 4.0),
                max_height: px(320),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ));
    });
}

#[cfg(test)]
mod tests;
