//! Titanium's admitted head reference, separate from the movement capsule.
//!
//! Provenance: the head top at 6.2 model units times the drawn scale, the
//! three-quarter Froglok factor, the `ROffset` key of `Resources/moddat.ini`
//! and its 3.125 default are not yet cited to a Titanium program address in
//! this repository; treat them as inferred (recording checklist 6.393).
use super::Unavailable;
use bevy::prelude::Component;
use eq_client_assets::scale::{TITANIUM, ZoneScale};
use eq_client_core::{PlayerState, world::ClientWorld};
use std::path::Path;

/// The model's checked native reference above its soles before scaling.
#[derive(Clone, Copy, Component)]
pub(crate) struct Basis(f32);

impl Basis {
    /// Read the installed model metadata once as the zone prepares its body.
    /// A placeholder or unsupported model supplies no basis.
    pub(crate) fn load(
        directory: &Path,
        player: &PlayerState,
        model: Option<&str>,
    ) -> Option<Self> {
        if !matches!(player.race, 1..=12 | 128 | 130 | 330) || player.gender > 1 {
            return None;
        }
        let model = model?;
        let text = std::fs::read_to_string(directory.join("Resources").join("moddat.ini")).ok()?;
        reference(&text, model)
    }

    pub(super) fn for_player(
        self,
        world: &ClientWorld,
        player: &PlayerState,
    ) -> Result<f32, Unavailable> {
        if world.own_model_changed() {
            return Err(Unavailable::ModelChanged);
        }
        if world.spawn(player.spawn_id).is_some_and(|spawn| {
            let model = &spawn.state;
            (model.race, model.gender, model.size.to_bits())
                != (player.race, player.gender, player.size.to_bits())
        }) {
            return Err(Unavailable::ModelMismatch);
        }
        let flag = world
            .classic_model_scale_flag()
            .ok_or(Unavailable::MissingZoneScale)?;
        self.offset(player, flag)
            .ok_or(Unavailable::InvalidHeadOffset)
    }

    fn offset(self, player: &PlayerState, zone_flag: u32) -> Option<f32> {
        if !player.size.is_finite() || player.size < 0.0 {
            return None;
        }
        let scale = TITANIUM.drawn_scale(
            player.race,
            player.gender,
            player.size,
            ZoneScale {
                classic_reduced: zone_flag != 0,
            },
        );
        let top = 6.2 * scale * if player.race == 330 { 0.75 } else { 1.0 };
        let offset = top - self.0 * scale;
        (offset.is_finite() && offset > 0.0).then_some(offset)
    }
}

fn reference(text: &str, model: &str) -> Option<Basis> {
    let mut section = false;
    let mut offset = None;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.split(';').next()?.trim();
        if let Some(name) = line.strip_prefix('[') {
            section = name.strip_suffix(']')?.trim().eq_ignore_ascii_case(model);
        } else if section
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("roffset")
        {
            if offset.is_some() {
                return None;
            }
            let value = value.trim().parse::<f32>().ok()?;
            if !value.is_finite() || value <= 0.0 {
                return None;
            }
            offset = Some(value);
        }
    }
    // Titanium uses this authored reference when the key is absent.
    Some(Basis(offset.unwrap_or(3.125)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;

    #[test]
    fn native_head_uses_the_models_reference_and_zone_scale() {
        let mut player = testing::player(7);
        player.size = 6.5;
        let basis = reference("[Example]\nROffset=3.2\n[Other]\nROffset=1.0", "Example").unwrap();
        assert!((basis.offset(&player, 0).unwrap() - 3.9).abs() < 0.00001);
        assert!((basis.offset(&player, 1).unwrap() - 3.0).abs() < 0.00001);
        assert_eq!(
            reference("[Example]\nFPCOffset=1", "Example").unwrap().0,
            3.125
        );
        assert!(reference("[Example]\nROffset=invalid", "Example").is_none());
        assert!(Basis::load(Path::new("."), &player, None).is_none());
        player.size = 0.0;
        assert!((basis.offset(&player, 0).unwrap() - 3.6).abs() < 0.00001);
        player.size = f32::NAN;
        assert_eq!(basis.offset(&player, 0), None);
    }

    #[test]
    fn head_probe_distinguishes_missing_scale_from_a_conflicting_spawn() {
        use crate::online::OnlineState;

        let mut online = OnlineState::new(true);
        let player = testing::player(7);
        testing::admit(&mut online, 5, player.clone());
        let basis = Basis(3.125);
        assert_eq!(
            basis.for_player(online.world(), &player),
            Err(Unavailable::MissingZoneScale)
        );
        let mut spawn = testing::pet(7, 9);
        spawn.kind = eq_client_core::SpawnKind::Player;
        spawn.pet_owner = None;
        spawn.race = player.race;
        spawn.gender = player.gender;
        spawn.size = 8.0;
        testing::spawns(&mut online, vec![spawn]);
        assert_eq!(
            basis.for_player(online.world(), &player),
            Err(Unavailable::ModelMismatch)
        );
        testing::admit(&mut online, 6, player.clone());
        assert_eq!(
            basis.for_player(online.world(), &player),
            Err(Unavailable::MissingZoneScale)
        );
    }
}
