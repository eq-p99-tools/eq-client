//! Clickable chat item links and a read-only server-backed details panel.
use super::{online::OnlineState, outbox::Outbox};
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, ItemDetails, ItemLink, qol::Fix};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Component)]
pub(super) struct ItemButton {
    link: ItemLink,
    arrival: Option<(u64, usize)>,
}

#[cfg(test)]
impl ItemButton {
    pub(super) const fn item_id(&self) -> u32 {
        self.link.item_id
    }
}

#[derive(Component)]
pub(super) struct ItemPanel;
#[derive(Component)]
pub(super) struct ItemText;
#[derive(Component)]
pub(super) struct CloseItem;
/// The skin's item display's title bar words: the item's name.
#[derive(Component)]
pub(crate) struct ItemTitle;
/// The skin's item display's box for the item's picture, with where in it
/// the picture goes: over the whole button, as the program draws the item's
/// icon as the button's Normal piece, with no template background under it
/// and no decal (from the program, Titanium sitting 9e). In the program the
/// template's Pressed, Flyby or Disabled piece would show in its state if
/// the template fills it; ours draws none of them (to check against the
/// skin's `IDW_IconButton` template in the 2a look).
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ItemIcon {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) size: f32,
}
/// The item's picture in its box.
#[derive(Component)]
pub(crate) struct ItemIconPicture;
/// The item panel: the item chosen, and the definition on screen, which
/// stays while it shows whatever the world's cache forgets.
#[derive(Resource, Default)]
pub(super) struct ItemState {
    pub hovered: bool,
    selected: Option<(u32, String)>,
    shown: Option<ItemDetails>,
    raise_on_present: bool,
    pending: Option<PendingInspection>,
    /// The link body used to request a particular cached observation.
    inspected: BTreeMap<u32, (String, u64)>,
    /// Unanswered requests replaced after timeout. None records mixed bodies
    /// for an ID whose replies cannot distinguish them.
    retired: BTreeMap<u32, Option<String>>,
    status: &'static str,
}

struct PendingInspection {
    id: u32,
    body: String,
    prior: Option<u64>,
    since: Instant,
}

const VARIANT_UNCONFIRMED: &str = "Received item details with an unconfirmed link variant.";

impl PendingInspection {
    fn expired(&self) -> bool {
        self.since.elapsed() > Duration::from_secs(8)
    }
}

impl ItemState {
    /// Shares pointer activation's cached-definition and outbox paths.
    pub(super) fn inspect_link(
        &mut self,
        link: &ItemLink,
        online: &OnlineState,
        outbox: &Outbox,
    ) -> Result<(), &'static str> {
        let world = online.world();
        self.observe(world);
        let cached = world.item_observation(link.item_id);
        if let Some((revision, item)) = cached
            && self
                .inspected
                .get(&link.item_id)
                .is_some_and(|(body, known)| {
                    *known == revision && body.eq_ignore_ascii_case(&link.body)
                })
        {
            self.open_received(item.clone());
            return Ok(());
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| !pending.expired())
        {
            return Err("Wait for the current item inspection");
        }
        outbox
            .try_post(world, |stamp| ClientCommand::InspectItem {
                session_id: stamp.session_id,
                link_body: link.body.clone(),
            })
            .map_err(crate::outbox::Refusal::text)?;
        debug!("Item inspection requested: ID {}", link.item_id);
        self.selected = Some((link.item_id, link.text.clone()));
        self.shown = None;
        self.raise_on_present = true;
        if let Some(previous) = self.pending.take() {
            self.retire(previous.id, previous.body);
        }
        self.pending = Some(PendingInspection {
            id: link.item_id,
            body: link.body.clone(),
            prior: cached.map(|(revision, _)| revision),
            since: Instant::now(),
        });
        self.status = "Loading item from server...";
        Ok(())
    }

    pub(super) fn inspection(&self) -> Option<(u32, bool)> {
        Some((self.selected()?, self.definition().is_some()))
    }

    /// Opens a received inventory definition without making an inspection request.
    pub(super) fn open_received(&mut self, item: ItemDetails) {
        self.selected = Some((item.id, item.name.clone()));
        self.shown = Some(item);
        self.raise_on_present = true;
        self.status = "";
    }

    fn retire(&mut self, id: u32, body: String) {
        self.retired
            .entry(id)
            .and_modify(|previous| {
                if previous
                    .as_ref()
                    .is_some_and(|previous| !previous.eq_ignore_ascii_case(&body))
                {
                    *previous = None;
                }
            })
            .or_insert(Some(body));
    }

    fn observe(&mut self, world: &eq_client_core::world::ClientWorld) {
        let selected = self.selected();
        let Some(pending) = &mut self.pending else {
            return;
        };
        let waiting = selected == Some(pending.id) && self.shown.is_none();
        if let Some((revision, item)) = world.item_observation(pending.id)
            && Some(revision) != pending.prior
        {
            let id = pending.id;
            let body = pending.body.clone();
            // The packet identifies the item ID. A prior unanswered request
            // with a different body makes this response's variant ambiguous.
            if self.retired.get(&id).is_some_and(|previous| {
                previous
                    .as_ref()
                    .is_none_or(|previous| !previous.eq_ignore_ascii_case(&body))
            }) {
                // The latest request may still be unanswered. Keep its
                // original timeout and observe each received definition once.
                pending.prior = Some(revision);
                self.retire(id, body);
                if waiting {
                    self.status = VARIANT_UNCONFIRMED;
                }
                return;
            }
            self.pending = None;
            self.inspected.retain(|id, (_, known)| {
                world
                    .item_observation(*id)
                    .is_some_and(|(current, _)| current == *known)
            });
            self.inspected.insert(id, (body, revision));
            if waiting {
                self.shown = Some(item.clone());
                self.raise_on_present = true;
                self.status = "";
            }
        } else if waiting && pending.expired() && self.status != VARIANT_UNCONFIRMED {
            // Another request can replace this one, but a late definition
            // can still arrive while this is the most recent request.
            self.status = "No item definition received. Click the link to retry.";
        }
    }

    /// The shown item's picture, when its definition carries one.
    fn icon(&self) -> Option<u32> {
        self.definition().and_then(|item| item.icon)
    }

    /// The item the panel shows or waits for.
    pub(super) fn selected(&self) -> Option<u32> {
        self.selected.as_ref().map(|(id, _)| *id)
    }

    /// The definition the panel shows, when it has one for the chosen item.
    fn definition(&self) -> Option<&ItemDetails> {
        self.shown
            .as_ref()
            .filter(|item| Some(item.id) == self.selected())
    }
}

/// Creates an initially hidden item panel; closing it has no server-side effect.
pub(super) fn spawn(commands: &mut Commands) {
    let frame = super::windows::frame(
        commands,
        super::windows::WindowId::Item,
        Node {
            width: px(310),
            max_height: percent(68),
            overflow: Overflow::scroll_y(),
            padding: UiRect::all(px(12)),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            display: Display::None,
            ..default()
        },
    );
    commands.entity(frame).insert((
        super::hud::HudRoot,
        ItemPanel,
        super::windows::pointer::TakesWheel,
        ScrollPosition::default(),
    ));
    commands.entity(frame).with_children(|panel| {
        panel.spawn((
            Button,
            CloseItem,
            Text::new("Close item"),
            theme::font(Size::Label),
            TextColor(theme::INK_WARM),
        ));
        panel.spawn((
            ItemText,
            Text::new(""),
            theme::font(Size::Label),
            TextColor(theme::INK_BRIGHT),
        ));
    });
}

/// Shift-click inserts a received item link. A normal click inspects it
/// through the cache or the ordinary request for the active session.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn input(
    settings: Res<super::ViewerSettings>,
    mut chat: ResMut<super::chat::ChatState>,
    mut typing: Option<ResMut<super::keys::Typing>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut attempted: Local<bool>,
    (buttons, close): (
        crate::clicks::Clicks<ItemButton>,
        crate::clicks::Clicks<CloseItem>,
    ),
    online: Res<OnlineState>,
    sender: Res<Outbox>,
    escape: Res<super::escape::Escape>,
    mut state: ResMut<ItemState>,
) {
    if close.any() || *escape == super::escape::Escape::Close(super::windows::WindowId::Item) {
        state.selected = None;
        state.shown = None;
    }
    state.observe(online.world());
    let automatic = if !*attempted
        && settings.0.validation == Some(super::ValidationAction::InspectFirstItem)
        && online.world().connected()
    {
        chat.history
            .lines(eq_client_core::chat::ChatTab::All)
            .into_iter()
            .flat_map(|(_, line)| &line.message.item_links)
            .find(|link| link.item_id != 0 && link.item_id != 0xfffff)
            .cloned()
    } else {
        None
    };
    if automatic.is_some() {
        *attempted = true;
    }
    let shift =
        keys.is_some_and(|keys| keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
    for button in buttons.iter() {
        if shift && let Some(typing) = typing.as_mut() {
            let composed = button
                .arrival
                .ok_or("This item link is not a received chat link")
                .and_then(|arrival| chat.compose_clicked_link(arrival, &online));
            match composed {
                Ok(()) => typing.composing = true,
                Err(reason) => chat.refuse(reason),
            }
        } else if let Err(reason) = state.inspect_link(&button.link, &online, &sender) {
            chat.refuse(reason);
        }
    }
    if let Some(link) = automatic.as_ref()
        && let Err(reason) = state.inspect_link(link, &online, &sender)
    {
        chat.refuse(reason);
    }
}

/// Raises a new selection or its received details, even while the panel is open.
pub(super) fn raise(mut state: ResMut<ItemState>, mut stack: ResMut<super::windows::Stack>) {
    if !state.raise_on_present {
        return;
    }
    state.raise_on_present = false;
    if state.selected.is_some() {
        stack.raise(super::windows::WindowId::Item);
    }
}

/// Shows only received statistics; a missing response never becomes fabricated stats.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    (state, options): (Res<ItemState>, Res<super::options::OptionsState>),
    mut panels: Query<&mut Node, With<ItemPanel>>,
    mut texts: Query<&mut Text, With<ItemText>>,
    mut titles: Query<&mut Text, (With<ItemTitle>, Without<ItemText>)>,
) {
    for mut panel in &mut panels {
        panel.display = if state.selected.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    let Some((_, name)) = &state.selected else {
        return;
    };
    // The skin's window names the item on its title bar (inferred: the
    // skin gives the window no words of its own there).
    for mut title in &mut titles {
        if title.0 != *name {
            title.0.clone_from(name);
        }
    }
    let text = state.definition().map_or_else(
        || format!("{name}\n\n{}", state.status),
        |item| {
            let mut properties = item
                .stats
                .iter()
                .map(|s| format!("{}: {}", s.label, s.value))
                .collect::<Vec<_>>();
            if options.options.qol.on(Fix::WeaponRatio)
                && let Some((under, ratio)) = weapon_ratio(item)
            {
                properties.insert(under, ratio);
            }
            let properties = properties.join("\n");
            format!(
                "{}\n{}\nWeight: {}.{}\n\n{}\n\nClasses: {}\nRaces: {}\nSlots: {}",
                item.name,
                item.flags.join(" / "),
                item.weight_tenths / 10,
                item.weight_tenths % 10,
                properties,
                mask(
                    item.classes,
                    &[
                        "WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD", "ROG", "SHM",
                        "NEC", "WIZ", "MAG", "ENC", "BST", "BER"
                    ]
                ),
                mask(
                    item.races,
                    &[
                        "HUM", "BAR", "ERU", "ELF", "HIE", "DEF", "HEF", "DWF", "TRL", "OGR",
                        "HFL", "GNM", "IKS", "VAH", "FRG", "DRK"
                    ]
                ),
                mask(
                    item.slots,
                    &[
                        "Charm",
                        "Ear",
                        "Head",
                        "Face",
                        "Ear",
                        "Neck",
                        "Shoulders",
                        "Arms",
                        "Back",
                        "Wrist",
                        "Wrist",
                        "Ranged",
                        "Hands",
                        "Primary",
                        "Secondary",
                        "Finger",
                        "Finger",
                        "Chest",
                        "Legs",
                        "Feet",
                        "Waist",
                        "Ammo"
                    ]
                )
            )
        },
    );
    for mut label in &mut texts {
        if label.0 != text {
            label.0.clone_from(&text);
        }
    }
}
/// A weapon's damage over its delay, the ratio players weigh weapons by, and
/// the line of its statistics it goes under: the later of those two
/// ([`Fix::WeaponRatio`]).
fn weapon_ratio(item: &ItemDetails) -> Option<(usize, String)> {
    let at = |label: &str| item.stats.iter().position(|stat| stat.label == label);
    let (damage, delay) = (at("Damage")?, at("Delay")?);
    let (damage_value, delay_value) = (item.stats[damage].value, item.stats[delay].value);
    (damage_value > 0 && delay_value > 0).then(|| {
        let ratio = f64::from(damage_value) / f64::from(delay_value);
        (damage.max(delay) + 1, format!("Ratio: {ratio:.3}"))
    })
}

/// The picture the item's box last showed, if any.
type Shown<'s> = Local<'s, Option<Option<u32>>>;

/// Draws the shown item's picture in the skin's box, when the item or its
/// picture changes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn icon(
    mut commands: Commands,
    state: Res<ItemState>,
    mut art: crate::sheets::Art,
    (mut last, added): (Shown, Query<(), Added<ItemIcon>>),
    boxes: Query<(Entity, &ItemIcon, Option<&Children>)>,
    old: Query<(), With<ItemIconPicture>>,
) {
    let shown = state.icon();
    if *last == Some(shown) && added.is_empty() {
        return;
    }
    *last = Some(shown);
    for (entity, place, children) in &boxes {
        for child in children.into_iter().flatten() {
            if old.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        if let Some(image) = shown.and_then(|icon| art.item(icon)) {
            commands.spawn((
                ItemIconPicture,
                image,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(place.x),
                    top: px(place.y),
                    width: px(place.size),
                    height: px(place.size),
                    ..default()
                },
                ChildOf(entity),
            ));
        }
    }
}

fn mask(bits: u32, labels: &[&str]) -> String {
    let known = (1u32 << labels.len()) - 1;
    if bits & known == known {
        return "ALL".into();
    }
    let mut names = Vec::new();
    for (bit, label) in labels.iter().enumerate() {
        if bits & (1 << bit) != 0 && !names.contains(label) {
            names.push(*label);
        }
    }
    if names.is_empty() {
        "None".into()
    } else {
        names.join(" ")
    }
}

/// Renders item and web links within the same wrapping text paragraph,
/// returning the paragraph.
pub(super) fn spawn_message(
    parent: &mut ChildSpawnerCommands,
    message: &eq_client_core::chat::Message,
    arrival: Option<u64>,
    color: Color,
    size: Size,
) -> Entity {
    parent
        .spawn((
            Text::default(),
            theme::font(size),
            TextColor(color),
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .with_children(|text| {
            let mut end = 0;
            for (ordinal, link) in message.item_links.iter().enumerate() {
                if link.text_start < end
                    || message.text.get(end..link.text_start).is_none()
                    || message.text.get(link.text_start..link.text_end).is_none()
                {
                    continue;
                }
                super::chat::links::spawn_text(
                    text,
                    &message.text[end..link.text_start],
                    color,
                    size,
                );
                text.spawn((
                    TextSpan::new(&message.text[link.text_start..link.text_end]),
                    theme::font(size),
                    TextColor(theme::LINK),
                    ItemButton {
                        link: link.clone(),
                        arrival: arrival.map(|id| (id, ordinal)),
                    },
                    Interaction::None,
                ));
                end = link.text_end;
            }
            super::chat::links::spawn_text(text, &message.text[end..], color, size);
        })
        .id()
}

/// Hit-tests the actual shaped text runs, including each wrapped part of a link.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(super) fn link_input(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    paragraphs: Query<
        (
            Entity,
            &UiGlobalTransform,
            &ComputedNode,
            &bevy::text::TextLayoutInfo,
            &bevy::text::ComputedTextBlock,
            &InheritedVisibility,
            Option<&bevy::ui::CalculatedClip>,
        ),
        With<super::chat::LineId>,
    >,
    mut links: Query<&mut Interaction, Or<(With<ItemButton>, With<super::chat::links::WebLink>)>>,
    viewports: Query<(), With<super::chat::Viewport>>,
    parents: Query<&ChildOf>,
    pointer: super::windows::pointer::PointerUi,
) {
    for mut interaction in &mut links {
        if *interaction != Interaction::None {
            *interaction = Interaction::None;
        }
    }
    let Some(cursor) = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position)
    else {
        return;
    };
    // Only the viewport in front can receive a link click.
    let Some(viewport) = pointer
        .top_at(cursor)
        .filter(|hit| viewports.contains(*hit))
    else {
        return;
    };
    for (paragraph, transform, node, layout, block, visible, clip) in &paragraphs {
        if !visible.get()
            || node.is_empty()
            || clip.is_some_and(|c| !c.clip.contains(cursor))
            || parents
                .iter_ancestors(paragraph)
                .find(|ancestor| viewports.contains(*ancestor))
                != Some(viewport)
        {
            continue;
        }
        let Some(inverse) = transform.try_inverse() else {
            continue;
        };
        // Match Bevy's decoration transform: physical UI coordinates, relative to content origin.
        let point = inverse.transform_point2(cursor) - node.content_box().min;
        for section in hit_sections(&layout.run_geometry, point) {
            if let Some(span) = block.entities().get(section)
                && let Ok(mut interaction) = links.get_mut(span.entity)
            {
                *interaction = if mouse.just_pressed(MouseButton::Left) {
                    Interaction::Pressed
                } else {
                    Interaction::Hovered
                };
                return;
            }
        }
    }
}

fn hit_sections(runs: &[bevy::text::RunGeometry], point: Vec2) -> impl Iterator<Item = usize> + '_ {
    runs.iter()
        .filter(move |run| run.bounds.contains(point))
        .map(|run| run.section_index)
}

/// Scrolls long definitions while keeping the camera still beneath the panel.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut panels: Query<
        (
            Entity,
            &UiGlobalTransform,
            &ComputedNode,
            &mut ScrollPosition,
        ),
        With<ItemPanel>,
    >,
    wheel: Res<super::windows::pointer::Wheel>,
    mut state: ResMut<ItemState>,
) {
    state.hovered = false;
    let Some(cursor) = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position)
    else {
        return;
    };
    if state.selected.is_none() {
        return;
    }
    for (entity, transform, node, mut position) in &mut panels {
        if super::windows::contains(cursor, transform, node) {
            state.hovered = true;
        }
        if wheel.surface == Some(entity) {
            super::windows::scroll_by(&mut position, node, wheel.pixels);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u32) -> ItemDetails {
        let mut item = crate::preview::items()[0].details.clone();
        item.id = id;
        item
    }

    fn link(id: u32) -> ItemLink {
        let text = format!("Item {id}");
        ItemLink {
            body: format!("0{id:05X}{:031X}1234ABCD", 0),
            item_id: id,
            start: 0,
            end: 0,
            text_start: 0,
            text_end: text.len(),
            text,
        }
    }

    #[test]
    fn a_cached_inspection_can_replace_the_panel_without_replacing_the_pending_request() {
        use crate::online::testing;
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let sender = Outbox::new(Some(sender));
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 77, testing::player(1));
        let mut state = ItemState::default();
        let cached = link(42);
        let waiting = link(43);
        state.inspect_link(&cached, &online, &sender).unwrap();
        receiver.try_recv().unwrap();
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::ItemDetails(item(42))],
        );
        state.observe(online.world());
        state.inspect_link(&waiting, &online, &sender).unwrap();
        receiver.try_recv().unwrap();
        // Closing the panel leaves its wire request outstanding.
        state.selected = None;
        state.shown = None;
        state.inspect_link(&cached, &online, &sender).unwrap();
        assert!(receiver.try_recv().is_err());
        assert_eq!(state.inspection(), Some((42, true)));

        let mut variant = cached.clone();
        variant.body.replace_range(6..11, "00001");
        assert_eq!(
            state.inspect_link(&variant, &online, &sender),
            Err("Wait for the current item inspection")
        );
        assert!(state.inspect_link(&link(44), &online, &sender).is_err());
        assert!(receiver.try_recv().is_err());
        state.pending.as_mut().unwrap().since -= Duration::from_secs(9);
        state.observe(online.world());
        assert_eq!(state.status, "");
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::ItemDetails(item(43))],
        );
        state.observe(online.world());
        assert_eq!(state.inspection(), Some((42, true)));
        assert!(state.pending.is_none());

        state.inspect_link(&variant, &online, &sender).unwrap();
        receiver.try_recv().unwrap();
        state.observe(online.world());
        assert_eq!(state.inspection(), Some((42, false)));
        assert!(state.pending.is_some());
    }

    #[test]
    fn an_untagged_reply_cannot_choose_between_different_bodies_for_the_same_item() {
        use crate::online::testing;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let sender = Outbox::new(Some(sender));
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 77, testing::player(1));
        let mut state = ItemState::default();
        let first = link(42);
        let mut other = first.clone();
        other.body.replace_range(6..11, "00001");
        state.inspect_link(&first, &online, &sender).unwrap();
        receiver.try_recv().unwrap();
        state.pending.as_mut().unwrap().since -= Duration::from_secs(9);
        state.inspect_link(&other, &online, &sender).unwrap();
        receiver.try_recv().unwrap();
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::ItemDetails(item(42))],
        );
        state.observe(online.world());
        assert!(online.world().item(42).is_some());
        assert!(state.pending.is_some());
        assert_eq!(state.inspection(), Some((42, false)));
        assert_eq!(state.status, VARIANT_UNCONFIRMED);
        assert!(!state.inspected.contains_key(&42));
        assert!(state.inspect_link(&other, &online, &sender).is_err());
        assert!(receiver.try_recv().is_err());
        state.pending.as_mut().unwrap().since -= Duration::from_secs(9);
        state.inspect_link(&other, &online, &sender).unwrap();
        assert!(
            matches!(receiver.try_recv(), Ok(ClientCommand::InspectItem { link_body, .. }) if link_body == other.body)
        );
    }

    #[test]
    fn a_weapon_shows_its_ratio_under_its_damage_and_delay() {
        let stat = |label: &str, value| eq_client_core::ItemStat {
            label: label.into(),
            value,
        };
        let mut weapon = item(1);
        weapon.stats = vec![
            stat("STR", 2),
            stat("Delay", 30),
            stat("Range", 10),
            stat("Damage", 9),
            stat("Haste", 5),
        ];
        assert_eq!(weapon_ratio(&weapon), Some((4, "Ratio: 0.300".into())));
        // Neither half alone, nor a zero, makes a ratio.
        weapon.stats.retain(|stat| stat.label != "Damage");
        assert_eq!(weapon_ratio(&weapon), None);
        weapon.stats.push(stat("Damage", 0));
        assert_eq!(weapon_ratio(&weapon), None);
    }

    #[test]
    fn an_item_shows_the_picture_its_definition_carries() {
        let mut state = ItemState::default();
        let mut held = item(1);
        held.icon = Some(640);
        state.open_received(held);
        assert_eq!(state.icon(), Some(640));
        let mut linked = item(2);
        linked.icon = None;
        state.open_received(linked);
        assert_eq!(state.icon(), None);
    }

    #[test]
    fn the_definition_on_screen_belongs_to_the_chosen_item() {
        let mut state = ItemState::default();
        state.open_received(item(1));
        assert_eq!(state.definition().map(|item| item.id), Some(1));
        state.selected = Some((2, "Another".into()));
        assert!(state.definition().is_none());
    }
    #[test]
    fn wrapped_link_uses_each_line_without_linking_the_gap() {
        let runs = [
            bevy::text::RunGeometry {
                section_index: 1,
                bounds: Rect::new(0.0, 0.0, 40.0, 12.0),
                ..default()
            },
            bevy::text::RunGeometry {
                section_index: 2,
                bounds: Rect::new(40.0, 0.0, 80.0, 12.0),
                ..default()
            },
            bevy::text::RunGeometry {
                section_index: 2,
                bounds: Rect::new(0.0, 12.0, 25.0, 24.0),
                ..default()
            },
        ];
        assert_eq!(
            hit_sections(&runs, Vec2::new(50.0, 6.0)).collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(
            hit_sections(&runs, Vec2::new(10.0, 18.0)).collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(
            hit_sections(&runs, Vec2::new(10.0, 6.0)).collect::<Vec<_>>(),
            [1]
        );
        assert!(hit_sections(&runs, Vec2::new(50.0, 18.0)).next().is_none());
    }

    #[test]
    fn inline_link_preserves_unicode_and_message_order_without_duplicate_labels() {
        let mut app = App::new();
        app.add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(|parent| {
                spawn_message(
                    parent,
                    &eq_client_core::chat::Message {
                        message: None,
                        message_hex: None,
                        text: "WTS Épée now".into(),
                        item_links: vec![ItemLink {
                            body: "synthetic".into(),
                            text: "Épée".into(),
                            item_id: 42,
                            start: 0,
                            end: 0,
                            text_start: 4,
                            text_end: 10,
                        }],
                    },
                    Some(15),
                    Color::WHITE,
                    Size::Label,
                );
            });
        });
        app.update();
        let world = app.world_mut();
        let mut paragraphs = world.query::<(&Text, &Children)>();
        let (text, children) = paragraphs.single(world).unwrap();
        let mut combined = text.0.clone();
        for child in children {
            combined.push_str(&world.get::<TextSpan>(*child).unwrap().0);
        }
        assert_eq!(combined, "WTS Épée now");
        let mut links = world.query::<(&ItemButton, &TextColor)>();
        let (link, color) = links.single(world).unwrap();
        assert_eq!(link.link.item_id, 42);
        assert_eq!(link.arrival, Some((15, 0)));
        assert_eq!(color.0, crate::theme::LINK);
    }

    #[test]
    fn clicking_an_item_queues_inspection_and_reconnect_clears_the_panel() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let mut app = crate::testing::app();
        let mut online = OnlineState::new(true);
        crate::online::testing::admit(&mut online, 77, crate::online::testing::player(1));
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, input);
        let body = format!("00002A{}1234ABCD", "0".repeat(31));
        let link = app
            .world_mut()
            .spawn((
                Interaction::None,
                ItemButton {
                    link: ItemLink {
                        body: body.clone(),
                        text: "Synthetic blade".into(),
                        item_id: 42,
                        start: 0,
                        end: 0,
                        text_start: 0,
                        text_end: 0,
                    },
                    arrival: None,
                },
            ))
            .id();
        crate::clicks::testing::click(&mut app, link);
        assert_eq!(
            receiver.try_recv().unwrap(),
            ClientCommand::InspectItem {
                session_id: 77,
                link_body: body
            }
        );
        assert_eq!(
            app.world()
                .resource::<ItemState>()
                .selected
                .as_ref()
                .unwrap()
                .0,
            42
        );
        app.update();
        assert!(receiver.try_recv().is_err());
        // The server's answer reaches the panel through the world.
        let mut answer = item(42);
        answer.name = "Synthetic blade".into();
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::ItemDetails(answer)],
        );
        app.update();
        let state = app.world().resource::<ItemState>();
        assert_eq!(
            state.definition().map(|item| item.name.as_str()),
            Some("Synthetic blade")
        );
        assert!(state.pending.is_none());
    }
}
