//! Received own-HP counts reach the existing worker posture owner. This
//! host sends no raw posture, death, or predicted environmental HP here.
use crate::{online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, control::HitPointBasis, world::ClientWorld};
use std::time::{Duration, Instant};

type Submitted = Option<(u64, HitPointBasis, Instant)>;

#[allow(clippy::needless_pass_by_value)]
pub(super) fn report_hp(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    names: Option<Res<crate::spellbook::SpellNames>>,
    mut submitted: Local<Submitted>,
) {
    let world = online.world();
    let Some((mut basis, stamp)) = world.hit_point_basis().zip(outbox.peek(world)) else {
        *submitted = None;
        return;
    };
    basis.damage_floor = known_damage_floor(world, names.as_deref());
    if world.accepted_hit_point_basis() == Some(basis) {
        *submitted = None;
        return;
    }
    // Retry a dropped/refused count without filling the ordinary command
    // queue while its accepted-state echo is in flight.
    if submitted.is_some_and(|(admission, previous, sent)| {
        admission == stamp.session_id
            && previous == basis
            && sent.elapsed() < Duration::from_millis(250)
    }) {
        return;
    }
    if outbox.tell(
        world,
        ClientCommand::OwnHitPoints {
            session_id: stamp.session_id,
            basis,
            created: stamp.created,
        },
    ) {
        *submitted = Some((stamp.session_id, basis, Instant::now()));
    }
}

/// Resolve the unmodified cutoff only when effect 328 is proven absent.
/// Its nonzero formula and AA contribution are unread; unknown stays None.
fn known_damage_floor(
    world: &ClientWorld,
    names: Option<&crate::spellbook::SpellNames>,
) -> Option<i64> {
    let player = world.player()?;
    if player.titanium_environment?.has_alternate_advancement {
        return None;
    }
    let names = names?;
    let equipment = eq_client_core::resources::eqemu_equipped_modifiers(
        world.worker_inventory(),
        player.class?,
        player.race,
        u16::from(player.level),
    )
    .ok()?;
    world.buffs().slots()?;
    let spell_ids = world.buffs().spells().chain(
        equipment
            .iter()
            .filter_map(|(_, item)| item.worn.map(|worn| worn.spell_id)),
    );
    for spell in spell_ids {
        if names
            .mechanics(spell)?
            .effects
            .iter()
            .any(|effect| effect.id == 328)
        {
            return None;
        }
    }
    Some(-10)
}

/// The native gauge's fixed ARGB colours override the skin only on its
/// negative branches (0x5049fe). Normal HP restores the original tint.
pub(super) fn hp_tint(world: &ClientWorld) -> Option<Color> {
    match world.hit_point_gauge()?.value() {
        -2 => Some(Color::srgb_u8(128, 0, 0)),
        -110..=-10 => Some(Color::srgb_u8(128, 0, 128)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{WorldEvent, inventory::InventoryUpdate};
    use std::sync::mpsc;

    #[test]
    fn hp_count_uses_the_normal_outbox_and_waits_for_the_worker_echo() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::news(
            &mut online,
            [
                WorldEvent::Inventory(InventoryUpdate::Snapshot(Vec::new())),
                WorldEvent::HitPoints {
                    spawn_id: 7,
                    current: 0,
                    maximum: 100,
                    items: eq_client_core::ItemHitPoints::LeftOut,
                },
            ],
        );
        let (sender, queue) = mpsc::sync_channel(4);
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(Outbox::new(Some(sender)))
            .add_systems(Update, report_hp);
        app.update();
        app.update();
        let sent: Vec<_> = queue.try_iter().collect();
        assert!(matches!(
            sent[..],
            [ClientCommand::OwnHitPoints { session_id: 1, .. }]
        ));
        let basis = app
            .world()
            .resource::<OnlineState>()
            .world()
            .hit_point_basis()
            .unwrap();
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::OwnUnconscious {
                session_id: 1,
                revision: 1,
                unconscious: true,
                basis,
            }],
        );
        app.update();
        assert!(queue.try_recv().is_err());
        assert_eq!(hp_tint(app.world().resource::<OnlineState>().world()), None);
    }

    #[test]
    fn unconscious_outbox_refuses_jump_and_manual_stand_but_preserves_damage_reporting() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::news(
            &mut online,
            [
                WorldEvent::Inventory(InventoryUpdate::Snapshot(Vec::new())),
                WorldEvent::HitPoints {
                    spawn_id: 7,
                    current: 0,
                    maximum: 100,
                    items: eq_client_core::ItemHitPoints::LeftOut,
                },
            ],
        );
        let (sender, queue) = mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(sender));
        let created = Instant::now();
        assert_eq!(
            outbox.send(
                online.world(),
                ClientCommand::Jump {
                    session_id: 1,
                    posture: eq_client_core::JumpPosture::Keep,
                    created,
                }
            ),
            Err(crate::outbox::Refusal::Unconscious)
        );
        assert_eq!(
            outbox.send(
                online.world(),
                ClientCommand::SetPosture {
                    session_id: 1,
                    spawn_id: 7,
                    posture: eq_client_core::Posture::Standing,
                    created,
                }
            ),
            Err(crate::outbox::Refusal::Unconscious)
        );
        assert!(queue.try_recv().is_err());
        outbox
            .send(
                online.world(),
                ClientCommand::EnvironmentalDamage {
                    session_id: 1,
                    hazard: eq_client_core::hazards::Hazard::Falling,
                    amount: 2,
                    safe_fall: false,
                },
            )
            .unwrap();
        assert!(matches!(
            queue.try_recv(),
            Ok(ClientCommand::EnvironmentalDamage { .. })
        ));
    }
}
