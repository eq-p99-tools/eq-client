//! Compact server-owned buff display; clicking never cancels an effect.
use super::{online::OnlineState, spell_icons, spellbook::SpellNames, windows};
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_core::{buffs::TimeLeft, qol::Fix};
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
        return;
    };
    if previous.as_ref() == Some(buffs)
        && *previous_effects == *online.world().buffs().effects()
        && !panels.is_empty()
    {
        return;
    }
    *previous = Some(buffs.clone());
    previous_effects.clone_from(online.world().buffs().effects());
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
    content(&mut commands, body, buffs, online.world().buffs().effects());
}

/// Builds icons from occupied server slots without renumbering holes.
fn content(
    commands: &mut Commands,
    body: Entity,
    buffs: &BTreeMap<u32, eq_client_core::Buff>,
    effects: &BTreeMap<u16, eq_client_core::SpellEffect>,
) {
    commands.entity(body).with_children(|parent| {
        parent
            .spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(3),
                row_gap: px(3),
                ..default()
            })
            .with_children(|grid| {
                for &slot in buffs.keys() {
                    grid.spawn((
                        Entry(slot),
                        Button,
                        Node {
                            width: px(42),
                            height: px(42),
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BorderColor::all(theme::EDGE),
                        BackgroundColor(theme::INSET),
                    ))
                    .with_children(|icon| {
                        icon.spawn(spell_icons::artwork(spell_icons::Source::Buff(slot), 36.0));
                        icon.spawn((
                            Text::new((u64::from(slot) + 1).to_string()),
                            theme::font(Size::Caption),
                            Node {
                                position_type: PositionType::Absolute,
                                bottom: px(0),
                                right: px(2),
                                ..default()
                            },
                        ));
                    });
                }
                for &spell in effects.keys() {
                    grid.spawn((
                        Unplaced(spell),
                        Button,
                        Node {
                            width: px(42),
                            height: px(42),
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BorderColor::all(theme::EDGE),
                        BackgroundColor(theme::INSET),
                    ))
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
            Text::new(if buffs.is_empty() && effects.is_empty() {
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
            Some(eq_client_core::buffs::Shown::Unplaced(effect)) => effect_details(
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
