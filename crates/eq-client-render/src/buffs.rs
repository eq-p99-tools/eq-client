//! Buff display and explicit right-click cancellation of a beneficial buff
//! in a received or modelled slot.
use super::{online::OnlineState, spell_icons, spellbook::SpellNames, windows};
use crate::{
    skinned::Shows,
    theme::{self, Size},
};
use bevy::prelude::*;
use eq_client_core::{
    buffs::{
        BuffTracker, Shown, TimeLeft,
        cancel::{Placement, Request},
    },
    qol::Fix,
};
use std::{collections::BTreeMap, time::Instant};

#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct Body;
#[derive(Component)]
pub(super) struct Entry(u32);
#[derive(Component)]
pub(super) struct Unplaced(u16);
#[derive(Component)]
pub(super) struct Hint;

/// A right press binds the effect and its available cancellation evidence.
/// Releasing over a replacement, another control or another admission drops it.
#[derive(Resource, Default)]
pub(crate) struct BuffPress(Option<(Entity, Selection)>);

impl BuffPress {
    /// Discards a canceled script's press before a native release can use it.
    pub(crate) fn cancel(&mut self) {
        self.0 = None;
    }
}

enum Selection {
    Slot(Request),
    Unplaced {
        session_id: u64,
        spell: u16,
        landed: Instant,
    },
}

impl Selection {
    fn from_shown(
        shown: Shown<'_>,
        world: &eq_client_core::world::ClientWorld,
        names: &SpellNames,
        outbox: &crate::outbox::Outbox,
    ) -> Option<Self> {
        Some(match shown {
            Shown::Slot(slot, buff) => {
                let stamp = outbox.peek(world)?;
                Self::Slot(Request {
                    session_id: stamp.session_id,
                    revision: world.buffs().revision(),
                    slot,
                    spell_id: buff.spell_id,
                    beneficial: names.beneficial(buff.spell_id),
                    cancellable: cancellable(names, buff.spell_id),
                    created: stamp.created,
                    placement: eq_client_core::buffs::cancel::Placement::Received,
                })
            }
            Shown::Modelled(slot, effect) => {
                let stamp = outbox.peek(world)?;
                let spell_id = u32::from(effect.spell_id);
                Self::Slot(Request {
                    session_id: stamp.session_id,
                    revision: world.buffs().revision(),
                    slot,
                    spell_id,
                    beneficial: names.beneficial(spell_id),
                    cancellable: cancellable(names, spell_id),
                    created: stamp.created,
                    placement: Placement::Modelled(
                        world.buffs().modelled_record(effect.spell_id)?.clone(),
                    ),
                })
            }
            Shown::Unplaced(effect) => Self::Unplaced {
                session_id: world.session_id()?,
                spell: effect.spell_id,
                landed: world.buffs().landed(effect.spell_id)?,
            },
        })
    }
}

fn shown_buff<'a>(
    buffs: &'a BuffTracker,
    entry: Option<&Entry>,
    unplaced: Option<&Unplaced>,
    shows: Option<&Shows>,
) -> Option<Shown<'a>> {
    if let Some(&Entry(slot)) = entry {
        return buffs
            .slots()?
            .get(&slot)
            .map(|buff| Shown::Slot(slot, buff));
    }
    if let Some(&Unplaced(spell)) = unplaced {
        // A landing the program would slot shows at its modelled slot.
        let effect = buffs.effects().get(&spell)?;
        return Some(
            buffs
                .slot_of(spell)
                .map_or(Shown::Unplaced(effect), |slot| {
                    Shown::Modelled(slot, effect)
                }),
        );
    }
    let &Shows::Buff(window, button) = shows? else {
        return None;
    };
    buffs.in_window(window, button)
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::too_many_arguments
)]
pub(super) fn cancel(
    mouse: Res<ButtonInput<MouseButton>>,
    online: Res<OnlineState>,
    names: Res<SpellNames>,
    outbox: Res<crate::outbox::Outbox>,
    buttons: Query<(
        Entity,
        &Interaction,
        Option<&Entry>,
        Option<&Unplaced>,
        Option<&Shows>,
    )>,
    mut selected: ResMut<BuffPress>,
    mut chat: ResMut<crate::chat::ChatState>,
) {
    let world = online.world();
    if mouse.just_pressed(MouseButton::Right) {
        selected.cancel();
        for (entity, interaction, entry, unplaced, shows) in &buttons {
            if *interaction == Interaction::None {
                continue;
            }
            let Some(selection) = shown_buff(world.buffs(), entry, unplaced, shows)
                .and_then(|buff| Selection::from_shown(buff, world, &names, &outbox))
            else {
                continue;
            };
            selected.0 = Some((entity, selection));
            break;
        }
    }
    if mouse.just_released(MouseButton::Right) {
        let Some((entity, selection)) = selected.0.take() else {
            return;
        };
        let Ok((_, interaction, entry, unplaced, shows)) = buttons.get(entity) else {
            return;
        };
        if *interaction == Interaction::None {
            return;
        }
        let request = match selection {
            Selection::Slot(request) => request,
            Selection::Unplaced {
                session_id,
                spell,
                landed,
            } => {
                // Only a buff the player could otherwise cancel is refused
                // for its unknown slot; others stay silent, as a slotted
                // harmful or protected effect would be refused for its kind.
                if world.session_id() == Some(session_id)
                    && names.beneficial(u32::from(spell)) == Some(true)
                    && cancellable(&names, u32::from(spell)) == Some(true)
                    && world.buffs().landed(spell) == Some(landed)
                    && matches!(
                        shown_buff(world.buffs(), entry, unplaced, shows),
                        Some(Shown::Unplaced(effect)) if effect.spell_id == spell
                    )
                {
                    chat.refuse(format!(
                        "Cannot cancel {}: its current buff slot is unknown.",
                        names.label(u32::from(spell))
                    ));
                }
                return;
            }
        };
        let check = (|| {
            anyhow::ensure!(
                world.session_id() == Some(request.session_id),
                "The zone visit changed"
            );
            anyhow::ensure!(
                names.beneficial(request.spell_id) == request.beneficial,
                "Spell data changed; select the buff again"
            );
            anyhow::ensure!(
                cancellable(&names, request.spell_id) == request.cancellable,
                "Spell effects changed; select the buff again"
            );
            if let Placement::Modelled(record) = &request.placement {
                let spell = u16::try_from(request.spell_id)?;
                anyhow::ensure!(
                    world.buffs().slot_of(spell) == Some(request.slot)
                        && world.buffs().modelled_record(spell) == Some(record),
                    "The selected buff slot changed or is unavailable"
                );
            }
            request.check(world.buffs().revision(), world.buffs().slots())
        })();
        if let Err(error) = check {
            chat.refuse(format!("Buff cancellation refused: {error}"));
            return;
        }
        let _ = outbox.send(world, eq_client_core::ClientCommand::CancelBuff(request));
    }
    if !mouse.pressed(MouseButton::Right) {
        selected.cancel();
    }
}

/// Ordinary cancellation excludes effect 101.
/// Missing mechanics never become permission to cancel.
fn cancellable(names: &SpellNames, spell: u32) -> Option<bool> {
    Some(
        !names
            .mechanics(spell)?
            .effects
            .iter()
            .any(|effect| effect.id == 101),
    )
}

/// Rebuilds only buff content, keeping the title bar and its saved layout
/// intact. Where the skin draws the window, it keeps only the frame, which
/// the skin fills.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    online: Res<OnlineState>,
    (shown, skinned): (Res<windows::Shown>, Res<crate::skinned::Skinned>),
    mut panels: Query<(Entity, &mut Node), With<Panel>>,
    bodies: Query<Entity, With<Body>>,
    mut previous: Local<Option<BTreeMap<u32, eq_client_core::Buff>>>,
    mut previous_effects: Local<BTreeMap<u16, eq_client_core::SpellEffect>>,
    mut previous_modelled: Local<BTreeMap<u32, u16>>,
) {
    // The player closes and opens the window from the selector.
    let display = if shown.is_open(windows::WindowId::Effects) {
        Display::Flex
    } else {
        Display::None
    };
    for (_, mut node) in &mut panels {
        if node.display != display {
            node.display = display;
        }
    }
    let Some(buffs) = online.world().buffs().slots() else {
        for (entity, _) in &panels {
            commands.entity(entity).despawn();
        }
        *previous = None;
        previous_effects.clear();
        previous_modelled.clear();
        return;
    };
    if previous.as_ref() == Some(buffs)
        && *previous_effects == *online.world().buffs().effects()
        && *previous_modelled == *online.world().buffs().modelled()
        && !panels.is_empty()
    {
        return;
    }
    *previous = Some(buffs.clone());
    previous_effects.clone_from(online.world().buffs().effects());
    previous_modelled.clone_from(online.world().buffs().modelled());
    if skinned.has(windows::WindowId::Effects) {
        if panels.is_empty() {
            let frame = windows::frame(
                &mut commands,
                windows::WindowId::Effects,
                Node {
                    display,
                    ..default()
                },
            );
            commands.entity(frame).insert(Panel);
        }
        return;
    }
    let body = if let Ok(body) = bodies.single() {
        commands.entity(body).despawn_children();
        body
    } else {
        let frame = windows::frame(
            &mut commands,
            windows::WindowId::Effects,
            Node {
                width: px(242),
                padding: UiRect::all(px(6)),
                row_gap: px(4),
                flex_direction: FlexDirection::Column,
                display,
                ..default()
            },
        );
        commands.entity(frame).insert(Panel);
        let body = commands
            .spawn((
                Body,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                },
            ))
            .id();
        commands.entity(frame).add_child(body);
        body
    };
    content(&mut commands, body, buffs, online.world().buffs());
}

/// Builds icons from occupied server slots without renumbering holes.
/// The own list's order: received and modelled buffs together in slot
/// order, as the program holds them, a modelled one by its spell; then
/// those without a slot, as they landed.
fn drawn_order(
    buffs: &BTreeMap<u32, eq_client_core::Buff>,
    tracker: &eq_client_core::buffs::BuffTracker,
) -> (Vec<(u32, Option<u16>)>, Vec<u16>) {
    let mut slotted: Vec<(u32, Option<u16>)> = buffs.keys().map(|slot| (*slot, None)).collect();
    slotted.extend(
        tracker
            .modelled()
            .iter()
            .filter(|(slot, _)| !buffs.contains_key(slot))
            .map(|(slot, spell)| (*slot, Some(*spell))),
    );
    slotted.sort_unstable_by_key(|(slot, _)| *slot);
    let unslotted = tracker
        .unslotted()
        .map(|(effect, _)| effect.spell_id)
        .collect();
    (slotted, unslotted)
}

fn content(
    commands: &mut Commands,
    body: Entity,
    buffs: &BTreeMap<u32, eq_client_core::Buff>,
    tracker: &eq_client_core::buffs::BuffTracker,
) {
    let long_slots = tracker.long_slots();
    let shows = |slot: u32| {
        crate::skinned::Shows::Buff(
            if slot < long_slots {
                eq_client_core::buffs::EffectWindow::Long
            } else {
                eq_client_core::buffs::EffectWindow::Short
            },
            if slot < long_slots {
                slot
            } else {
                slot - long_slots
            },
        )
    };
    let button = || {
        (
            Button,
            Node {
                width: px(42),
                height: px(42),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(theme::EDGE),
            BackgroundColor(theme::INSET),
        )
    };
    let caption = |slot: u32| {
        (
            Text::new((u64::from(slot) + 1).to_string()),
            theme::font(Size::Caption),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                right: px(2),
                ..default()
            },
        )
    };
    let (slotted, unslotted) = drawn_order(buffs, tracker);
    commands.entity(body).with_children(|parent| {
        parent
            .spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(3),
                row_gap: px(3),
                ..default()
            })
            .with_children(|grid| {
                for &(slot, modelled) in &slotted {
                    match modelled {
                        None => grid
                            .spawn((Entry(slot), shows(slot), button()))
                            .with_children(|icon| {
                                icon.spawn(spell_icons::artwork(
                                    spell_icons::Source::Buff(slot),
                                    36.0,
                                ));
                                icon.spawn(caption(slot));
                            }),
                        Some(spell) => grid
                            .spawn((Unplaced(spell), shows(slot), button()))
                            .with_children(|icon| {
                                icon.spawn(spell_icons::artwork(
                                    spell_icons::Source::Effect(spell),
                                    36.0,
                                ));
                                icon.spawn(caption(slot));
                            }),
                    };
                }
                for &spell in &unslotted {
                    grid.spawn((Unplaced(spell), button()))
                        .with_children(|icon| {
                            icon.spawn(spell_icons::artwork(
                                spell_icons::Source::Effect(spell),
                                36.0,
                            ));
                        });
                }
            });
        parent.spawn((
            Hint,
            Text::new(if slotted.is_empty() && unslotted.is_empty() {
                "No active buffs"
            } else {
                "Hover an effect for details"
            }),
            theme::font(Size::Body),
            TextColor(theme::INK),
        ));
    });
}

/// Says what the hovered buff is and how long it has left, about, where the
/// quality-of-life fix counts it down ([`Fix::BuffTimeLeft`]), and otherwise
/// the duration the server gave without pretending it is a countdown.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn hover(
    (online, options): (Res<OnlineState>, Res<crate::options::OptionsState>),
    names: Res<SpellNames>,
    entries: Query<(&Entry, &Interaction)>,
    effects: Query<(&Unplaced, &Interaction)>,
    mut hints: Query<&mut Text, With<Hint>>,
) {
    let now = Instant::now();
    let qol = &options.options.qol;
    let buffs = online.world().buffs();
    let hovered = entries
        .iter()
        .find(|(_, interaction)| **interaction != Interaction::None)
        .and_then(|(entry, _)| Some((entry.0, buffs.slots()?.get(&entry.0)?)));
    let text = hovered.map_or_else(
        || {
            if let Some((entry, _)) = effects
                .iter()
                .find(|(_, interaction)| **interaction != Interaction::None)
            {
                let effect = buffs.effects().get(&entry.0);
                let landed = buffs.landed(entry.0);
                return effect_details(entry.0, (effect, landed), &names, (qol, now));
            }
            if online
                .world()
                .buffs()
                .slots()
                .is_some_and(BTreeMap::is_empty)
                && online.world().buffs().effects().is_empty()
            {
                "No active buffs".into()
            } else {
                "Hover an effect for details".into()
            }
        },
        |(slot, buff)| details(buff, buffs.time_left(slot, now), &names, qol),
    );
    for mut hint in &mut hints {
        if hint.0 != text {
            hint.0.clone_from(&text);
        }
    }
}

/// What an effect without a slot is: its spell, its duration, counted down
/// from when it landed where the quality-of-life fix counts buffs down, and
/// its bonuses as the spell file has them for the caster's level.
fn effect_details(
    spell: u16,
    (effect, landed): (Option<&eq_client_core::SpellEffect>, Option<Instant>),
    names: &SpellNames,
    (qol, now): (&eq_client_core::qol::Settings, Instant),
) -> String {
    let spell = u32::from(spell);
    let duration =
        effect.and_then(|effect| names.mechanics(spell)?.base_duration(effect.caster_level));
    let level = effect.map(|effect| effect.caster_level);
    let duration = match duration.zip(landed) {
        Some((duration, landed)) if qol.on(Fix::BuffTimeLeft) => {
            time_left_label(base_time_left(duration, landed, now))
        }
        _ => base_duration_label(duration),
    };
    format!(
        "{}\n{duration}{}",
        names.label(spell),
        resource_hint(names.mechanics(spell), level)
    )
}

/// What a buff in a slot is: its spell, how long it has left where the
/// quality-of-life fix counts buffs down, or else the duration the server
/// last gave, and its bonuses as the spell file has them.
fn details(
    buff: &eq_client_core::Buff,
    left: Option<TimeLeft>,
    names: &SpellNames,
    qol: &eq_client_core::qol::Settings,
) -> String {
    let duration = match left {
        Some(left) if qol.on(Fix::BuffTimeLeft) => time_left_label(left),
        _ => format!("Server duration: {} ticks", buff.duration_ticks),
    };
    format!(
        "{}\n{duration}{}",
        names.label(buff.spell_id),
        resource_hint(
            names.mechanics(buff.spell_id),
            Some(u16::from(buff.caster_level))
        )
    )
}

/// What is left at `now` of a spell's own duration, counted from when it
/// landed: the spell file's formula, as `EQEmu` reckons it, which the
/// server keeps to unless it gives the buff a slot of its own.
fn base_time_left(
    duration: eq_client_assets::spells::BaseDuration,
    landed: Instant,
    now: Instant,
) -> TimeLeft {
    use eq_client_assets::spells::BaseDuration;
    match duration {
        BaseDuration::Ticks(ticks) => TimeLeft::of_ticks(i64::from(ticks), landed, now),
        BaseDuration::Permanent | BaseDuration::Aura => TimeLeft::Lasting,
    }
}

/// How long a buff has left, in this client's words: about, since the
/// server's ticks do not keep to the client's clock.
fn time_left_label(left: TimeLeft) -> String {
    match left {
        TimeLeft::Lasting => "No time limit".into(),
        TimeLeft::About(left) if left.is_zero() => "About to fade".into(),
        TimeLeft::About(left) => {
            let seconds = left.as_secs();
            let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
            if hours > 0 {
                format!("About {hours}h {minutes}m left")
            } else if minutes > 0 {
                format!("About {minutes}m {seconds}s left")
            } else {
                format!("About {seconds}s left")
            }
        }
    }
}

/// Names the buff under the pointer in the skin's effects windows, as the
/// client's own window does.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn skinned_details(
    (online, options): (Res<OnlineState>, Res<crate::options::OptionsState>),
    names: Res<SpellNames>,
    mut buttons: Query<(
        &crate::skinned::Shows,
        &Interaction,
        &mut crate::tooltip::Tooltip,
    )>,
) {
    let now = Instant::now();
    let qol = &options.options.qol;
    let buffs = online.world().buffs();
    for (shows, interaction, mut tooltip) in &mut buttons {
        let crate::skinned::Shows::Buff(window, button) = *shows else {
            continue;
        };
        if *interaction == Interaction::None {
            continue;
        }
        let text = match buffs.in_window(window, button) {
            Some(eq_client_core::buffs::Shown::Slot(slot, buff)) => {
                details(buff, buffs.time_left(slot, now), &names, qol)
            }
            Some(
                eq_client_core::buffs::Shown::Modelled(_, effect)
                | eq_client_core::buffs::Shown::Unplaced(effect),
            ) => effect_details(
                effect.spell_id,
                (Some(effect), buffs.landed(effect.spell_id)),
                &names,
                (qol, now),
            ),
            None => String::new(),
        };
        if tooltip.0 != text {
            tooltip.0 = text;
        }
    }
}

/// Opens the skin's short effects window while the player has a short
/// effect, such as a song, and closes it with the last one. Without the
/// skin's effects window, or a client known to have the short one, the
/// client's own lists them with the rest.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn short_window(
    (online, settings): (Res<OnlineState>, Res<crate::ViewerSettings>),
    skinned: Res<crate::skinned::Skinned>,
    mut shown: ResMut<windows::Shown>,
) {
    let id = windows::WindowId::ShortEffects;
    // Only the client generations known to have the window open it.
    let wanted = settings.0.installed_client.short_effects()
        && skinned.has(windows::WindowId::Effects)
        && online.world().buffs().has_short();
    if wanted != shown.is_open(id) {
        if wanted {
            shown.open(id);
        } else {
            shown.close(id);
        }
    }
}

/// Shows unmodified local bonuses without presenting them as server-confirmed totals.
fn resource_hint(
    mechanics: Option<&eq_client_assets::spells::Mechanics>,
    level: Option<u16>,
) -> String {
    let Some((mechanics, level)) = mechanics.zip(level.filter(|level| *level != 0)) else {
        return "\nBonuses unavailable".into();
    };
    let projection = mechanics.resource_projection(level);
    let stats = projection.modifiers;
    let mut lines = Vec::new();
    let attributes = [
        ("STR", stats.strength),
        ("STA", stats.stamina),
        ("DEX", stats.dexterity),
        ("AGI", stats.agility),
        ("INT", stats.intelligence),
        ("WIS", stats.wisdom),
        ("CHA", stats.charisma),
    ]
    .into_iter()
    .filter(|(_, value)| *value != 0)
    .map(|(label, value)| format!("{label} {value:+}"))
    .collect::<Vec<_>>();
    if !attributes.is_empty() {
        lines.push(attributes.join(" | "));
    }
    for (label, value) in [
        ("Max HP", stats.hit_points),
        ("Max mana", stats.mana),
        ("Max endurance", stats.endurance),
    ] {
        if value != 0 {
            lines.push(format!("{label} {value:+}"));
        }
    }
    let mut hint = if lines.is_empty() {
        String::new()
    } else {
        format!("\nBase bonuses (estimate)\n{}", lines.join("\n"))
    };
    if !projection.unresolved.is_empty() {
        hint.push_str("\nSome effects unavailable");
    }
    hint
}

/// Keeps local formula estimates distinct from remaining time reported by the server.
fn base_duration_label(duration: Option<eq_client_assets::spells::BaseDuration>) -> String {
    use eq_client_assets::spells::BaseDuration;
    match duration {
        Some(BaseDuration::Ticks(ticks)) => {
            format!("Local base: {ticks} ticks\nRemaining time unavailable")
        }
        Some(BaseDuration::Permanent) => "Local base: permanent".into(),
        Some(BaseDuration::Aura) => "Local base: aura".into(),
        None => "Duration unavailable".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_buff(spell_id: u32) -> eq_client_core::Buff {
        eq_client_core::Buff {
            spell_id,
            caster_level: 12,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks: 30,
            counters: 0,
            caster_id: 7,
        }
    }

    /// Spell 42, "Constructed buff", a lasting spell with its band's short
    /// flag (field 154) as given.
    fn constructed_buff(beneficial: bool, short: &str) -> SpellNames {
        let mut fields = vec!["0"; 203];
        fields[0] = "42";
        fields[1] = "Constructed buff";
        fields[83] = if beneficial { "1" } else { "0" };
        fields[154] = short;
        // A lasting spell: formula 3, up to 30 ticks.
        fields[16] = "3";
        fields[17] = "30";
        SpellNames::parse(&fields.join("^"))
    }

    fn cancellation_fixture(
        skinned: bool,
        beneficial: Option<bool>,
    ) -> (
        App,
        Entity,
        std::sync::mpsc::Receiver<eq_client_core::ClientCommand>,
    ) {
        let mut app = crate::testing::app();
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let mut online = OnlineState::new(true);
        crate::online::testing::admit(&mut online, 5, crate::online::testing::player(7));
        crate::online::testing::buffs(&mut online, BTreeMap::from([(3, synthetic_buff(42))]));
        let names = beneficial.map_or_else(SpellNames::default, |beneficial| {
            constructed_buff(beneficial, "0")
        });
        app.insert_resource(online)
            .insert_resource(names)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, cancel);
        let mut button = app
            .world_mut()
            .spawn((Node::default(), Button, Interaction::Hovered));
        if skinned {
            button.insert(crate::skinned::Shows::Buff(
                eq_client_core::buffs::EffectWindow::Long,
                3,
            ));
        } else {
            button.insert(Entry(3));
        }
        let button_id = button.id();
        (app, button_id, receiver)
    }

    /// A Titanium table of 25 empty slots.
    fn clear_buffs(app: &mut App) {
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::BuffSnapshot(vec![None; 25])],
        );
    }

    /// Spell 42 taking hold on the player, with this flag, read with the
    /// app's spell data.
    fn lands(app: &mut App, effect_flag: u8) {
        let names = app.world_mut().remove_resource::<SpellNames>().unwrap();
        crate::online::testing::news_with(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::SpellEffect(
                eq_client_core::SpellEffect {
                    target_id: 7,
                    caster_id: 7,
                    caster_level: 12,
                    instrument_modifier: 10,
                    spell_id: 42,
                    spell_level: 12,
                    effect_flag,
                    push: None,
                },
            )],
            &names,
        );
        app.insert_resource(names);
    }

    #[test]
    fn right_click_a_modelled_landing_cancels_the_record_the_program_holds() {
        for skinned in [false, true] {
            let (mut app, button, receiver) = cancellation_fixture(skinned, Some(true));
            clear_buffs(&mut app);
            let mut button = app.world_mut().entity_mut(button);
            if skinned {
                button.insert(Shows::Buff(eq_client_core::buffs::EffectWindow::Long, 0));
            } else {
                button.remove::<Entry>().insert(Unplaced(42));
            }
            // A flag with other bits set besides 4 takes hold too.
            lands(&mut app, 0x14);
            right_press(&mut app);
            right_release(&mut app);
            let eq_client_core::ClientCommand::CancelBuff(request) = receiver.try_recv().unwrap()
            else {
                panic!("wrong command")
            };
            assert_eq!(
                (request.session_id, request.slot, request.spell_id),
                (5, 0, 42)
            );
            assert!(matches!(
                &request.placement,
                Placement::Modelled(record) if record.spell_id == 42
            ));
            assert!(receiver.try_recv().is_err());
        }
    }

    fn right_press(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear_just_pressed(MouseButton::Right);
    }

    fn right_release(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Right);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }

    #[test]
    fn right_click_submits_the_exact_current_beneficial_slot_in_both_presentations() {
        for skinned in [false, true] {
            let (mut app, _, receiver) = cancellation_fixture(skinned, Some(true));
            right_press(&mut app);
            assert!(receiver.try_recv().is_err());
            right_release(&mut app);
            let eq_client_core::ClientCommand::CancelBuff(request) = receiver.try_recv().unwrap()
            else {
                panic!("wrong command")
            };
            assert_eq!(
                (
                    request.session_id,
                    request.revision,
                    request.slot,
                    request.spell_id
                ),
                (5, 1, 3, 42)
            );
            assert_eq!(request.beneficial, Some(true));
            assert_eq!(
                app.world()
                    .resource::<OnlineState>()
                    .world()
                    .buffs()
                    .slots()
                    .unwrap()[&3]
                    .spell_id,
                42
            );
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn right_release_cannot_follow_replacement_admission_or_pointer_changes() {
        for change in 0..4 {
            let (mut app, button, receiver) = cancellation_fixture(true, Some(true));
            right_press(&mut app);
            match change {
                0 => crate::online::testing::buffs(
                    &mut app.world_mut().resource_mut::<OnlineState>(),
                    BTreeMap::from([(3, synthetic_buff(43))]),
                ),
                1 => crate::online::testing::admit(
                    &mut app.world_mut().resource_mut::<OnlineState>(),
                    6,
                    crate::online::testing::player(7),
                ),
                2 => *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None,
                _ => {
                    app.world_mut().insert_resource(SpellNames::default());
                }
            }
            right_release(&mut app);
            assert!(receiver.try_recv().is_err(), "change {change}");
        }
    }

    #[test]
    fn harmful_unknown_and_protected_buffs_never_submit_a_cancel() {
        for beneficial in [None, Some(false)] {
            let (mut app, _, receiver) = cancellation_fixture(true, beneficial);
            right_press(&mut app);
            right_release(&mut app);
            assert!(receiver.try_recv().is_err());
        }
        let (mut app, _, receiver) = cancellation_fixture(true, Some(true));
        let mut fields = vec!["0"; 203];
        fields[0] = "42";
        fields[1] = "Constructed blocked buff";
        fields[83] = "1";
        fields[86] = "101";
        app.insert_resource(SpellNames::parse(&fields.join("^")));
        right_press(&mut app);
        right_release(&mut app);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn right_click_an_unslotted_effect_explains_why_without_submitting() {
        for (skinned, beneficial) in [(false, true), (true, true), (false, false), (true, false)] {
            let (mut app, button, receiver) = cancellation_fixture(skinned, Some(beneficial));
            // Nothing holds the spell, and a malformed band flag leaves the
            // program's slot for its landing unknown.
            clear_buffs(&mut app);
            app.insert_resource(constructed_buff(beneficial, "bad"));
            let mut button = app.world_mut().entity_mut(button);
            if skinned {
                // The first empty long-window button shows the received effect,
                // but this presentation position is not an actual server slot.
                button.insert(Shows::Buff(eq_client_core::buffs::EffectWindow::Long, 0));
            } else {
                button.remove::<Entry>().insert(Unplaced(42));
            }
            lands(&mut app, 4);
            right_press(&mut app);
            assert_eq!(
                app.world().resource::<crate::chat::ChatState>().newest(),
                ""
            );
            right_release(&mut app);
            // A harmful effect could not be cancelled anyway, so its slot is
            // not offered as the reason.
            assert_eq!(
                app.world().resource::<crate::chat::ChatState>().newest(),
                if beneficial {
                    "Cannot cancel Constructed buff: its current buff slot is unknown."
                } else {
                    ""
                }
            );
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn a_buff_says_about_how_long_it_has_left() {
        use std::time::Duration;
        let about = |seconds| time_left_label(TimeLeft::About(Duration::from_secs(seconds)));
        assert_eq!(about(42), "About 42s left");
        assert_eq!(about(270), "About 4m 30s left");
        assert_eq!(about(4385), "About 1h 13m left");
        assert_eq!(about(0), "About to fade");
        assert_eq!(time_left_label(TimeLeft::Lasting), "No time limit");
        // A buff in a slot counts down from the server's ticks.
        let names = SpellNames::default();
        let qol = eq_client_core::qol::Settings::default();
        let buff = eq_client_core::Buff {
            spell_id: 42,
            caster_level: 0,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks: 10,
            counters: 0,
            caster_id: 7,
        };
        let left = Some(TimeLeft::About(Duration::from_secs(45)));
        let text = details(&buff, left, &names, &qol);
        assert_eq!(text.lines().nth(1), Some("About 45s left"));
        // An effect without a slot counts down its spell's own duration
        // from when it landed.
        let now = Instant::now();
        let landed = now.checked_sub(Duration::from_secs(20)).unwrap();
        let ticks = |count| eq_client_assets::spells::BaseDuration::Ticks(count);
        assert_eq!(
            base_time_left(ticks(10), landed, now),
            TimeLeft::About(Duration::from_secs(40))
        );
        assert_eq!(
            base_time_left(eq_client_assets::spells::BaseDuration::Aura, landed, now),
            TimeLeft::Lasting
        );
    }

    #[test]
    fn bonus_hint_distinguishes_estimates_partial_effects_and_missing_levels() {
        let mut mechanics =
            eq_client_assets::spells::Mechanics::from_fields(&vec!["0"; 98]).unwrap();
        mechanics.effects[0] = eq_client_assets::spells::Effect {
            id: 9,
            base: -3,
            limit: 0,
            maximum: 0,
            formula: 100,
        };
        mechanics.effects[1] = eq_client_assets::spells::Effect {
            id: 97,
            base: 10,
            limit: 0,
            maximum: 0,
            formula: 102,
        };
        assert_eq!(
            resource_hint(Some(&mechanics), Some(2)),
            "\nBase bonuses (estimate)\nWIS -3\nMax mana +12"
        );
        mechanics.effects[2].id = 999;
        assert!(resource_hint(Some(&mechanics), Some(2)).ends_with("Some effects unavailable"));
        assert_eq!(
            resource_hint(Some(&mechanics), Some(0)),
            "\nBonuses unavailable"
        );
        assert_eq!(resource_hint(None, Some(2)), "\nBonuses unavailable");
        assert_eq!(
            resource_hint(Some(&mechanics), None),
            "\nBonuses unavailable"
        );
    }

    #[test]
    fn the_own_list_draws_received_and_modelled_buffs_in_slot_order_then_unslotted_ones() {
        let mut app = crate::testing::app();
        app.insert_resource({
            let mut online = OnlineState::new(true);
            crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
            online
        })
        .add_systems(Update, update);
        // 42 held in 1; 50 and 51 lasting with their bands; 52 with a
        // malformed band flag.
        let line = |id: &str, short: &str| {
            let mut fields = vec!["0"; 203];
            fields[0] = id;
            fields[1] = "Constructed";
            fields[16] = "3";
            fields[17] = "30";
            fields[154] = short;
            // No effects (254 in each of the twelve), so none could replace
            // another.
            fields[86..98].fill("254");
            fields.join("^")
        };
        let names = SpellNames::parse(
            &[
                line("42", "0"),
                line("50", "0"),
                line("51", "0"),
                line("52", "bad"),
            ]
            .join("\n"),
        );
        let mut table = vec![None; 25];
        table[1] = Some(synthetic_buff(42));
        let landing = |spell_id| {
            eq_client_core::WorldEvent::SpellEffect(eq_client_core::SpellEffect {
                target_id: 7,
                caster_id: 7,
                caster_level: 12,
                instrument_modifier: 10,
                spell_id,
                spell_level: 12,
                effect_flag: 4,
                push: None,
            })
        };
        crate::online::testing::news_with(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [
                eq_client_core::WorldEvent::BuffSnapshot(table),
                landing(50),
                landing(51),
                landing(52),
            ],
            &names,
        );
        app.update();
        let grid = app
            .world_mut()
            .query_filtered::<&ChildOf, With<Entry>>()
            .single(app.world())
            .unwrap()
            .parent();
        let children: Vec<Entity> = app.world().get::<Children>(grid).unwrap().to_vec();
        let drawn: Vec<(Option<u32>, Option<u16>, Option<String>)> = children
            .iter()
            .map(|child| {
                let caption = app
                    .world()
                    .get::<Children>(*child)
                    .into_iter()
                    .flatten()
                    .find_map(|part| app.world().get::<Text>(*part).map(|text| text.0.clone()));
                (
                    app.world().get::<Entry>(*child).map(|entry| entry.0),
                    app.world().get::<Unplaced>(*child).map(|effect| effect.0),
                    caption,
                )
            })
            .collect();
        assert_eq!(
            drawn,
            [
                (None, Some(50), Some("1".into())),
                (Some(1), None, Some("2".into())),
                (None, Some(51), Some("3".into())),
                (None, Some(52), None),
            ]
        );
    }

    #[test]
    fn updates_keep_the_frame_and_remove_faded_icons_and_stale_admissions() {
        let mut app = crate::testing::app();
        app.insert_resource({
            let mut online = OnlineState::new(true);
            crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
            online
        })
        .add_systems(Update, (update, hover).chain());
        let buff = eq_client_core::Buff {
            spell_id: 42,
            caster_level: 1,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks: 5,
            counters: 0,
            caster_id: 7,
        };
        crate::online::testing::buffs(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            BTreeMap::from([(3, buff.clone())]),
        );
        app.update();
        let frame = app
            .world_mut()
            .query_filtered::<Entity, With<Panel>>()
            .single(app.world())
            .unwrap();
        let entries: Vec<_> = app
            .world_mut()
            .query::<&Entry>()
            .iter(app.world())
            .map(|entry| entry.0)
            .collect();
        assert_eq!(entries, [3]);
        crate::online::testing::buffs(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            BTreeMap::from([(6, buff)]),
        );
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<Panel>>()
                .single(app.world())
                .unwrap(),
            frame
        );
        let entries: Vec<_> = app
            .world_mut()
            .query::<&Entry>()
            .iter(app.world())
            .map(|entry| entry.0)
            .collect();
        assert_eq!(entries, [6]);
        crate::online::testing::connect(&mut app.world_mut().resource_mut::<OnlineState>(), false);
        app.update();
        assert_eq!(
            app.world_mut().query::<&Entry>().iter(app.world()).count(),
            0
        );
        assert!(app.world().get_entity(frame).is_err());
    }
}
