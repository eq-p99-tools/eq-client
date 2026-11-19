//! The abilities on the skin's Actions window, as the official client has
//! them: the Combat page's four buttons and the Abilities page's six. Each
//! holds the ability the player chose for it from the skills picker
//! ([`picker`], read R17), or until the first choice this client's own
//! order ([`choices`]), named on its button and greyed while its recovery
//! timer runs. A click uses one, and Ctrl and a number binds the one
//! under the pointer to the action bar, as gems and items bind. The session
//! says why an ability did not go.
use crate::{online::OnlineState, outbox::Outbox, theme};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, abilities::Ability, hotbar::Binding, knights::KnightAbility, world::ClientWorld,
};

/// A page of the Actions window that holds ability buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    /// Four buttons, for strikes and taunt.
    Combat,
    /// Six buttons, for what the Combat page has no room for and the rest.
    Abilities,
}

/// One of the Actions window's ability buttons: its page and its place on it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AbilityButton {
    pub(crate) page: Page,
    pub(crate) index: usize,
}

/// The words on an ability button: its ability's name.
#[derive(Component, Clone, Copy)]
pub(crate) struct AbilityLabel(pub(crate) AbilityButton);

pub(crate) mod choices;
pub(crate) mod picker;

pub(crate) use choices::{Choices, assigned};

/// The class ability that belongs to the admitted player, if any.
pub(crate) fn knight(world: &ClientWorld) -> Option<KnightAbility> {
    let player = world.player()?;
    KnightAbility::for_player(player.class, player.level)
}

/// Build the same request from a button, hotbar or a named command.
pub(super) fn knight_command(
    ability: KnightAbility,
    world: &ClientWorld,
    stamp: crate::outbox::Stamp,
) -> Result<ClientCommand, String> {
    if knight(world) != Some(ability) {
        return Err("Your class does not have that ability".into());
    }
    let target = world
        .target_intent()
        .filter(|target| target.spawn_id.is_some())
        .ok_or("Select a target for that first")?;
    Ok(ClientCommand::UseKnightAbility {
        session_id: stamp.session_id,
        ability,
        target,
        created: stamp.created,
    })
}

/// Uses an ability, telling the session whether the player stands in a
/// liquid; the session, or the outbox, says why one did not go.
pub(crate) fn use_ability(
    ability: Ability,
    online: &OnlineState,
    outbox: &Outbox,
    in_liquid: bool,
) {
    let _ = outbox.post(online.world(), |stamp| ClientCommand::UseAbility {
        session_id: stamp.session_id,
        ability,
        target: online.world().target_intent(),
        in_liquid,
        created: stamp.created,
    });
}

/// Keeps what each ability button needs in step with the ability it holds:
/// one the server type does not list is veiled, with the reason on hover.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn needs(
    online: Res<OnlineState>,
    choices: Res<Choices>,
    mut buttons: Query<(&AbilityButton, &mut crate::outbox::Needs)>,
) {
    use crate::outbox::Needs;
    for (button, mut needs) in &mut buttons {
        let wanted = assigned(online.world(), &choices, *button).map_or(
            Needs::Capability(eq_client_core::Capability::Abilities),
            |binding| crate::hud::hotbar::need(Some(binding)),
        );
        if *needs != wanted {
            *needs = wanted;
        }
    }
}

/// `/doability`'s refusal for a number outside 1-10 or none (0x46a7d1).
const NO_SUCH_ABILITY: u32 = 0x33c8;

/// The button `/doability N` presses (0x46a7d1, read R45): 1-6 the
/// Abilities page's, 7-10 the Combat page's. That 1-6 are the Abilities
/// page and 7-10 the Combat page is inferred from the program's adjacent
/// tables (recording checklist 6.487). The number is read as the program's
/// `atoi` reads it: leading digits, anything after them ignored.
pub(crate) fn numbered(words: &str) -> Option<AbilityButton> {
    let words = words.trim_start();
    let (sign, digits) = match words.as_bytes().first() {
        Some(b'-') => (-1, &words[1..]),
        Some(b'+') => (1, &words[1..]),
        _ => (1, words),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    let number = sign * digits[..end].parse::<i64>().unwrap_or(0);
    match number {
        1..=6 => Some(AbilityButton {
            page: Page::Abilities,
            index: usize::try_from(number - 1).ok()?,
        }),
        7..=10 => Some(AbilityButton {
            page: Page::Combat,
            index: usize::try_from(number - 7).ok()?,
        }),
        _ => None,
    }
}

/// What pressing an ability button does, by click or by `/doability`: its
/// ability goes through the session, which says why one did not. Nothing
/// happens for an empty button or one holding what the client cannot use.
fn press(
    button: AbilityButton,
    online: &OnlineState,
    outbox: &Outbox,
    chat: &mut crate::chat::ChatState,
    choices: &Choices,
    in_liquid: bool,
) {
    match assigned(online.world(), choices, button) {
        Some(Binding::Ability(ability)) => {
            use_ability(ability, online, outbox, in_liquid);
        }
        Some(Binding::KnightAbility(ability)) => {
            let Ok(stamp) = outbox.stamp(online.world()) else {
                return;
            };
            match knight_command(ability, online.world(), stamp) {
                Ok(command) => {
                    let _ = outbox.send(online.world(), command);
                }
                Err(reason) => chat.refuse(reason),
            }
        }
        // The same use as `/discipline` and the Combat Abilities window.
        Some(Binding::Discipline(spell)) => {
            chat.disciplines.push(crate::disciplines::Asked::Use(spell));
        }
        _ => (),
    }
}

/// A clicked ability button uses its ability; an empty one opens the
/// skills picker for it, and one holding what the client cannot use does
/// nothing (6.449). `/doability N` presses the same button, except that an
/// empty one does nothing, as the program returns there with no line.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn input(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut chat: ResMut<crate::chat::ChatState>,
    buttons: crate::clicks::Clicks<AbilityButton>,
    liquid: crate::motion::Liquid,
    (choices, mut picker, messages): (
        Res<Choices>,
        ResMut<picker::Picker>,
        Option<Res<crate::hud::messages::Messages>>,
    ),
) {
    let in_liquid = || liquid.feet_in(online.world());
    for button in buttons.iter() {
        // An empty button opens the picker for it, as the program's does.
        if choices::empty(online.world(), &choices, *button) {
            picker.ask(*button);
            continue;
        }
        press(*button, &online, &outbox, &mut chat, &choices, in_liquid());
    }
    if let Some(words) = chat.doability.take() {
        match numbered(&words) {
            Some(button) if choices::empty(online.world(), &choices, button) => {}
            Some(button) => press(button, &online, &outbox, &mut chat, &choices, in_liquid()),
            None => {
                let empty = crate::hud::messages::Messages::default();
                let messages = messages.as_deref().unwrap_or(&empty);
                chat.refuse(messages.said_text(NO_SUCH_ABILITY, "Use /doability 1 through 10."));
            }
        }
    }
}

/// Names each button's ability, greyed while its timer runs.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn present(
    online: Res<OnlineState>,
    choices: Res<Choices>,
    names: Res<crate::spellbook::SpellNames>,
    mut labels: Query<(&AbilityLabel, &mut Text, &mut TextColor)>,
) {
    let now = std::time::Instant::now();
    for (AbilityLabel(button), mut text, mut color) in &mut labels {
        let binding = assigned(online.world(), &choices, *button);
        let wanted = choices::caption(online.world(), &choices, &names, *button);
        if text.0 != wanted {
            text.0 = wanted.into_owned();
        }
        // Dim while its timer runs; one the server does not offer is veiled
        // instead (see `needs`).
        let waiting = match binding {
            Some(Binding::Ability(ability)) => online.world().ability_wait(ability, now).is_some(),
            Some(Binding::KnightAbility(_)) => online
                .world()
                .casting()
                .knight_wait(now)
                .is_some_and(|wait| !wait.is_zero()),
            Some(Binding::Discipline(spell)) => {
                crate::disciplines::waits(online.world(), &names, spell, now)
            }
            _ => false,
        };
        let ink = if waiting {
            theme::INK_DIM
        } else {
            theme::INK_BRIGHT
        };
        if color.0 != ink {
            color.0 = ink;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;

    /// An admitted monk with every strike, taunt, feign death and mend.
    fn monk() -> OnlineState {
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        let mut skills = vec![0; 100];
        for ability in [
            Ability::Kick,
            Ability::FlyingKick,
            Ability::RoundKick,
            Ability::TigerClaw,
            Ability::EagleStrike,
            Ability::Taunt,
            Ability::FeignDeath,
            Ability::Mend,
        ] {
            skills[usize::try_from(ability.skill()).unwrap()] = 1;
        }
        player.skills = Some(skills);
        testing::admit(&mut online, 1, player);
        online
    }

    #[test]
    fn doability_numbers_one_to_six_the_abilities_page_and_seven_to_ten_combat() {
        let button = |page, index| Some(AbilityButton { page, index });
        assert_eq!(numbered("1"), button(Page::Abilities, 0));
        assert_eq!(numbered(" 6"), button(Page::Abilities, 5));
        assert_eq!(numbered("7"), button(Page::Combat, 0));
        assert_eq!(numbered("10"), button(Page::Combat, 3));
        // Read as atoi reads it.
        assert_eq!(numbered("3 then more"), button(Page::Abilities, 2));
        assert_eq!(numbered("+8"), button(Page::Combat, 1));
        for words in ["", "0", "11", "-1", "kick", "99999999999999999999"] {
            assert_eq!(numbered(words), None, "{words}");
        }
    }

    /// `/doability` runs what a click on the button runs, is silent on an
    /// empty button, and says the program's line for a number outside 1-10.
    #[test]
    fn doability_presses_the_button_and_is_silent_on_an_empty_one() {
        let mut app = crate::testing::app();
        let (queue, received) = std::sync::mpsc::sync_channel(4);
        app.insert_resource(monk())
            .insert_resource(Outbox::new(Some(queue)))
            .init_resource::<Choices>()
            .init_resource::<picker::Picker>()
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, input);
        let run = |app: &mut App, words: &str| {
            app.world_mut()
                .resource_mut::<crate::chat::ChatState>()
                .doability = Some(words.into());
            app.update();
        };
        // 7: the Combat page's first button, Kick for a monk.
        run(&mut app, "7");
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::UseAbility {
                ability: Ability::Kick,
                ..
            })
        ));
        assert!(received.try_recv().is_err());
        // A button chosen empty (the program's -1): no line, no send, no
        // picker.
        let empty = AbilityButton {
            page: Page::Abilities,
            index: 5,
        };
        let mut chosen = Choices::default();
        chosen.choose(monk().world(), empty, choices::Slot::Empty);
        app.insert_resource(chosen);
        let before = app
            .world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::All)
            .len();
        run(&mut app, "6");
        assert!(received.try_recv().is_err());
        assert_eq!(
            format!("{:?}", app.world().resource::<picker::Picker>()),
            format!("{:?}", picker::Picker::default())
        );
        let chat = app.world().resource::<crate::chat::ChatState>();
        assert_eq!(
            chat.history.lines(eq_client_core::chat::ChatTab::All).len(),
            before
        );
        for words in ["11", ""] {
            run(&mut app, words);
            assert!(received.try_recv().is_err());
            assert_eq!(
                app.world().resource::<crate::chat::ChatState>().newest(),
                "Use /doability 1 through 10."
            );
        }
    }

    #[test]
    fn a_knight_action_can_be_bound_from_the_combat_page() {
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.class = Some(3);
        player.level = 1;
        testing::admit(&mut online, 1, player);
        let button = AbilityButton {
            page: Page::Combat,
            index: 0,
        };
        assert_eq!(
            assigned(online.world(), &Choices::default(), button),
            Some(Binding::KnightAbility(KnightAbility::LayOnHands))
        );
    }

    #[test]
    fn a_session_without_knight_abilities_keeps_the_combat_page_unchanged() {
        use eq_client_core::{Capability, WorldEvent};
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.class = Some(3);
        player.level = 1;
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: Capability::ALL
                    .into_iter()
                    .filter(|capability| {
                        !matches!(
                            capability,
                            Capability::KnightAbilities | Capability::UsingDisciplines
                        )
                    })
                    .collect(),
                choices: Vec::new(),
                session_id: 1,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        let button = AbilityButton {
            page: Page::Combat,
            index: 0,
        };
        assert_ne!(
            assigned(online.world(), &Choices::default(), button),
            Some(Binding::KnightAbility(KnightAbility::LayOnHands))
        );
    }

    #[test]
    fn a_button_needs_the_ability_it_holds() {
        use crate::outbox::Needs;
        use eq_client_core::{Capability, WorldEvent};
        let mut online = monk();
        // The server offers everything but feign death.
        testing::news(
            &mut online,
            [WorldEvent::AbilitiesOffered(
                Ability::ALL
                    .into_iter()
                    .filter(|ability| *ability != Ability::FeignDeath)
                    .collect(),
            )],
        );
        let mut app = crate::testing::app();
        app.init_resource::<Choices>().add_systems(Update, needs);
        app.insert_resource(online);
        let button = |page, index| {
            (
                AbilityButton { page, index },
                Needs::Capability(Capability::Abilities),
            )
        };
        let kick = app.world_mut().spawn(button(Page::Combat, 0)).id();
        let feign = app.world_mut().spawn(button(Page::Abilities, 3)).id();
        let empty = app.world_mut().spawn(button(Page::Abilities, 6)).id();
        app.update();
        let needs = |entity| *app.world().get::<Needs>(entity).unwrap();
        assert_eq!(needs(kick), Needs::Ability(Ability::Kick));
        assert_eq!(needs(feign), Needs::Ability(Ability::FeignDeath));
        assert_eq!(needs(empty), Needs::Capability(Capability::Abilities));
        let world = app.world().resource::<OnlineState>().world();
        assert!(needs(kick).offered(world));
        assert!(!needs(feign).offered(world));
    }

    #[test]
    fn the_combat_page_takes_four_strikes_and_the_abilities_page_the_rest() {
        let online = monk();
        let on = |page, index| match assigned(
            online.world(),
            &Choices::default(),
            AbilityButton { page, index },
        ) {
            Some(Binding::Ability(ability)) => Some(ability),
            _ => None,
        };
        assert_eq!(
            (0..4)
                .map(|index| on(Page::Combat, index))
                .collect::<Vec<_>>(),
            [
                Some(Ability::Kick),
                Some(Ability::FlyingKick),
                Some(Ability::RoundKick),
                Some(Ability::TigerClaw)
            ]
        );
        assert_eq!(
            (0..7)
                .map(|index| on(Page::Abilities, index))
                .collect::<Vec<_>>(),
            [
                Some(Ability::EagleStrike),
                Some(Ability::Taunt),
                Some(Ability::Mend),
                Some(Ability::FeignDeath),
                // Anyone can bandage and fish, so they come last.
                Some(Ability::BindWound),
                Some(Ability::Fishing),
                None
            ]
        );
    }

    #[test]
    fn queued_abilities_name_the_displayed_authority_and_pending_choices_have_no_token() {
        use eq_client_core::{WorldEvent, targets::TargetState};
        let mut online = monk();
        testing::spawns(&mut online, vec![testing::pet(42, 7), testing::pet(43, 7)]);
        let (sender, receive) = std::sync::mpsc::sync_channel(8);
        let outbox = Outbox::new(Some(sender));
        let state = |revision, spawn_id| TargetState {
            revision,
            spawn_id,
            available: true,
        };
        for (revision, spawn_id) in [(1, Some(42)), (2, Some(43)), (3, None)] {
            let target = state(revision, spawn_id);
            testing::news(
                &mut online,
                [WorldEvent::TargetChanged {
                    session_id: 1,
                    request_id: 0,
                    state: target,
                }],
            );
            assert_eq!(online.world().target().selected, spawn_id);
            use_ability(Ability::Taunt, &online, &outbox, false);
            assert!(matches!(receive.try_recv().unwrap(),
                ClientCommand::UseAbility { target: Some(actual), .. } if actual == target));
            assert!(
                receive.try_recv().is_err(),
                "incoming projection never sends a target request"
            );
        }
        crate::target::select(&mut online, &outbox, Some(42));
        assert!(matches!(
            receive.try_recv().unwrap(),
            ClientCommand::SelectTarget {
                request_id: 1,
                spawn_id: Some(42),
                ..
            }
        ));
        use_ability(Ability::Taunt, &online, &outbox, false);
        assert!(matches!(
            receive.try_recv().unwrap(),
            ClientCommand::UseAbility { target: None, .. }
        ));
        assert_eq!(online.world().target().selected, Some(42));
        assert!(!online.world().target().sent);
        let target = state(4, Some(42));
        testing::news(
            &mut online,
            [WorldEvent::TargetSent {
                session_id: 1,
                request_id: 1,
                state: target,
            }],
        );
        use_ability(Ability::Taunt, &online, &outbox, false);
        assert!(matches!(receive.try_recv().unwrap(),
            ClientCommand::UseAbility { target: Some(actual), .. } if actual == target));
    }
}
