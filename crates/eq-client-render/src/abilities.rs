//! The abilities on the skin's Actions window, as the official client has
//! them: the Combat page's four buttons and the Abilities page's six. They
//! hold the abilities the player has, strikes and taunt first on the Combat
//! page and the rest after, each named on its button and greyed while its
//! recovery timer runs. A click uses one, and Ctrl and a number binds the one
//! under the pointer to the action bar, as gems and items bind. The session
//! says why an ability did not go.
use crate::{online::OnlineState, outbox::Outbox, theme};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand,
    abilities::Ability,
    disciplines::{Active, Discipline},
    hotbar::Binding,
    knights::KnightAbility,
    world::ClientWorld,
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

/// How many buttons the Combat page has for abilities.
const COMBAT_BUTTONS: usize = 4;

/// The ability a button holds, if the player has one for it.
pub(crate) fn assigned(world: &ClientWorld, button: AbilityButton) -> Option<Binding> {
    let (targeted, other): (Vec<Ability>, Vec<Ability>) = world
        .abilities()
        .into_iter()
        .partition(|ability| ability.at_target());
    // Only a session that offers them gives these their own buttons, so the
    // ordinary abilities keep their places everywhere else.
    let targeted = knight(world)
        .filter(|_| world.can(eq_client_core::Capability::KnightAbilities))
        .map(Binding::KnightAbility)
        .into_iter()
        .chain(
            Discipline::SUPPORTED
                .into_iter()
                .filter(|_| world.can(eq_client_core::Capability::UsingDisciplines))
                .filter(|&discipline| knows_discipline(world, discipline))
                .map(Binding::Discipline),
        )
        .chain(targeted.into_iter().map(Binding::Ability));
    match button.page {
        Page::Combat => targeted.take(COMBAT_BUTTONS).nth(button.index),
        Page::Abilities => targeted
            .skip(COMBAT_BUTTONS)
            .chain(other.into_iter().map(Binding::Ability))
            .nth(button.index),
    }
}

/// The class ability that belongs to the admitted player, if any.
pub(crate) fn knight(world: &ClientWorld) -> Option<KnightAbility> {
    let player = world.player()?;
    KnightAbility::for_player(player.class, player.level)
}

/// Whether the character learned this exact discipline and can use its level.
pub(crate) fn knows_discipline(world: &ClientWorld, discipline: Discipline) -> bool {
    world.player().is_some_and(|player| {
        discipline.learned(player.class, player.level, player.disciplines.as_deref())
    })
}

/// The discipline button and ordinary command share the same request path.
pub(super) fn discipline_command(
    discipline: Discipline,
    world: &ClientWorld,
    stamp: crate::outbox::Stamp,
) -> Result<ClientCommand, String> {
    if !knows_discipline(world, discipline) {
        return Err("The discipline is unavailable to your character".into());
    }
    Ok(ClientCommand::UseDiscipline {
        session_id: stamp.session_id,
        discipline,
        created: stamp.created,
    })
}

/// Whether the discipline buttons wait on received reuse or an active slot.
pub(crate) fn discipline_waits(
    world: &ClientWorld,
    discipline: Discipline,
    now: std::time::Instant,
) -> bool {
    matches!(world.casting().active_discipline(), Active::Titanium(_))
        || world
            .casting()
            .discipline_wait(discipline.timer(), now)
            .is_some_and(|wait| !wait.is_zero())
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
    mut buttons: Query<(&AbilityButton, &mut crate::outbox::Needs)>,
) {
    use crate::outbox::Needs;
    for (button, mut needs) in &mut buttons {
        let wanted = assigned(online.world(), *button).map_or(
            Needs::Capability(eq_client_core::Capability::Abilities),
            |binding| crate::hud::hotbar::need(Some(binding)),
        );
        if *needs != wanted {
            *needs = wanted;
        }
    }
}

/// A clicked ability button uses its ability.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn input(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut chat: ResMut<crate::chat::ChatState>,
    buttons: crate::clicks::Clicks<AbilityButton>,
    controls: Option<Res<crate::motion::Controls>>,
) {
    for button in buttons.iter() {
        match assigned(online.world(), *button) {
            Some(Binding::Ability(ability)) => {
                use_ability(
                    ability,
                    &online,
                    &outbox,
                    controls.as_ref().is_some_and(|controls| controls.in_liquid),
                );
            }
            Some(binding @ (Binding::KnightAbility(_) | Binding::Discipline(_))) => {
                let Ok(stamp) = outbox.stamp(online.world()) else {
                    continue;
                };
                let command = match binding {
                    Binding::KnightAbility(ability) => {
                        knight_command(ability, online.world(), stamp)
                    }
                    Binding::Discipline(discipline) => {
                        discipline_command(discipline, online.world(), stamp)
                    }
                    _ => unreachable!("the action was matched above"),
                };
                match command {
                    Ok(command) => {
                        let _ = outbox.send(online.world(), command);
                    }
                    Err(reason) => chat.refuse(reason),
                }
            }
            _ => (),
        }
    }
}

/// Names each button's ability, greyed while its timer runs.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn present(
    online: Res<OnlineState>,
    mut labels: Query<(&AbilityLabel, &mut Text, &mut TextColor)>,
) {
    let now = std::time::Instant::now();
    for (AbilityLabel(button), mut text, mut color) in &mut labels {
        let binding = assigned(online.world(), *button);
        let wanted = match binding {
            Some(Binding::Ability(ability)) => ability.name(),
            Some(Binding::KnightAbility(ability)) => ability.name(),
            Some(Binding::Discipline(discipline)) => discipline.name(),
            _ => "",
        };
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
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
            Some(Binding::Discipline(discipline)) => {
                discipline_waits(online.world(), discipline, now)
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
            assigned(online.world(), button),
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
            assigned(online.world(), button),
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
        app.add_systems(Update, needs);
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
        let on = |page, index| match assigned(online.world(), AbilityButton { page, index }) {
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
