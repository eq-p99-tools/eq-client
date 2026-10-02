//! Admission-scoped buff state, independent of rendering and local spell assets.
use crate::{Buff, BuffUpdate, SpellEffect};
use std::collections::BTreeMap;

/// The official client's two effects windows, which split the server's buff
/// slots between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectWindow {
    /// The lasting effects: the slots of the admission's buff table.
    Long,
    /// The short ones, such as songs: the slots the server numbers after
    /// the table's.
    Short,
}

/// What a button of an effects window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown<'a> {
    /// A buff in the server slot the button stands for.
    Slot(&'a Buff),
    /// An effect the server has given no slot, which takes one of the long
    /// window's empty buttons, in the order the effects landed.
    Unplaced(&'a SpellEffect),
}

impl Shown<'_> {
    /// The spell of the buff or effect.
    #[must_use]
    pub fn spell_id(self) -> u32 {
        match self {
            Self::Slot(buff) => buff.spell_id,
            Self::Unplaced(effect) => u32::from(effect.spell_id),
        }
    }
}

/// Server slots and observed lasting effects awaiting explicit slot information.
#[derive(Clone, Debug, Default)]
pub struct BuffTracker {
    slots: Option<BTreeMap<u32, Buff>>,
    effects: BTreeMap<u16, SpellEffect>,
    /// The effects without a slot, by spell, in the order they landed.
    landed: Vec<u16>,
    /// How many slots the admission's buff table holds: the long window's.
    long_slots: u32,
}

impl BuffTracker {
    /// Starts a new authoritative snapshot from the admission's buff table,
    /// which holds this many slots, discarding previous-admission effects.
    pub fn replace_snapshot(&mut self, slots: BTreeMap<u32, Buff>, long_slots: u32) {
        self.slots = Some(slots);
        self.long_slots = long_slots;
        self.effects.clear();
        self.landed.clear();
    }

    /// What a window's button shows: the long window's buttons are the
    /// table's slots in order, the short window's the slots after them. An
    /// effect without a slot takes the long window's first empty button
    /// that no earlier one took, since a server need not say where a new
    /// buff went: `EQEmu` sends a landing buff's slot only for a level
    /// override, a hit counter or a duration past the spell's formula, at
    /// the next buff tick, and gives a new buff the first empty slot of its
    /// range, as here.
    #[must_use]
    pub fn in_window(&self, window: EffectWindow, button: u32) -> Option<Shown<'_>> {
        let slots = self.slots.as_ref()?;
        match window {
            EffectWindow::Short => slots
                .get(&self.long_slots.checked_add(button)?)
                .map(Shown::Slot),
            EffectWindow::Long if button >= self.long_slots => None,
            EffectWindow::Long => slots.get(&button).map(Shown::Slot).or_else(|| {
                let earlier = (0..button).filter(|slot| !slots.contains_key(slot)).count();
                let spell = self.landed.get(earlier)?;
                self.effects.get(spell).map(Shown::Unplaced)
            }),
        }
    }

    /// Whether the player has a short effect, such as a song.
    #[must_use]
    pub fn has_short(&self) -> bool {
        self.slots
            .as_ref()
            .is_some_and(|slots| slots.range(self.long_slots..).next().is_some())
    }

    /// Creates a tracker with a known empty admission snapshot.
    #[must_use]
    pub fn empty_snapshot() -> Self {
        Self {
            slots: Some(BTreeMap::new()),
            ..Self::default()
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
        let spell = effect.spell_id;
        if self.effects.insert(spell, effect).is_none() {
            self.landed.push(spell);
        }
    }

    /// Applies an explicit replacement or fade after entity filtering by the session.
    /// A fade removes its unslotted effect too; repeated empty-slot fades are harmless.
    pub fn apply(&mut self, update: BuffUpdate) {
        if let Ok(id) = u16::try_from(update.spell_id)
            && self.effects.remove(&id).is_some()
        {
            self.landed.retain(|landed| *landed != id);
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

    #[test]
    fn the_long_window_shows_the_tables_slots_and_the_short_one_those_after() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(3, buff(42)), (25, buff(43))]), 25);
        let spell = |state: &BuffTracker, window, button| {
            state.in_window(window, button).map(Shown::spell_id)
        };
        assert_eq!(spell(&state, EffectWindow::Long, 3), Some(42));
        assert_eq!(spell(&state, EffectWindow::Short, 0), Some(43));
        // A long window's button past the table shows nothing.
        assert_eq!(spell(&state, EffectWindow::Long, 25), None);
        assert!(state.has_short());
        state.apply(update(25, 43, false));
        assert!(!state.has_short());
        // Effects without a slot take the empty buttons, as they landed.
        state.observe_effect(effect(51));
        state.observe_effect(effect(50));
        assert_eq!(spell(&state, EffectWindow::Long, 0), Some(51));
        assert_eq!(spell(&state, EffectWindow::Long, 1), Some(50));
        assert_eq!(spell(&state, EffectWindow::Long, 2), None);
        assert_eq!(spell(&state, EffectWindow::Long, 3), Some(42));
        // A fade frees the button for the next.
        state.apply(update(9, 51, false));
        assert_eq!(spell(&state, EffectWindow::Long, 0), Some(50));
    }

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
    fn observed_p99_self_recast_leaves_one_unslotted_effect() {
        let mut state = BuffTracker::empty_snapshot();
        let mut begin = effect(42);
        begin.effect_flag = 0;
        // First cast: begin action, then landed action; no slot update followed.
        state.observe_effect(begin.clone());
        state.observe_effect(effect(42));
        // Recast: begin action, identified fade of slot 0, empty-slot fade, landed action.
        state.observe_effect(begin);
        state.apply(update(0, 42, false));
        assert!(state.effects().is_empty());
        state.apply(update(0, u32::MAX, false));
        state.observe_effect(effect(42));
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(state.effects().keys().copied().collect::<Vec<_>>(), [42]);
    }

    #[test]
    fn new_admission_discards_old_effects_and_preserves_snapshot_holes() {
        let mut state = BuffTracker::empty_snapshot();
        state.observe_effect(effect(42));
        state.clear();
        assert!(state.slots().is_none());
        assert!(state.effects().is_empty());
        state.replace_snapshot(BTreeMap::from([(6, buff(43))]), 25);
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
