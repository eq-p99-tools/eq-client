//! The speed modifier the player's buffs give, which the host reports to
//! the session on admission and whenever it changes: the session works out
//! the player's speeds from it where it knows the generation's rule, and
//! grants no movement there before the first report.
//!
//! The program's sum of its buffs' modifiers is unread, so the report is
//! the slower bound: 0 where no buff carries a movement-speed effect (3)
//! below 0, raised speeds left out, and the installed client's slowed value
//! where one does. Both are provisional: they stand in for the host's
//! modifier until the +0xbc writer formula is read (Decompile)
//! ([`eq_client_assets::ui::InstalledClient::slowed_speed_modifier`]). A
//! buff whose spell the installed data lacks counts as none.
use crate::{online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_core::ClientCommand;

/// The spell effect that changes how fast the player moves.
const MOVEMENT_SPEED: u32 = 3;

/// Whether a buff on the player slows them: one whose spell carries a
/// movement-speed effect below 0, where `effects` gives a spell's effects
/// as (id, base value).
pub(crate) fn slowed(
    slots: Option<&std::collections::BTreeMap<u32, eq_client_core::Buff>>,
    effects: impl Fn(u32) -> Option<Vec<(u32, i32)>>,
) -> bool {
    slots.into_iter().flatten().any(|(_, buff)| {
        effects(buff.spell_id).is_some_and(|effects| {
            effects
                .iter()
                .any(|&(id, base)| id == MOVEMENT_SPEED && base < 0)
        })
    })
}

/// Reports the modifier to the session on admission and whenever the
/// player's buffs change it. A slowed player on a client whose slowed value
/// is unread reports nothing new.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn report(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    (settings, spells): (
        Res<crate::ViewerSettings>,
        Res<crate::spellbook::SpellNames>,
    ),
    mut sent: Local<Option<(u64, f32)>>,
) {
    let world = online.world();
    let Some(session) = world.session_id() else {
        *sent = None;
        return;
    };
    let slowed = slowed(world.buffs().slots(), |spell| {
        spells.mechanics(spell).map(|mechanics| {
            mechanics
                .effects
                .iter()
                .map(|effect| (effect.id, effect.base))
                .collect()
        })
    });
    let modifier = if slowed {
        settings.0.installed_client.slowed_speed_modifier()
    } else {
        Some(0.0)
    };
    let Some(modifier) = modifier else {
        return;
    };
    if *sent == Some((session, modifier)) {
        return;
    }
    let posted = outbox.post(world, |stamp| ClientCommand::SpeedModifier {
        session_id: stamp.session_id,
        modifier,
    });
    if posted.is_ok() {
        *sent = Some((session, modifier));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_assets::ui::InstalledClient;
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

    /// Spell 1 raises speed, 2 slows it, 3 does something else; 4 is not in
    /// the data.
    fn effects(spell: u32) -> Option<Vec<(u32, i32)>> {
        match spell {
            1 => Some(vec![(MOVEMENT_SPEED, 40)]),
            2 => Some(vec![(0, -10), (MOVEMENT_SPEED, -35)]),
            3 => Some(vec![(0, -10)]),
            _ => None,
        }
    }

    #[test]
    fn only_a_buff_that_lowers_speed_slows_the_player() {
        assert!(!slowed(None, effects));
        assert!(!slowed(Some(&buffs(&[1, 3, 4])), effects));
        assert!(slowed(Some(&buffs(&[1, 2])), effects));
    }

    #[test]
    fn the_modifier_goes_on_admission_and_again_only_when_it_changes() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, report);
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = InstalledClient::EqMac;
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        app.insert_resource(online);
        app.update();
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::SpeedModifier { session_id: 1, modifier }) if modifier == 0.0
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
            Ok(ClientCommand::SpeedModifier { session_id: 2, .. })
        ));
    }
}
