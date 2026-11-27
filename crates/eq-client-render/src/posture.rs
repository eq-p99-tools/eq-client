//! Ducked collision and the manual posture transition's clearance.
use crate::{Collision, Player, PlayerBody, ViewerSettings, online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{Posture, PostureState, world::ClientWorld};

/// One decision at launch for every host jump source. Both programs attempt
/// a tested stand while ducked, then launch even if it fails (read 2026-11-24).
/// The worker executes this answer without making a geometry decision of its
/// own. Passive motion and ordinary ducked moves never call this function.
pub(super) fn jump_posture(world: &ClientWorld, outbox: &Outbox) -> eq_client_core::JumpPosture {
    let ducked = world
        .player()
        .is_some_and(|player| world.posture(player.spawn_id) == Some(PostureState::Ducking));
    if ducked
        && world.generation().ducking().jump_attempts_stand
        && outbox.posture_allowed(world, Posture::Standing)
    {
        eq_client_core::JumpPosture::StandIfDucked
    } else {
        eq_client_core::JumpPosture::Keep
    }
}

/// The headroom test's S uses the same model scale as the held gate work:
/// default player `ROffset` 3.125 times the drawn scale, including the zone's
/// classic-scale flag. Posture never changes the origin or this value.
pub(super) fn stand_height(client: InstalledClient, world: &ClientWorld, body: &PlayerBody) -> f32 {
    let Some((player, flag)) = world.player().zip(world.classic_model_scale_flag()) else {
        return body.feet_offset;
    };
    let scale = client.model_scale().drawn_scale(
        player.race,
        player.gender,
        player.size,
        eq_client_assets::scale::ZoneScale {
            classic_reduced: flag != 0,
        },
    );
    eq_client_core::drawn_feet_offset(scale)
}

/// Read probe terms; the Titanium model radius remains the existing estimate.
/// Its engine's actor radius factor is not yet available to this host.
pub(super) fn headroom_probe(
    client: InstalledClient,
    world: &ClientWorld,
    stand: f32,
) -> eq_client_core::movement::HeadroomProbe {
    let mut probe = headroom_dimensions(client, stand);
    probe.permits_missing_floor = match client {
        InstalledClient::EqMac => false,
        InstalledClient::Titanium | InstalledClient::Rof2 => {
            matches!(world.zone_number(), Some(0x47 | 0x4b | 0x96))
        }
    };
    probe
}

pub(super) fn headroom_dimensions(
    client: InstalledClient,
    stand: f32,
) -> eq_client_core::movement::HeadroomProbe {
    let mac = match client {
        InstalledClient::EqMac => true,
        InstalledClient::Titanium | InstalledClient::Rof2 => false,
    };
    let radius = if mac { 0.25 * stand } else { 0.1 * stand };
    eq_client_core::movement::HeadroomProbe {
        // Both native floor helpers use .1 only for an exactly zero product.
        floor_radius: if radius == 0.0 { 0.1 } else { radius },
        ceiling_radius: if mac {
            0.5
        } else {
            eq_client_assets::falls::sphere_radius(stand)
        },
        floor_bias: 0.001,
        // Mac 0x504554's -.5 is cancelled by 0x4b3aa2's +.5.
        floor_offset: 0.0,
        retreat: if mac { 0.0 } else { 0.001 },
        permits_missing_floor: false,
        permits_missing_ceiling: !mac,
    }
}

/// Need and zero ducked lift are read on Titanium and Mac; the capsule
/// conversion is ours. Its additional native-radius bound uses the standing
/// sweep's one estimate; on Mac that bound is inferred, not a read actor shape.
/// `RoF2` follows Titanium provisionally, still unread.
pub(super) fn movement_height(
    client: InstalledClient,
    world: &ClientWorld,
    body: &PlayerBody,
    posture: Option<PostureState>,
) -> f32 {
    if posture != Some(PostureState::Ducking) {
        return body.height;
    }
    let Some(rule) = client.headroom() else {
        return body.height;
    };
    let stand = stand_height(client, world, body);
    eq_client_core::movement::ducked_height(
        stand,
        rule.room(stand, true, false),
        eq_client_assets::falls::sphere_radius(stand),
        body.height,
    )
}

/// Admission-local geometry shared by every command source. Query the current
/// world position at dispatch, rather than caching a permission at an old XYZ.
#[derive(Clone)]
pub(super) struct Context {
    admission: u64,
    client: InstalledClient,
    body: PlayerBody,
    collision: eq_client_core::movement::CollisionWorld,
    /// A resolved summon-mount buff is a proxy for the native actor link,
    /// which this client does not hold (the separate headroom work's gap).
    mounted: bool,
}

impl Context {
    pub(super) fn allows(&self, world: &ClientWorld, posture: Posture) -> bool {
        let Some(player) = world.player() else {
            return true;
        };
        if world.session_id() != Some(self.admission) {
            return true;
        }
        let current = world.posture(player.spawn_id);
        let ducking = match self.client {
            InstalledClient::EqMac => false,
            InstalledClient::Titanium | InstalledClient::Rof2 => {
                current.is_none_or(|posture| posture == PostureState::Standing)
                    && posture == Posture::Ducking
            }
        };
        let tested = match self.client {
            InstalledClient::EqMac => posture == Posture::Standing,
            // RoF2's manual transition follows Titanium provisionally.
            InstalledClient::Titanium | InstalledClient::Rof2 => {
                matches!(posture, Posture::Standing | Posture::Sitting)
            }
        };
        if !ducking && (current != Some(PostureState::Ducking) || !tested) {
            return true;
        }
        let Some(rule) = self.client.headroom() else {
            return true;
        };
        let stand = stand_height(self.client, world, &self.body);
        let origin = Vec3::from_array(eq_client_core::render_position(player.position));
        let probe = headroom_probe(self.client, world, stand);
        let bounds = self.collision.probe_headroom(origin, probe);
        if ducking {
            // Our origin never loses the native standing lift when ducking,
            // so a rejected transition needs no compensating origin raise.
            return probe.fits(bounds, rule.needed(stand, true, self.mounted), rule.margin);
        }
        probe.fits(bounds, rule.needed(stand, false, self.mounted), rule.margin)
            && bounds
                .ceiling
                .is_none_or(|ceiling| ceiling - origin.y >= stand)
    }
}

/// Refresh after scene collision reconciliation and before any typed, scripted
/// or pressed command. Losing the admission/body/geometry withdraws the context.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn refresh(
    online: Res<OnlineState>,
    settings: Res<ViewerSettings>,
    collision: Res<Collision>,
    players: Query<&PlayerBody, With<Player>>,
    outbox: Res<Outbox>,
    spells: Option<Res<crate::spellbook::SpellNames>>,
) {
    let mounted = on_mount(online.world(), spells.as_deref());
    let context = online
        .world()
        .session_id()
        .zip(players.single().ok())
        .zip(collision.0.as_ref())
        .map(|((admission, body), collision)| Context {
            admission,
            client: settings.0.installed_client,
            body: *body,
            collision: collision.clone(),
            mounted,
        });
    outbox.set_posture_context(context);
}

/// A resolved summon-mount buff stands in for the native actor mount link,
/// which this client does not hold. Unknown spells do not establish a mount.
pub(super) fn on_mount(world: &ClientWorld, spells: Option<&crate::spellbook::SpellNames>) -> bool {
    use eq_client_core::world::SpellCatalog;
    spells.is_some_and(|spells| {
        world.buffs().spells().any(|spell| {
            spells
                .effect_ids(spell)
                .is_some_and(|effects| effects.contains(&113))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{ClientCommand, WorldEvent, WorldPosition};
    use std::{sync::mpsc, time::Instant};

    const HUMAN: PlayerBody = PlayerBody {
        feet_offset: 3.75,
        height: 6.677_319_5,
        size: 6.0,
    };

    fn admitted(flag: Option<u8>) -> OnlineState {
        let mut state = OnlineState::new(true);
        let mut player = testing::player(7);
        player.race = 1;
        player.size = 6.0;
        player.position = WorldPosition {
            z: 3.75,
            ..WorldPosition::default()
        };
        testing::admit(&mut state, 11, player);
        testing::news(
            &mut state,
            [WorldEvent::Posture {
                spawn_id: 7,
                posture: PostureState::Ducking,
            }],
        );
        if let Some(flag) = flag {
            let mut header = [0; 700];
            header[592] = flag;
            testing::news(
                &mut state,
                [WorldEvent::ZoneDescription(
                    eq_client_core::zones::titanium_description(&header).unwrap(),
                )],
            );
        }
        state
    }

    fn context(client: InstalledClient, ceiling: f32) -> Context {
        let plane = |y| {
            [
                [[-20.0, y, -20.0], [20.0, y, 20.0], [20.0, y, -20.0]],
                [[-20.0, y, -20.0], [-20.0, y, 20.0], [20.0, y, 20.0]],
            ]
        };
        Context {
            admission: 11,
            client,
            body: HUMAN,
            mounted: false,
            collision: eq_client_core::movement::CollisionWorld::new(plane(0.0).into_iter().chain(
                plane(ceiling).map(|mut face| {
                    face.swap(1, 2);
                    face
                }),
            ))
            .unwrap(),
        }
    }

    #[test]
    fn standing_to_duck_is_tested_on_titanium_and_silent_for_every_command_source() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            // These are authored planes, not returned bounds: the floor bias
            // and Titanium's engine retreat also consume clearance. Exact
            // compared-value equality is covered by the core headroom tests.
            for (ceiling, clears) in [(5.25, false), (5.26, true)] {
                for scripted in [false, true] {
                    let mut state = admitted(Some(0));
                    testing::news(
                        &mut state,
                        [WorldEvent::Posture {
                            spawn_id: 7,
                            posture: PostureState::Standing,
                        }],
                    );
                    let (tx, rx) = mpsc::sync_channel(4);
                    let outbox = Outbox::new(Some(tx));
                    outbox.set_posture_context(Some(context(client, ceiling)));
                    let script = outbox.for_script(Some(11));
                    let sender = if scripted { &script } else { &outbox };
                    sender
                        .post(state.world(), |stamp| ClientCommand::SetPosture {
                            session_id: stamp.session_id,
                            spawn_id: 7,
                            posture: Posture::Ducking,
                            created: stamp.created,
                        })
                        .unwrap();
                    let sent = rx.try_recv().is_ok();
                    assert_eq!(sent, client == InstalledClient::EqMac || clears);
                    assert_eq!(state.world().posture(7), Some(PostureState::Standing));
                }
            }
        }
    }

    #[test]
    fn ducked_top_is_per_generation_and_standing_is_bitwise_unchanged() {
        let state = admitted(Some(0));
        for (client, top) in [
            (InstalledClient::Titanium, 5.25),
            (InstalledClient::EqMac, 4.4),
            (InstalledClient::Rof2, 5.25),
        ] {
            let height =
                movement_height(client, state.world(), &HUMAN, Some(PostureState::Ducking));
            assert!((height + 0.85 - top).abs() < 1e-5, "{client:?}: {height}");
            for posture in [
                None,
                Some(PostureState::Standing),
                Some(PostureState::Sitting),
            ] {
                assert_eq!(
                    movement_height(client, state.world(), &HUMAN, posture).to_bits(),
                    HUMAN.height.to_bits()
                );
            }
        }
        let classic = admitted(Some(1));
        let height = movement_height(
            InstalledClient::Titanium,
            classic.world(),
            &HUMAN,
            Some(PostureState::Ducking),
        );
        assert!((height + 0.85 - (3.75 / 1.3 + 1.5)).abs() < 1e-5);
        let mac = movement_height(
            InstalledClient::EqMac,
            classic.world(),
            &HUMAN,
            Some(PostureState::Ducking),
        );
        // Mac's model-scale table does not apply Titanium's zone reduction.
        assert!((mac + 0.85 - 4.4).abs() < 1e-5);
    }

    #[test]
    fn manual_and_script_transitions_share_the_silent_generation_specific_guard() {
        for client in [
            InstalledClient::Titanium,
            InstalledClient::EqMac,
            InstalledClient::Rof2,
        ] {
            for scripted in [false, true] {
                for target in [Posture::Standing, Posture::Sitting] {
                    for (ceiling, clears) in [(6.0, false), (9.0, true)] {
                        let state = admitted(Some(0));
                        let (tx, rx) = mpsc::sync_channel(4);
                        let outbox = Outbox::new(Some(tx));
                        outbox.set_posture_context(Some(context(client, ceiling)));
                        let script = outbox.for_script(Some(11));
                        let sender = if scripted { &script } else { &outbox };
                        assert!(
                            sender
                                .post(state.world(), |stamp| ClientCommand::SetPosture {
                                    session_id: stamp.session_id,
                                    spawn_id: 7,
                                    posture: target,
                                    created: Instant::now(),
                                })
                                .is_ok()
                        );
                        let allowed = clears
                            || client == InstalledClient::EqMac && target == Posture::Sitting;
                        assert_eq!(
                            rx.try_recv().is_ok(),
                            allowed,
                            "{client:?}, script={scripted}, {target:?}, ceiling={ceiling}"
                        );
                        assert_eq!(outbox.take_refused(), Vec::<crate::outbox::Refusal>::new());
                        assert_eq!(state.world().posture(7), Some(PostureState::Ducking));
                    }
                }
            }
        }
    }

    #[test]
    fn an_old_admissions_geometry_never_refuses_the_new_admissions_transition() {
        let state = admitted(None);
        let mut old_context = context(InstalledClient::Titanium, 6.0);
        old_context.admission = 10;
        assert!(old_context.allows(state.world(), Posture::Standing));
    }

    #[test]
    fn a_ducked_jump_carries_the_one_tested_stand_decision_on_both_generations() {
        for (client, generation) in [
            (
                InstalledClient::Titanium,
                eq_client_core::GameDialect::Titanium,
            ),
            (InstalledClient::EqMac, eq_client_core::GameDialect::EqMac),
        ] {
            for (ceiling, expected) in [
                (6.0, eq_client_core::JumpPosture::Keep),
                (9.0, eq_client_core::JumpPosture::StandIfDucked),
            ] {
                let mut state = admitted(Some(0));
                testing::generation(&mut state, generation);
                let (tx, rx) = mpsc::sync_channel(4);
                let outbox = Outbox::new(Some(tx));
                outbox.set_posture_context(Some(context(client, ceiling)));
                let answer = jump_posture(state.world(), &outbox);
                assert_eq!(answer, expected, "{client:?}, ceiling={ceiling}");
                outbox
                    .post(state.world(), |stamp| ClientCommand::Jump {
                        session_id: stamp.session_id,
                        posture: answer,
                        created: stamp.created,
                    })
                    .unwrap();
                assert!(
                    matches!(rx.try_recv(), Ok(ClientCommand::Jump { posture, .. }) if posture==expected)
                );
                assert!(rx.try_recv().is_err());
                assert_eq!(outbox.take_refused(), Vec::new());
                assert_eq!(state.world().posture(7), Some(PostureState::Ducking));
            }
        }
    }
}
