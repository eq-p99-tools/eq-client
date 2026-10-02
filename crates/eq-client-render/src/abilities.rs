//! The abilities on the skin's Actions window, as the official client has
//! them: the Combat page's four buttons and the Abilities page's six. They
//! hold the abilities the player has, strikes and taunt first on the Combat
//! page and the rest after, each named on its button and greyed while its
//! recovery timer runs. A click uses one, and Ctrl and a number binds the one
//! under the pointer to the action bar, as gems and items bind. The session
//! says why an ability did not go.
use crate::{online::OnlineState, outbox::Outbox, theme};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, abilities::Ability, world::ClientWorld};

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
pub(crate) fn assigned(world: &ClientWorld, button: AbilityButton) -> Option<Ability> {
    let (targeted, other): (Vec<Ability>, Vec<Ability>) = world
        .abilities()
        .into_iter()
        .partition(|ability| ability.at_target());
    match button.page {
        Page::Combat => targeted.get(button.index).copied(),
        Page::Abilities => targeted
            .iter()
            .skip(COMBAT_BUTTONS)
            .chain(&other)
            .nth(button.index)
            .copied(),
    }
}

/// Uses an ability; the session, or the outbox, says why one did not go.
pub(crate) fn use_ability(ability: Ability, online: &OnlineState, outbox: &Outbox) {
    let _ = outbox.post(online.world(), |stamp| ClientCommand::UseAbility {
        session_id: stamp.session_id,
        ability,
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
            Needs::Ability,
        );
        if *needs != wanted {
            *needs = wanted;
        }
    }
}

/// A pressed ability button uses its ability.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn input(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    buttons: Query<(&Interaction, &AbilityButton), Changed<Interaction>>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed
            && let Some(ability) = assigned(online.world(), *button)
        {
            use_ability(ability, &online, &outbox);
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
        let ability = assigned(online.world(), *button);
        let wanted = ability.map_or("", Ability::name);
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
        // Dim while its timer runs; one the server does not offer is veiled
        // instead (see `needs`).
        let waiting =
            ability.is_some_and(|ability| online.world().ability_wait(ability, now).is_some());
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
        let on = |page, index| assigned(online.world(), AbilityButton { page, index });
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
}
