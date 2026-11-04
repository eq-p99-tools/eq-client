//! Opt-in corpse opening; transfers keep the ordinary reply-driven Loot All path.
use super::{LootWindow, PendingLoot, TradeState};
use crate::{
    online::OnlineState,
    outbox::{Outbox, Stamp},
};
use eq_client_core::{ClientCommand, SpawnKind, world::ClientWorld};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// Local automation policy, not a server permission or an ownership claim.
const REACH: f32 = 20.0;
const LIFETIME: Duration = Duration::from_secs(30);
const LIMIT: usize = 16;

#[derive(Default)]
pub(super) struct AutoLoot {
    candidates: VecDeque<Candidate>,
    pub(super) active: Option<(u64, u16)>,
}

struct Candidate {
    session_id: u64,
    corpse_id: u16,
    revision: u64,
    seen: Instant,
}

impl TradeState {
    /// Only newly received deaths naming this player as killer enter the queue.
    pub(crate) fn killed(&mut self, world: &ClientWorld, ids: &[u16]) {
        let Some(session_id) = world.session_id() else {
            return;
        };
        if !world.loot_continuation().automatic() {
            return;
        }
        for &corpse_id in ids {
            let Some(spawn) = world.spawn(corpse_id) else {
                continue;
            };
            if self.automatic.candidates.len() == LIMIT {
                self.automatic.candidates.pop_front();
            }
            self.automatic.candidates.push_back(Candidate {
                session_id,
                corpse_id,
                revision: spawn.revision,
                seen: Instant::now(),
            });
        }
    }

    pub(super) fn follow_automatic(&mut self, world: &ClientWorld, enabled: bool) {
        if !enabled || !world.in_world() || world.is_dead() {
            self.automatic.candidates.clear();
            if self.automatic.active.take().is_some()
                && let Some(window) = self.loot.as_mut()
            {
                window.loot_all = false;
            }
            return;
        }
        self.automatic.candidates.retain(|candidate| {
            candidate.session_id == world.session_id().unwrap_or_default()
                && candidate.seen.elapsed() < LIFETIME
                && world.spawn(candidate.corpse_id).is_some_and(|spawn| {
                    spawn.revision == candidate.revision && spawn.state.kind == SpawnKind::NpcCorpse
                })
        });
        if self.automatic.active.is_some_and(|(session, id)| {
            world.session_id() != Some(session)
                || world.loot().is_none_or(|loot| loot.corpse_id != id)
        }) {
            self.automatic.active = None;
        }
    }

    pub(super) fn open_automatic(
        &mut self,
        online: &mut OnlineState,
        outbox: &Outbox,
        stamp: Stamp,
    ) {
        let world = online.world();
        if self.loot.is_some()
            || world.loot().is_some()
            || world.merchant().is_some()
            || world.exchange().is_some()
            || world.is_stunned()
            || world.is_dead()
            || crate::spellbook::action_pending(world)
            || world.inventory().predicted()
            || world.inventory().stale()
            || !world.inventory().received()
            || !world.can(eq_client_core::Capability::Looting)
            || !world.loot_continuation().automatic()
            || world
                .inventory()
                .items()
                .contains_key(&eq_client_core::inventory::InventorySlot::CURSOR)
        {
            return;
        }
        let Some(player) = world.player() else { return };
        let Some(index) = self.automatic.candidates.iter().position(|candidate| {
            candidate.session_id == stamp.session_id
                && world.spawn(candidate.corpse_id).is_some_and(|spawn| {
                    spawn.revision == candidate.revision
                        && spawn.state.kind == SpawnKind::NpcCorpse
                        && eq_client_core::entities::within(
                            player.position,
                            spawn.state.position,
                            REACH,
                        )
                })
        }) else {
            return;
        };
        // One attempt per death. A local/server refusal never starts a retry loop.
        let candidate = self
            .automatic
            .candidates
            .remove(index)
            .expect("selected candidate");
        let corpse_id = candidate.corpse_id;
        let name = world
            .spawn(corpse_id)
            .map(|spawn| eq_client_core::entities::display_name(&spawn.state.name))
            .unwrap_or_default();
        if outbox
            .send(
                world,
                ClientCommand::Loot {
                    session_id: stamp.session_id,
                    corpse_id,
                    created: stamp.created,
                },
            )
            .is_err()
        {
            return;
        }
        online.open_loot(corpse_id);
        self.loot = Some(LootWindow {
            name,
            opening: Some(PendingLoot::new(
                stamp,
                eq_client_core::loot::LootRequest::Open { corpse_id },
            )),
            pending: None,
            loot_all: true,
        });
        self.automatic.active = Some((stamp.session_id, corpse_id));
        self.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::WorldEvent;

    fn fixture() -> (
        OnlineState,
        TradeState,
        Outbox,
        std::sync::mpsc::Receiver<ClientCommand>,
        Stamp,
    ) {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 4, testing::player(7));
        testing::news(
            &mut online,
            [WorldEvent::Inventory(
                eq_client_core::inventory::InventoryUpdate::Snapshot(Vec::new()),
            )],
        );
        let mut npc = testing::pet(9, 0);
        npc.pet_owner = None;
        testing::spawns(&mut online, vec![npc]);
        testing::news(
            &mut online,
            [WorldEvent::Death(eq_client_core::Death {
                spawn_id: 9,
                killer_id: 7,
                corpse_id: 9,
                bind_zone_id: 0,
                corpse_name: None,
            })],
        );
        let mut state = TradeState::default();
        state.killed(online.world(), &[9]);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let outbox = Outbox::new(Some(tx));
        let stamp = outbox.peek(online.world()).unwrap();
        (online, state, outbox, rx, stamp)
    }

    #[test]
    fn opening_is_once_and_disabling_stops_automatic_items() {
        let (mut online, mut state, outbox, rx, stamp) = fixture();
        state.follow_automatic(online.world(), true);
        state.open_automatic(&mut online, &outbox, stamp);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::Loot { corpse_id: 9, .. }
        ));
        assert!(state.loot.as_ref().unwrap().loot_all);
        assert!(online.world().loot().is_some());
        state.follow_automatic(online.world(), false);
        assert!(!state.loot.as_ref().unwrap().loot_all);
        assert!(online.world().loot().is_some());
        state.open_automatic(&mut online, &outbox, stamp);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn replaced_expired_far_and_old_admission_corpses_are_not_opened() {
        for case in 0..4 {
            let (mut online, mut state, outbox, rx, stamp) = fixture();
            match case {
                0 => {
                    let mut replacement = testing::pet(9, 0);
                    replacement.kind = SpawnKind::NpcCorpse;
                    testing::spawns(&mut online, vec![replacement]);
                }
                1 => {
                    state.automatic.candidates[0].seen =
                        Instant::now().checked_sub(LIFETIME).unwrap();
                }
                2 => {
                    let mut far = testing::player(7);
                    far.position.x = REACH + 1.0;
                    testing::news(
                        &mut online,
                        [WorldEvent::Position {
                            spawn_id: 7,
                            position: far.position,
                            velocity: [0.0; 3],
                            forward: 0.0,
                        }],
                    );
                }
                _ => testing::admit(&mut online, 5, testing::player(7)),
            }
            state.follow_automatic(online.world(), true);
            state.open_automatic(&mut online, &outbox, stamp);
            assert!(rx.try_recv().is_err(), "case {case}");
        }
    }
}
