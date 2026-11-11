//! Admission-scoped buff state, independent of rendering and local spell assets.
use crate::{Buff, BuffUpdate, SpellEffect};
pub use eq_network_game::buffs::{
    Applies, cancel,
    placement::{self, Band},
    scale_instrument_effect,
};
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shown<'a> {
    /// A buff in the server slot the button stands for, and that slot.
    Slot(u32, &'a Buff),
    /// An effect the server has given no slot, in the slot the program
    /// would give it (modelled from its slot finder, never received).
    Modelled(u32, &'a SpellEffect),
    /// An effect the server has given no slot and no slot could be
    /// modelled for, which takes one of the long window's empty buttons, in
    /// the order the effects landed.
    Unplaced(&'a SpellEffect),
}

impl Shown<'_> {
    /// The spell of the buff or effect.
    #[must_use]
    pub fn spell_id(self) -> u32 {
        match self {
            Self::Slot(_, buff) => buff.spell_id,
            Self::Modelled(_, effect) | Self::Unplaced(effect) => u32::from(effect.spell_id),
        }
    }
}

/// Why a landed effect has no modelled slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoSlot {
    /// The generation's program is unread for this, so no slot finder is
    /// ported ([`eq_network_game::GameDialect::buff_placement`]).
    NoFinder,
    /// The program's slot finder, as far as it is ported, names none.
    Unplaceable(placement::Unplaced),
    /// Another landed effect is held without a slot, so where the program's
    /// table has room is unknown until the next zone-in gives every slot.
    OccupancyUnknown,
    /// A held spell could be one the slot finder's unported step 4 would
    /// replace in its own slot: it shares an effect at a position both
    /// compare, or its effects are not known.
    PossibleReplacement,
}

/// What the client knows about a landing on the player, for placing it as
/// the Titanium program does (R3, R20).
#[derive(Clone, Copy, Debug, Default)]
pub struct Landing {
    /// The generation's slot finder, where its program is read
    /// ([`eq_network_game::GameDialect::buff_placement`]).
    pub finder: Option<placement::Finder>,
    /// The spell's band fields, `None` where the spell data has none.
    pub band: Option<Band>,
    /// Whether a held spell carries effect 148, step 2's blocker.
    pub blocker: bool,
    /// Whether a held spell could be one step 4 replaces ([`NoSlot::PossibleReplacement`]).
    pub conflict: bool,
    /// The record's type byte the program writes (R20 (d)): 4 for a spell
    /// of target type 13 or 20 the caster cast on themselves, else 2;
    /// `None` where the target type is not known.
    pub record_type: Option<u8>,
    /// The caster's level as the program records it, where known.
    pub caster_level: Option<u8>,
    /// Whether the spell carries effect 0x43, 0x65 or 0x71, on which step 3
    /// refuses a refresh and keeps the old record (R20 (g)).
    pub keeps_record: bool,
}

/// A landing held in a modelled slot.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Model {
    /// The record the program would hold, as far as it is read (R20 (d)).
    /// Fields not read, or not known to this client, are 0.
    record: Buff,
    /// Whether the record's type byte is known; one that isn't matches any.
    type_known: bool,
}

/// Server slots and observed lasting effects awaiting explicit slot information.
#[derive(Clone, Debug, Default)]
pub struct BuffTracker {
    revision: u64,
    cancellation: Option<cancel::Update>,
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
    /// The slot modelled for each landed effect that has one, by slot.
    placed: BTreeMap<u32, u16>,
    /// The record of each landed effect in a modelled slot, by spell.
    models: BTreeMap<u16, Model>,
    /// Why each landed effect without a modelled slot has none.
    unplaced: BTreeMap<u16, NoSlot>,
    /// How many times in this admission the server's word disagreed with
    /// the slots as held: a fade or update naming a slot other than the
    /// one the spell is held in, an update for a spell held nowhere, or a
    /// fade that left its spell held for its type byte.
    mismatches: u32,
}

impl BuffTracker {
    /// Starts a new authoritative snapshot from the admission's buff table,
    /// which holds this many slots, given at `now`, discarding
    /// previous-admission effects.
    pub fn replace_snapshot(&mut self, slots: BTreeMap<u32, Buff>, long_slots: u32, now: Instant) {
        self.revision = self.revision.wrapping_add(1);
        self.cancellation = None;
        self.given = slots.keys().map(|slot| (*slot, now)).collect();
        self.slots = Some(slots);
        self.long_slots = long_slots;
        self.effects.clear();
        self.landed.clear();
        self.landed_at.clear();
        self.placed.clear();
        self.models.clear();
        self.unplaced.clear();
        self.mismatches = 0;
    }

    /// What a window's button shows: the long window's buttons are the
    /// table's slots in order, the short window's the slots after them. A
    /// received buff shows in its slot, then a landed effect in the slot
    /// modelled for it. An effect that has neither takes the long window's
    /// first button that holds nothing and that no earlier one took.
    #[must_use]
    pub fn in_window(&self, window: EffectWindow, button: u32) -> Option<Shown<'_>> {
        let slots = self.slots.as_ref()?;
        let at = |slot: u32| {
            slots
                .get(&slot)
                .map(|buff| Shown::Slot(slot, buff))
                .or_else(|| {
                    let spell = self.placed.get(&slot)?;
                    self.effects
                        .get(spell)
                        .map(|effect| Shown::Modelled(slot, effect))
                })
        };
        match window {
            EffectWindow::Short => at(self.long_slots.checked_add(button)?),
            EffectWindow::Long if button >= self.long_slots => None,
            EffectWindow::Long => at(button).or_else(|| {
                let earlier = (0..button)
                    .filter(|slot| !slots.contains_key(slot) && !self.placed.contains_key(slot))
                    .count();
                let spell = self
                    .landed
                    .iter()
                    .filter(|spell| self.slot_of(**spell).is_none())
                    .nth(earlier)?;
                self.effects.get(spell).map(Shown::Unplaced)
            }),
        }
    }

    /// The slot modelled for a landed effect.
    #[must_use]
    pub fn slot_of(&self, spell: u16) -> Option<u32> {
        self.placed
            .iter()
            .find(|(_, placed)| **placed == spell)
            .map(|(slot, _)| *slot)
    }

    /// Why a landed effect has no modelled slot.
    #[must_use]
    pub fn unplaced_reason(&self, spell: u16) -> Option<NoSlot> {
        self.unplaced.get(&spell).copied()
    }

    /// How many times the server contradicted a modelled slot in this
    /// admission.
    #[must_use]
    pub const fn slot_mismatches(&self) -> u32 {
        self.mismatches
    }

    /// The landed effects with a modelled slot, by slot.
    #[must_use]
    pub const fn modelled(&self) -> &BTreeMap<u32, u16> {
        &self.placed
    }

    /// The record the program would hold for a landed effect in a modelled
    /// slot, as far as it is read (R20 (d)); unread fields are 0.
    #[must_use]
    pub fn modelled_record(&self, spell: u16) -> Option<&Buff> {
        self.models.get(&spell).map(|model| &model.record)
    }

    /// Number of slots in the admission table, which the long window shows.
    #[must_use]
    pub const fn long_slots(&self) -> u32 {
        self.long_slots
    }

    /// Whether the player has a short effect, such as a song, received or
    /// modelled.
    #[must_use]
    pub fn has_short(&self) -> bool {
        self.slots.as_ref().is_some_and(|slots| {
            slots.range(self.long_slots..).next().is_some()
                || self.placed.range(self.long_slots..).next().is_some()
        })
    }

    /// The landed effects without a modelled slot, in the order they
    /// landed, with why each has none (`None` before any admission table).
    pub fn unslotted(&self) -> impl Iterator<Item = (&SpellEffect, Option<NoSlot>)> + '_ {
        self.landed
            .iter()
            .filter(|spell| self.slot_of(**spell).is_none())
            .filter_map(|spell| Some((self.effects.get(spell)?, self.unplaced_reason(*spell))))
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

    /// Revision of received snapshots and own-player slot notifications.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// The last cancellation report, separate from received membership.
    #[must_use]
    pub const fn cancellation(&self) -> Option<&cancel::Update> {
        self.cancellation.as_ref()
    }

    /// Records an admission-filtered report without predicting membership.
    pub(crate) fn cancellation_news(&mut self, update: cancel::Update) {
        self.cancellation = Some(update);
    }

    /// Drops request progress while keeping received membership.
    pub(crate) fn clear_cancellation(&mut self) {
        self.cancellation = None;
    }

    /// Returns occupied server slots; `None` means no snapshot has arrived.
    #[must_use]
    pub fn slots(&self) -> Option<&BTreeMap<u32, Buff>> {
        self.slots.as_ref()
    }

    /// The spells of every buff the player holds: those in the server's
    /// slots, then those that landed with no slot given. What a buff does
    /// to the player (its speed, its strength) is read through this, so a
    /// buff counts whether or not the server said where it went.
    pub fn spells(&self) -> impl Iterator<Item = u32> + '_ {
        self.slots
            .iter()
            .flat_map(BTreeMap::values)
            .map(|buff| buff.spell_id)
            .chain(self.effects.keys().map(|spell| u32::from(*spell)))
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
    /// known instant spells, and models the slot the program gives it with
    /// the generation's slot finder (`landing.finder`) from what `landing`
    /// says. A refresh of a buff held in a slot keeps that slot, as the
    /// program's step 3 does, and takes the slot over from a received
    /// record, whose duration no longer holds; where step 3 refuses the
    /// refresh, the old record stays as it was. While another landed effect
    /// is held without a slot, where the program's table has room is
    /// unknown, so a new landing gets no slot: a wrong slot would go out in
    /// a click-off and could take off another buff.
    pub fn observe_effect(&mut self, effect: SpellEffect, landing: Landing, now: Instant) {
        if !effect.took_hold() || matches!(effect.spell_id, 0 | u16::MAX) {
            return;
        }
        let spell = effect.spell_id;
        let held: BTreeMap<u32, u32> = self
            .slots
            .iter()
            .flatten()
            .map(|(slot, buff)| (*slot, buff.spell_id))
            .chain(
                self.placed
                    .iter()
                    .map(|(slot, spell)| (*slot, u32::from(*spell))),
            )
            .collect();
        // The record step 3 would refresh: the spell's first slot's.
        let old = held
            .iter()
            .find(|(_, held)| **held == u32::from(spell))
            .and_then(|(slot, _)| {
                self.slots
                    .as_ref()
                    .and_then(|slots| slots.get(slot))
                    .map(|buff| (buff.clone(), true))
                    .or_else(|| {
                        let model = self.models.get(&spell)?;
                        Some((model.record.clone(), model.type_known))
                    })
            });
        let unslotted = self
            .landed
            .iter()
            .any(|landed| self.slot_of(*landed).is_none());
        let placed = self.slots.as_ref().map(|_| {
            let finder = landing.finder.ok_or(NoSlot::NoFinder)?;
            if old.is_none() {
                if landing.conflict {
                    return Err(NoSlot::PossibleReplacement);
                }
                if unslotted {
                    return Err(NoSlot::OccupancyUnknown);
                }
            }
            finder(&held, u32::from(spell), landing.band, landing.blocker)
                .map_err(NoSlot::Unplaceable)
        });
        if let Some(slots) = &mut self.slots {
            slots.retain(|_, buff| buff.spell_id != u32::from(spell));
            self.given.retain(|slot, _| slots.contains_key(slot));
        }
        self.placed.retain(|_, placed| *placed != spell);
        self.models.remove(&spell);
        self.unplaced.remove(&spell);
        match placed {
            Some(Ok(slot)) => {
                let caster_level = landing.caster_level.unwrap_or(0);
                // Step 3 refuses a refresh where the stored level is above
                // the caster's, or for effects 0x43, 0x65 and 0x71, and
                // keeps the old record (R20 (g)).
                let model = match old {
                    Some((record, type_known))
                        if landing.keeps_record || record.caster_level > caster_level =>
                    {
                        Model { record, type_known }
                    }
                    _ => Model {
                        record: Buff {
                            spell_id: u32::from(spell),
                            caster_level,
                            effect_type: landing.record_type.unwrap_or(2),
                            // A constant 10, not the action's modifier.
                            bard_modifier: 10,
                            // The action's duration is not read, and the
                            // program's own computation (0x410e5a,
                            // 0x40ebc3) is unread: 0.
                            duration_ticks: 0,
                            // The counters (+0x1d0/+0x1d4) and the caster's
                            // character id are not read: 0.
                            counters: 0,
                            caster_id: 0,
                        },
                        type_known: landing.record_type.is_some(),
                    },
                };
                self.placed.insert(slot, spell);
                self.models.insert(spell, model);
            }
            Some(Err(why)) => {
                self.unplaced.insert(spell, why);
            }
            None => {}
        }
        self.landed_at.insert(spell, now);
        if self.effects.insert(spell, effect).is_none() {
            self.landed.push(spell);
        }
    }

    /// Removes a landed effect, wherever it is held.
    fn end_effect(&mut self, spell: u16) {
        if self.effects.remove(&spell).is_some() {
            self.landed.retain(|landed| *landed != spell);
            self.landed_at.remove(&spell);
        }
        self.unplaced.remove(&spell);
        self.models.remove(&spell);
        self.placed.retain(|_, placed| *placed != spell);
    }

    /// Applies a buff update, given at `now`, after entity filtering by the
    /// session, as the receiving program applies it ([`Applies`]). Returns
    /// whether it is this admission's first disagreement between the
    /// server's word and the slots as held, which the caller reports once.
    pub fn apply(&mut self, update: BuffUpdate, now: Instant) -> bool {
        self.revision = self.revision.wrapping_add(1);
        let before = self.mismatches;
        match update.applies {
            Applies::Received => self.apply_received(update, now),
            Applies::FadeBySpell => self.fade_by_spell(&update),
            Applies::BySpell => self.update_by_spell(update, now),
            Applies::BySlot => self.write_slot(update, now),
        }
        before == 0 && self.mismatches > 0
    }

    fn mismatch(&mut self) {
        self.mismatches = self.mismatches.saturating_add(1);
    }

    /// The Mac-era reading: a fade removes its unslotted effect too, and one
    /// carrying the unknown-slot sentinel removes the slotted buff of its
    /// spell: TAKP fades a buff by its spell alone, as its own client finds
    /// it. Repeated empty-slot fades are harmless.
    fn apply_received(&mut self, update: BuffUpdate, now: Instant) {
        if let Ok(id) = u16::try_from(update.spell_id) {
            self.end_effect(id);
        }
        if let Some(slots) = &mut self.slots {
            if let Some(buff) = update.buff {
                slots.insert(update.slot, buff);
                self.given.insert(update.slot, now);
            } else if slots
                .get(&update.slot)
                .is_some_and(|buff| buff.spell_id == update.spell_id)
            {
                slots.remove(&update.slot);
                self.given.remove(&update.slot);
            } else if update.slot == eq_network_game::buffs::UNKNOWN_SLOT {
                slots.retain(|_, buff| buff.spell_id != update.spell_id);
                self.given.retain(|slot, _| slots.contains_key(slot));
            }
        }
    }

    /// Whether a fade or by-spell update of `record_type` reaches a slot of
    /// `held_type`: the program matches the type byte (R20 (e)).
    /// A type-4 record (target types 13 and 20 on oneself) therefore
    /// outlives a type-2 fade, as in the program: a known parity item,
    /// counted as a mismatch (recording checklist 6.459).
    fn same_type(held_type: u8, known: bool, record_type: u8) -> bool {
        held_type != 0 && (!known || held_type == record_type)
    }

    /// Titanium action 1: removes every slot holding the spell with the
    /// record's type byte, and the unslotted effect of the spell. A held
    /// slot of another type stays, as in the program, and counts as a
    /// mismatch.
    fn fade_by_spell(&mut self, update: &BuffUpdate) {
        let Ok(id) = u16::try_from(update.spell_id) else {
            return;
        };
        if matches!(id, 0 | u16::MAX) {
            return;
        }
        let record_type = update.record_type;
        let mut elsewhere = false;
        let mut stuck = false;
        if let Some(slots) = &mut self.slots {
            slots.retain(|slot, buff| {
                if buff.spell_id != update.spell_id {
                    return true;
                }
                let goes = Self::same_type(buff.effect_type, true, record_type);
                elsewhere |= goes && *slot != update.slot;
                stuck |= !goes;
                !goes
            });
            self.given.retain(|slot, _| slots.contains_key(slot));
        }
        if let Some(slot) = self.slot_of(id) {
            let model = &self.models[&id];
            if Self::same_type(model.record.effect_type, model.type_known, record_type) {
                elsewhere |= slot != update.slot;
                self.end_effect(id);
            } else {
                stuck = true;
            }
        } else {
            // An unslotted landing ends on its fade, as ever.
            self.end_effect(id);
        }
        if elsewhere || stuck {
            self.mismatch();
        }
    }

    /// Any other Titanium action: the record is copied over every slot
    /// holding the spell with its type byte, where it is held; the slot it
    /// names is not read, and a spell held nowhere is not added.
    fn update_by_spell(&mut self, update: BuffUpdate, now: Instant) {
        let Some(buff) = update.buff else {
            // An empty record's type byte matches no held slot.
            return;
        };
        let record_type = update.record_type;
        let mut held = false;
        let mut elsewhere = false;
        if let Some(slots) = &mut self.slots {
            for (slot, old) in slots.iter_mut() {
                if old.spell_id == buff.spell_id
                    && Self::same_type(old.effect_type, true, record_type)
                {
                    held = true;
                    elsewhere |= *slot != update.slot;
                    *old = buff.clone();
                    self.given.insert(*slot, now);
                }
            }
        }
        if let Ok(id) = u16::try_from(buff.spell_id) {
            if let Some(slot) = self.slot_of(id) {
                let model = self.models.get_mut(&id).expect("a placed effect's model");
                if Self::same_type(model.record.effect_type, model.type_known, record_type) {
                    held = true;
                    elsewhere |= slot != update.slot;
                    model.record = buff.clone();
                    model.type_known = true;
                    self.landed_at.insert(id, now);
                }
            } else if self.effects.contains_key(&id) {
                // Held without a slot: it stays as it is, and nothing is
                // added.
                held = true;
            }
        }
        if !held || elsewhere {
            self.mismatch();
        }
    }

    /// Titanium action 3: the record, empty or not, goes into the named slot
    /// as it stands, over whatever was held there, received or modelled.
    /// A spell modelled in another slot is then held in two, as in the
    /// program (read). A spell held without a slot ends there: where the
    /// program put its own copy is unknown, so the write is taken as its
    /// place, as before this model (inferred). Only a write over a modelled
    /// slot of another spell contradicts the model.
    fn write_slot(&mut self, update: BuffUpdate, now: Instant) {
        if let Some(spell) = self.placed.get(&update.slot).copied() {
            self.end_effect(spell);
            if u32::from(spell) != update.spell_id {
                self.mismatch();
            }
        }
        if let Ok(id) = u16::try_from(update.spell_id)
            && self.effects.contains_key(&id)
            && self.slot_of(id).is_none()
        {
            self.end_effect(id);
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

/// The invisibility state the player's buffs give, as the `EQMac` program
/// numbers it: 1 invisible (effect 12), 2 versus animals (29), 3 versus
/// undead (28), 0 none. Which wins where buffs give more than one is this
/// client's reading (TAKP Batch 1 R1 did not read the recompute's order).
fn invisibility_state<'a>(spells: impl IntoIterator<Item = &'a [u32]>) -> u32 {
    let mut state = 0;
    for effects in spells {
        for (effect, value) in [(12, 1), (29, 2), (28, 3)] {
            if effects.contains(&effect) && (state == 0 || value < state) {
                state = value;
            }
        }
    }
    state
}

/// The steps of the `EQMac` program's drop of the player's invisibility
/// before a cast, a song or an item's click (from the program, TAKP Batch 1
/// R1; recording checklist item 6.163): for each spell effect in `order`,
/// each buff whose spell carries it, lowest slot first, is faded; where
/// that changes the invisibility its buffs give, the new state goes first.
/// `effects` gives a spell's effect ids, where the installed client's spell
/// data has them; a buff without them is left alone. The program's second
/// pass, for an invisibility no buff explains, is not taken: the client
/// knows the player's invisibility only from these buffs.
#[must_use]
pub fn invisibility_drop(
    slots: &BTreeMap<u32, Buff>,
    order: &[u32],
    effects: impl Fn(u32) -> Option<Vec<u32>>,
) -> Vec<crate::InvisibilityStep> {
    use crate::InvisibilityStep;
    let mut remaining: BTreeMap<u32, (u32, Vec<u32>)> = slots
        .iter()
        .filter_map(|(slot, buff)| Some((*slot, (buff.spell_id, effects(buff.spell_id)?))))
        .collect();
    let mut state = invisibility_state(remaining.values().map(|(_, ids)| ids.as_slice()));
    let mut steps = Vec::new();
    for effect in order {
        while let Some(slot) = remaining
            .iter()
            .find(|(_, (_, ids))| ids.contains(effect))
            .map(|(slot, _)| *slot)
        {
            let Some((spell_id, ids)) = remaining.remove(&slot) else {
                break;
            };
            if ids.iter().any(|id| matches!(id, 12 | 28 | 29)) {
                let now = invisibility_state(remaining.values().map(|(_, ids)| ids.as_slice()));
                if now != state {
                    state = now;
                    steps.push(InvisibilityStep::State(now));
                }
            }
            // The wire carries a 16-bit spell; a larger one is no buff the
            // servers sent.
            if let Ok(spell_id) = u16::try_from(spell_id) {
                steps.push(InvisibilityStep::Fade { slot, spell_id });
            }
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_revision_tracks_received_slots_and_preserves_late_fade_replacements() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(3, buff(42))]), 25, now());
        assert_eq!(state.revision(), 1);
        let request = cancel::Request {
            session_id: 5,
            revision: state.revision(),
            slot: 3,
            spell_id: 42,
            beneficial: Some(true),
            cancellable: Some(true),
            created: now(),
            placement: cancel::Placement::Received,
        };
        assert!(request.check(state.revision(), state.slots()).is_ok());
        state.cancellation_news(request.update(cancel::Progress::Submitted));
        assert_eq!(state.slots().unwrap()[&3].spell_id, 42);
        state.apply(update(3, 43, true), now());
        assert_eq!(state.revision(), 2);
        assert!(request.check(state.revision(), state.slots()).is_err());
        state.apply(update(3, 42, false), now());
        assert_eq!(state.slots().unwrap()[&3].spell_id, 43);
        state.apply(update(3, u32::MAX, false), now());
        assert_eq!(state.slots().unwrap()[&3].spell_id, 43);
        state.observe_effect(effect(43), Landing::default(), now());
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(
            state.revision(),
            4,
            "Unslotted landings do not invent slot revisions"
        );
        state.clear();
        assert_eq!(state.revision(), 0);
        assert!(state.cancellation().is_none());
    }

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
        state.observe_effect(effect(51), Landing::default(), now());
        state.observe_effect(effect(50), Landing::default(), now());
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

    #[test]
    fn invisibility_drops_in_the_programs_order_with_each_state_change_first() {
        use crate::InvisibilityStep::{Fade, State};
        // Slot 0 a versus-undead buff, slot 2 an invisibility buff, slot 4
        // effect 94 alone, slot 6 an unrelated buff, slot 8 a spell the
        // installed data lacks.
        let slots = BTreeMap::from([
            (0, buff(28)),
            (2, buff(12)),
            (4, buff(94)),
            (6, buff(5)),
            (8, buff(999)),
        ]);
        let effects = |spell| (spell != 999).then(|| vec![spell, 254]);
        assert_eq!(
            invisibility_drop(&slots, &[94, 12, 28, 29], effects),
            [
                Fade {
                    slot: 4,
                    spell_id: 94
                },
                // Invisible until the 12 goes; then versus undead.
                State(3),
                Fade {
                    slot: 2,
                    spell_id: 12
                },
                State(0),
                Fade {
                    slot: 0,
                    spell_id: 28
                },
            ]
        );
        // Nothing to drop, nothing sent.
        assert_eq!(
            invisibility_drop(&BTreeMap::from([(6, buff(5))]), &[94, 12, 28, 29], effects),
            []
        );
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
            push: None,
        }
    }

    fn update(slot: u32, spell_id: u32, present: bool) -> BuffUpdate {
        BuffUpdate {
            applies: eq_network_game::buffs::Applies::Received,
            record_type: 0,
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
        state.observe_effect(effect(43), Landing::default(), now());
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
        state.observe_effect(effect(42), Landing::default(), now());
        state.observe_effect(effect(42), Landing::default(), now());
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(state.effects().len(), 1);
        state.apply(update(3, 42, false), now());
        state.apply(update(3, 42, false), now());
        assert!(state.effects().is_empty());
    }

    #[test]
    fn a_fade_naming_no_slot_the_table_holds_removes_the_buff_of_its_spell() {
        // TAKP's admission table, and its fade of spell 42, which names no
        // slot.
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(2, buff(42)), (5, buff(43))]), 15, now());
        state.apply(update(u32::MAX, 42, false), now());
        assert_eq!(state.slots().unwrap(), &BTreeMap::from([(5, buff(43))]));
        assert_eq!(state.time_left(2, now()), None);
        // A fade of a slot the table holds still removes that slot alone.
        state.replace_snapshot(BTreeMap::from([(2, buff(42)), (5, buff(42))]), 15, now());
        state.apply(update(5, 42, false), now());
        assert_eq!(state.slots().unwrap(), &BTreeMap::from([(2, buff(42))]));
        // An empty-slot fade names no spell anything holds.
        state.apply(update(5, u32::MAX, false), now());
        assert_eq!(state.slots().unwrap(), &BTreeMap::from([(2, buff(42))]));
    }

    #[test]
    fn observed_p99_self_recast_leaves_one_unslotted_effect() {
        let mut state = BuffTracker::empty_snapshot();
        let mut begin = effect(42);
        begin.effect_flag = 0;
        // First cast: begin action, then landed action; no slot update followed.
        state.observe_effect(begin.clone(), Landing::default(), now());
        state.observe_effect(effect(42), Landing::default(), now());
        // Recast: begin action, identified fade of slot 0, empty-slot fade, landed action.
        state.observe_effect(begin, Landing::default(), now());
        state.apply(update(0, 42, false), now());
        assert!(state.effects().is_empty());
        state.apply(update(0, u32::MAX, false), now());
        state.observe_effect(effect(42), Landing::default(), now());
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
        state.observe_effect(effect(50), Landing::default(), later(10));
        state.observe_effect(effect(50), Landing::default(), later(20));
        assert_eq!(state.landed(50), Some(later(20)));
        state.apply(update(9, 50, false), later(21));
        assert_eq!(state.landed(50), None);
        // A refresh of a slotted spell drops the slot's count with it.
        state.observe_effect(effect(42), Landing::default(), later(40));
        assert_eq!(state.time_left(3, later(40)), None);
        assert_eq!(state.landed(42), Some(later(40)));
    }

    #[test]
    fn new_admission_discards_old_effects_and_preserves_snapshot_holes() {
        let mut state = BuffTracker::empty_snapshot();
        state.observe_effect(effect(42), Landing::default(), now());
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
        state.observe_effect(non_buff, Landing::default(), now());
        assert!(state.effects().is_empty());
    }

    const NORMAL: Option<Band> = Some(Band {
        beneficial: false,
        short: false,
        last: false,
    });

    fn lands(band: Option<Band>) -> Landing {
        Landing {
            band,
            finder: crate::GameDialect::Titanium.buff_placement(),
            ..Landing::default()
        }
    }

    /// A Titanium update as its action word says the program applies it.
    fn titanium(applies: Applies, slot: u32, spell_id: u32, record: Option<Buff>) -> BuffUpdate {
        BuffUpdate {
            applies,
            record_type: record.as_ref().map_or(2, |buff| buff.effect_type),
            entity_id: 7,
            slot,
            spell_id,
            buff: record,
        }
    }

    fn spells(state: &BuffTracker) -> Vec<u32> {
        let mut spells: Vec<u32> = state.spells().collect();
        spells.sort_unstable();
        spells
    }

    #[test]
    fn modelled_slots_keep_their_place_when_an_earlier_buff_fades() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        state.observe_effect(effect(50), lands(NORMAL), now());
        state.observe_effect(effect(51), lands(NORMAL), now());
        assert_eq!(state.modelled(), &BTreeMap::from([(0, 50), (1, 51)]));
        // The server's fade pair for slot 0.
        assert!(!state.apply(update(0, 50, false), now()));
        assert!(!state.apply(update(0, u32::MAX, false), now()));
        assert_eq!(state.in_window(EffectWindow::Long, 0), None);
        assert!(matches!(
            state.in_window(EffectWindow::Long, 1),
            Some(Shown::Modelled(1, effect)) if effect.spell_id == 51
        ));
        // The next landing takes the lowest empty slot.
        state.observe_effect(effect(52), lands(NORMAL), now());
        assert_eq!(state.slot_of(52), Some(0));
        assert_eq!(state.slot_mismatches(), 0);
    }

    #[test]
    fn placement_treats_received_slots_as_held_and_a_refresh_keeps_its_slot() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(0, buff(43)), (2, buff(45))]), 25, now());
        state.observe_effect(effect(44), lands(NORMAL), now());
        assert_eq!(state.slot_of(44), Some(1));
        // A refresh of the received 43 takes over its slot.
        state.observe_effect(effect(43), lands(NORMAL), now());
        assert_eq!(state.slot_of(43), Some(0));
        assert_eq!(
            state.slots().unwrap().keys().copied().collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(spells(&state), [43, 44, 45]);
        state.observe_effect(effect(46), lands(NORMAL), now());
        assert_eq!(state.slot_of(46), Some(3));
        // A received record for a modelled spell's own slot takes it over.
        assert!(!state.apply(update(3, 46, true), now()));
        assert_eq!(state.slot_of(46), None);
        assert_eq!(state.slots().unwrap()[&3].spell_id, 46);
    }

    #[test]
    fn songs_are_modelled_into_the_short_window_and_unknown_bands_stay_unplaced() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        let song = Some(Band {
            beneficial: true,
            short: true,
            last: false,
        });
        assert!(!state.has_short());
        state.observe_effect(effect(700), lands(song), now());
        assert_eq!(state.slot_of(700), Some(25));
        // A modelled song opens the short window.
        assert!(state.has_short());
        assert!(matches!(
            state.in_window(EffectWindow::Short, 0),
            Some(Shown::Modelled(25, effect)) if effect.spell_id == 700
        ));
        state.observe_effect(effect(50), lands(None), now());
        assert_eq!(
            state.unplaced_reason(50),
            Some(NoSlot::Unplaceable(placement::Unplaced::SpellFieldsUnknown))
        );
        assert!(matches!(
            state.in_window(EffectWindow::Long, 0),
            Some(Shown::Unplaced(effect)) if effect.spell_id == 50
        ));
    }

    #[test]
    fn a_recast_after_its_fade_takes_the_lowest_empty_slot_as_the_program_does() {
        // R20 (f): A in 0, B in 1; A fades; B is recast, and the server's
        // fade of B comes first. The program puts B in 0.
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        state.observe_effect(effect(50), lands(NORMAL), now());
        state.observe_effect(effect(51), lands(NORMAL), now());
        state.apply(update(0, 50, false), now());
        state.apply(update(0, u32::MAX, false), now());
        state.apply(update(1, 51, false), now());
        state.apply(update(1, u32::MAX, false), now());
        state.observe_effect(effect(51), lands(NORMAL), now());
        assert_eq!(state.modelled(), &BTreeMap::from([(0, 51)]));
        assert_eq!(state.slot_mismatches(), 0);
        // A held effect-148 spell leaves a landing unplaced.
        state.observe_effect(
            effect(52),
            Landing {
                blocker: true,
                ..lands(NORMAL)
            },
            now(),
        );
        assert_eq!(
            state.unplaced_reason(52),
            Some(NoSlot::Unplaceable(placement::Unplaced::Blocked))
        );
    }

    #[test]
    fn a_titanium_fade_ends_its_spell_wherever_it_is_held_and_its_empty_half_does_nothing() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(4, buff(60))]), 25, now());
        state.observe_effect(effect(50), lands(NORMAL), now());
        state.observe_effect(effect(51), lands(NORMAL), now());
        // Modelled 50 in 0, but the server held it in 5.
        assert!(state.apply(titanium(Applies::FadeBySpell, 5, 50, None), now()));
        assert_eq!(spells(&state), [51, 60]);
        // The empty half of the pair, for the slot 51 is modelled in.
        assert!(!state.apply(titanium(Applies::FadeBySpell, 1, u32::MAX, None), now()));
        assert_eq!(state.slot_of(51), Some(1));
        // A received slot's own fade, where it is held: no mismatch.
        state.apply(titanium(Applies::FadeBySpell, 4, 60, None), now());
        assert_eq!(spells(&state), [51]);
        assert_eq!(state.slot_mismatches(), 1);
        // An unslotted landing ends on its fade, as ever (A1).
        state.observe_effect(effect(52), Landing::default(), now());
        assert_eq!(state.slot_of(52), None);
        state.apply(titanium(Applies::FadeBySpell, 9, 52, None), now());
        assert_eq!(spells(&state), [51]);
    }

    #[test]
    fn a_fade_reaches_only_slots_of_its_type_byte_and_says_when_one_stays() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        let typed = |record_type| Landing {
            record_type,
            ..lands(NORMAL)
        };
        state.observe_effect(effect(50), typed(Some(4)), now());
        state.observe_effect(effect(51), typed(Some(2)), now());
        state.observe_effect(effect(52), typed(None), now());
        assert_eq!(state.modelled_record(50).unwrap().effect_type, 4);
        // A type-2 fade leaves the type-4 record held, as the program does,
        // and says so.
        assert!(state.apply(titanium(Applies::FadeBySpell, 0, 50, None), now()));
        assert_eq!(state.slot_of(50), Some(0));
        // Against a type-2 record, or one whose type is unknown, it goes.
        state.apply(titanium(Applies::FadeBySpell, 1, 51, None), now());
        state.apply(titanium(Applies::FadeBySpell, 2, 52, None), now());
        assert_eq!(spells(&state), [50]);
        assert_eq!(state.slot_mismatches(), 1);
        // A received type-4 record stays for a type-2 fade too.
        let mut inverse = buff(60);
        inverse.effect_type = 4;
        state.replace_snapshot(BTreeMap::from([(0, inverse)]), 25, now());
        state.apply(titanium(Applies::FadeBySpell, 0, 60, None), now());
        assert_eq!(spells(&state), [60]);
        assert_eq!(state.slot_mismatches(), 1);
    }

    #[test]
    fn an_update_by_spell_changes_the_record_in_place_and_never_moves_or_adds() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(3, buff(60))]), 25, now());
        state.observe_effect(effect(50), lands(NORMAL), now());
        // EQEmu's duration correction names its own slot, 7.
        let mut corrected = buff(50);
        corrected.duration_ticks = 99;
        assert!(state.apply(
            titanium(Applies::BySpell, 7, 50, Some(corrected.clone())),
            now()
        ));
        assert_eq!(state.slot_of(50), Some(0));
        assert_eq!(state.modelled_record(50), Some(&corrected));
        assert!(!state.slots().unwrap().contains_key(&7));
        // A received record is updated in its own slot.
        let mut received = buff(60);
        received.duration_ticks = 5;
        state.apply(
            titanium(Applies::BySpell, 3, 60, Some(received.clone())),
            now(),
        );
        assert_eq!(state.slots().unwrap()[&3], received);
        // A spell held nowhere is not added (A4), and is counted.
        let before = spells(&state);
        state.apply(titanium(Applies::BySpell, 8, 70, Some(buff(70))), now());
        assert_eq!(spells(&state), before);
        assert_eq!(state.slot_mismatches(), 2);
    }

    #[test]
    fn a_slot_write_puts_the_record_there_as_it_stands() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(2, buff(60))]), 25, now());
        state.observe_effect(effect(50), lands(NORMAL), now());
        // Over the modelled 50 in 0.
        state.apply(titanium(Applies::BySlot, 0, 61, Some(buff(61))), now());
        assert_eq!(spells(&state), [60, 61]);
        // The same spell in a second slot: held twice, as in the program.
        state.apply(titanium(Applies::BySlot, 5, 60, Some(buff(60))), now());
        assert_eq!(spells(&state), [60, 60, 61]);
        // An empty record empties the slot.
        state.apply(titanium(Applies::BySlot, 2, u32::MAX, None), now());
        assert_eq!(spells(&state), [60, 61]);
        assert_eq!(
            state.slots().unwrap().keys().copied().collect::<Vec<_>>(),
            [0, 5]
        );
        // Over a modelled 50 in 0, the write of 50 itself contradicts
        // nothing.
        assert_eq!(state.slot_mismatches(), 1);
        state.observe_effect(effect(50), lands(NORMAL), now());
        assert_eq!(state.slot_of(50), Some(1));
        state.apply(titanium(Applies::BySlot, 1, 50, Some(buff(50))), now());
        assert_eq!(state.slot_mismatches(), 1);
        assert_eq!(spells(&state), [50, 60, 61]);
    }

    #[test]
    fn a_slot_write_ends_the_unslotted_copy_of_its_spell() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        state.observe_effect(effect(50), lands(None), now());
        assert_eq!(state.slot_of(50), None);
        state.apply(titanium(Applies::BySlot, 4, 50, Some(buff(50))), now());
        // One icon, in the slot written.
        assert_eq!(spells(&state), [50]);
        assert!(state.effects().is_empty());
        assert_eq!(state.slots().unwrap()[&4].spell_id, 50);
        assert_eq!(state.slot_mismatches(), 0);
    }

    #[test]
    fn while_a_landing_is_held_without_a_slot_no_new_one_is_modelled() {
        // (i) 50 in 0; 51 could replace it, so the program has it in 0, in
        // 1 or nowhere; 52 could then be in 1 or 2.
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        state.observe_effect(effect(50), lands(NORMAL), now());
        let conflict = Landing {
            conflict: true,
            ..lands(NORMAL)
        };
        state.observe_effect(effect(51), conflict, now());
        state.observe_effect(effect(52), lands(NORMAL), now());
        assert_eq!(state.unplaced_reason(52), Some(NoSlot::OccupancyUnknown));
        assert_eq!(state.slot_of(52), None);
        // A refresh of a spell with a slot keeps it.
        state.observe_effect(effect(50), lands(NORMAL), now());
        assert_eq!(state.slot_of(50), Some(0));
        // (ii) A spell held without a slot landing again, as a song pulses:
        // step 3 keeps it where the program has it, which is unknown.
        state.observe_effect(effect(51), lands(NORMAL), now());
        assert_eq!(state.slot_of(51), None);
        assert_eq!(state.unplaced_reason(51), Some(NoSlot::OccupancyUnknown));
        assert_eq!(
            state
                .unslotted()
                .map(|(effect, why)| (effect.spell_id, why))
                .collect::<Vec<_>>(),
            [
                (51, Some(NoSlot::OccupancyUnknown)),
                (52, Some(NoSlot::OccupancyUnknown))
            ]
        );
        // Once nothing is held without a slot, placement resumes.
        state.apply(titanium(Applies::FadeBySpell, 1, 51, None), now());
        state.apply(titanium(Applies::FadeBySpell, 2, 52, None), now());
        state.observe_effect(effect(53), lands(NORMAL), now());
        assert_eq!(state.slot_of(53), Some(1));
        // The next zone-in gives every slot, and the uncertainty ends.
        state.observe_effect(effect(54), conflict, now());
        state.replace_snapshot(BTreeMap::from([(0, buff(50))]), 25, now());
        state.observe_effect(effect(55), lands(NORMAL), now());
        assert_eq!(state.slot_of(55), Some(1));
    }

    #[test]
    fn a_sixteenth_normal_landing_stays_unslotted_and_shown() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        for spell in 100..115 {
            state.observe_effect(effect(spell), lands(NORMAL), now());
        }
        assert_eq!(state.modelled().len(), 15);
        assert_eq!(state.modelled().keys().max(), Some(&14));
        state.observe_effect(effect(115), lands(NORMAL), now());
        assert_eq!(state.slot_of(115), None);
        assert_eq!(
            state.unplaced_reason(115),
            Some(NoSlot::Unplaceable(placement::Unplaced::TopUnknown))
        );
        // It takes the long window's first free button, past the fifteen.
        assert!(matches!(
            state.in_window(EffectWindow::Long, 15),
            Some(Shown::Unplaced(effect)) if effect.spell_id == 115
        ));
        // A later landing waits until the next zone-in; the fifteen keep
        // their slots on a refresh.
        state.observe_effect(effect(116), lands(NORMAL), now());
        assert_eq!(state.unplaced_reason(116), Some(NoSlot::OccupancyUnknown));
        state.observe_effect(effect(100), lands(NORMAL), now());
        assert_eq!(state.slot_of(100), Some(0));
        assert_eq!(state.slot_mismatches(), 0);
    }

    #[test]
    fn a_generation_without_a_read_finder_models_no_slot() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::from([(0, buff(43))]), 15, now());
        let mac = Landing {
            finder: crate::GameDialect::EqMac.buff_placement(),
            ..lands(NORMAL)
        };
        state.observe_effect(effect(278), mac, now());
        assert_eq!(state.slot_of(278), None);
        assert_eq!(state.unplaced_reason(278), Some(NoSlot::NoFinder));
        // A refresh of a received buff drops its record, as before.
        state.observe_effect(effect(43), mac, now());
        assert_eq!(state.slot_of(43), None);
        assert!(state.slots().unwrap().is_empty());
        assert_eq!(state.effects().len(), 2);
    }

    #[test]
    fn a_refresh_step_3_refuses_keeps_the_old_record() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        let level = |caster_level| Landing {
            caster_level: Some(caster_level),
            ..lands(NORMAL)
        };
        state.observe_effect(effect(50), level(40), now());
        // A lower caster's refresh keeps the stored level.
        state.observe_effect(effect(50), level(20), now());
        assert_eq!(state.modelled_record(50).unwrap().caster_level, 40);
        // A higher one's writes its own.
        state.observe_effect(effect(50), level(50), now());
        assert_eq!(state.modelled_record(50).unwrap().caster_level, 50);
        // Effects 0x43, 0x65 and 0x71 refuse any refresh.
        let kept = Landing {
            keeps_record: true,
            ..level(60)
        };
        state.observe_effect(effect(50), kept, now());
        assert_eq!(state.modelled_record(50).unwrap().caster_level, 50);
        assert_eq!(state.slot_of(50), Some(0));
    }

    #[test]
    fn a_landing_beside_a_buff_step_4_could_replace_is_left_unplaced() {
        let mut state = BuffTracker::default();
        state.replace_snapshot(BTreeMap::new(), 25, now());
        state.observe_effect(effect(50), lands(NORMAL), now());
        let conflict = Landing {
            conflict: true,
            ..lands(NORMAL)
        };
        state.observe_effect(effect(51), conflict, now());
        assert_eq!(state.unplaced_reason(51), Some(NoSlot::PossibleReplacement));
        // A refresh keeps its own slot: step 3 comes first.
        state.observe_effect(effect(50), conflict, now());
        assert_eq!(state.slot_of(50), Some(0));
    }
}
