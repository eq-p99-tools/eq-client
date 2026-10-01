//! Consider, hail and melee auto-attack requests, plus Titanium-style combat text.
//!
//! The server decides every outcome; this module only sends explicit requests for
//! the current target and prints what the server reports.
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, OutboundChat, SpawnKind,
    combat::{ConColor, Consideration, Damage, DamageOutcome, SPELL_DAMAGE_KIND},
    entities::display_name,
};

use super::hud::messages::Messages;

/// Local request state; nothing here claims the server accepted a request.
#[derive(Resource, Default)]
pub(super) struct CombatState {
    /// Whether this client last asked the server to auto-attack.
    pub auto_attack: bool,
    /// Target that the current auto-attack request was made against.
    attack_target: Option<u16>,
}

fn capitalized(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// Handles K (consider), H (hail) and G (toggle auto-attack, as the skin's
/// melee attack button does) for the current target.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn input(
    keys: super::keys::Keys,
    online: Res<super::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    messages: Res<Messages>,
    mut combat: ResMut<CombatState>,
    mut chat: ResMut<super::chat::ChatState>,
    attack_button: Query<&Interaction, (Changed<Interaction>, With<super::skinned::AttackButton>)>,
) {
    use super::keys::Act;
    let Some(player) = online.world().player() else {
        return;
    };
    let world = online.world();
    let attack = |enabled| {
        move |stamp: crate::outbox::Stamp| ClientCommand::AutoAttack {
            session_id: stamp.session_id,
            enabled,
            created: stamp.created,
        }
    };
    let spawn = online
        .world()
        .target()
        .selected
        .filter(|id| *id != player.spawn_id)
        .and_then(|id| online.world().spawn(id).map(|spawn| (id, &spawn.state)));
    let attackable = spawn.filter(|(_, spawn)| spawn.kind == SpawnKind::Npc && !spawn.invisible);
    // The official client stops attacking when its target goes away.
    if combat.auto_attack
        && (!online.world().connected()
            || online.world().death().is_some()
            || attackable.map(|(id, _)| id) != combat.attack_target)
    {
        combat.auto_attack = false;
        combat.attack_target = None;
        if online.world().connected() && outbox.post(world, attack(false)).is_ok() {
            chat.history
                .push(super::chat::system_line(messages.format(1466, &[])));
        }
    }
    if !online.world().connected() || online.world().death().is_some() || !keys.focused() {
        return;
    }
    let mut feedback = |text: String| chat.history.push(super::chat::system_line(text));
    // The outbox shows why a request did not leave.
    if keys.pressed(Act::Consider) {
        match spawn {
            Some((target_id, _)) => {
                let _ = outbox.post(world, |stamp| ClientCommand::Consider {
                    session_id: stamp.session_id,
                    own_id: player.spawn_id,
                    target_id,
                    created: stamp.created,
                });
            }
            None => feedback(messages.format(12240, &[])),
        }
    }
    if keys.pressed(Act::Hail) {
        let text = spawn.map_or_else(
            || "Hail".to_owned(),
            |(_, spawn)| format!("Hail, {}", display_name(&spawn.name)),
        );
        let _ = outbox.send(world, ClientCommand::SendChat(OutboundChat::Say(text)));
    }
    if keys.pressed(Act::Attack)
        || attack_button
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed)
    {
        let enable = !combat.auto_attack;
        if enable && attackable.is_none() {
            feedback("Target a creature to attack it".into());
            return;
        }
        if outbox.post(world, attack(enable)).is_err() {
            return;
        }
        combat.auto_attack = enable;
        combat.attack_target = attackable.filter(|_| enable).map(|(id, _)| id);
        feedback(if enable {
            "Auto attack is on.".into()
        } else {
            messages.format(1466, &[])
        });
    }
}

/// Colors the target name by the latest consider result.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn target_color(
    online: Res<super::online::OnlineState>,
    mut names: Query<&mut TextColor, With<super::target::TargetName>>,
) {
    let color = crate::theme::con(
        online
            .world()
            .target()
            .selected
            .and_then(|id| online.world().considered(id)),
    );
    for mut text in &mut names {
        if text.0 != color {
            text.0 = color;
        }
    }
}

/// `%1 %2 -- %3`: the target, its faction standing, and the level assessment.
pub(super) fn consideration_text(
    messages: &Messages,
    name: &str,
    consideration: &Consideration,
) -> String {
    let standing = match consideration.faction {
        1..=5 => 12211 + consideration.faction,
        6 => 12220,
        7 => 12219,
        8 => 12218,
        _ => 12217,
    };
    // Best-effort mapping of level colors to the client's assessment strings.
    let assessment = match consideration.color {
        ConColor::Gray | ConColor::Green => 12236,
        ConColor::LightBlue => 12226,
        ConColor::Blue => 12227,
        ConColor::White | ConColor::Other(_) => 12229,
        ConColor::Yellow => 12231,
        ConColor::Red => 12225,
    };
    messages.format(
        12239,
        &[
            capitalized(name),
            messages.format(standing, &[]),
            messages.format(assessment, &[]),
        ],
    )
}

/// Titanium-style melee text for fights involving the player; None for other fights.
pub(super) fn damage_text(
    messages: &Messages,
    own_id: u16,
    names: impl Fn(u16) -> String,
    damage: &Damage,
) -> Option<String> {
    let own_attack = damage.source_id == own_id;
    let own_defense = damage.target_id == own_id;
    if damage.kind == SPELL_DAMAGE_KIND {
        return match damage.outcome {
            DamageOutcome::Hit(points) if !own_defense && damage.source_id == own_id => {
                Some(messages.format(
                    434,
                    &[capitalized(&names(damage.target_id)), points.to_string()],
                ))
            }
            _ => None,
        };
    }
    if !own_attack && !own_defense {
        return None;
    }
    let (base, third) = verbs(damage.kind);
    let attacker = if own_attack {
        messages.format(12185, &[])
    } else {
        capitalized(&names(damage.source_id))
    };
    let defender = if own_defense {
        messages.format(12187, &[])
    } else {
        names(damage.target_id)
    };
    let verb = if own_attack { base } else { third };
    let avoided = |first: u32, third: u32| {
        if own_defense {
            messages.format(first, &[messages.format(12187, &[])])
        } else {
            messages.format(third, std::slice::from_ref(&defender))
        }
    };
    let reason = match damage.outcome {
        DamageOutcome::Hit(points) => {
            let amount = messages.format(
                if points == 1 { 12161 } else { 12160 },
                &[points.to_string()],
            );
            return Some(messages.format(
                12162,
                &[attacker, messages.format(verb, &[]), defender, amount],
            ));
        }
        DamageOutcome::Miss => messages.format(if own_attack { 12156 } else { 12157 }, &[]),
        DamageOutcome::Block => avoided(12144, 12145),
        DamageOutcome::Parry => avoided(12146, 12147),
        DamageOutcome::Riposte => avoided(12148, 12149),
        DamageOutcome::Dodge => avoided(12150, 12151),
        DamageOutcome::Invulnerable => avoided(12152, 12153),
        DamageOutcome::Rune => {
            if own_defense {
                messages.format(12154, &[])
            } else {
                messages.format(12155, std::slice::from_ref(&defender))
            }
        }
        DamageOutcome::Other(_) => return None,
    };
    Some(messages.format(
        if own_attack { 12158 } else { 12159 },
        &[attacker, messages.format(base, &[]), defender, reason],
    ))
}

/// First- and third-person verb string IDs for a Titanium skill number.
const fn verbs(skill: u8) -> (u32, u32) {
    match skill {
        0 | 2 => (12191, 12192),
        1 | 3 => (12179, 12180),
        8 => (12199, 12200),
        10 => (12201, 12202),
        26 | 30 | 38 => (12195, 12196),
        36 => (12193, 12194),
        21 | 23 | 52 => (12197, 12198),
        _ => (12183, 12184),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Messages {
        Messages::parse(
            "EQST0002\n0 0\n1466 Auto attack off.\n12156 miss\n12157 misses\n\
             12158 %1 try to %2 %3, but %4!\n12159 %1 tries to %2 %3, but %4!\n\
             12160 for %1 points of damage.\n12161 for %1 point of damage.\n\
             12162 %1 %2 %3 %4\n12150 %1 dodge\n12151 %1 dodges\n12179 slash\n\
             12180 slashes\n12183 hit\n12184 hits\n12185 You\n12187 YOU\n\
             12217 scowls at you, ready to attack\n12231 looks like quite a gamble.\n\
             12239 %1 %2 -- %3\n434 %1 was hit by non-melee for %2 points of damage.\n",
        )
    }

    fn hit(source_id: u16, target_id: u16, kind: u8, outcome: DamageOutcome) -> Damage {
        Damage {
            target_id,
            source_id,
            kind,
            spell_id: None,
            outcome,
        }
    }

    #[test]
    fn melee_text_follows_the_titanium_templates() {
        let messages = table();
        let names = |id: u16| {
            if id == 9 {
                "a rat".to_owned()
            } else {
                "someone".into()
            }
        };
        let text = |damage: Damage| damage_text(&messages, 7, names, &damage);
        assert_eq!(
            text(hit(7, 9, 1, DamageOutcome::Hit(5))).unwrap(),
            "You slash a rat for 5 points of damage."
        );
        assert_eq!(
            text(hit(9, 7, 28, DamageOutcome::Hit(1))).unwrap(),
            "A rat hits YOU for 1 point of damage."
        );
        assert_eq!(
            text(hit(7, 9, 1, DamageOutcome::Miss)).unwrap(),
            "You try to slash a rat, but miss!"
        );
        assert_eq!(
            text(hit(9, 7, 28, DamageOutcome::Dodge)).unwrap(),
            "A rat tries to hit YOU, but YOU dodge!"
        );
        assert_eq!(
            text(hit(7, 9, 1, DamageOutcome::Dodge)).unwrap(),
            "You try to slash a rat, but a rat dodges!"
        );
        assert_eq!(text(hit(9, 11, 1, DamageOutcome::Hit(5))), None);
        let spell = Damage {
            kind: SPELL_DAMAGE_KIND,
            spell_id: Some(93),
            ..hit(7, 9, 0, DamageOutcome::Hit(8))
        };
        assert_eq!(
            text(spell).unwrap(),
            "A rat was hit by non-melee for 8 points of damage."
        );
    }

    #[test]
    fn consider_text_combines_standing_and_level_color() {
        let text = consideration_text(
            &table(),
            "a rat",
            &Consideration {
                target_id: 9,
                faction: 9,
                color: ConColor::Yellow,
                hit_points: None,
            },
        );
        assert_eq!(
            text,
            "A rat scowls at you, ready to attack -- looks like quite a gamble."
        );
    }
}
