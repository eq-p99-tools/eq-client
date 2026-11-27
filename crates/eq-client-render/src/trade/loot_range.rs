//! Titanium's shown loot window asks once to close, then awaits the server.
use super::{Panel, Rows, TradeState};
use crate::{
    ViewerConfig, ViewerSettings, breath::head::Basis, online::OnlineState, outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_assets::{
    scale::{TITANIUM, ZoneScale},
    ui::InstalledClient,
};
use eq_client_core::{ClientCommand, races, world::ClientWorld};
use std::time::{Duration, Instant};

const PERIOD: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(super) struct RangeClose {
    identity: Option<(u64, u16)>,
    reference: Option<Basis>,
    next: Option<Instant>,
    queued: bool,
}

impl RangeClose {
    pub(super) fn closing(&self, world: &ClientWorld) -> bool {
        self.queued && self.identity == identity(world)
    }

    pub(super) fn check(
        &mut self,
        world: &ClientWorld,
        settings: &ViewerConfig,
        outbox: &Outbox,
        shown: bool,
        now: Instant,
    ) {
        if settings.installed_client != InstalledClient::Titanium {
            *self = Self::default();
            return;
        }
        let current = identity(world);
        if self.identity != current {
            *self = Self {
                identity: current,
                ..Self::default()
            };
            self.reference = current.and_then(|(_, id)| {
                let spawn = &world.spawn(id)?.state;
                let model = races::model(spawn.race, spawn.gender)?;
                Basis::load_model(settings.eq_directory.as_deref()?, model)
            });
        }
        if !shown || self.queued {
            return;
        }
        if self.next.is_some_and(|next| now < next) {
            return;
        }
        self.next = Some(now + PERIOD);
        let Some((session_id, corpse_id)) = current else {
            return;
        };
        let Some(corpse) = world.spawn(corpse_id).map(|spawn| &spawn.state) else {
            return;
        };
        let Some(player) = world.player() else {
            return;
        };
        let Some(flag) = world.classic_model_scale_flag() else {
            return;
        };
        let Some(reference) = self.reference else {
            return;
        };
        if !corpse.size.is_finite() || corpse.size < 0.0 {
            return;
        }
        let scale = TITANIUM.drawn_scale(
            corpse.race,
            corpse.gender,
            corpse.size,
            ZoneScale {
                classic_reduced: flag != 0,
            },
        );
        let Some(offset) = reference.scaled(scale) else {
            return;
        };
        if !beyond(player.position, corpse.position, offset) {
            return;
        }
        // Queue acceptance is not a server reply. Retain the frame, all
        // rows and pending item custody; never resend an uncertain close.
        if outbox
            .send(
                world,
                ClientCommand::EndLoot {
                    session_id,
                    corpse_id,
                },
            )
            .is_ok()
        {
            self.queued = true;
        }
    }
}

fn identity(world: &ClientWorld) -> Option<(u64, u16)> {
    Some((
        world.session_id()?,
        world.loot().filter(|loot| loot.granted)?.corpse_id,
    ))
}

fn beyond(
    own: eq_client_core::WorldPosition,
    corpse: eq_client_core::WorldPosition,
    offset: f32,
) -> bool {
    let distance = (own.x - corpse.x).hypot(own.y - corpse.y);
    distance.is_finite() && offset.is_finite() && offset > 0.0 && distance > offset * 1.5 + 20.0
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(crate) fn input(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    settings: Res<ViewerSettings>,
    mut trade: ResMut<TradeState>,
    panels: Query<(&Panel, &Node, Option<&InheritedVisibility>)>,
) {
    let shown = panels.iter().any(|(panel, node, visible)| {
        panel.0 == Rows::Loot
            && node.display != Display::None
            && visible.is_none_or(InheritedVisibility::get)
    });
    trade
        .range
        .check(online.world(), &settings.0, &outbox, shown, Instant::now());
}

#[cfg(test)]
mod tests;
