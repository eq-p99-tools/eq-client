//! Titanium's shown loot window asks once to close, then awaits the server.
use super::{Panel, Rows, TradeState};
use crate::{
    ViewerConfig, ViewerSettings, breath::head::Basis, online::OnlineState, outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{ClientCommand, world::ClientWorld};
use std::time::{Duration, Instant};

const PERIOD: Duration = Duration::from_millis(500);

/// The actual prepared actor's model and scale, including a Human stand-in.
/// A marker, pending model or stale incarnation cannot supply native S.
#[derive(Component, Clone)]
pub(crate) struct Model {
    spawn_id: u16,
    revision: u64,
    zone_flag: Option<u32>,
    code: String,
    scale: f32,
}

impl Model {
    pub(crate) fn new(
        spawn_id: u16,
        revision: u64,
        zone_flag: Option<u32>,
        code: &str,
        scale: f32,
    ) -> Self {
        Self {
            spawn_id,
            revision,
            zone_flag,
            code: code.into(),
            scale,
        }
    }

    fn current(&self, world: &ClientWorld, corpse: u16) -> bool {
        self.spawn_id == corpse
            && world
                .spawn(corpse)
                .is_some_and(|spawn| spawn.revision == self.revision)
            && self.zone_flag.is_some()
            && self.zone_flag == world.classic_model_scale_flag()
            && self.scale.is_finite()
            && self.scale > 0.0
    }
}

#[derive(Default)]
pub(super) struct RangeClose {
    identity: Option<(u64, u16)>,
    model: Option<Model>,
    reference: Option<Basis>,
    reference_for: Option<String>,
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
        view: (bool, Option<&Model>),
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
        }
        if !view.0 || self.queued {
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
        if let Some(model) = view.1.filter(|model| model.current(world, corpse_id)) {
            self.model = Some(model.clone());
        }
        // Native S belongs to the spawn, so rendering it outside the view
        // need not discard a previously checked basis. Revision/zone-scale
        // changes still invalidate that basis until the new actor is known.
        let Some(model) = self
            .model
            .as_ref()
            .filter(|model| model.current(world, corpse_id))
        else {
            return;
        };
        if self.reference_for.as_deref() != Some(model.code.as_str()) {
            self.reference_for = Some(model.code.clone());
            self.reference = settings
                .eq_directory
                .as_deref()
                .and_then(|directory| Basis::load_model(directory, &model.code));
        }
        let Some(reference) = self.reference else {
            return;
        };
        let Some(offset) = reference.scaled(model.scale) else {
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
    models: Query<&Model>,
) {
    let shown = panels.iter().any(|(panel, node, visible)| {
        panel.0 == Rows::Loot
            && node.display != Display::None
            && visible.is_none_or(|visible| visible.get())
    });
    let model = online.world().loot().and_then(|loot| {
        models
            .iter()
            .find(|model| model.current(online.world(), loot.corpse_id))
    });
    trade.range.check(
        online.world(),
        &settings.0,
        &outbox,
        (shown, model),
        Instant::now(),
    );
}

#[cfg(test)]
mod tests;
