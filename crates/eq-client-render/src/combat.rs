//! Consider, hail and melee auto-attack requests, plus Titanium-style combat text.
//!
//! The server decides every outcome; this module only sends explicit requests for
//! the current target and prints what the server reports.
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, OutboundChat, PostureState,
    combat::{ConColor, Damage, DamageOutcome, SPELL_DAMAGE_KIND},
    entities::display_name,
};

use super::hud::messages::Messages;

/// Capture the current target and loadout for a single Actions, hotbar or
/// slash-command request. The session checks both again before sending.
pub(super) fn ranged_command(
    world: &eq_client_core::world::ClientWorld,
    stamp: crate::outbox::Stamp,
) -> Result<ClientCommand, String> {
    use eq_client_core::combat::ranged::{RangedAttack, RangedLoadout};
    let target = world
        .target_intent()
        .filter(|target| target.spawn_id.is_some())
        .ok_or("Select a ranged attack target first")?;
    let loadout = RangedLoadout::current(world.inventory())?;
    Ok(ClientCommand::RangedAttack(RangedAttack {
        session_id: stamp.session_id,
        target,
        loadout,
        created: stamp.created,
    }))
}

/// Local request state; nothing here claims the server accepted a request.
#[derive(Resource, Default)]
pub(super) struct CombatState {
    /// Whether this client last asked the server to auto-attack.
    pub auto_attack: bool,
    /// How far each hand is through its swing while auto attack is on with
    /// no one to swing at.
    idle: IdleSwings,
    /// The zone session auto attack was asked of.
    session_id: Option<u64>,
    /// Auto attack the client turned off on its own, as the player stopped
    /// standing or died, whose off has not left yet: the session follows
    /// only a toggle it was sent, so the off goes again until it leaves.
    off_unsent: bool,
    /// The unsent off says nothing when it leaves: the Feign Death press
    /// turns auto attack off without a line (read R19b 3; the 0x117 line of
    /// its second call is not modelled, recording checklist 6.455).
    off_quiet: bool,
}

impl CombatState {
    /// A new zone session starts with auto attack off, as the session does,
    /// so the two agree; whether the program turns it off at a zone change
    /// is unread (recording checklist 6.445).
    fn follow_session(&mut self, session_id: Option<u64>) {
        if self.session_id != session_id {
            self.session_id = session_id;
            self.auto_attack = false;
            self.off_unsent = false;
            self.off_quiet = false;
            self.idle = IdleSwings::default();
        }
    }

    /// Turns auto attack off without a line, as the Titanium Feign Death
    /// press does. The off still goes to the session, which follows only a
    /// toggle it was sent; whatever the program's own sender does is unread
    /// (recording checklist 6.455).
    pub(super) fn end_quietly(&mut self) {
        if self.auto_attack {
            self.auto_attack = false;
            self.off_unsent = true;
            self.off_quiet = true;
        }
    }
}

/// How far the primary and secondary hands are through their swings, in
/// seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct IdleSwings([f32; 2]);

impl IdleSwings {
    /// Moves each hand `seconds` on through a swing of its delay, if it
    /// swings, and says how many swings came due: at most one a hand.
    fn advance(&mut self, seconds: f32, delays: [Option<f32>; 2]) -> usize {
        let mut due = 0;
        for (elapsed, delay) in self.0.iter_mut().zip(delays) {
            let Some(delay) = delay.filter(|delay| delay.is_finite() && *delay > 0.0) else {
                *elapsed = 0.0;
                continue;
            };
            *elapsed += seconds.max(0.0);
            if *elapsed >= delay {
                *elapsed = (*elapsed - delay) % delay;
                due += 1;
            }
        }
        due
    }
}

/// How long each hand's swing takes, in seconds: the primary's weapon's
/// delay, or the bare hand's with none; the secondary's weapon's, with no
/// swing there without one. An item's delay is in tenths of a second, as
/// the program's strike timer takes it; haste is not applied (recording
/// checklist 6.356).
fn swing_delays(
    inventory: &eq_client_core::inventory::Inventory,
    bare_hand_tenths: u32,
) -> [Option<f32>; 2] {
    use eq_client_core::inventory::InventorySlot;
    let delay = |slot: i32| {
        let item = inventory.items().get(&InventorySlot(slot))?;
        let stat = item
            .details
            .stats
            .iter()
            .find(|stat| stat.label == "Delay")?;
        u16::try_from(stat.value)
            .ok()
            .filter(|tenths| *tenths > 0)
            .map(|tenths| f32::from(tenths) / 10.0)
    };
    #[allow(clippy::cast_precision_loss)] // A few hundred at most.
    let bare = bare_hand_tenths as f32 / 10.0;
    [Some(delay(13).unwrap_or(bare)), delay(14)]
}

fn capitalized(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// Handles the configured consider, hail and auto-attack keys, as well as
/// the skin's melee attack button and a hotbutton bound to it, for the
/// current target.
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
        crate::clicks::Clicks<super::skinned::AttackButton>,
        crate::clicks::Clicks<super::hud::hotbar::Slot>,
        Res<super::hud::hotbar::Bindings>,
    ),
) {
    use super::keys::Act;
    let world = online.world();
    combat.follow_session(world.session_id());
    let Some(player) = world.player() else {
        return;
    };
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
        .and_then(|id| world.spawn(id).map(|spawn| (id, &spawn.state)));
    let toggle = settings.0.installed_client.attack_toggle();
    let posture = world
        .posture(player.spawn_id)
        .filter(|posture| *posture != PostureState::Standing);
    // Titanium keeps attacking through a sit, a duck or a lie, the server
    // deciding the swings (read R19cd 3); its Feign Death press turns
    // auto attack off on its own ([`CombatState::end_quietly`]). The
    // Mac-era client's program is unread, so it keeps the earlier rule and
    // stops attacking when the player stops standing (checklist 6.455).
    // Both keep on whatever becomes of the target.
    let live = world.connected() && !world.is_dead();
    let posture_ends = world.generation().posture_ends_attack() && posture.is_some();
    if combat.auto_attack && (!live || posture_ends) {
        combat.auto_attack = false;
        combat.off_unsent = world.connected();
    }
    // A full queue to the session is the one refusal that passes: the off
    // goes again each frame until it leaves, so the session never stays
    // attacking while the host shows it off. Any other refusal means the
    // session cannot be attacking on this connection.
    if combat.off_unsent {
        match outbox.try_post(world, attack(false)) {
            Ok(()) => {
                combat.off_unsent = false;
                if !std::mem::take(&mut combat.off_quiet) {
                    chat.history.push(toggle_line(toggle, false, &messages));
                }
            }
            Err(crate::outbox::Refusal::Busy) => (),
            Err(_) => {
                combat.off_unsent = false;
                combat.off_quiet = false;
            }
        }
    }
    if !live || !keys.focused() {
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
    if pressed {
        let enable = !combat.auto_attack;
        // Not standing, the key only turns auto attack off. It needs no
        // target, but the Titanium client will not turn it on against the
        // player themselves.
        if enable && let Some(posture) = posture {
            feedback(messages.said(refusal(toggle, posture), &[]));
            return;
        }
        let own = world.target().selected == Some(player.spawn_id);
        if enable
            && own
            && let Some(id) = toggle.self_target
        {
            feedback(messages.said(id, &[]));
            return;
        }
        if outbox.post(world, attack(enable)).is_err() {
            return;
        }
        combat.auto_attack = enable;
        combat.idle = IdleSwings::default();
        chat.history.push(toggle_line(toggle, enable, &messages));
    }
}

/// While auto attack is on with no one to swing at, says the installed
/// client's line each time a hand's swing comes due, as its program does,
/// and leaves auto attack on: with no target, or with the player as their
/// own. A swing at another target is the server's to answer; the lines the
/// program says for one out of reach, out of sight or not faced are unread
/// (recording checklist 6.357).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(super) fn idle_swings(
    time: Res<Time>,
    online: Res<super::online::OnlineState>,
    settings: Res<crate::ViewerSettings>,
    messages: Res<Messages>,
    mut combat: ResMut<CombatState>,
    mut chat: ResMut<super::chat::ChatState>,
) {
    let world = online.world();
    let idle = settings.0.installed_client.attack_toggle().idle;
    let line = idle
        .zip(world.player())
        .and_then(|(idle, player)| match world.target().selected {
            None => Some((idle, idle.no_target)),
            Some(id) if id == player.spawn_id => Some((idle, idle.self_target)),
            Some(_) => None,
        });
    let Some((idle, id)) = line.filter(|_| combat.auto_attack && world.connected()) else {
        combat.idle = IdleSwings::default();
        return;
    };
    let delays = swing_delays(world.inventory(), idle.bare_hand_tenths);
    for _ in 0..combat.idle.advance(time.delta_secs(), delays) {
        let mut said = super::chat::system_line(messages.said(id, &[]));
        said.message_type = Some(idle.line_type);
        chat.history.push(said);
    }
}

/// The installed client's refusal to turn auto attack on for a player in
/// this posture, which is not standing.
const fn refusal(toggle: eq_client_assets::ui::AttackToggle, posture: PostureState) -> u32 {
    if matches!(posture, PostureState::Frozen) {
        toggle.frozen
    } else {
        toggle.not_standing
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
            .and_then(|id| online.world().consider_color(id)),
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
    faction: u32,
    color: Option<ConColor>,
) -> super::chat::Said {
    use super::chat::Said;
    let standing = (1..=9)
        .contains(&faction)
        .then(|| messages.format(12211 + faction, &[]));
    // Titanium picks phrases by own level, level difference and a reply field
    // (0x49bbe8–0x49be16). Here each color has one representative summary.
    // Gray reuses green wording. See ARCHITECTURE.md for the native branches.
    let assessment = match color {
        Some(ConColor::Gray) => Some((12236, 6)),
        Some(ConColor::Green) => Some((12236, 2)),
        Some(ConColor::LightBlue) => Some((12232, 18)),
        Some(ConColor::Blue) => Some((12227, 4)),
        Some(ConColor::White) => Some((12229, 10)),
        Some(ConColor::Yellow) => Some((12231, 15)),
        Some(ConColor::Red) => Some((12225, 13)),
        None | Some(ConColor::Other(_)) => None,
    };
    let name = capitalized(name);
    match (standing, assessment) {
        (Some(standing), Some((id, kind))) => messages
            .said(12239, &[name, standing, messages.format(id, &[])])
            .typed(kind),
        (None, Some((id, kind))) => Said::own(format!(
            "{name} — faction standing unknown (code {faction}) — {}",
            messages.format(id, &[])
        ))
        .typed(kind),
        (Some(standing), None) => {
            Said::own(format!("{name} {standing} — level assessment unknown."))
        }
        (None, None) => Said::own(format!(
            "{name} — faction standing unknown (code {faction}) — level assessment unknown."
        )),
    }
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
             12212 standing 1\n12213 standing 2\n12214 standing 3\n12215 standing 4\n\
             12216 standing 5\n12217 standing 6\n12218 standing 7\n12219 standing 8\n\
             12220 standing 9\n12231 synthetic yellow.\n\
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
    fn a_new_zone_session_turns_the_hosts_auto_attack_off_as_the_sessions_is() {
        let mut combat = CombatState::default();
        combat.follow_session(Some(1));
        combat.auto_attack = true;
        combat.follow_session(Some(1));
        assert!(combat.auto_attack);
        combat.follow_session(Some(2));
        assert!(!combat.auto_attack);
        combat.auto_attack = true;
        combat.follow_session(None);
        assert!(!combat.auto_attack);
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
        let table = table();
        for faction in 1..=9 {
            let text = consideration_text(&table, "a rat", faction, Some(ConColor::Yellow));
            assert_eq!(
                text,
                super::super::chat::Said::official(format!(
                    "A rat standing {faction}; synthetic yellow."
                ))
                .typed(15)
            );
        }
        let unknown = consideration_text(&table, "a rat", 77, None);
        assert_eq!(
            unknown,
            super::super::chat::Said::own(
                "A rat — faction standing unknown (code 77) — level assessment unknown."
            )
        );
    }

    #[test]
    fn consider_text_uses_a_native_light_blue_summary() {
        let table = Messages::parse(
            "EQST0002\n0 1\n12239 %1 %2 %3\n12216 synthetic standing\n12232 synthetic light blue.\n",
        );
        assert_eq!(
            consideration_text(&table, "a rat", 5, Some(ConColor::LightBlue)),
            super::super::chat::Said::official("A rat synthetic standing synthetic light blue.")
                .typed(18),
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
        testing::generation(
            &mut online,
            match client {
                eq_client_assets::ui::InstalledClient::EqMac => eq_client_core::GameDialect::EqMac,
                eq_client_assets::ui::InstalledClient::Titanium => {
                    eq_client_core::GameDialect::Titanium
                }
            },
        );
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
            .insert_resource(Messages::parse(
                "EQST0002\n0 0\n12370 On.\n12371 Off.\n12696 Self.\n12697 Nobody.\n",
            ))
            .init_resource::<Time>()
            .add_systems(Update, (input, idle_swings).chain());
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

    #[test]
    fn native_mac_keys_submit_attack_and_consider_through_the_normal_input_system() {
        let (mut app, rx) = attacking(eq_client_assets::ui::InstalledClient::EqMac);
        app.insert_resource(crate::keys::KeyMap::for_client(
            eq_client_assets::ui::InstalledClient::EqMac,
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyA);
        app.update();
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::AutoAttack {
                enabled: true,
                session_id: 7,
                ..
            })
        ));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::KeyC);
        }
        app.update();
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::Consider {
                session_id: 7,
                own_id: 12,
                target_id: 30,
                ..
            })
        ));
        assert!(rx.try_recv().is_err());
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

    /// Runs the frame `seconds` after the last with no key held.
    fn idle(app: &mut App, seconds: f32) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(seconds));
        app.update();
    }

    fn lines(app: &App) -> Vec<(String, Option<u32>)> {
        let chat = app.world().resource::<super::super::chat::ChatState>();
        chat.history
            .lines(eq_client_core::chat::ChatTab::All)
            .iter()
            .map(|(_, line)| (line.message.text.clone(), line.message_type))
            .collect()
    }

    #[test]
    fn titaniums_auto_attack_stays_on_without_a_target_and_says_so_each_swing() {
        let (mut app, rx) = attacking(eq_client_assets::ui::InstalledClient::Titanium);
        app.world_mut()
            .resource_mut::<crate::online::OnlineState>()
            .select_target(None);
        press(&mut app);
        assert_eq!(sent(&rx), Some(true));
        assert_eq!(newest(&app), ("On.".into(), Some(13)));
        // Bare-handed, a swing comes due each 3.5 s: a line each time, and
        // auto attack stays on.
        idle(&mut app, 2.0);
        assert_eq!(lines(&app).len(), 1);
        idle(&mut app, 2.0);
        assert_eq!(newest(&app), ("Nobody.".into(), Some(303)));
        idle(&mut app, 3.5);
        assert_eq!(lines(&app).len(), 3);
        assert!(app.world().resource::<CombatState>().auto_attack);
        assert_eq!(sent(&rx), None);
        // A target the server can answer for ends the lines.
        app.world_mut()
            .resource_mut::<crate::online::OnlineState>()
            .select_target(Some(30));
        idle(&mut app, 4.0);
        assert_eq!(lines(&app).len(), 3);
    }

    #[test]
    fn titanium_refuses_to_turn_auto_attack_on_against_the_player_themselves() {
        let (mut app, rx) = attacking(eq_client_assets::ui::InstalledClient::Titanium);
        app.world_mut()
            .resource_mut::<crate::online::OnlineState>()
            .select_target(Some(12));
        press(&mut app);
        assert_eq!(sent(&rx), None);
        assert!(!app.world().resource::<CombatState>().auto_attack);
        assert_eq!(newest(&app).0, "Self.");
    }

    #[test]
    fn a_weapon_swings_on_its_own_delay() {
        let mut swings = IdleSwings::default();
        // A 2.2 s primary and a 3.0 s secondary.
        let delays = [Some(2.2), Some(3.0)];
        assert_eq!(swings.advance(2.0, delays), 0);
        assert_eq!(swings.advance(0.5, delays), 1);
        assert_eq!(swings.advance(0.5, delays), 1);
        // A long frame says each hand's line once.
        assert_eq!(swings.advance(31.0, delays), 2);
        // A hand with nothing to swing starts again when it has.
        assert_eq!(swings.advance(1.0, [Some(2.2), None]), 0);
        assert!(swings.0[1].abs() < f32::EPSILON);
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

    #[test]
    fn titanium_keeps_attacking_through_a_posture_and_the_feign_press_ends_it_quietly() {
        use crate::online::testing;
        let (mut app, rx) = attacking(eq_client_assets::ui::InstalledClient::Titanium);
        let posture = |app: &mut App, posture| {
            testing::news(
                &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
                [eq_client_core::WorldEvent::Posture {
                    spawn_id: 12,
                    posture,
                }],
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.update();
        };
        press(&mut app);
        assert_eq!(sent(&rx), Some(true));
        // Sitting, ducking and lying leave it on and send nothing (read
        // R19cd 3); the server decides the swings.
        for stance in [
            PostureState::Sitting,
            PostureState::Ducking,
            PostureState::Lying,
        ] {
            posture(&mut app, stance);
            assert_eq!(sent(&rx), None, "{stance:?}");
            assert!(app.world().resource::<CombatState>().auto_attack);
        }
        // Not standing, the key still turns it off, and refuses to turn it
        // on (line 0x3051).
        posture(&mut app, PostureState::Sitting);
        press(&mut app);
        assert_eq!(sent(&rx), Some(false));
        press(&mut app);
        assert_eq!(sent(&rx), None);
        assert!(!app.world().resource::<CombatState>().auto_attack);
        // The Feign Death press turns it off once, saying nothing.
        posture(&mut app, PostureState::Standing);
        press(&mut app);
        assert_eq!(sent(&rx), Some(true));
        let before = lines(&app).len();
        posture(&mut app, PostureState::Lying);
        app.world_mut().resource_mut::<CombatState>().end_quietly();
        app.update();
        assert_eq!(sent(&rx), Some(false));
        assert_eq!(lines(&app).len(), before, "no line: {:?}", lines(&app));
        // Standing from the feign leaves it off.
        posture(&mut app, PostureState::Standing);
        assert_eq!(sent(&rx), None);
        assert!(!app.world().resource::<CombatState>().auto_attack);
        // Off already, the press sends nothing.
        app.world_mut().resource_mut::<CombatState>().end_quietly();
        app.update();
        assert_eq!(sent(&rx), None);
    }

    #[test]
    fn a_forced_off_that_meets_a_full_queue_goes_again_until_it_leaves() {
        use crate::online::testing;
        // The Mac-era client's posture rule makes the forced off.
        let (mut app, rx) = attacking(eq_client_assets::ui::InstalledClient::EqMac);
        press(&mut app);
        assert_eq!(sent(&rx), Some(true));
        // A queue to the session with its one place taken.
        let (tx, full) = std::sync::mpsc::sync_channel(1);
        let filler = ClientCommand::AutoAttack {
            session_id: 7,
            enabled: true,
            created: std::time::Instant::now(),
        };
        tx.try_send(filler).unwrap();
        app.insert_resource(crate::outbox::Outbox::new(Some(tx)));
        testing::news(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            [eq_client_core::WorldEvent::Posture {
                spawn_id: 12,
                posture: PostureState::Sitting,
            }],
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert!(!app.world().resource::<CombatState>().auto_attack);
        assert!(app.world().resource::<CombatState>().off_unsent);
        // The place frees: the off leaves on the next frame, once.
        assert!(full.try_recv().is_ok());
        app.update();
        assert_eq!(sent(&full), Some(false));
        app.update();
        assert_eq!(sent(&full), None);
        assert!(!app.world().resource::<CombatState>().off_unsent);
    }
}
