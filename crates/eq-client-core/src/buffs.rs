//! Admission-scoped buff state, independent of rendering and local spell assets.
use crate::{Buff, BuffUpdate, SpellEffect};
use std::collections::BTreeMap;

/// Server slots and observed lasting effects awaiting explicit slot information.
#[derive(Clone, Debug, Default)]
pub struct BuffTracker {
    slots: Option<BTreeMap<u32, Buff>>,
    effects: BTreeMap<u16, SpellEffect>,
}

impl BuffTracker {
    /// Starts a new authoritative snapshot, discarding previous-admission effects.
    pub fn replace_snapshot(&mut self, slots: BTreeMap<u32, Buff>) {
        self.slots = Some(slots);
        self.effects.clear();
    }

    /// Creates a tracker with a known empty admission snapshot.
    #[must_use]
    pub fn empty_snapshot() -> Self {
        Self {
            slots: Some(BTreeMap::new()),
            effects: BTreeMap::new(),
        }
    }

    /// Clears all state on disconnect, death or a new zone admission.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Returns occupied server slots; `None` means no snapshot has arrived.
    #[must_use]
    pub fn slots(&self) -> Option<&BTreeMap<u32, Buff>> {
        self.slots.as_ref()
    }

    /// Returns observed effects whose server slot and duration are not supplied.
    #[must_use]
    pub fn effects(&self) -> &BTreeMap<u16, SpellEffect> {
        &self.effects
    }

    /// Records an icon-bearing action after the caller excludes known instant spells.
    /// A refresh invalidates the old duration without inventing a replacement slot.
    pub fn observe_effect(&mut self, effect: SpellEffect) {
        if effect.effect_flag != 4 || matches!(effect.spell_id, 0 | u16::MAX) {
            return;
        }
        if let Some(slots) = &mut self.slots {
            slots.retain(|_, buff| buff.spell_id != u32::from(effect.spell_id));
        }
        self.effects.insert(effect.spell_id, effect);
    }

    /// Applies an explicit replacement or fade after entity filtering by the session.
    /// A fade removes its unslotted effect too; repeated empty-slot fades are harmless.
    pub fn apply(&mut self, update: BuffUpdate) {
        if let Ok(id) = u16::try_from(update.spell_id) {
            self.effects.remove(&id);
        }
        if let Some(slots) = &mut self.slots {
            if let Some(buff) = update.buff {
                slots.insert(update.slot, buff);
            } else {
                slots.remove(&update.slot);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buff(spell_id: u32) -> Buff {
        Buff {
            spell_id,
            caster_level: 1,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks: 10,
            counters: 0,
            caster_id: 7,
        }
    }

    fn effect(spell_id: u16) -> SpellEffect {
        SpellEffect {
            spell_id,
            caster_level: 1,
            effect_flag: 4,
            instrument_modifier: 10,
            target_id: 7,
            caster_id: 7,
            spell_level: 1,
        }
    }

    fn update(slot: u32, spell_id: u32, present: bool) -> BuffUpdate {
        BuffUpdate {
            entity_id: 7,
            slot,
            spell_id,
            buff: present.then(|| buff(spell_id)),
        }
    }

    #[test]
    fn replacement_fade_then_action_then_slot_has_one_active_buff() {
        let mut state = BuffTracker::empty_snapshot();
        state.apply(update(3, 42, true));
        state.apply(update(3, 42, false));
        // EQEmu follows its identified fade with an empty-slot fade.
        state.apply(update(3, u32::MAX, false));
        state.observe_effect(effect(43));
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(state.effects().len(), 1);
        state.apply(update(3, 43, true));
        assert!(state.effects().is_empty());
        assert_eq!(state.slots().unwrap(), &BTreeMap::from([(3, buff(43))]));
    }

    #[test]
    fn unslotted_fades_and_refreshes_do_not_leave_duplicate_icons() {
        let mut state = BuffTracker::empty_snapshot();
        state.apply(update(3, 42, true));
        state.observe_effect(effect(42));
        state.observe_effect(effect(42));
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(state.effects().len(), 1);
        state.apply(update(3, 42, false));
        state.apply(update(3, 42, false));
        assert!(state.effects().is_empty());
    }

    #[test]
    fn new_admission_discards_old_effects_and_preserves_snapshot_holes() {
        let mut state = BuffTracker::empty_snapshot();
        state.observe_effect(effect(42));
        state.clear();
        assert!(state.slots().is_none());
        assert!(state.effects().is_empty());
        state.replace_snapshot(BTreeMap::from([(6, buff(43))]));
        assert_eq!(
            state.slots().unwrap().keys().copied().collect::<Vec<_>>(),
            [6]
        );
        let mut non_buff = effect(44);
        non_buff.effect_flag = 0;
        state.observe_effect(non_buff);
        assert!(state.effects().is_empty());
    }
}
