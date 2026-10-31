//! What the player's speeds depend on that only the host sees, which it
//! reports to the session on admission and whenever it changes: the
//! movement rate the buffs give, what the player carries and what they can
//! carry. The session works out the speeds from them with the generation's
//! rule, and grants no movement there before the first report.
use crate::{online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_core::ClientCommand;

/// The spell effect that changes how fast the player moves.
const MOVEMENT_SPEED: u32 = 3;
/// The spell effect that changes strength.
const STRENGTH: u32 = 4;
/// The spell effect that roots the player.
const ROOT: u32 = 99;
/// The movement rate the host reports for a rooted player: the program's
/// root marker, at or below which the forward input is 0.
const ROOTED: i16 = -1000;
/// The most carried weight the host reports.
const MOST_WEIGHT: f32 = 32_766.0;

/// The player's movement rate: the sum of the movement-speed effect's (3)
/// base values over the player's buffs, held to 16 bits, as the program's
/// signed total (TAKP speeds read, writer 0x4caaba; that the total is the
/// buffs' effect 3 was read at 0x4c6b6d's entry only, so it is labelled).
/// A buff that roots (effect 99) makes it the root marker, -1000 (inferred
/// from the 0x4cb30d path, recording checklist item 6.288). `effects` gives
/// a spell's effects as (id, base value); a buff whose spell the installed
/// data lacks counts as none.
pub(crate) fn movement_rate(
    slots: Option<&std::collections::BTreeMap<u32, eq_client_core::Buff>>,
    effects: impl Fn(u32) -> Option<Vec<(u32, i32)>>,
) -> i16 {
    let mut total = 0i64;
    for buff in slots.into_iter().flatten().map(|(_, buff)| buff) {
        for (id, base) in effects(buff.spell_id).unwrap_or_default() {
            match id {
                ROOT => return ROOTED,
                MOVEMENT_SPEED => total += i64::from(base),
                _ => (),
            }
        }
    }
    i16::try_from(total.clamp(i64::from(i16::MIN), i64::from(i16::MAX))).expect("held to 16 bits")
}

/// What the player carries, in the program's weight units, whole and held
/// to 0 through 32,766 ([`eq_client_core::world::ClientWorld::carried_weight`]).
pub(crate) fn carried_weight(world: &eq_client_core::world::ClientWorld) -> u16 {
    // Held to the range first.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let weight = world.carried_weight().clamp(0.0, MOST_WEIGHT) as u16;
    weight
}

/// What the player can carry before it slows them: the profile's strength
/// plus the strength effect's (4) base values over the player's buffs, so a
/// debuff lowers it, from 0. Unsure STR: the program's total also adds worn
/// items, and its level and ability terms are unread; leaving them out
/// never puts it above the program's, so it errs slow. `effects` is as
/// [`movement_rate`]'s.
pub(crate) fn capacity(
    player: &eq_client_core::PlayerState,
    slots: Option<&std::collections::BTreeMap<u32, eq_client_core::Buff>>,
    effects: impl Fn(u32) -> Option<Vec<(u32, i32)>>,
) -> Option<u32> {
    let base = i64::from(player.base_attributes?.strength);
    let buffs: i64 = slots
        .into_iter()
        .flatten()
        .flat_map(|(_, buff)| effects(buff.spell_id).unwrap_or_default())
        .filter(|(id, _)| *id == STRENGTH)
        .map(|(_, base)| i64::from(base))
        .sum();
    Some(u32::try_from((base + buffs).max(0)).unwrap_or(u32::MAX))
}

/// What the speeds depend on that only the host sees: the movement rate,
/// what the player carries and what they can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Inputs {
    rate: i16,
    weight: u16,
    capacity: u32,
}

/// Reports the speed inputs to the session on admission and whenever they
/// change: the buffs, the inventory, the coins or the profile. Capacity is
/// 0 only before the profile is in, which the session takes as unknown.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn report(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    spells: Res<crate::spellbook::SpellNames>,
    mut sent: Local<Option<(u64, Inputs)>>,
) {
    let world = online.world();
    let (Some(session), Some(player)) = (world.session_id(), world.player()) else {
        *sent = None;
        return;
    };
    let effects = |spell| {
        spells.mechanics(spell).map(|mechanics| {
            mechanics
                .effects
                .iter()
                .map(|effect| (effect.id, effect.base))
                .collect()
        })
    };
    let slots = world.buffs().slots();
    let inputs = Inputs {
        rate: movement_rate(slots, effects),
        weight: carried_weight(world),
        capacity: capacity(player, slots, effects).unwrap_or(0),
    };
    if *sent == Some((session, inputs)) {
        return;
    }
    let posted = outbox.post(world, |stamp| ClientCommand::SpeedInputs {
        session_id: stamp.session_id,
        rate: inputs.rate,
        weight: inputs.weight,
        capacity: inputs.capacity,
    });
    if posted.is_ok() {
        *sent = Some((session, inputs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::Buff;
    use std::collections::BTreeMap;

    fn buffs(spells: &[u32]) -> BTreeMap<u32, Buff> {
        spells
            .iter()
            .enumerate()
            .map(|(slot, &spell_id)| {
                (
                    u32::try_from(slot).unwrap(),
                    Buff {
                        spell_id,
                        caster_level: 1,
                        effect_type: 2,
                        bard_modifier: 10,
                        duration_ticks: 10,
                        counters: 0,
                        caster_id: 7,
                    },
                )
            })
            .collect()
    }

    /// Spell 1 raises speed and strength, 2 slows it and lowers strength,
    /// 3 does something else, 5 roots; 4 is not in the data.
    fn effects(spell: u32) -> Option<Vec<(u32, i32)>> {
        match spell {
            1 => Some(vec![(MOVEMENT_SPEED, 40), (STRENGTH, 10)]),
            2 => Some(vec![(0, -10), (MOVEMENT_SPEED, -35), (STRENGTH, -25)]),
            3 => Some(vec![(0, -10)]),
            5 => Some(vec![(ROOT, 0)]),
            _ => None,
        }
    }

    #[test]
    fn the_rate_sums_the_buffs_speed_effects_and_a_root_marks_it() {
        assert_eq!(movement_rate(None, effects), 0);
        assert_eq!(movement_rate(Some(&buffs(&[1, 3, 4])), effects), 40);
        assert_eq!(movement_rate(Some(&buffs(&[1, 2])), effects), 5);
        assert_eq!(movement_rate(Some(&buffs(&[1, 5])), effects), -1000);
        // Held to 16 bits.
        let fast = |_| Some(vec![(MOVEMENT_SPEED, 40_000)]);
        assert_eq!(movement_rate(Some(&buffs(&[1])), fast), i16::MAX);
    }

    #[test]
    fn capacity_is_the_profiles_strength_and_the_buffs() {
        let mut player = testing::player(7);
        assert_eq!(capacity(&player, None, effects), None);
        player.base_attributes = Some(eq_client_core::BaseAttributes {
            strength: 80,
            ..eq_client_core::BaseAttributes::default()
        });
        assert_eq!(capacity(&player, None, effects), Some(80));
        assert_eq!(capacity(&player, Some(&buffs(&[1, 2])), effects), Some(65));
        player.base_attributes = Some(eq_client_core::BaseAttributes {
            strength: 10,
            ..eq_client_core::BaseAttributes::default()
        });
        assert_eq!(capacity(&player, Some(&buffs(&[2])), effects), Some(0));
    }

    #[test]
    fn the_inputs_go_on_admission_and_again_only_when_they_change() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, report);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        app.insert_resource(online);
        app.update();
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::SpeedInputs {
                session_id: 1,
                rate: 0,
                weight: 0,
                capacity: 0
            })
        ));
        app.update();
        assert!(rx.try_recv().is_err());
        // A new admission reports again.
        testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            2,
            testing::player(7),
        );
        app.update();
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::SpeedInputs { session_id: 2, .. })
        ));
    }
}
