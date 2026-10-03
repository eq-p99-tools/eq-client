//! Admission-scoped buff state, independent of rendering and local spell assets.
use crate::{Buff, BuffUpdate, SpellEffect};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// How long a server tick lasts: the unit buff durations count in.
pub const TICK: Duration = Duration::from_secs(6);

/// How long a buff or effect has left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeLeft {
    /// It has no set end, as a permanent buff or an aura's.
    Lasting,
    /// About this long, counted down from when it started: the server's
    /// ticks do not keep to the client's clock, so it can be a tick out.
    About(Duration),
}

impl TimeLeft {
    /// What is left at `now` of this many ticks, counted from `since`; a
    /// negative count, as a server gives a buff with no end, lasts.
    #[must_use]
    pub fn of_ticks(ticks: i64, since: Instant, now: Instant) -> Self {
        u64::try_from(ticks).map_or(Self::Lasting, |ticks| {
            let whole = Duration::from_secs(ticks.saturating_mul(TICK.as_secs()));
            Self::About(whole.saturating_sub(now.saturating_duration_since(since)))
        })
    }
}

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
    /// A buff in the server slot the button stands for, and that slot.
    Slot(u32, &'a Buff),
    /// An effect the server has given no slot, which takes one of the long
    /// window's empty buttons, in the order the effects landed.
    Unplaced(&'a SpellEffect),
}

impl Shown<'_> {
    /// The spell of the buff or effect.
    #[must_use]
    pub fn spell_id(self) -> u32 {
        match self {
            Self::Slot(_, buff) => buff.spell_id,
            Self::Unplaced(effect) => u32::from(effect.spell_id),
        }
    }
}

/// Server slots and observed lasting effects awaiting explicit slot information.
#[derive(Clone, Debug, Default)]
pub struct BuffTracker {
    slots: Option<BTreeMap<u32, Buff>>,
    /// When the server last gave each slot's buff, which its ticks count
    /// down from.
    given: BTreeMap<u32, Instant>,
    effects: BTreeMap<u16, SpellEffect>,
    /// The effects without a slot, by spell, in the order they landed.
    landed: Vec<u16>,
    /// When each effect without a slot last landed.
    landed_at: BTreeMap<u16, Instant>,
    /// How many slots the admission's buff table holds: the long window's.
    long_slots: u32,
}

impl BuffTracker {
    /// Starts a new authoritative snapshot from the admission's buff table,
    /// which holds this many slots, given at `now`, discarding
    /// previous-admission effects.
    pub fn replace_snapshot(&mut self, slots: BTreeMap<u32, Buff>, long_slots: u32, now: Instant) {
        self.given = slots.keys().map(|slot| (*slot, now)).collect();
        self.slots = Some(slots);
        self.long_slots = long_slots;
        self.effects.clear();
        self.landed.clear();
        self.landed_at.clear();
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
            EffectWindow::Short => {
                let slot = self.long_slots.checked_add(button)?;
                slots.get(&slot).map(|buff| Shown::Slot(slot, buff))
            }
            EffectWindow::Long if button >= self.long_slots => None,
            EffectWindow::Long => slots
                .get(&button)
                .map(|buff| Shown::Slot(button, buff))
                .or_else(|| {
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

    /// How long the buff in a server slot has left at `now`: the ticks the
    /// server last gave it, counted down since.
    #[must_use]
    pub fn time_left(&self, slot: u32, now: Instant) -> Option<TimeLeft> {
        let buff = self.slots.as_ref()?.get(&slot)?;
        let given = self.given.get(&slot)?;
        Some(TimeLeft::of_ticks(
            i64::from(buff.duration_ticks),
            *given,
            now,
        ))
    }

    /// When an effect without a slot last landed, which the spell's own
    /// duration counts down from.
    #[must_use]
    pub fn landed(&self, spell: u16) -> Option<Instant> {
        self.landed_at.get(&spell).copied()
    }

    /// Records an icon-bearing action at `now`, after the caller excludes
    /// known instant spells. A refresh invalidates the old duration without
    /// inventing a replacement slot.
    pub fn observe_effect(&mut self, effect: SpellEffect, now: Instant) {
        if effect.effect_flag != 4 || matches!(effect.spell_id, 0 | u16::MAX) {
            return;
        }
        if let Some(slots) = &mut self.slots {
            slots.retain(|_, buff| buff.spell_id != u32::from(effect.spell_id));
            self.given.retain(|slot, _| slots.contains_key(slot));
        }
        let spell = effect.spell_id;
        self.landed_at.insert(spell, now);
        if self.effects.insert(spell, effect).is_none() {
            self.landed.push(spell);
        }
    }

    /// Applies an explicit replacement or fade, given at `now`, after entity
    /// filtering by the session. A fade removes its unslotted effect too;
    /// repeated empty-slot fades are harmless.
    pub fn apply(&mut self, update: BuffUpdate, now: Instant) {
        if let Ok(id) = u16::try_from(update.spell_id)
            && self.effects.remove(&id).is_some()
        {
            self.landed.retain(|landed| *landed != id);
            self.landed_at.remove(&id);
        }
        if let Some(slots) = &mut self.slots {
            if let Some(buff) = update.buff {
                slots.insert(update.slot, buff);
                self.given.insert(update.slot, now);
            } else {
                slots.remove(&update.slot);
                self.given.remove(&update.slot);
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
        state.replace_snapshot(BTreeMap::from([(3, buff(42)), (25, buff(43))]), 25, now());
        let spell = |state: &BuffTracker, window, button| {
            state.in_window(window, button).map(Shown::spell_id)
        };
        assert_eq!(spell(&state, EffectWindow::Long, 3), Some(42));
        assert_eq!(spell(&state, EffectWindow::Short, 0), Some(43));
        // A long window's button past the table shows nothing.
        assert_eq!(spell(&state, EffectWindow::Long, 25), None);
        assert!(state.has_short());
        state.apply(update(25, 43, false), now());
        assert!(!state.has_short());
        // Effects without a slot take the empty buttons, as they landed.
        state.observe_effect(effect(51), now());
        state.observe_effect(effect(50), now());
        assert_eq!(spell(&state, EffectWindow::Long, 0), Some(51));
        assert_eq!(spell(&state, EffectWindow::Long, 1), Some(50));
        assert_eq!(spell(&state, EffectWindow::Long, 2), None);
        assert_eq!(spell(&state, EffectWindow::Long, 3), Some(42));
        // A fade frees the button for the next.
        state.apply(update(9, 51, false), now());
        assert_eq!(spell(&state, EffectWindow::Long, 0), Some(50));
    }

    fn now() -> Instant {
        Instant::now()
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
        state.apply(update(3, 42, true), now());
        state.apply(update(3, 42, false), now());
        // EQEmu follows its identified fade with an empty-slot fade.
        state.apply(update(3, u32::MAX, false), now());
        state.observe_effect(effect(43), now());
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(state.effects().len(), 1);
        state.apply(update(3, 43, true), now());
        assert!(state.effects().is_empty());
        assert_eq!(state.slots().unwrap(), &BTreeMap::from([(3, buff(43))]));
    }

    #[test]
    fn unslotted_fades_and_refreshes_do_not_leave_duplicate_icons() {
        let mut state = BuffTracker::empty_snapshot();
        state.apply(update(3, 42, true), now());
        state.observe_effect(effect(42), now());
        state.observe_effect(effect(42), now());
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(state.effects().len(), 1);
        state.apply(update(3, 42, false), now());
        state.apply(update(3, 42, false), now());
        assert!(state.effects().is_empty());
    }

    #[test]
    fn observed_p99_self_recast_leaves_one_unslotted_effect() {
        let mut state = BuffTracker::empty_snapshot();
        let mut begin = effect(42);
        begin.effect_flag = 0;
        // First cast: begin action, then landed action; no slot update followed.
        state.observe_effect(begin.clone(), now());
        state.observe_effect(effect(42), now());
        // Recast: begin action, identified fade of slot 0, empty-slot fade, landed action.
        state.observe_effect(begin, now());
        state.apply(update(0, 42, false), now());
        assert!(state.effects().is_empty());
        state.apply(update(0, u32::MAX, false), now());
        state.observe_effect(effect(42), now());
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(state.effects().keys().copied().collect::<Vec<_>>(), [42]);
    }

    #[test]
    fn a_buff_counts_down_from_when_it_was_given_or_landed() {
        let start = Instant::now();
        let later = |seconds| start + Duration::from_secs(seconds);
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(3, buff(42))]), 25, start);
        // Ten ticks given at the start leave a minute, then less.
        assert_eq!(state.time_left(3, start), Some(TimeLeft::About(TICK * 10)));
        assert_eq!(
            state.time_left(3, later(45)),
            Some(TimeLeft::About(Duration::from_secs(15)))
        );
        assert_eq!(
            state.time_left(3, later(90)),
            Some(TimeLeft::About(Duration::ZERO))
        );
        // The server giving the slot again starts it over.
        state.apply(update(3, 42, true), later(30));
        assert_eq!(
            state.time_left(3, later(30)),
            Some(TimeLeft::About(TICK * 10))
        );
        // A negative count has no end.
        let mut lasting = buff(43);
        lasting.duration_ticks = -1;
        state.apply(
            BuffUpdate {
                buff: Some(lasting),
                ..update(4, 43, true)
            },
            later(30),
        );
        assert_eq!(state.time_left(4, later(9000)), Some(TimeLeft::Lasting));
        state.apply(update(4, 43, false), later(31));
        assert_eq!(state.time_left(4, later(31)), None);
        // An effect without a slot keeps when it last landed.
        state.observe_effect(effect(50), later(10));
        state.observe_effect(effect(50), later(20));
        assert_eq!(state.landed(50), Some(later(20)));
        state.apply(update(9, 50, false), later(21));
        assert_eq!(state.landed(50), None);
        // A refresh of a slotted spell drops the slot's count with it.
        state.observe_effect(effect(42), later(40));
        assert_eq!(state.time_left(3, later(40)), None);
        assert_eq!(state.landed(42), Some(later(40)));
    }

    #[test]
    fn new_admission_discards_old_effects_and_preserves_snapshot_holes() {
        let mut state = BuffTracker::empty_snapshot();
        state.observe_effect(effect(42), now());
        state.clear();
        assert!(state.slots().is_none());
        assert!(state.effects().is_empty());
        state.replace_snapshot(BTreeMap::from([(6, buff(43))]), 25, now());
        assert_eq!(
            state.slots().unwrap().keys().copied().collect::<Vec<_>>(),
            [6]
        );
        let mut non_buff = effect(44);
        non_buff.effect_flag = 0;
        state.observe_effect(non_buff, now());
        assert!(state.effects().is_empty());
    }
}
