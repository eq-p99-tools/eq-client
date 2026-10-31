//! Consider, hail and melee auto-attack requests, plus Titanium-style combat text.
//!
//! The server decides every outcome; this module only sends explicit requests for
//! the current target and prints what the server reports.
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, OutboundChat, PostureState, SpawnKind,
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
/// melee attack button and a hotbutton bound to it do) for the current
/// target.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn input(
    keys: super::keys::Keys,
    online: Res<super::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    messages: Res<Messages>,
    mut combat: ResMut<CombatState>,
    mut chat: ResMut<super::chat::ChatState>,
    settings: Res<crate::ViewerSettings>,
    (attack_button, bar_clicks, bindings): (
        super::hud::hotbar::Clicks<super::skinned::AttackButton>,
        super::hud::hotbar::Clicks<super::hud::hotbar::Slot>,
        Res<super::hud::hotbar::Bindings>,
    ),
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
    let toggle = settings.0.installed_client.attack_toggle();
    let posture = world
        .posture(player.spawn_id)
        .filter(|posture| *posture != PostureState::Standing);
    // The Titanium client stops attacking when its target goes away; the
    // Mac-era client when the player stops standing.
    let stopped = match toggle {
        Some(_) => posture.is_some(),
        None => attackable.map(|(id, _)| id) != combat.attack_target,
    };
    if combat.auto_attack
        && (!online.world().connected() || online.world().death().is_some() || stopped)
    {
        combat.auto_attack = false;
        combat.attack_target = None;
        if online.world().connected() && outbox.post(world, attack(false)).is_ok() {
            chat.history.push(match toggle {
                Some(toggle) => toggle_line(toggle, false, &messages),
                None => super::chat::system_line(messages.said(1466, &[])),
            });
        }
    }
    if !online.world().connected() || online.world().death().is_some() || !keys.focused() {
        return;
    }
    let mut feedback = |said: super::chat::Said| chat.refuse(said);
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
            None => feedback(messages.said(12240, &[])),
        }
    }
    if keys.pressed(Act::Hail) {
        let text = spawn.map_or_else(
            || "Hail".to_owned(),
            |(_, spawn)| format!("Hail, {}", display_name(&spawn.name)),
        );
        let _ = outbox.send(world, ClientCommand::SendChat(OutboundChat::Say(text)));
    }
    let bar_attack = super::hud::hotbar::requested(&keys, &bindings, &bar_clicks)
        == Some(super::hud::hotbar::Action::Attack);
    let pressed = keys.pressed(Act::Attack) || attack_button.iter().next().is_some() || bar_attack;
    if let Some(toggle) = toggle.filter(|_| pressed) {
        let enable = !combat.auto_attack;
        // Not standing, the key only turns auto attack off.
        if enable && let Some(posture) = posture {
            let refusal = if posture == PostureState::Frozen {
                toggle.frozen
            } else {
                toggle.not_standing
            };
            feedback(messages.said(refusal, &[]));
            return;
        }
        if outbox.post(world, attack(enable)).is_err() {
            return;
        }
        combat.auto_attack = enable;
        combat.attack_target = None;
        chat.history.push(toggle_line(toggle, enable, &messages));
        return;
    }
    if pressed {
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
            messages.said(1466, &[])
        });
    }
}

/// The line an installed client says as its key turns auto attack on or
/// off, in its colour.
fn toggle_line(
    toggle: eq_client_assets::ui::AttackToggle,
    enabled: bool,
    messages: &Messages,
) -> eq_client_core::chat::ChatLine {
    let id = if enabled { toggle.on } else { toggle.off };
    let mut line = super::chat::system_line(messages.said(id, &[]));
    line.message_type = Some(toggle.line_type);
    line
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

/// The consider line: the target, its faction standing, and the level
/// assessment.
pub(super) fn consideration_text(
    messages: &Messages,
    name: &str,
    consideration: &Consideration,
) -> super::chat::Said {
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
    messages.said(
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
) -> Option<super::chat::Said> {
    let own_attack = damage.source_id == own_id;
    let own_defense = damage.target_id == own_id;
    if damage.kind == SPELL_DAMAGE_KIND {
        return match damage.outcome {
            DamageOutcome::Hit(points) if !own_defense && damage.source_id == own_id => {
                Some(messages.said(
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
            return Some(messages.said(
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
        // A spell's landing and a lifetap come in spell records, which the
        // arm above takes; an unknown code has no line.
        DamageOutcome::Landed | DamageOutcome::Tapped(_) | DamageOutcome::Other(_) => return None,
    };
    Some(messages.said(
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
            "EQST0002\n0 0\n1466 Melee stops.\n12156 whiff\n12157 whiffs\n\
             12158 %1 aim to %2 %3 and %4.\n12159 %1 aims to %2 %3 and %4.\n\
             12160 dealing %1 damage.\n12161 dealing just %1 damage point.\n\
             12162 %1 %2 %3, %4\n12150 %1 duck\n12151 %1 ducks\n12179 cut\n\
             12180 cuts\n12183 strike\n12184 strikes\n12185 Thou\n12187 THEE\n\
             12217 glares, set to fight\n12231 seems a fair match.\n\
             12239 %1 %2; %3\n434 %1 took %2 damage from a spell.\n",
        )
    }

    fn hit(source_id: u16, target_id: u16, kind: u8, outcome: DamageOutcome) -> Damage {
        Damage {
            target_id,
            source_id,
            kind,
            spell_id: None,
            outcome,
            push: None,
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
        let text =
            |damage: Damage| damage_text(&messages, 7, names, &damage).map(|said| said.words.text);
        assert_eq!(
            text(hit(7, 9, 1, DamageOutcome::Hit(5))).unwrap(),
            "Thou cut a rat, dealing 5 damage."
        );
        assert_eq!(
            text(hit(9, 7, 28, DamageOutcome::Hit(1))).unwrap(),
            "A rat strikes THEE, dealing just 1 damage point."
        );
        assert_eq!(
            text(hit(7, 9, 1, DamageOutcome::Miss)).unwrap(),
            "Thou aim to cut a rat and whiff."
        );
        assert_eq!(
            text(hit(9, 7, 28, DamageOutcome::Dodge)).unwrap(),
            "A rat aims to strike THEE and THEE duck."
        );
        assert_eq!(
            text(hit(7, 9, 1, DamageOutcome::Dodge)).unwrap(),
            "Thou aim to cut a rat and a rat ducks."
        );
        assert_eq!(text(hit(9, 11, 1, DamageOutcome::Hit(5))), None);
        let spell = Damage {
            kind: SPELL_DAMAGE_KIND,
            spell_id: Some(93),
            ..hit(7, 9, 0, DamageOutcome::Hit(8))
        };
        assert_eq!(text(spell).unwrap(), "A rat took 8 damage from a spell.");
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
            super::super::chat::Said::official("A rat glares, set to fight; seems a fair match.")
        );
    }

    /// A player admitted with a creature targeted, the attack bound to the
    /// third hotbutton, under this installed client.
    fn attacking(
        client: eq_client_assets::ui::InstalledClient,
    ) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
        use crate::{hud::hotbar, online::testing};
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut online = crate::online::OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(12));
        let mut creature = testing::pet(30, 12);
        creature.pet_owner = None;
        testing::spawn_entry(&mut online, 30, creature);
        online.select_target(Some(30));
        let mut bindings = hotbar::Bindings::default();
        bindings.0[2] = Some(hotbar::Action::Attack);
        let mut settings = crate::ViewerSettings::default();
        settings.0.installed_client = client;
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(tx)))
            .insert_resource(bindings)
            .insert_resource(settings)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<CombatState>()
            .init_resource::<Messages>()
            .add_systems(Update, input);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        (app, rx)
    }

    fn press(app: &mut App) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::Digit3);
        app.update();
    }

    /// The auto attack the frame sent, if it sent one, and nothing else.
    fn sent(rx: &std::sync::mpsc::Receiver<ClientCommand>) -> Option<bool> {
        let enabled = rx.try_recv().ok().map(|command| {
            let ClientCommand::AutoAttack {
                session_id: 7,
                enabled,
                ..
            } = command
            else {
                panic!("the hotbutton sent something else: {command:?}");
            };
            enabled
        });
        assert!(rx.try_recv().is_err());
        enabled
    }

    fn newest(app: &App) -> (String, Option<u32>) {
        let chat = app.world().resource::<super::super::chat::ChatState>();
        let lines = chat.history.lines(eq_client_core::chat::ChatTab::All);
        let (_, line) = lines.last().expect("a line");
        (line.message.text.clone(), line.message_type)
    }

    #[test]
    fn a_hotbutton_bound_to_melee_attack_turns_auto_attack_on_and_off() {
        let (mut app, rx) = attacking(eq_client_assets::ui::InstalledClient::Titanium);
        press(&mut app);
        assert_eq!(sent(&rx), Some(true));
        assert!(app.world().resource::<CombatState>().auto_attack);
        press(&mut app);
        assert_eq!(sent(&rx), Some(false));
        assert!(!app.world().resource::<CombatState>().auto_attack);
    }

    #[test]
    fn the_mac_era_key_needs_no_target_but_a_standing_player_and_stops_when_they_sit() {
        use crate::online::testing;
        let (mut app, rx) = attacking(eq_client_assets::ui::InstalledClient::EqMac);
        let posture = |app: &mut App, posture| {
            testing::news(
                &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
                [eq_client_core::WorldEvent::Posture {
                    spawn_id: 12,
                    posture,
                }],
            );
        };
        // Without a target, on, in the program's words and colour.
        app.world_mut()
            .resource_mut::<crate::online::OnlineState>()
            .select_target(None);
        press(&mut app);
        assert_eq!(sent(&rx), Some(true));
        assert_eq!(newest(&app).1, Some(13));
        // Sitting stops it by itself.
        posture(&mut app, PostureState::Sitting);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert_eq!(sent(&rx), Some(false));
        assert!(!app.world().resource::<CombatState>().auto_attack);
        assert_eq!(newest(&app).1, Some(13));
        // Sitting, the key refuses and does not stand the player up.
        press(&mut app);
        assert_eq!(sent(&rx), None);
        assert!(!app.world().resource::<CombatState>().auto_attack);
        assert_eq!(
            app.world()
                .resource::<crate::online::OnlineState>()
                .world()
                .posture(12),
            Some(PostureState::Sitting)
        );
    }
}
