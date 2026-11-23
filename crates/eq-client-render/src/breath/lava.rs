//! The feet hazards from the character's actual body origin: ordinary
//! type-250 heat in lava and type-246 cold in Velious water.
//!
//! Titanium's liquid routine 0x41d4ba takes the origin's region kind once a
//! tick: 3 or 7 is lava, 4 or 8 is cold (reads R49-3 b and R49-26), so a
//! tick reports at most one of them, on the one clock. The routine also
//! takes the rune absorbs (effects 55, 78, 162 and 163) off a cold amount
//! (R49-3 b). This client doesn't model them for any hazard: it reports
//! the unabsorbed amount and the server applies its own, a departure owed
//! (checklist 6.491d).
use super::modifiers::{self, Need};
use crate::{motion::LiquidBasis, online::OnlineState, spellbook::SpellNames};
use eq_client_assets::regions::{Medium, RegionKind};
use eq_client_core::{
    Capability,
    environment::{Tick, cold_damage, lava_damage},
    hazards::LiquidHazard,
    qol::{Fix, Settings},
};
use std::time::Instant;

/// The feet hazard a region kind gives, where the session takes it.
fn feet_hazard(kind: RegionKind, can: impl Fn(Capability) -> bool) -> Option<LiquidHazard> {
    if kind.medium() == Medium::Lava {
        can(Capability::LavaHeat).then_some(LiquidHazard::Lava)
    } else if kind.is_cold() {
        can(Capability::ColdWater).then_some(LiquidHazard::Cold)
    } else {
        None
    }
}

/// The once-a-tick feet hazard check, shared by lava's heat and Velious
/// water's cold.
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
        movement: LiquidBasis,
    ) -> Option<(LiquidHazard, u32)> {
        let world = online.world();
        let eligible = qol.on(Fix::TakeEnvironmentalDamage)
            && (world.can(Capability::LavaHeat) || world.can(Capability::ColdWater))
            && world.in_world()
            && !online.scene_loading();
        if eligible && movement != LiquidBasis::Observed {
            // The result establishes contact for the next check. Leave the
            // clock due without evaluating heat at an unconfirmed position.
            return None;
        }
        let player = world.player();
        // Virtual surface support never supplies a sample: the region is
        // the origin's own.
        let hazard = player
            .filter(|_| eligible)
            .and_then(|player| {
                online
                    .regions
                    .kind_at(eq_client_core::render_position(player.position))
            })
            .and_then(|kind| feet_hazard(kind, |capability| world.can(capability)));
        let contact = world
            .session_id()
            .zip(player)
            .filter(|_| hazard.is_some())
            .map(|(admission, player)| (admission, player.spawn_id, online.placement_revision()));
        if !self.due(now, contact, qol) {
            return None;
        }
        let (player, hazard) = player.zip(hazard)?;
        let profile = player.titanium_environment?;
        let need = match hazard {
            LiquidHazard::Lava => Need::Heat,
            LiquidHazard::Cold => Need::Cold,
            LiquidHazard::Drowning | LiquidHazard::ZoneHeat => return None,
        };
        let inputs = modifiers::read(world, spells, player, need).ok()?;
        if inputs.invulnerable {
            return None;
        }
        let amount = match hazard {
            LiquidHazard::Lava => lava_damage(profile.zone_id, inputs.fire_resistance?),
            LiquidHazard::Cold => cold_damage(inputs.cold_resistance?),
            LiquidHazard::Drowning | LiquidHazard::ZoneHeat => None,
        }?;
        Some((hazard, amount))
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
    fn actual_body_heat_waits_for_the_move_result_and_rechecks_contact() {
        use crate::online::testing;
        use eq_client_assets::regions::{Liquid, ZoneRegions};
        use eq_client_core::{
            WorldEvent, WorldPosition, hazards::TitaniumEnvironment, inventory::InventoryUpdate,
        };

        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.position.z = -1.0;
        player.titanium_environment = Some(TitaniumEnvironment {
            zone_id: 27,
            base_fire_resistance: 25,
            base_cold_resistance: 25,
            has_alternate_advancement: false,
        });
        testing::admit(&mut online, 5, player);
        testing::inventory(&mut online, InventoryUpdate::Snapshot(Vec::new()));
        testing::buffs(&mut online, std::collections::BTreeMap::new());
        online.regions = ZoneRegions::test_liquid_surface(0.0, Liquid::Lava);
        let spells = SpellNames::default();
        let mut qol = Settings::default();
        qol.set(Fix::TakeEnvironmentalDamage, true);
        let mut heat = Heat::default();
        let start = Instant::now();
        // The program's tick runs strictly more than a second apart.
        let at = |seconds: u64| start + Duration::from_millis(seconds * 1_001);
        assert_eq!(
            heat.advance(at(0), &online, &qol, &spells, LiquidBasis::Observed),
            None
        );
        assert_eq!(
            heat.advance(at(1), &online, &qol, &spells, LiquidBasis::Pending),
            None
        );
        assert_eq!(
            heat.advance(at(1), &online, &qol, &spells, LiquidBasis::Observed),
            Some((LiquidHazard::Lava, 42))
        );
        assert_eq!(
            heat.advance(at(2), &online, &qol, &spells, LiquidBasis::Pending),
            None
        );
        assert_eq!(
            heat.advance(at(2), &online, &qol, &spells, LiquidBasis::Unconfirmed),
            None
        );
        assert_eq!(
            heat.advance(at(2), &online, &qol, &spells, LiquidBasis::Observed),
            Some((LiquidHazard::Lava, 42))
        );
        assert_eq!(
            heat.advance(at(2), &online, &qol, &spells, LiquidBasis::Observed),
            None
        );
        testing::news(
            &mut online,
            [WorldEvent::MotionSent {
                session_id: 5,
                created: at(3),
                position: WorldPosition {
                    z: 1.0,
                    ..Default::default()
                },
                refused: None,
            }],
        );
        assert_eq!(
            heat.advance(at(4), &online, &qol, &spells, LiquidBasis::Observed),
            None
        );
    }

    /// A player at `z` over a surface at 0 of `kind`, admitted with these
    /// capabilities, with a cold base of `cold` and fire base 25.
    fn feet_at(
        z: f32,
        kind: eq_client_assets::regions::RegionKind,
        capabilities: Vec<Capability>,
        (race, cold): (u32, i32),
    ) -> OnlineState {
        use crate::online::testing;
        use eq_client_assets::regions::ZoneRegions;
        use eq_client_core::{hazards::TitaniumEnvironment, inventory::InventoryUpdate};

        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.position.z = z;
        player.race = race;
        player.titanium_environment = Some(TitaniumEnvironment {
            zone_id: 27,
            base_fire_resistance: 25,
            base_cold_resistance: cold,
            has_alternate_advancement: false,
        });
        testing::enter_with(&mut online, 5, player, None, capabilities);
        testing::inventory(&mut online, InventoryUpdate::Snapshot(Vec::new()));
        testing::buffs(&mut online, std::collections::BTreeMap::new());
        online.regions = ZoneRegions::test_kind_surface(0.0, kind);
        online
    }

    /// What two ticks of the feet check report, a tick apart.
    fn ticks(online: &OnlineState, qol: &Settings) -> [Option<(LiquidHazard, u32)>; 2] {
        let spells = SpellNames::default();
        let mut heat = Heat::default();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_millis(seconds * 1_001);
        [0, 1].map(|second| heat.advance(at(second), online, qol, &spells, LiquidBasis::Observed))
    }

    #[test]
    fn velious_water_at_the_origin_is_cold_by_resist_on_the_lavas_clock() {
        let mut qol = Settings::default();
        qol.set(Fix::TakeEnvironmentalDamage, true);
        let all = Capability::ALL.to_vec();
        let vw = RegionKind::VELIOUS_WATER;
        // 50 − 30 / 3: one hazard a tick, on the same clock as lava.
        assert_eq!(
            ticks(&feet_at(-1.0, vw, all.clone(), (1, 30)), &qol),
            [None, Some((LiquidHazard::Cold, 40))]
        );
        assert_eq!(
            ticks(&feet_at(-1.0, vw, all.clone(), (1, 200)), &qol)[1],
            Some((LiquidHazard::Cold, 10))
        );
        // Lava at the origin is still heat, by fire resist (50 − 25 / 3).
        assert_eq!(
            ticks(&feet_at(-1.0, RegionKind::LAVA, all.clone(), (1, 30)), &qol)[1],
            Some((LiquidHazard::Lava, 42))
        );
        // A Barbarian's base is 35 whatever the profile says, and an
        // Iksar's is 10 under it (0x4188ef): 50 − 35 / 3 and 50 − 20 / 3.
        assert_eq!(
            ticks(&feet_at(-1.0, vw, all.clone(), (2, 1050)), &qol)[1],
            Some((LiquidHazard::Cold, 39))
        );
        assert_eq!(
            ticks(&feet_at(-1.0, vw, all.clone(), (128, 30)), &qol)[1],
            Some((LiquidHazard::Cold, 44))
        );
        // The origin above the water: Velious water at the feet alone
        // doesn't count.
        assert_eq!(
            ticks(&feet_at(1.0, vw, all.clone(), (1, 30)), &qol)[1],
            None
        );
        // Other water ticks nothing.
        for kind in [
            RegionKind::WATER,
            RegionKind::SLIME,
            RegionKind::WATER_2,
            RegionKind::WATER_3,
        ] {
            assert_eq!(
                ticks(&feet_at(-1.0, kind, all.clone(), (1, 30)), &qol)[1],
                None,
                "{kind:?}"
            );
        }
        // A session that doesn't take cold, and the QoL off, report none.
        let without: Vec<_> = Capability::ALL
            .into_iter()
            .filter(|capability| *capability != Capability::ColdWater)
            .collect();
        assert_eq!(ticks(&feet_at(-1.0, vw, without, (1, 30)), &qol)[1], None);
        qol.set(Fix::TakeEnvironmentalDamage, false);
        assert_eq!(ticks(&feet_at(-1.0, vw, all, (1, 30)), &qol), [None, None]);
    }

    #[test]
    fn a_region_kind_gives_one_feet_hazard_where_the_session_takes_it() {
        let every = |_| true;
        for (kind, hazard) in [
            (RegionKind(3), Some(LiquidHazard::Lava)),
            (RegionKind::LAVA, Some(LiquidHazard::Lava)),
            (RegionKind(4), Some(LiquidHazard::Cold)),
            (RegionKind::VELIOUS_WATER, Some(LiquidHazard::Cold)),
            (RegionKind::DRY, None),
            (RegionKind::WATER, None),
            (RegionKind::SLIME, None),
        ] {
            assert_eq!(feet_hazard(kind, every), hazard, "{kind:?}");
        }
        let no_heat = |capability| capability != Capability::LavaHeat;
        assert_eq!(feet_hazard(RegionKind::LAVA, no_heat), None);
        assert_eq!(
            feet_hazard(RegionKind::VELIOUS_WATER, no_heat),
            Some(LiquidHazard::Cold)
        );
    }

    #[test]
    fn gills_leave_heat_running_and_environment_prevention_stops_it() {
        let mut heat = Heat::default();
        let mut qol = Settings::default();
        let start = Instant::now();
        // The program's tick runs strictly more than a second apart.
        let at = |seconds: u64| start + Duration::from_millis(seconds * 1_001);
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
