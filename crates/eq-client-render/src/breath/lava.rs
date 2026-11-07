//! Ordinary type-250 heat from the character's actual body origin.
use super::modifiers::{self, Need};
use crate::{online::OnlineState, spellbook::SpellNames};
use eq_client_assets::regions::Medium;
use eq_client_core::{
    Capability,
    environment::{Tick, lava_damage},
    qol::{Fix, Settings},
};
use std::time::Instant;

#[derive(Default)]
pub(super) struct Heat {
    contact: Option<(u64, u16, u64)>,
    clock: Tick,
}

impl Heat {
    pub(super) fn advance(
        &mut self,
        now: Instant,
        online: &OnlineState,
        qol: &Settings,
        spells: &SpellNames,
    ) -> Option<u32> {
        let world = online.world();
        let player = world.player();
        let contact = world
            .session_id()
            .zip(player)
            .filter(|(_, player)| {
                world.in_world()
                    && world.can(Capability::LavaHeat)
                    && !online.scene_loading()
                    // Virtual surface support never supplies a heat sample.
                    && online.regions.medium_at(eq_client_core::render_position(player.position))
                        == Some(Medium::Lava)
            })
            .map(|(admission, player)| (admission, player.spawn_id, online.placement_revision()));
        if !self.due(now, contact, qol) {
            return None;
        }
        let player = player?;
        let profile = player.titanium_environment?;
        let inputs = modifiers::read(world, spells, player, Need::Heat).ok()?;
        if inputs.invulnerable {
            return None;
        }
        lava_damage(profile.zone_id, inputs.fire_resistance?)
    }

    fn due(&mut self, now: Instant, contact: Option<(u64, u16, u64)>, qol: &Settings) -> bool {
        let contact = contact.filter(|_| qol.on(Fix::TakeEnvironmentalDamage));
        if self.contact != contact {
            self.contact = contact;
            self.clock.interrupt();
        }
        contact.is_some() && self.clock.due(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn gills_leave_heat_running_and_environment_prevention_stops_it() {
        let mut heat = Heat::default();
        let mut qol = Settings::default();
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let contact = Some((5, 7, 1));

        assert!(!heat.due(at(0), contact, &qol));
        qol.set(Fix::GillsForDays, true);
        assert!(heat.due(at(1), contact, &qol));
        assert!(heat.due(at(10), contact, &qol));
        assert!(!heat.due(at(10), contact, &qol));

        // Above the lava, a virtual surface is still outside the volume.
        assert!(!heat.due(at(11), None, &qol));
        assert!(!heat.due(at(12), contact, &qol));
        assert!(heat.due(at(13), contact, &qol));

        qol.set(Fix::TakeEnvironmentalDamage, false);
        assert!(!heat.due(at(14), contact, &qol));
        qol.set(Fix::TakeEnvironmentalDamage, true);
        assert!(!heat.due(at(15), contact, &qol));
        assert!(heat.due(at(16), contact, &qol));

        assert!(!heat.due(at(17), Some((5, 7, 2)), &qol));
    }
}
