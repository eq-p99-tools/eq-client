//! Availability feedback uses the same validation as the scribe action.
use super::{ScribeCursor, SpellNames, action_pending, prepare_scribe};
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct Label;

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn presentation(
    online: Option<Res<crate::online::OnlineState>>,
    outbox: Option<Res<crate::outbox::Outbox>>,
    names: Res<SpellNames>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor), With<ScribeCursor>>,
    mut labels: Query<(&mut Text, &mut TextColor), With<Label>>,
) {
    let world = crate::online::world(online.as_deref());
    let available = if action_pending(world) {
        Err(anyhow::anyhow!("Wait for the current spell action"))
    } else {
        let stamp = outbox.as_deref().and_then(|outbox| outbox.peek(world));
        prepare_scribe(online.as_deref(), world.spell_book(), stamp)
    };
    let enabled = available.is_ok();
    let label = match available {
        Ok(eq_client_core::ClientCommand::ScribeSpell { spell_id, .. }) => {
            format!("Scribe {}", names.label(spell_id))
        }
        Ok(_) => unreachable!("scribe validation only prepares a scribe command"),
        Err(error) => error.to_string(),
    };
    for (interaction, mut color) in &mut buttons {
        color.0 = if !enabled {
            Color::srgb(0.055, 0.065, 0.08)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.16, 0.24, 0.32)
        } else {
            Color::srgb(0.10, 0.16, 0.22)
        };
    }
    for (mut text, mut color) in &mut labels {
        if text.0 != label {
            text.0.clone_from(&label);
        }
        color.0 = if enabled {
            Color::srgb(0.9, 0.93, 0.96)
        } else {
            Color::srgb(0.55, 0.59, 0.64)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_and_pending_states_explain_why_scribing_is_disabled() {
        let mut app = App::new();
        app.init_resource::<crate::hud::HudState>()
            .init_resource::<SpellNames>()
            .add_systems(Update, presentation);
        let label = app
            .world_mut()
            .spawn((
                Label,
                Text::new("Scribe cursor scroll"),
                TextColor::default(),
            ))
            .id();
        let button = app
            .world_mut()
            .spawn((
                ScribeCursor,
                Interaction::Hovered,
                BackgroundColor::default(),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Text>(label).unwrap().0,
            "Connect to scribe a scroll"
        );
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        let (queue, _received) = std::sync::mpsc::sync_channel(1);
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(queue)))
            .init_resource::<crate::inventory::InventoryState>();
        app.update();
        assert_eq!(
            app.world().get::<Text>(label).unwrap().0,
            "Inventory awaiting refresh"
        );
        crate::online::testing::book_action(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::BookActionStatus::Preparing,
        );
        app.update();
        assert_eq!(
            app.world().get::<Text>(label).unwrap().0,
            "Wait for the current spell action"
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            Color::srgb(0.055, 0.065, 0.08)
        );
    }
}
