//! Pet window snapshots carry a spawn ID. The zone's spawn table supplies
//! ownership and the events that invalidate a binding to that ID.
use super::{ClientWorld, Spawn};
use crate::{SpawnKind, pets::PetBuffs};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct PetWindow {
    current: Option<PetBuffs>,
    early: Option<PetBuffs>,
    /// IDs whose earlier incarnation or ownership can no longer be inferred
    /// from an unbound snapshot. A fresh snapshot for the resolved pet works
    /// even when its ID is here.
    retired: BTreeSet<u16>,
    /// The session can publish staged world data after Entered and before Connected.
    pub(super) entering: bool,
}

impl PetWindow {
    fn receive(
        &mut self,
        buffs: &PetBuffs,
        owner: u16,
        current: Option<u16>,
        spawn: Option<&Spawn>,
    ) {
        if current == Some(buffs.pet) {
            self.current = Some(buffs.clone());
            if self.early_id() == Some(buffs.pet) {
                self.early = None;
            }
        } else if buffs.pet != 0
            && buffs.pet != owner
            && !self.retired.contains(&buffs.pet)
            && spawn.is_none_or(|spawn| can_belong(spawn, owner))
        {
            self.early = Some(buffs.clone());
        }
    }

    pub(super) fn for_pet(&self, id: u16) -> Option<&PetBuffs> {
        self.current.as_ref().filter(|buffs| buffs.pet == id)
    }

    fn early_id(&self) -> Option<u16> {
        self.early.as_ref().map(|buffs| buffs.pet)
    }

    /// Promote the early record only after the unique owned pet resolves.
    fn confirm(&mut self, id: Option<u16>) {
        if id.is_some() && id == self.early_id() {
            self.current = self.early.take();
        }
    }

    /// A despawn, corpse, replacement or ended ownership invalidates
    /// snapshots for that identity and any future unbound data for its ID.
    pub(super) fn retire(&mut self, id: u16) {
        self.retired.insert(id);
        if self.for_pet(id).is_some() {
            self.current = None;
        }
        if self.early_id() == Some(id) {
            self.early = None;
        }
    }

    pub(super) fn keep_only(&mut self, id: u16) {
        if self.for_pet(id).is_none() {
            self.current = None;
        }
        if self.early_id() != Some(id) {
            self.early = None;
        }
    }

    /// Stop admitting early snapshots across a pause in this admission.
    pub(super) fn suspend(&mut self) {
        self.early = None;
        self.entering = false;
    }

    pub(super) fn clear_current(&mut self) {
        self.current = None;
    }
}

fn can_belong(spawn: &Spawn, owner: u16) -> bool {
    matches!(spawn.state.kind, SpawnKind::Npc | SpawnKind::Player)
        && spawn.state.pet_owner.is_none_or(|actual| actual == owner)
}

impl ClientWorld {
    pub(super) fn receive_pet_buffs(&mut self, buffs: &PetBuffs) {
        let Some(owner) = self.player.as_ref().map(|player| player.spawn_id) else {
            return;
        };
        if !self.in_world() && !self.zone.pet_buffs.entering {
            return;
        }
        let pet = self.pet_of(owner).map(|spawn| spawn.state.spawn_id);
        let spawn = self.zone.spawns.get(&buffs.pet);
        self.zone.pet_buffs.receive(buffs, owner, pet, spawn);
    }

    /// Resolve any early snapshot against the zone's updated spawn table.
    pub(super) fn reconcile_pet_buffs(&mut self) {
        let Some(id) = self.zone.pet_buffs.early_id() else {
            return;
        };
        let Some(owner) = self.player.as_ref().map(|player| player.spawn_id) else {
            return;
        };
        if self
            .spawn(id)
            .is_some_and(|spawn| !can_belong(spawn, owner))
        {
            self.zone.pet_buffs.retire(id);
            return;
        }
        let pet = self.pet_of(owner).map(|spawn| spawn.state.spawn_id);
        self.zone.pet_buffs.confirm(pet);
    }
}
