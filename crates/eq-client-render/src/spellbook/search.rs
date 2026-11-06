//! Compact read-only browse results, in increasing server book-slot order.
use super::{BookView, SpellNames, known_entries, sets};
use crate::{
    keys::{Typing, search, window_owns_input},
    theme::{self, Size},
    tooltip::Tooltip,
    windows::{self, Frame, Shown, WindowId},
};
use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    text::LineBreak,
    window::PrimaryWindow,
};
use eq_client_core::SpellBook;
use std::collections::HashSet;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Search {
    query: String,
    applied: String,
    book: Option<SpellBook>,
    pub(super) entries: Vec<(usize, u32)>,
    pub(super) page: usize,
    /// The previous frame's controls still describe an earlier result layout.
    pub(super) changed: bool,
}

impl Search {
    pub(super) fn active(&self) -> bool {
        !self.applied.is_empty()
    }

    fn refresh(&mut self, book: Option<&SpellBook>, names: &SpellNames, labels_changed: bool) {
        let query = self.query.trim().to_lowercase();
        self.changed = query != self.applied;
        if self.changed {
            self.applied = query;
            self.page = 0;
        }
        if !self.active() {
            self.book = None;
            self.entries.clear();
            return;
        }
        if self.changed || labels_changed || self.book.as_ref() != book {
            let mut entries = known_entries(book);
            entries.retain(|(_, spell)| names.label(*spell).to_lowercase().contains(&self.applied));
            self.changed |= entries != self.entries;
            self.entries = entries;
            self.book = book.cloned();
        }
    }
}

#[derive(Component)]
pub(crate) struct Field;
#[derive(Component)]
pub(crate) struct Clear;

/// A slim row below the skin's original geometry, or inside the fallback body.
pub(crate) const FOOTER_HEIGHT: f32 = 26.0;

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands, node: Node) {
    parent
        .spawn((
            windows::KeepsPress,
            Node {
                column_gap: px(4),
                align_items: AlignItems::Stretch,
                padding: UiRect::all(px(2)),
                ..node
            },
            BackgroundColor(theme::COVER),
        ))
        .with_children(|row| {
            row.spawn((
                Button,
                Field,
                Node {
                    min_width: px(20),
                    flex_grow: 1.0,
                    padding: UiRect::axes(px(4), px(2)),
                    border: UiRect::all(px(1)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(theme::INSET),
                BorderColor::all(theme::EDGE),
                theme::text("Search spells…", Size::Small, theme::INK_BRIGHT),
                TextLayout::new(Justify::Left, LineBreak::NoWrap),
                Tooltip("Find learned spells by name. Enter or Escape finishes. Delete or Clear restores the book.".into()),
            ));
            theme::button_with(row, Clear, "Clear", Size::Small);
            sets::ui::toggle(row);
        });
}

/// Share the normal typing owner before chat or gameplay sees the input.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn focus(
    mouse: Res<ButtonInput<MouseButton>>,
    shown: Res<Shown>,
    windows: Query<&Window, With<PrimaryWindow>>,
    script: Option<Res<crate::script::Script>>,
    frames: Query<(&WindowId, &Frame)>,
    fields: Query<&Interaction, With<Field>>,
    clears: Query<&Interaction, With<Clear>>,
    (sets, names): (
        Option<Res<sets::Sets>>,
        Query<&Interaction, With<sets::ui::NameField>>,
    ),
    mut typing: ResMut<Typing>,
    mut view: ResMut<BookView>,
) {
    let was_focused = typing.spell_search || typing.spell_set_name;
    let available = shown.displayed(WindowId::Spellbook)
        && window_owns_input(&windows, script.as_deref())
        && frames
            .iter()
            .any(|(id, frame)| *id == WindowId::Spellbook && !frame.minimized())
        && !fields.is_empty()
        && !typing.logging_in
        && !typing.creating_character
        && !typing.rebinding
        && !typing.editing_script;
    if !available {
        typing.spell_search = false;
        typing.spell_set_name = false;
    } else if mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]) {
        typing.spell_set_name = mouse.just_pressed(MouseButton::Left)
            && sets.as_ref().is_some_and(|sets| sets.can_edit_name())
            && names
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed);
        typing.spell_search = !typing.spell_set_name
            && fields
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed);
        if typing.spell_search || typing.spell_set_name {
            typing.composing = false;
            typing.counting = false;
            typing.map_search = false;
        }
        if mouse.just_pressed(MouseButton::Left)
            && clears
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed)
        {
            view.search.query.clear();
        }
    }
    if sets.as_ref().is_none_or(|sets| !sets.can_edit_name()) {
        typing.spell_set_name = false;
    }
    typing.spell_search_claimed = was_focused || typing.spell_search || typing.spell_set_name;
}

/// Suppress both this frame's bindings and native repeat from keys pressed in
/// the editor, even if the field loses focus before those keys are released.
pub(crate) fn capture(
    mut keyboard: MessageReader<KeyboardInput>,
    mut typing: ResMut<Typing>,
    mut physical: ResMut<ButtonInput<KeyCode>>,
) {
    let Typing {
        spell_search_claimed: claimed,
        spell_search_held: held,
        spell_search_events: captured,
        ..
    } = &mut *typing;
    captured.clear();
    let mut reset = HashSet::new();
    if *claimed {
        held.extend(physical.get_pressed().copied());
    }
    for (event, id) in keyboard.read_with_id() {
        let key = event.key_code;
        let pressed = event.state.is_pressed();
        if *claimed || (held.contains(&key) && (!pressed || event.repeat)) {
            captured.insert(id);
            physical.reset(key);
            reset.insert(key);
        } else if reset.contains(&key) {
            // Bevy has already aggregated this frame's physical input.
            // Restore a fresh tap after a captured repeat or release.
            if pressed {
                physical.press(key);
            } else {
                physical.release(key);
            }
        }
        if *claimed && pressed {
            held.insert(key);
        } else if !pressed || !event.repeat {
            held.remove(&key);
        }
    }
    for key in &*held {
        physical.reset(*key);
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn input(
    mut keyboard: MessageReader<KeyboardInput>,
    mut typing: ResMut<Typing>,
    (mut view, mut sets): (ResMut<BookView>, Option<ResMut<sets::Sets>>),
    keys: Res<ButtonInput<Key>>,
    mut clipboard: Option<ResMut<Clipboard>>,
    mut pending: Local<Option<ClipboardRead>>,
    mut name_editor: Local<bool>,
) {
    if !typing.spell_search && !typing.spell_set_name {
        keyboard.clear();
        *pending = None;
        return;
    }
    if *name_editor != typing.spell_set_name {
        *pending = None;
        *name_editor = typing.spell_set_name;
    }
    let text = if typing.spell_set_name {
        let Some(sets) = sets.as_mut() else {
            return;
        };
        sets.name_input()
    } else {
        &mut view.search.query
    };
    let events: Vec<_> = keyboard.read().cloned().collect();
    if let Some(finish) = search::edit(text, &events, &keys, clipboard.as_deref_mut(), &mut pending)
    {
        typing.spell_search = false;
        typing.spell_set_name = false;
        typing.escape_consumed |= finish == search::Finish::Escape;
        *pending = None;
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn refresh(
    online: Res<crate::online::OnlineState>,
    names: Res<SpellNames>,
    mut view: ResMut<BookView>,
) {
    view.search
        .refresh(online.world().spell_book(), &names, names.is_changed());
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn present(
    view: Res<BookView>,
    typing: Res<Typing>,
    mut fields: Query<(&mut Text, &mut BorderColor), With<Field>>,
) {
    let query = &view.search.query;
    let focused = typing.spell_search;
    let text = if focused {
        format!("{query}|")
    } else if query.is_empty() {
        "Search spells…".into()
    } else if view.search.entries.is_empty() && view.search.active() {
        format!("{query} · No matches")
    } else {
        query.clone()
    };
    let border = BorderColor::all(if focused { theme::FOCUS } else { theme::EDGE });
    for (mut field, mut edge) in &mut fields {
        if field.0 != text {
            field.0.clone_from(&text);
        }
        edge.set_if_neq(border);
    }
}
