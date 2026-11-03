//! The core forwards local disposition; the requesting front end correlates it.
use super::*;
use crate::loot::{LootRequest, LootRequestStatus};

#[test]
fn loot_request_results_retain_original_context_without_changing_world_authority() {
    let mut world = admitted();
    world.open_loot(9);
    let created = Instant::now();
    let loot = world.loot().cloned();
    let inventory = world.inventory().items().clone();
    let coins = world.coins().copied();
    for (created, request) in [
        (Some(created), LootRequest::Open { corpse_id: 9 }),
        (
            Some(created),
            LootRequest::Item {
                corpse_id: 9,
                own_id: 7,
                place: 13,
                auto: false,
            },
        ),
        (None, LootRequest::Close { corpse_id: 9 }),
    ] {
        for status in [
            LootRequestStatus::Sent,
            LootRequestStatus::Refused("Local refusal".into()),
        ] {
            let changes = game(
                &mut world,
                WorldEvent::LootRequest {
                    session_id: 1,
                    created,
                    request,
                    status: status.clone(),
                },
            );
            assert_eq!(
                changes.replies,
                [Reply::LootRequest {
                    session_id: 1,
                    created,
                    request,
                    status
                }]
            );
            assert!(
                changes.notices.is_empty(),
                "only the pending owner may present it"
            );
            assert!(!changes.inventory && !changes.trade && !changes.ignored);
            assert!(changes.reset.is_none());
            assert_eq!(world.loot(), loot.as_ref());
            assert_eq!(world.inventory().items(), &inventory);
            assert_eq!(world.coins().copied(), coins);
        }
    }
}

#[test]
fn local_loot_results_from_an_earlier_admission_or_disconnected_session_are_ignored() {
    let mut world = admitted();
    world.open_loot(9);
    let loot = world.loot().cloned();
    for (session_id, connected) in [(2, true), (1, false)] {
        world.connected = connected;
        for request in [
            LootRequest::Open { corpse_id: 9 },
            LootRequest::Close { corpse_id: 9 },
        ] {
            let changes = game(
                &mut world,
                WorldEvent::LootRequest {
                    session_id,
                    created: None,
                    request,
                    status: LootRequestStatus::Refused("Old refusal".into()),
                },
            );
            assert!(changes.ignored);
            assert!(changes.replies.is_empty() && changes.notices.is_empty());
            assert_eq!(world.loot(), loot.as_ref());
        }
    }
}
