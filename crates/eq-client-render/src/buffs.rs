//! Compact server-owned buff display; clicking never cancels an effect.
use super::{online::OnlineState, spell_icons, spellbook::SpellNames, windows};
use bevy::prelude::*;
use std::collections::BTreeMap;

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

/// Rebuilds only buff content, keeping the title bar and its saved layout intact.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    online: Res<OnlineState>,
    panels: Query<Entity, With<Panel>>,
    bodies: Query<Entity, With<Body>>,
    mut previous: Local<Option<BTreeMap<u32, eq_client_core::Buff>>>,
    mut previous_effects: Local<BTreeMap<u16, eq_client_core::SpellEffect>>,
) {
    let Some(buffs) = online.world.buffs().slots() else {
        for entity in &panels {
            commands.entity(entity).despawn();
        }
        *previous = None;
        previous_effects.clear();
        return;
    };
    if previous.as_ref() == Some(buffs)
        && *previous_effects == *online.world.buffs().effects()
        && !panels.is_empty()
    {
        return;
    }
    *previous = Some(buffs.clone());
    previous_effects.clone_from(online.world.buffs().effects());
    let body = if let Ok(body) = bodies.single() {
        commands.entity(body).despawn_children();
        body
    } else {
        let frame = commands
            .spawn((
                Panel,
                windows::Frame::default(),
                Node {
                    position_type: PositionType::Absolute,
                    right: px(340),
                    top: px(150),
                    width: px(242),
                    padding: UiRect::all(px(6)),
                    row_gap: px(4),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                GlobalZIndex(12),
                BackgroundColor(Color::srgba(0.025, 0.032, 0.04, 0.92)),
            ))
            .id();
        commands
            .entity(frame)
            .with_children(|parent| windows::title_bar(parent, frame, "BUFFS"));
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
    content(&mut commands, body, buffs, online.world.buffs().effects());
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
                        BorderColor::all(Color::srgb(0.28, 0.34, 0.40)),
                        BackgroundColor(Color::srgb(0.08, 0.11, 0.14)),
                    ))
                    .with_children(|icon| {
                        icon.spawn(spell_icons::artwork(spell_icons::Source::Buff(slot), 36.0));
                        icon.spawn((
                            Text::new((u64::from(slot) + 1).to_string()),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
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
                        BorderColor::all(Color::srgb(0.28, 0.34, 0.40)),
                        BackgroundColor(Color::srgb(0.08, 0.11, 0.14)),
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
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::srgb(0.73, 0.77, 0.81)),
        ));
    });
}

/// Labels the server's duration without pretending it is a synchronized countdown.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn hover(
    online: Res<OnlineState>,
    names: Res<SpellNames>,
    entries: Query<(&Entry, &Interaction)>,
    effects: Query<(&Unplaced, &Interaction)>,
    mut hints: Query<&mut Text, With<Hint>>,
) {
    let hovered = entries
        .iter()
        .find(|(_, interaction)| **interaction != Interaction::None)
        .and_then(|(entry, _)| online.world.buffs().slots()?.get(&entry.0));
    let text = hovered.map_or_else(
        || {
            if let Some((entry, _)) = effects
                .iter()
                .find(|(_, interaction)| **interaction != Interaction::None)
            {
                let duration = online
                    .world
                    .buffs()
                    .effects()
                    .get(&entry.0)
                    .and_then(|effect| {
                        names
                            .mechanics(u32::from(entry.0))?
                            .base_duration(effect.caster_level)
                    });
                let level = online
                    .world
                    .buffs()
                    .effects()
                    .get(&entry.0)
                    .map(|effect| effect.caster_level);
                return format!(
                    "{}\n{}{}",
                    names.label(u32::from(entry.0)),
                    base_duration_label(duration),
                    resource_hint(names.mechanics(u32::from(entry.0)), level)
                );
            }
            if online.world.buffs().slots().is_some_and(BTreeMap::is_empty)
                && online.world.buffs().effects().is_empty()
            {
                "No active buffs".into()
            } else {
                "Hover an effect for details".into()
            }
        },
        |buff| {
            format!(
                "{}\nServer duration: {} ticks{}",
                names.label(buff.spell_id),
                buff.duration_ticks,
                resource_hint(
                    names.mechanics(buff.spell_id),
                    Some(u16::from(buff.caster_level))
                )
            )
        },
    );
    for mut hint in &mut hints {
        if hint.0 != text {
            hint.0.clone_from(&text);
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
        let mut app = App::new();
        app.insert_resource({
            let mut online = OnlineState::new(true);
            crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
            online
        })
        .init_resource::<SpellNames>()
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
