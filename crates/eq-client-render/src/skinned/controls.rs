//! The skin's sliders, drop-downs and lists, drawn with its own pieces and
//! tied to what the client keeps: the Options window's Far Clip Plane, Max
//! FPS and Mouselook Sensitivity sliders, and its key list with the filter
//! above it. A control the client keeps nothing for is drawn greyed out, as
//! its buttons are.
use super::{Area, WindowId, at, border, picture, to_f32};
use crate::keys::{KeyGroup, KeyMap};
use crate::options::OptionsState;
use crate::theme::{self, Size};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use eq_client_assets::sidl::{Combobox, Listbox, Slider};
use eq_client_core::options::Level;

/// The Options window's sliders this client keeps settings for, with the
/// labels that show their values, by screen ID.
const LEVEL_SLIDERS: [(&str, &str, Level); 3] = [
    (
        "ODP_ClipPlaneSlider",
        "ODP_ClipPlaneValueLabel",
        Level::ClipPlane,
    ),
    (
        "ODP_MaxFPSSlider",
        "ODP_MaxFPSSliderValueLabel",
        Level::MaxFps,
    ),
    (
        "OMP_MouseSensitivitySlider",
        "OMP_MouseSensitivityValueLabel",
        Level::MouseSensitivity,
    ),
];

/// The Keyboard page's list of key assignments, and the drop-down that
/// filters it.
const KEY_LIST: &str = "OKP_KeyboardAssignmentList";
const KEY_FILTER: &str = "OKP_KeyboardFilterCombobox";

/// How tall a list's heading and each of its rows are.
const ROW_HEIGHT: f32 = 15.0;

/// The setting a slider sets, if the client keeps one for it.
fn slider_level(id: Option<&str>, owner: WindowId) -> Option<Level> {
    let id = id?;
    (owner == WindowId::Options)
        .then(|| LEVEL_SLIDERS.iter().find(|(slider, _, _)| *slider == id))
        .flatten()
        .map(|(_, _, level)| *level)
}

/// What a slider's value label shows: the setting, if the client keeps one
/// for the slider, or nothing.
pub(super) enum ValueLabel {
    Shows(Level),
    Blank,
}

/// What a label shows, if it is one of the Options window's sliders' value
/// labels; a slider the client keeps nothing for shows no value, rather
/// than the skin's sample.
pub(super) fn value_label(name: &str, owner: WindowId) -> Option<ValueLabel> {
    if owner != WindowId::Options || !name.ends_with("ValueLabel") {
        return None;
    }
    Some(
        LEVEL_SLIDERS
            .iter()
            .find(|(_, label, _)| *label == name)
            .map_or(ValueLabel::Blank, |(_, _, level)| ValueLabel::Shows(*level)),
    )
}

/// A slider the client keeps a setting for: pressing or dragging along it
/// sets it.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct LevelSlider {
    pub(crate) level: Level,
    /// The slider's width.
    width: f32,
    /// The thumb's width.
    thumb: f32,
}

impl LevelSlider {
    /// The setting's place along the slider for the pointer at this point
    /// across it, from -0.5 at its left edge to 0.5 at its right: the
    /// thumb's middle follows the pointer.
    fn fraction(&self, across: f32) -> f32 {
        let travel = (self.width - self.thumb).max(1.0);
        ((across + 0.5) * self.width - self.thumb / 2.0) / travel
    }

    /// Where across the slider the pointer puts the setting at this place
    /// along it, for a script's press.
    pub(crate) fn across(&self, fraction: f32) -> f32 {
        let travel = (self.width - self.thumb).max(1.0);
        (fraction * travel + self.thumb / 2.0) / self.width.max(1.0) - 0.5
    }
}

/// A slider's thumb, which sits where its setting does along the track.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SliderThumb {
    level: Level,
    /// How far the thumb travels from the track's left.
    travel: f32,
}

/// A label that shows a slider's setting.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct LevelValue(pub(crate) Level);

/// A slider: its track between its end caps, and its thumb where its
/// setting is.
pub(super) fn slider(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    slider: &Slider,
    inside: &Area,
    owner: WindowId,
) {
    let area = slider.area;
    let look = &slider.look;
    let level = slider_level(slider.id.as_deref(), owner);
    let size = |piece: Option<&eq_client_assets::sidl::Piece>| {
        piece.map_or((0.0, 0.0), |piece| {
            (to_f32(piece.width), to_f32(piece.height))
        })
    };
    let thumb_piece = if level.is_some() {
        look.thumb.normal.as_ref()
    } else {
        look.thumb.disabled.as_ref().or(look.thumb.normal.as_ref())
    };
    let (thumb_width, thumb_height) = size(thumb_piece);
    let travel = (area.width - thumb_width).max(0.0);
    let mut node = window.spawn(at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    ));
    if let Some(level) = level {
        node.insert((
            Button,
            LevelSlider {
                level,
                width: area.width,
                thumb: thumb_width,
            },
            RelativeCursorPosition::default(),
        ));
    } else {
        node.insert(super::missing());
    }
    node.with_children(|track| {
        let (left, left_height) = size(look.cap_left.as_ref());
        let (right, right_height) = size(look.cap_right.as_ref());
        let (_, height) = size(look.background.as_ref());
        let middle = |height: f32| (area.height - height) / 2.0;
        if let Some(piece) = &look.cap_left {
            picture(
                track,
                art,
                piece,
                at(0.0, middle(left_height), left, left_height),
            );
        }
        if let Some(piece) = &look.background {
            let width = (area.width - left - right).max(0.0);
            picture(track, art, piece, at(left, middle(height), width, height));
        }
        if let Some(piece) = &look.cap_right {
            picture(
                track,
                art,
                piece,
                at(
                    area.width - right,
                    middle(right_height),
                    right,
                    right_height,
                ),
            );
        }
        if let Some(image) = thumb_piece.and_then(|piece| art.cut(piece)) {
            let mut thumb = track.spawn((
                image,
                at(0.0, middle(thumb_height), thumb_width, thumb_height),
            ));
            if let Some(level) = level {
                thumb.insert(SliderThumb { level, travel });
            }
        }
    });
}

/// Sets a slider's setting from where the pointer presses or drags it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn slide(
    sliders: Query<(&Interaction, &LevelSlider, &RelativeCursorPosition)>,
    mut state: ResMut<OptionsState>,
) {
    for (interaction, slider, cursor) in &sliders {
        let (Interaction::Pressed, Some(at)) = (interaction, cursor.normalized) else {
            continue;
        };
        let value = slider.level.at(slider.fraction(at.x));
        if state.options.level(slider.level) != value {
            state.options.set_level(slider.level, value);
        }
    }
}

/// Puts each slider's thumb where its setting is, and each value label's
/// words to it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show_levels(
    state: Res<OptionsState>,
    mut thumbs: Query<(&SliderThumb, &mut Node)>,
    mut labels: Query<(&LevelValue, &mut Text)>,
) {
    for (thumb, mut node) in &mut thumbs {
        let left = px(thumb.level.fraction(state.options.level(thumb.level)) * thumb.travel);
        if node.left != left {
            node.left = left;
        }
    }
    for (LevelValue(level), mut text) in &mut labels {
        let words = level.words(state.options.level(*level));
        if text.0 != words {
            text.0 = words;
        }
    }
}

/// What a drop-down the client keeps a choice for chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choosing {
    /// Which keys the Keyboard page lists.
    KeyFilter,
}

/// The Keyboard page's filter: the choice its drop-down shows, by name.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyFilter(pub(crate) Option<String>);

/// A drop-down the client keeps a choice for: a click opens its list of
/// choices, and another closes it.
#[derive(Component, Debug)]
pub(crate) struct DropDown {
    choosing: Choosing,
    choices: Vec<String>,
    /// How tall the open list may grow.
    list_height: f32,
    /// The open list.
    open: Option<Entity>,
    /// How the skin draws its box, which its open list is drawn in too.
    look: Option<eq_client_assets::sidl::WindowTemplate>,
}

/// The words a drop-down shows: what is chosen.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ChosenWords(Choosing);

/// One of an open drop-down's choices.
#[derive(Component, Clone, Debug)]
pub(crate) struct DropDownChoice {
    drop_down: Entity,
    choosing: Choosing,
    choice: String,
    /// Its place in the list, from zero.
    pub(crate) index: usize,
}

impl DropDown {
    /// What the drop-down chooses.
    pub(crate) const fn choosing(&self) -> Choosing {
        self.choosing
    }
}

/// What a drop-down chooses, if the client keeps a choice for it.
fn choosing(id: Option<&str>, owner: WindowId) -> Option<Choosing> {
    (owner == WindowId::Options && id == Some(KEY_FILTER)).then_some(Choosing::KeyFilter)
}

/// A drop-down: its frame, the choice in it and the button beside it.
pub(super) fn combobox(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    combobox: &Combobox,
    inside: &Area,
    owner: WindowId,
) {
    let area = combobox.area;
    let choosing = choosing(combobox.id.as_deref(), owner);
    let mut node = window.spawn(at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    ));
    if let Some(choosing) = choosing {
        node.insert((
            Button,
            DropDown {
                choosing,
                choices: combobox.choices.clone(),
                list_height: combobox.list_height,
                open: None,
                look: combobox.template.clone(),
            },
        ));
    } else {
        node.insert((BackgroundColor(theme::INSET), super::missing()));
    }
    node.with_children(|frame| {
        let mut client = Area {
            x: 0.0,
            y: 0.0,
            width: area.width,
            height: area.height,
        };
        if let Some(template) = &combobox.template {
            client = border(frame, art, &template.border, (area.width, area.height));
        }
        let look = &combobox.button;
        let arrow = if choosing.is_some() {
            look.normal.as_ref()
        } else {
            look.disabled.as_ref().or(look.normal.as_ref())
        };
        let (arrow_width, arrow_height) = arrow.map_or((0.0, 0.0), |piece| {
            (to_f32(piece.width), to_f32(piece.height))
        });
        if let Some(piece) = arrow {
            picture(
                frame,
                art,
                piece,
                at(
                    client.x + client.width - arrow_width,
                    client.y + (client.height - arrow_height) / 2.0,
                    arrow_width,
                    arrow_height,
                ),
            );
        }
        // One the client keeps no choice for shows none, rather than a
        // choice it would not make.
        let mut text = frame.spawn((
            theme::text("", Size::Small, theme::INK_BRIGHT),
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
            Node {
                position_type: PositionType::Absolute,
                left: px(client.x + 4.0),
                top: px(client.y + (client.height - 13.0) / 2.0),
                ..default()
            },
        ));
        if let Some(choosing) = choosing {
            text.insert(ChosenWords(choosing));
        }
    });
}

/// Opens and closes drop-downs, and takes a choice from an open one. An
/// open list is drawn in its box's skin: its background and border, with
/// the choices inside.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn drop_downs(
    mut commands: Commands,
    mut art: crate::sheets::Art,
    mut drop_downs: Query<(Entity, Ref<Interaction>, &mut DropDown, &ComputedNode)>,
    choices: Query<(&Interaction, &DropDownChoice), Changed<Interaction>>,
    mut filter: ResMut<KeyFilter>,
) {
    for (interaction, choice) in &choices {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match choice.choosing {
            Choosing::KeyFilter => filter.0 = Some(choice.choice.clone()),
        }
        if let Ok((_, _, mut drop_down, _)) = drop_downs.get_mut(choice.drop_down)
            && let Some(list) = drop_down.open.take()
        {
            commands.entity(list).despawn();
        }
    }
    for (entity, interaction, mut drop_down, node) in &mut drop_downs {
        if !interaction.is_changed() || *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(list) = drop_down.open.take() {
            commands.entity(list).despawn();
            continue;
        }
        let (width, height) = (node.size() * node.inverse_scale_factor()).into();
        let rows = f32::from(u16::try_from(drop_down.choices.len()).unwrap_or(u16::MAX));
        let list_height = (rows * ROW_HEIGHT + 4.0).min(drop_down.list_height);
        let insets = drop_down
            .look
            .as_ref()
            .map(|look| super::border_insets(&look.border));
        let padding = insets.map_or(UiRect::all(px(2)), |insets| UiRect {
            left: px(insets.left + 2.0),
            right: px(insets.right + 2.0),
            top: px(insets.top + 2.0),
            bottom: px(insets.bottom + 2.0),
        });
        let mut list = commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(height),
                width: percent(100),
                height: px(list_height),
                flex_direction: FlexDirection::Column,
                padding,
                overflow: Overflow::clip(),
                ..default()
            },
            GlobalZIndex(100),
            ChildOf(entity),
        ));
        dress(
            &mut list,
            &mut art,
            drop_down.look.as_ref(),
            (width, list_height),
        );
        let list = list.id();
        for (index, choice) in drop_down.choices.iter().enumerate() {
            commands.spawn((
                Button,
                DropDownChoice {
                    drop_down: entity,
                    choosing: drop_down.choosing,
                    choice: choice.clone(),
                    index,
                },
                Node {
                    height: px(ROW_HEIGHT),
                    flex_shrink: 0.0,
                    padding: UiRect::horizontal(px(3)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                ChildOf(list),
                children![(
                    theme::text(choice.as_str(), Size::Small, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                )],
            ));
        }
        drop_down.open = Some(list);
    }
}

/// Draws an open drop-down's list in its box's skin, background and
/// border, over the client's fill: a skin's background may let what lies
/// beneath show through, as the Velious skin's do, and an open list covers
/// other words. Without the skin's box it keeps the client's own look.
fn dress(
    list: &mut EntityCommands,
    art: &mut crate::sheets::Art,
    look: Option<&eq_client_assets::sidl::WindowTemplate>,
    size: (f32, f32),
) {
    list.insert(BackgroundColor(theme::INSET));
    let Some(look) = look else {
        list.insert(BorderColor::all(theme::EDGE));
        return;
    };
    if let Some(image) = look
        .background
        .as_deref()
        .and_then(|file| art.texture(file))
    {
        list.insert(ImageNode {
            image,
            image_mode: NodeImageMode::Tiled {
                tile_x: true,
                tile_y: true,
                stretch_value: 1.0,
            },
            ..default()
        });
    }
    list.with_children(|list| {
        border(list, art, &look.border, size);
    });
}

/// Lights the choice under the pointer in an open drop-down.
pub(crate) fn light_choices(
    mut choices: Query<(&Interaction, &mut BackgroundColor), With<DropDownChoice>>,
) {
    for (interaction, mut background) in &mut choices {
        let wanted = if *interaction == Interaction::None {
            Color::NONE
        } else {
            theme::BUTTON
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

/// Shows each drop-down's choice: the key filter's, or the skin's last
/// choice, "All", until one is made.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show_choices(
    filter: Res<KeyFilter>,
    drop_downs: Query<(&DropDown, &Children)>,
    mut words: Query<(&ChosenWords, &mut Text)>,
) {
    for (drop_down, children) in &drop_downs {
        let shown = match drop_down.choosing {
            Choosing::KeyFilter => filter
                .0
                .clone()
                .or_else(|| drop_down.choices.last().cloned()),
        }
        .unwrap_or_default();
        for child in children {
            if let Ok((ChosenWords(_), mut text)) = words.get_mut(*child)
                && text.0 != shown
            {
                text.0.clone_from(&shown);
            }
        }
    }
}

/// What a list shows, if the client fills it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Listing {
    /// The client's key assignments, as the key filter chooses.
    Keys,
    /// The skills a guildmaster teaches.
    Training,
    /// The player's skills.
    Skills,
}

/// A list's rows, which the client fills and the wheel scrolls.
#[derive(Component, Clone, Debug)]
pub(crate) struct ListRows {
    listing: Listing,
    /// Each column's width, from the left.
    columns: Vec<f32>,
    /// Whether the rows were filled yet.
    filled: bool,
    /// The key filter the rows were last filled for.
    filter: Option<String>,
}

/// What a list shows, if the client fills it.
fn listing(id: Option<&str>, owner: WindowId) -> Option<Listing> {
    match (owner, id) {
        (WindowId::Options, Some(KEY_LIST)) => Some(Listing::Keys),
        (WindowId::Training, Some("SkillList")) => Some(Listing::Training),
        (WindowId::Skills, Some("SkillList")) => Some(Listing::Skills),
        _ => None,
    }
}

/// A list: its frame, its columns' headings, and its rows under them.
pub(super) fn listbox(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    list: &Listbox,
    inside: &Area,
    owner: WindowId,
) {
    let area = list
        .anchors
        .map(|anchors| anchors.within(inside.width, inside.height))
        .or(list.area)
        .unwrap_or(Area {
            x: 0.0,
            y: 0.0,
            width: inside.width,
            height: inside.height,
        });
    let listing = listing(list.id.as_deref(), owner);
    let mut drawn = window.spawn((
        at(
            inside.x + area.x,
            inside.y + area.y,
            area.width,
            area.height,
        ),
        BackgroundColor(theme::INSET),
    ));
    if listing.is_none() {
        drawn.insert(super::missing());
    }
    drawn.with_children(|frame| {
        let mut client = Area {
            x: 0.0,
            y: 0.0,
            width: area.width,
            height: area.height,
        };
        if let Some(template) = &list.template {
            client = border(frame, art, &template.border, (area.width, area.height));
        }
        let ink = if listing.is_some() {
            theme::INK_BRIGHT
        } else {
            theme::INK_DIM
        };
        // A list the client fills scrolls, with the skin's scrollbar down
        // its right where the skin gives it one.
        let bar = list.scrollbar.as_ref().filter(|_| listing.is_some());
        let bar_width = bar.map_or(0.0, super::scrollbar::width);
        let heading = headings(frame, art, list, (&client, ink));
        let mut rows = frame.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..at(
                    client.x + 2.0,
                    client.y + heading + 2.0,
                    (client.width - 4.0 - bar_width).max(0.0),
                    (client.height - heading - 4.0).max(0.0),
                )
            },
            ScrollPosition::default(),
        ));
        let scrolled = rows.id();
        let columns = list.columns.iter().map(|column| column.width).collect();
        match listing {
            Some(Listing::Keys) => {
                rows.insert((
                    ListRows {
                        listing: Listing::Keys,
                        columns,
                        filled: false,
                        filter: None,
                    },
                    crate::windows::pointer::TakesWheel,
                ));
            }
            Some(Listing::Training) => {
                rows.insert((
                    crate::training::SkillRows::new(columns),
                    crate::windows::pointer::TakesWheel,
                ));
            }
            Some(Listing::Skills) => {
                rows.insert((
                    crate::skills::SkillsList::new(columns),
                    crate::windows::pointer::TakesWheel,
                ));
            }
            None => (),
        }
        if let Some(look) = bar {
            super::scrollbar::spawn(frame, art, look, &client, (scrolled, owner));
        }
    });
}

/// Each of a list's column headings across the top of its inside, on the
/// skin's heading frame where it has one; returns how high they are, which
/// is the frame's height.
fn headings(
    frame: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    list: &Listbox,
    (client, ink): (&Area, Color),
) -> f32 {
    let header = list.header.as_deref();
    let height = header
        .and_then(super::frame::heading_height)
        .unwrap_or(ROW_HEIGHT);
    let mut x = client.x;
    for column in &list.columns {
        frame
            .spawn(at(x, client.y, column.width, height))
            .with_children(|cell| {
                if let Some(look) = header {
                    super::frame::heading(cell, art, look);
                }
                cell.spawn((
                    theme::text(column.heading.as_str(), Size::Small, ink),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    at(
                        2.0,
                        (height - ROW_HEIGHT) / 2.0 + 1.0,
                        column.width - 4.0,
                        ROW_HEIGHT,
                    ),
                ));
            });
        x += column.width;
    }
    height
}

/// Fills each list the client fills, again whenever what it shows changes:
/// the key list with the keys the filter chooses.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill_lists(
    mut commands: Commands,
    filter: Res<KeyFilter>,
    keys: Res<KeyMap>,
    mut lists: Query<(Entity, &mut ListRows, Option<&Children>)>,
) {
    for (entity, mut rows, children) in &mut lists {
        let wanted = match rows.listing {
            Listing::Keys => filter.0.clone(),
            // The Training and Skills windows' lists fill themselves.
            Listing::Training | Listing::Skills => continue,
        };
        if rows.filled && rows.filter == wanted && !keys.is_changed() {
            continue;
        }
        if let Some(children) = children {
            for child in children {
                commands.entity(*child).despawn();
            }
        }
        let group = wanted.as_deref().and_then(KeyGroup::named);
        for (act, chords) in keys.assignments() {
            if group.is_some_and(|group| act.group() != group) {
                continue;
            }
            let cells = [
                act.command(),
                chords
                    .first()
                    .map(|chord| chord.label())
                    .unwrap_or_default(),
                chords.get(1).map(|chord| chord.label()).unwrap_or_default(),
            ];
            let row = commands
                .spawn((
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ChildOf(entity),
                ))
                .id();
            for (words, width) in cells.into_iter().zip(&rows.columns) {
                commands.spawn((
                    theme::text(words, Size::Small, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    Node {
                        width: px(*width),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ChildOf(row),
                ));
            }
        }
        rows.filled = true;
        rows.filter = wanted;
    }
}

/// The lists the wheel scrolls: those the client fills.
type ScrolledList = Or<(
    With<ListRows>,
    With<crate::training::SkillRows>,
    With<crate::skills::SkillsList>,
)>;

/// Scrolls the list the wheel turns.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn scroll_lists(
    wheel: Res<crate::windows::pointer::Wheel>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), ScrolledList>,
) {
    if let Some((node, mut position)) = wheel.surface.and_then(|list| lists.get_mut(list).ok()) {
        crate::windows::scroll_by(&mut position, node, wheel.pixels);
    }
}

impl KeyGroup {
    /// The group a key filter's choice names; None for "All" and for a
    /// name the client has no group for.
    fn named(choice: &str) -> Option<Self> {
        Some(match choice {
            "Movement" => Self::Movement,
            "Commands" => Self::Commands,
            "Spell casting" => Self::SpellCasting,
            "Target" => Self::Target,
            "Camera" => Self::Camera,
            "Chat" => Self::Chat,
            "UI" => Self::Ui,
            "Hotbar 1" => Self::Hotbar(1),
            "Hotbar 2" => Self::Hotbar(2),
            "Hotbar 3" => Self::Hotbar(3),
            "Hotbar 4" => Self::Hotbar(4),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scripted_press_lands_where_the_setting_is() {
        let slider = LevelSlider {
            level: Level::MaxFps,
            width: 100.0,
            thumb: 10.0,
        };
        for fraction in [0.0, 0.25, 1.0] {
            assert!((slider.fraction(slider.across(fraction)) - fraction).abs() < 1e-6);
        }
    }

    #[test]
    fn the_thumbs_middle_follows_the_pointer() {
        let slider = LevelSlider {
            level: Level::ClipPlane,
            width: 100.0,
            thumb: 10.0,
        };
        // The pointer at the left edge holds the thumb against it.
        assert!(slider.fraction(-0.5) < 0.0);
        assert_eq!(Level::ClipPlane.at(slider.fraction(-0.5)), 0);
        // Halfway across is halfway along.
        assert!((slider.fraction(0.0) - 0.5).abs() < 1e-6);
        assert_eq!(Level::ClipPlane.at(slider.fraction(0.5)), 100);
    }

    #[test]
    fn only_the_options_windows_controls_are_wired() {
        assert_eq!(
            slider_level(Some("ODP_MaxFPSSlider"), WindowId::Options),
            Some(Level::MaxFps)
        );
        assert_eq!(
            slider_level(Some("ODP_GammaSlider"), WindowId::Options),
            None
        );
        assert_eq!(
            slider_level(Some("ODP_MaxFPSSlider"), WindowId::Inventory),
            None
        );
        assert!(matches!(
            value_label("OMP_MouseSensitivityValueLabel", WindowId::Options),
            Some(ValueLabel::Shows(Level::MouseSensitivity))
        ));
        assert!(matches!(
            value_label("ODP_GammaValueLabel", WindowId::Options),
            Some(ValueLabel::Blank)
        ));
        assert!(value_label("ODP_GammaLabel", WindowId::Options).is_none());
        assert_eq!(
            choosing(Some(KEY_FILTER), WindowId::Options),
            Some(Choosing::KeyFilter)
        );
        assert_eq!(choosing(Some("ODP_SkyCombobox"), WindowId::Options), None);
        assert_eq!(
            listing(Some(KEY_LIST), WindowId::Options),
            Some(Listing::Keys)
        );
        assert_eq!(listing(Some("OFP_FilterList"), WindowId::Options), None);
    }

    #[test]
    fn the_filters_names_choose_their_groups_and_all_chooses_none() {
        assert_eq!(
            KeyGroup::named("Spell casting"),
            Some(KeyGroup::SpellCasting)
        );
        assert_eq!(KeyGroup::named("Hotbar 1"), Some(KeyGroup::Hotbar(1)));
        assert_eq!(KeyGroup::named("All"), None);
        // The key list keeps every act under one of the filter's groups.
        let keys = KeyMap::default();
        for (act, _) in keys.assignments() {
            let group = act.group();
            assert!(!act.command().is_empty(), "{act:?}");
            assert!(
                [
                    "Movement",
                    "Commands",
                    "Spell casting",
                    "Target",
                    "Camera",
                    "Chat",
                    "UI",
                    "Hotbar 1",
                ]
                .iter()
                .any(|name| KeyGroup::named(name) == Some(group)),
                "{act:?}"
            );
        }
    }
}
