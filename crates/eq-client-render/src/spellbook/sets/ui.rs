//! Shared footer controls for the skinned and fallback spellbook.
use super::{Gems, MAX_SETS, Sets, apply::Apply};
use crate::{
    chat::{ChatState, system_line},
    hud::action_bar::ActionRequests,
    online::OnlineState,
    outbox::Outbox,
    skinned::SkinId,
    theme::{self, Size},
    tooltip::Tooltip,
    windows::{Frame, KeepsPress, Shown, WindowId},
};
use bevy::{prelude::*, text::LineBreak};
use std::{fmt::Write as _, time::Instant};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Panel {
    expanded: bool,
    selected: Option<String>,
    draft: String,
    confirm_delete: bool,
}

#[derive(Component)]
pub(crate) struct NameField;

impl Sets {
    pub(crate) fn can_edit_name(&self) -> bool {
        self.panel.expanded && self.active.is_none() && self.writable()
    }

    pub(crate) fn name_input(&mut self) -> &mut String {
        &mut self.panel.draft
    }

    pub(crate) fn interrupt(&mut self, reason: &str) {
        if let Some(active) = self.active.as_mut() {
            active.stop(reason);
        }
    }

    fn select(&mut self, name: Option<String>) {
        self.panel.draft = name.clone().unwrap_or_default();
        self.panel.selected = name;
    }

    fn browse(&mut self, forward: bool) {
        let selected = self.panel.selected.as_ref();
        let name = if forward {
            self.saved
                .keys()
                .find(|key| selected.is_none_or(|name| *key > name))
        } else {
            self.saved
                .keys()
                .rev()
                .find(|key| selected.is_none_or(|name| *key < name))
        };
        let wrapped = if forward {
            self.saved.first_key_value()
        } else {
            self.saved.last_key_value()
        };
        self.select(name.or_else(|| wrapped.map(|(name, _)| name)).cloned());
        self.status.clear();
    }

    fn save_current(&mut self, gems: Gems) -> Result<String, &'static str> {
        if self.saved.len() >= MAX_SETS {
            return Err("Delete a set to make room for another");
        }
        let name = self.checked_name(&self.panel.draft, None)?;
        self.saved.insert(name.clone(), gems);
        self.select(Some(name));
        self.save();
        Ok("Saved the current gems".into())
    }

    fn rename(&mut self) -> Result<String, &'static str> {
        let selected = self.panel.selected.as_ref().ok_or("Choose a spell set")?;
        let name = self.checked_name(&self.panel.draft, Some(selected))?;
        let gems = self.saved.remove(selected).ok_or("Choose a spell set")?;
        self.saved.insert(name.clone(), gems);
        self.select(Some(name));
        self.save();
        Ok("Renamed the set".into())
    }

    fn delete(&mut self, confirm: bool) -> String {
        let Some(name) = self.panel.selected.as_ref() else {
            return "Choose a spell set".into();
        };
        if !confirm {
            self.panel.confirm_delete = true;
            return "Click Confirm delete to remove this saved layout.".into();
        }
        self.saved.remove(name);
        self.select(self.saved.keys().next().cloned());
        self.save();
        "Deleted the saved set".into()
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Toggle,
    Previous,
    Next,
    Save,
    Rename,
    Delete,
    Apply,
    Cancel,
}

impl Action {
    const fn id(self) -> &'static str {
        match self {
            Self::Toggle => "SpellSets_Toggle",
            Self::Previous => "SpellSets_Previous",
            Self::Next => "SpellSets_Next",
            Self::Save => "SpellSets_Save",
            Self::Rename => "SpellSets_Rename",
            Self::Delete => "SpellSets_Delete",
            Self::Apply => "SpellSets_Apply",
            Self::Cancel => "SpellSets_Cancel",
        }
    }

    fn label(self, sets: &Sets) -> &'static str {
        match self {
            Self::Toggle => {
                if sets.panel.expanded {
                    "Sets ▴"
                } else {
                    "Spell sets"
                }
            }
            Self::Previous => "‹",
            Self::Next => "›",
            Self::Save => "Save current",
            Self::Rename => "Rename",
            Self::Delete if sets.panel.confirm_delete => "Confirm delete",
            Self::Delete => "Delete",
            Self::Apply => "Apply",
            Self::Cancel => "Cancel apply",
        }
    }

    fn enabled(self, sets: &Sets) -> bool {
        if let Some(active) = &sets.active {
            return self == Self::Cancel && !active.stopping();
        }
        match self {
            Self::Toggle => true,
            Self::Previous | Self::Next => sets.saved.len() > 1,
            Self::Save => sets.writable(),
            Self::Rename | Self::Delete => sets.writable() && sets.panel.selected.is_some(),
            Self::Apply => sets.panel.selected.is_some(),
            Self::Cancel => false,
        }
    }
}

#[derive(Component)]
pub(crate) struct Body;
#[derive(Component)]
pub(crate) enum Label {
    Button(Action),
    Name,
    Selection,
    Status,
}
#[derive(Component)]
pub(crate) struct SkinPanel {
    frame: Entity,
    base_height: f32,
}

const PANEL_HEIGHT: f32 = 132.0;

fn script_id(id: &str) -> SkinId {
    SkinId {
        window: WindowId::Spellbook,
        id: id.into(),
    }
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, tooltip: &str) {
    parent
        .spawn((
            Button,
            action,
            script_id(action.id()),
            Node {
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(6), px(2)),
                ..default()
            },
            BackgroundColor(theme::BUTTON),
            Tooltip(tooltip.into()),
        ))
        .with_child((
            Label::Button(action),
            theme::text("", Size::Small, theme::INK_BRIGHT),
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
        ));
}

pub(crate) fn toggle(parent: &mut ChildSpawnerCommands) {
    button(
        parent,
        Action::Toggle,
        "Manage saved spell gem layouts for this character.",
    );
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands, skin: Option<(Entity, f32, f32)>) {
    let mut node = skin.map_or_else(Node::default, |(_, width, height)| {
        crate::skinned::at(0.0, height, width, PANEL_HEIGHT)
    });
    node.display = Display::None;
    node.flex_direction = FlexDirection::Column;
    node.row_gap = px(3);
    node.padding = UiRect::all(px(4));
    let mut body = parent.spawn((Body, KeepsPress, node, BackgroundColor(theme::COVER)));
    if let Some((frame, _, base_height)) = skin {
        body.insert(SkinPanel { frame, base_height });
    }
    body.with_children(|parent| {
        row(parent, |row| {
            button(row, Action::Previous, "Previous saved spell set");
            row.spawn((
                Label::Selection,
                Interaction::None,
                Node {
                    min_width: px(0),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
                theme::text("", Size::Body, theme::INK_BRIGHT),
                TextLayout::new(Justify::Center, LineBreak::NoWrap),
                Tooltip::default(),
            ));
            button(row, Action::Next, "Next saved spell set");
        });
        parent.spawn((
            Button,
            NameField,
            Label::Name,
            script_id("SpellSets_Name"),
            Node {
                min_height: px(19),
                padding: UiRect::axes(px(4), px(2)),
                border: UiRect::all(px(1)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::INSET),
            BorderColor::all(theme::EDGE),
            theme::text("Set name…", Size::Small, theme::INK_BRIGHT),
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
            Tooltip("Name the current gem layout or enter a new name for the selected set.".into()),
        ));
        row(parent, |row| {
            button(
                row,
                Action::Save,
                "Save all eight current gems, including empty slots.",
            );
            button(
                row,
                Action::Rename,
                "Rename the selected layout using the name above.",
            );
            button(
                row,
                Action::Delete,
                "Remove only the selected saved layout.",
            );
        });
        row(parent, |row| {
            button(
                row,
                Action::Apply,
                "Apply the selected layout, clearing gems saved as empty.",
            );
            button(
                row,
                Action::Cancel,
                "Stop after the current request settles.",
            );
        });
        parent.spawn((
            Label::Status,
            Interaction::None,
            Tooltip::default(),
            Node {
                height: px(38),
                overflow: Overflow::clip(),
                ..default()
            },
            theme::text("", Size::Body, theme::INK_BRIGHT),
        ));
    });
}

fn row(parent: &mut ChildSpawnerCommands, children: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            column_gap: px(4),
            min_height: px(18),
            align_items: AlignItems::Stretch,
            ..default()
        })
        .with_children(children);
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn input(
    mut sets: ResMut<Sets>,
    (online, outbox, settings, names): (
        Res<OnlineState>,
        Res<Outbox>,
        Res<crate::ViewerSettings>,
        Res<super::super::SpellNames>,
    ),
    (shown, frames): (Res<Shown>, Query<(&WindowId, &Frame)>),
    (keys, clicks, closes): (
        crate::keys::Keys,
        crate::clicks::Clicks<Action>,
        crate::clicks::Clicks<crate::skinned::Closes>,
    ),
    (mut requests, mut chat): (ResMut<ActionRequests>, ResMut<ChatState>),
) {
    let world = online.world();
    let open = shown.displayed(WindowId::Spellbook)
        && !closes.iter().any(|close| close.0 == WindowId::Spellbook)
        && frames
            .iter()
            .any(|(id, frame)| *id == WindowId::Spellbook && !frame.minimized());
    let clicked = (open && keys.window_focused())
        .then(|| clicks.iter().copied().find(|action| action.enabled(&sets)))
        .flatten();
    if let Some(active) = sets.active.as_mut() {
        if !open {
            active.stop("Spellbook closed or minimized.");
        } else if clicked == Some(Action::Cancel) {
            active.stop("Cancelled.");
        }
        if let Some(result) =
            active.tick(world, &outbox, Some(&mut requests), &names, Instant::now())
        {
            outbox.end_spell_set();
            chat.history.push(system_line(result.clone()));
            sets.status = result;
            sets.active = None;
        }
        return;
    }
    if !open {
        if std::mem::take(&mut sets.panel.confirm_delete) {
            sets.status.clear();
        }
        return;
    }
    if sets.store.profile.is_none()
        || !super::eligible(world, settings.0.installed_client)
        || outbox.peek(world).is_none()
    {
        return;
    }
    if let Some(action) = clicked.filter(|action| sets.panel.expanded || *action == Action::Toggle)
    {
        let confirm = std::mem::take(&mut sets.panel.confirm_delete);
        let result = match action {
            Action::Toggle => {
                sets.panel.expanded = !sets.panel.expanded;
                if sets.panel.selected.is_none() {
                    let first = sets.saved.keys().next().cloned();
                    sets.select(first);
                }
                return;
            }
            Action::Previous | Action::Next => {
                sets.browse(action == Action::Next);
                return;
            }
            Action::Save => sets
                .save_current(world.gems())
                .unwrap_or_else(str::to_owned),
            Action::Rename => sets.rename().unwrap_or_else(str::to_owned),
            Action::Delete => sets.delete(confirm),
            Action::Apply => match start(&mut sets, world, &outbox) {
                Ok(()) => String::new(),
                Err(error) => error.to_string(),
            },
            Action::Cancel => return,
        };
        sets.status = result;
    }
}

fn start(
    sets: &mut Sets,
    world: &eq_client_core::world::ClientWorld,
    outbox: &Outbox,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let selected = sets.panel.selected.as_ref().context("Choose a spell set")?;
    let target = *sets.saved.get(selected).context("Choose a spell set")?;
    sets.active = Some(Apply::start(selected.clone(), target, world, outbox)?);
    Ok(())
}

type Bodies<'w, 's> =
    Query<'w, 's, (&'static mut Node, Option<&'static SkinPanel>), (With<Body>, Without<Frame>)>;
type Controls<'w, 's> = Query<
    'w,
    's,
    (
        &'static Action,
        &'static Interaction,
        &'static mut BackgroundColor,
        &'static mut Node,
    ),
    (Without<Body>, Without<Frame>),
>;
type Labels<'w, 's> = Query<
    'w,
    's,
    (
        &'static Label,
        &'static mut Text,
        Option<&'static mut BorderColor>,
        Option<&'static mut Tooltip>,
    ),
>;

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn present(
    sets: Res<Sets>,
    (online, settings, names, typing): (
        Res<OnlineState>,
        Res<crate::ViewerSettings>,
        Res<super::super::SpellNames>,
        Res<crate::keys::Typing>,
    ),
    mut bodies: Bodies,
    mut frames: Query<(&Frame, &mut Node)>,
    mut controls: Controls,
    mut labels: Labels,
) {
    let visible = sets.store.profile.is_some()
        && super::eligible(online.world(), settings.0.installed_client);
    for (mut node, skin) in &mut bodies {
        let expanded = visible && sets.panel.expanded;
        let display = if expanded {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some(skin) = skin
            && let Ok((frame, mut root)) = frames.get_mut(skin.frame)
            && !frame.minimized()
        {
            let height = px(skin.base_height + if expanded { PANEL_HEIGHT } else { 0.0 });
            if root.height != height {
                root.height = height;
            }
        }
    }
    for (action, interaction, mut background, mut node) in &mut controls {
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let on = *action == Action::Toggle && sets.panel.expanded;
        background.set_if_neq(BackgroundColor(theme::button(
            action.enabled(&sets),
            on,
            *interaction,
        )));
    }
    let expanded = visible && sets.panel.expanded;
    let selected = sets
        .panel
        .selected
        .as_ref()
        .and_then(|name| sets.saved.get_key_value(name));
    for (label, mut text, border, tip) in &mut labels {
        let (value, hint) = match label {
            Label::Button(action) => (action.label(&sets).into(), None),
            _ if !expanded => continue,
            Label::Name => {
                let (value, edge) = if typing.spell_set_name {
                    (format!("{}|", sets.panel.draft), theme::FOCUS)
                } else if sets.panel.draft.is_empty() {
                    ("Set name…".into(), theme::EDGE)
                } else {
                    (sets.panel.draft.clone(), theme::EDGE)
                };
                if let Some(mut border) = border {
                    border.set_if_neq(BorderColor::all(edge));
                }
                (value, None)
            }
            Label::Selection => selected.map_or_else(
                || ("No saved sets".into(), Some(String::new())),
                |(name, gems)| (name.clone(), Some(layout_text(gems, &names))),
            ),
            Label::Status => {
                let status = status(&sets);
                (status.clone(), Some(status))
            }
        };
        if text.0 != value {
            text.0 = value;
        }
        if let (Some(mut tip), Some(hint)) = (tip, hint)
            && tip.0 != hint
        {
            tip.0 = hint;
        }
    }
}

fn status(sets: &Sets) -> String {
    if let Some(active) = &sets.active {
        return active.progress();
    }
    if sets.load_error {
        return "Could not read this character’s spell sets.".into();
    }
    if !sets.writable() {
        return "A settings folder is required to save spell sets.".into();
    }
    if sets
        .file_name()
        .is_some_and(|name| sets.store.pending(&name))
    {
        return "Changes waiting to save to disk.".into();
    }
    if sets.status.is_empty() {
        return "Hover a set to see its gems.".into();
    }
    sets.status.clone()
}

fn layout_text(gems: &Gems, names: &super::super::SpellNames) -> String {
    let mut text = String::new();
    for (index, spell) in gems.iter().enumerate() {
        if index != 0 {
            text.push('\n');
        }
        let name = spell.map_or_else(|| "Empty".into(), |spell| names.label(spell));
        let _ = write!(text, "Gem {} · {name}", index + 1);
    }
    text
}
