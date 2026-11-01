//! The inspector's explicit Target action shares the scene's submission path.
use super::{MapView, entities};
use crate::{chat::ChatState, online::OnlineState, outbox::Outbox, theme, tooltip::Tooltip};
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct TargetButton;

fn choice(online: &OnlineState, view: &MapView) -> Result<u16, &'static str> {
    let selected =
        entities::selected(online.world(), view).ok_or("This entity is no longer present")?;
    let id = selected.identity().id;
    if let Some(reason) = crate::target::unavailable(online, id) {
        return Err(reason);
    }
    Ok(id)
}

/// Only an explicit button click targets. Hover, pin and Track stay local.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn input(
    clicks: crate::clicks::Clicks<TargetButton>,
    view: Res<MapView>,
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    mut chat: ResMut<ChatState>,
) {
    if !clicks.any() {
        return;
    }
    match choice(&online, &view) {
        Ok(id) => crate::target::select(&mut online, &outbox, Some(id)),
        Err(reason) => chat.refuse(reason),
    }
}

/// Show availability and the current gameplay target without disabling inspection.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(crate) fn show(
    view: Res<MapView>,
    online: Res<OnlineState>,
    mut buttons: Query<
        (
            &mut Text,
            &mut TextColor,
            &mut BackgroundColor,
            &mut Tooltip,
        ),
        With<TargetButton>,
    >,
) {
    let choice = choice(&online, &view);
    let targeted = entities::selected(online.world(), &view)
        .is_some_and(|entity| online.world().target().selected == Some(entity.identity().id));
    let enabled = choice.is_ok();
    let label = if targeted { "Targeted" } else { "Target" };
    let reason = choice
        .err()
        .unwrap_or("Select as your gameplay target; does not attack or move.");
    for (mut text, mut ink, mut background, mut tooltip) in &mut buttons {
        if text.0 != label {
            label.clone_into(&mut text.0);
        }
        ink.set_if_neq(TextColor(if enabled {
            theme::INK_BRIGHT
        } else {
            theme::INK_DIM
        }));
        background.set_if_neq(BackgroundColor(if !enabled {
            theme::BUTTON_OFF
        } else if targeted {
            theme::BUTTON_ON
        } else {
            theme::BUTTON
        }));
        if tooltip.0 != reason {
            reason.clone_into(&mut tooltip.0);
        }
    }
}

#[cfg(test)]
mod tests;
