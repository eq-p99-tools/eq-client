//! Recalculate authoritative movement prerequisites; absence is never a zero load.
//!
//! The received instrument modifier is applied after level and cap evaluation.
//! Titanium's action handler (`eqgame.exe` 0x414913) stores the modifier on
//! the buff, and its effect cache (0x4102a6) scales the signed magnitude in
//! tenths for supported modifiers through 45, including snares. See
//! [`eq_client_core::buffs::scale_instrument_effect`] for the host projection.
use crate::{online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_assets::spells::Effect;
use eq_client_core::{ClientCommand, GameDialect, world::ClientWorld};
use eq_client_core::{
    EqMacFatigueInputs, SpeedInputState, SpeedInputUnavailable as Missing, SpeedInputs,
};

/// Evaluated direct spell contributions, before combining active effects.
#[derive(Clone, Copy, Default)]
struct Modifiers {
    rate: i64,
    /// The fastest positive movement modifier, apart from any snare: the
    /// rate an unproven term goes back to must never carry a snare.
    gain: i64,
    strength: i64,
    stamina: i64,
    rooted: bool,
}

impl Modifiers {
    /// The programs' rule for the movement effect (3): the strongest snare
    /// wins; otherwise the fastest positive modifier wins, each evaluated at
    /// its buff's caster level (Titanium totaller 0x4106a3 / 0x4107e4 over
    /// the cache 0x41002f builds, writer 0x410fdb; `EQMac` 0x4c6b6d, writer
    /// 0x4caaba; program reads 11-03). STR sums and an actual root dominates.
    fn add(&mut self, other: Self) {
        self.rate = if self.rate < 0 || other.rate < 0 {
            self.rate.min(other.rate)
        } else {
            self.rate.max(other.rate)
        };
        self.gain = self.gain.max(other.gain);
        self.strength += other.strength;
        self.stamina += other.stamina;
        self.rooted |= other.rooted;
    }
}

/// Evaluate only effects that influence these inputs, every one of them, so
/// an effect the client cannot evaluate never hides a root or a snare after
/// it. A term it cannot evaluate adds nothing here and marks the projection
/// inexact, as does an effect id it does not know; the caller decides what an
/// inexact projection proves. Reuse the asset layer's level/cap evaluation
/// instead of treating a scaling base as its value.
fn project(effects: &[Effect], level: u16, instrument: u32, dialect: GameDialect) -> Projection {
    let mut modifiers = Modifiers::default();
    let mut exact = true;
    for effect in effects {
        match effect.id {
            99 => modifiers.rooted = true,
            3 | 4 | 7 | 159 => {
                let Some(value) = evaluate(effect, level, instrument, dialect) else {
                    exact = false;
                    continue;
                };
                if effect.id == 3 {
                    modifiers.add(Modifiers {
                        rate: value,
                        gain: value.max(0),
                        ..Modifiers::default()
                    });
                } else {
                    if effect.id != 7 {
                        modifiers.strength += value;
                    }
                    if matches!(effect.id, 7 | 159) {
                        modifiers.stamina += value;
                    }
                }
            }
            // Classic effects that do not directly change strength or ground speed.
            0..=158 | 160..=254 => (),
            _ => exact = false,
        }
    }
    Projection { modifiers, exact }
}

/// What the evaluable terms of a set of effects add, and whether every term
/// was evaluable.
#[derive(Clone, Copy)]
struct Projection {
    modifiers: Modifiers,
    exact: bool,
}

/// The exact value of a movement or stat term, where the client can work it
/// out.
fn evaluate(effect: &Effect, level: u16, instrument: u32, dialect: GameDialect) -> Option<i64> {
    if level == 0 || matches!(effect.formula, 60 | 70) {
        return None;
    }
    let value = effect.base_magnitude(level)?;
    eq_client_core::buffs::scale_instrument_effect(dialect, value, instrument)
}

/// First food and drink in general-slot order, including each bag's contents.
/// Pending changes to stat food cannot be mistaken for confirmed removal. The
/// flag is false where a food's worn effect is unevaluated.
fn food(
    inventory: &eq_client_core::inventory::Inventory,
    level: u16,
) -> Result<(Modifiers, Option<i64>, bool), Missing> {
    let mut exact = true;
    let mut modifiers = Modifiers::default();
    let mut meals = [false; 2];
    let mut fire_resistance = Some(0);
    for slot in (22..=29).flat_map(|root| {
        let slot = eq_client_core::inventory::InventorySlot(root);
        std::iter::once(slot).chain((0..10).filter_map(move |index| slot.child(index)))
    }) {
        let Some(item) = inventory.items().get(&slot) else {
            continue;
        };
        if !matches!(item.rules.item_type, 14 | 15) {
            continue;
        }
        let index = usize::from(item.rules.item_type - 14);
        if meals[index] {
            continue;
        }
        meals[index] = true;
        let bonus = eq_client_core::resources::eqemu_equipment_by_level(&item.details, level)
            .ok_or(Missing::Inventory)?;
        modifiers.strength += i64::from(bonus.bonuses.strength);
        modifiers.stamina += i64::from(bonus.bonuses.stamina);
        fire_resistance = fire_resistance
            .zip(bonus.bonuses.fire_resistance)
            .map(|(total, resistance)| total + i64::from(resistance));
        if bonus.worn.is_some() {
            exact = false;
        }
    }
    if inventory.prediction_origins().any(|(_, item)| {
        item.is_some_and(|item| {
            matches!(item.rules.item_type, 14 | 15)
                && item
                    .details
                    .bonuses
                    .is_none_or(|b| b.strength != 0 || b.stamina != 0)
        })
    }) {
        return Err(Missing::Prediction);
    }
    if inventory.prediction_origins().any(|(_, item)| {
        item.is_some_and(|item| {
            matches!(item.rules.item_type, 14 | 15)
                && item.details.bonuses.and_then(|bonus| bonus.fire_resistance) != Some(0)
        })
    }) {
        fire_resistance = None;
    }
    Ok((modifiers, fire_resistance, exact))
}

/// Stamina and known fire resistance from the same stat food used for movement.
pub(super) fn food_environment(
    inventory: &eq_client_core::inventory::Inventory,
    level: u16,
) -> Option<(i64, Option<i64>)> {
    food(inventory, level)
        .ok()
        .filter(|(_, _, exact)| *exact)
        .map(|(modifiers, fire, _)| (modifiers.stamina, fire))
}

/// Effective inputs from complete inventory, purse and buff membership.
/// Equipment uses the decoded eligibility/required/recommended-level rules.
/// The base stat cap is 255 (higher-level/AA increases are deliberately not
/// granted). Every effect is scanned; one the client cannot evaluate (an
/// unsupported worn or food worn effect, a missing spell record, an unread
/// formula or instrument term, an unslotted STR/STA term) is left out and
/// marks the result inexact, so it never hides a root or a snare. Only state
/// with nothing provable fails.
fn calculate(
    world: &ClientWorld,
    effects: impl Fn(u32) -> Option<Vec<Effect>>,
) -> Result<Projected, Missing> {
    let mut exact = true;
    let player = world.player().ok_or(Missing::Attributes)?;
    let dialect = world.generation();
    let weight = u16::try_from(
        (world.movement_weight()? / 10)
            .min(u64::from(eq_client_core::inventory::MAX_CARRIED_WEIGHT)),
    )
    .expect("bounded weight");
    let base = player.base_attributes.ok_or(Missing::Attributes)?;
    let inventory = world.worker_inventory();
    let equipped = eq_client_core::resources::eqemu_equipped_modifiers(
        inventory,
        player.class.ok_or(Missing::Attributes)?,
        player.race,
        u16::from(player.level),
    )
    .map_err(|reason| match reason {
        eq_client_core::resources::EquipmentUnavailable::UnconfirmedInventory => {
            Missing::Prediction
        }
        _ => Missing::Inventory,
    })?;
    let (mut modifiers, _, fed) = food(inventory, u16::from(player.level))?;
    exact &= fed;
    for (_, item) in equipped {
        modifiers.strength += i64::from(item.bonuses.strength);
        modifiers.stamina += i64::from(item.bonuses.stamina);
        if let Some(worn) = item.worn {
            let level = if worn.level == 0 {
                Some(u16::from(player.level))
            } else {
                u16::try_from(worn.level).ok()
            };
            match (worn.effect_type == 2, level, effects(worn.spell_id)) {
                (true, Some(level), Some(list)) => {
                    let projected = project(&list, level, 10, dialect);
                    exact &= projected.exact;
                    modifiers.add(projected.modifiers);
                }
                _ => exact = false,
            }
        }
    }
    let (buffed, proven) = buff_terms(world, &effects)?;
    modifiers.add(buffed);
    exact &= proven;
    let strength = i64::from(base.strength) + modifiers.strength;
    let eqmac_fatigue = if dialect.uses_fatigue_for_carry_capacity() {
        Some(EqMacFatigueInputs {
            strength,
            stamina: Some(
                u8::try_from((i64::from(base.stamina) + modifiers.stamina).clamp(1, 255))
                    .expect("bounded stamina"),
            ),
        })
    } else {
        None
    };
    // A stable capped fallback keeps fatigue-only ticks out of the posted
    // report identity. The component alone owns current EQMac capacity.
    let capacity = eqmac_fatigue.map_or_else(
        || u32::try_from(strength.clamp(1, 255)).expect("bounded strength"),
        |inputs| inputs.capacity(None),
    );
    let inputs = SpeedInputs {
        rate: if modifiers.rooted {
            -1000
        } else {
            i16::try_from(
                modifiers
                    .rate
                    // Reserve the formula's root sentinel for an actual root.
                    .clamp(-999, i64::from(i16::MAX)),
            )
            .expect("bounded rate")
        },
        weight,
        capacity,
        eqmac_fatigue,
    };
    // Every effect was scanned, so a root is never hidden by one the client
    // cannot evaluate; anything else unevaluated leaves the inputs unproven.
    Ok(Projected {
        inputs,
        exact: exact || modifiers.rooted,
        complete: exact,
        terms: Terms::of(modifiers),
    })
}

/// What the player's buffs and unslotted effects add, and whether every
/// term was evaluable. Missing buff membership fails.
fn buff_terms(
    world: &ClientWorld,
    effects: &impl Fn(u32) -> Option<Vec<Effect>>,
) -> Result<(Modifiers, bool), Missing> {
    let dialect = world.generation();
    let buffs = world.buffs();
    let mut modifiers = Modifiers::default();
    let mut exact = true;
    for buff in buffs.slots().ok_or(Missing::Effects)?.values() {
        let Some(list) = effects(buff.spell_id) else {
            exact = false;
            continue;
        };
        let projected = project(
            &list,
            u16::from(buff.caster_level),
            u32::from(buff.bard_modifier),
            dialect,
        );
        exact &= projected.exact;
        modifiers.add(projected.modifiers);
    }
    for effect in buffs.effects().values() {
        let Some(list) = effects(u32::from(effect.spell_id)) else {
            exact = false;
            continue;
        };
        let mut projected = project(
            &list,
            effect.caster_level,
            effect.instrument_modifier,
            dialect,
        );
        // An unslotted STR effect could replace an existing contributor, so
        // its STR/STA is left out. Speed extremum/root selection is safe
        // across the union; additive STR isn't.
        if projected.modifiers.strength != 0 || projected.modifiers.stamina != 0 {
            projected.modifiers.strength = 0;
            projected.modifiers.stamina = 0;
            projected.exact = false;
        }
        exact &= projected.exact;
        modifiers.add(projected.modifiers);
    }
    Ok((modifiers, exact))
}

/// Inputs from every term the client can prove, and whether that was all of
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Projected {
    inputs: SpeedInputs,
    exact: bool,
    /// Whether every term was evaluable, a root aside.
    complete: bool,
    terms: Terms,
}

/// The movement terms the client can evaluate: the fastest gain apart from
/// any snare, and a root or the strongest snare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Terms {
    gain: i16,
    slowing: Option<i16>,
}

impl Terms {
    fn of(modifiers: Modifiers) -> Self {
        Self {
            gain: i16::try_from(modifiers.gain.min(i64::from(i16::MAX))).expect("bounded gain"),
            slowing: if modifiers.rooted {
                Some(-1000)
            } else if modifiers.rate < 0 {
                Some(i16::try_from(modifiers.rate.max(-999)).expect("bounded rate"))
            } else {
                None
            },
        }
    }

    /// The rate from a remembered one with no root or snare in it: the
    /// higher of it and the gain now, lowered by a root or snare now.
    fn rate(self, remembered: i16) -> i16 {
        let base = remembered.max(self.gain);
        self.slowing.map_or(base, |slowing| slowing.min(base))
    }
}

/// The movement terms among the player's buffs and effects that the client
/// can evaluate, when it cannot work the rest out: the fastest gain, and the
/// root or the strongest snare. By the programs' rule an actual root
/// dominates and the strongest snare wins over every positive modifier, so
/// either proves the rate is at most its own; neither may be masked by what
/// the client cannot evaluate.
fn known_terms(world: &ClientWorld, effects: impl Fn(u32) -> Option<Vec<Effect>>) -> Terms {
    let dialect = world.generation();
    let buffs = world.buffs();
    let slotted = buffs.slots().into_iter().flat_map(|slots| {
        slots.values().map(|buff| {
            (
                buff.spell_id,
                u16::from(buff.caster_level),
                u32::from(buff.bard_modifier),
            )
        })
    });
    let unslotted = buffs.effects().values().map(|effect| {
        (
            u32::from(effect.spell_id),
            effect.caster_level,
            effect.instrument_modifier,
        )
    });
    let mut known = Modifiers::default();
    for (spell, level, instrument) in slotted.chain(unslotted) {
        if let Some(list) = effects(spell) {
            known.add(project(&list, level, instrument, dialect).modifiers);
        }
    }
    Terms::of(known)
}

/// The root or the strongest snare among the effects the client can
/// evaluate on the player.
fn known_slowing(world: &ClientWorld, effects: impl Fn(u32) -> Option<Vec<Effect>>) -> Option<i16> {
    known_terms(world, effects).slowing
}

/// The same typed readiness result for movement reporting and script diagnostics.
pub(crate) fn inputs(
    world: &ClientWorld,
    spells: &crate::spellbook::SpellNames,
) -> SpeedInputState {
    match calculate(world, |id| spells.mechanics(id).map(|m| m.effects.to_vec())) {
        Ok(Projected {
            inputs,
            exact: true,
            ..
        }) => SpeedInputState::Ready(inputs),
        Ok(_) => SpeedInputState::Unavailable(Missing::Effects),
        Err(reason) => SpeedInputState::Unavailable(reason),
    }
}

/// What the host reports: the exact inputs where it can work them out.
/// Otherwise the inputs from every term it can prove
/// ([`SpeedInputState::Bounded`]; Adam's decision of 11-10, recording
/// checklist 6.451): each unproven term is taken from the last report where
/// one exists and left out where none does. The rate is the higher of the
/// remembered one and the gain evaluable now, lowered by a root or a snare
/// evaluable now; the weight is as proven. The session takes a bound as
/// reported. With no earlier report and nothing provable it says what is
/// missing, and the session keeps the grant in force, or waits where there
/// is none.
fn reported(
    world: &ClientWorld,
    effects: impl Fn(u32) -> Option<Vec<Effect>> + Copy,
    last: Option<SpeedInputs>,
) -> Reported {
    // What a later unproven term goes back to: the rate with no root or
    // snare in it, so one that fades frees the player.
    let remember = |inputs: SpeedInputs, complete: bool, terms: Terms| SpeedInputs {
        rate: match last {
            Some(last) if !complete => last.rate.max(terms.gain),
            _ => terms.gain,
        },
        ..inputs
    };
    match calculate(world, effects) {
        Ok(Projected {
            inputs,
            exact: true,
            complete,
            terms,
        }) => Reported {
            state: SpeedInputState::Ready(inputs),
            remember: Some(remember(inputs, complete, terms)),
        },
        // The effects are unproven: the rate from the remembered one and the
        // terms evaluable now, and the load as proven.
        Ok(Projected { inputs, terms, .. }) => {
            let bounded = last.map_or(inputs, |last| SpeedInputs {
                rate: terms.rate(last.rate),
                ..inputs
            });
            Reported {
                state: SpeedInputState::Bounded {
                    inputs: bounded,
                    missing: Missing::Effects,
                },
                remember: Some(remember(bounded, false, terms)),
            }
        }
        Err(missing) => {
            let Some(last) = last else {
                return Reported {
                    state: SpeedInputState::Unavailable(missing),
                    remember: None,
                };
            };
            let terms = known_terms(world, effects);
            let mut bounded = SpeedInputs {
                rate: terms.rate(last.rate),
                ..last
            };
            if let Ok(tenths) = world.movement_weight() {
                bounded.weight = u16::try_from(
                    (tenths / 10).min(u64::from(eq_client_core::inventory::MAX_CARRIED_WEIGHT)),
                )
                .expect("bounded weight");
            }
            Reported {
                state: SpeedInputState::Bounded {
                    inputs: bounded,
                    missing,
                },
                remember: Some(remember(bounded, false, terms)),
            }
        }
    }
}

/// A report, and the inputs a later unproven term goes back to, with no
/// root or snare in their rate; None keeps the earlier ones.
#[derive(Debug, PartialEq)]
struct Reported {
    state: SpeedInputState,
    remember: Option<SpeedInputs>,
}

/// Whether the player is rooted, as far as the client can prove: the exact
/// inputs say so, or a root among the effects it can evaluate.
pub(crate) fn rooted(world: &ClientWorld, spells: &crate::spellbook::SpellNames) -> bool {
    match inputs(world, spells) {
        SpeedInputState::Ready(inputs) => inputs.rate <= -1000,
        _ => known_slowing(world, |id| spells.mechanics(id).map(|m| m.effects.to_vec()))
            .is_some_and(|rate| rate <= -1000),
    }
}

/// Explain the actual calculation failure without changing its prerequisites.
/// A complete snapshot can still lack affecting item/spell details, which a
/// later server report does not necessarily repair. Cursor receipt matters;
/// knowledge of the bank balance does not.
fn readiness_reason(world: &ClientWorld, reason: Missing) -> &'static str {
    match reason {
        Missing::Attributes => "Movement paused: waiting for character information.",
        Missing::Inventory if world.worker_inventory().ground_drop_pending() => {
            "Movement paused: re-enter the world for a fresh inventory snapshot after the drop."
        }
        Missing::Inventory
            if !world.worker_inventory().received() || world.worker_inventory().stale() =>
        {
            "Movement paused: waiting for inventory information."
        }
        Missing::Inventory => "Movement paused: item details are unavailable.",
        Missing::Coins if world.coins().is_none() => {
            "Movement paused: waiting for carried coin information."
        }
        Missing::Coins => "Movement paused: waiting for cursor coin information.",
        // An affecting equipment history with no live prediction left to
        // confirm ends only with a full inventory snapshot or a settlement.
        Missing::Prediction
            if world.worker_inventory().equipment_stats_uncertain()
                && !world.worker_inventory().predicted() =>
        {
            "Movement paused: equipment changes need a full inventory refresh."
        }
        Missing::Prediction if !world.worker_inventory().predicted() => {
            "Movement paused: weight changes need a full inventory refresh."
        }
        Missing::Prediction => "Movement paused: waiting for item changes to be confirmed.",
        Missing::Effects if world.buffs().slots().is_none() => {
            "Movement paused: waiting for buff information."
        }
        Missing::Effects => "Movement paused: spell or item effect details are unavailable.",
    }
}

/// A displayable load only when its inputs are established.
pub(crate) fn load(
    world: &ClientWorld,
    spells: &crate::spellbook::SpellNames,
) -> Option<(u16, Option<u32>)> {
    match inputs(world, spells) {
        SpeedInputState::Ready(inputs) => {
            Some((inputs.weight, Some(effective_capacity(world, inputs))))
        }
        SpeedInputState::Bounded { .. } | SpeedInputState::Unavailable(_) => None,
    }
}

/// Whether the session grants the player positive movement now, kept or
/// exact.
fn moving_granted(world: &ClientWorld) -> bool {
    world.motion().is_some_and(|motion| {
        motion
            .units_per_second
            .is_some_and(|speed| speed.is_finite() && speed > 0.0)
    })
}

/// Display current capacity through the network-owned fatigue formula without
/// changing the posted report or manufacturing a new acceptance receipt.
fn effective_capacity(world: &ClientWorld, inputs: SpeedInputs) -> u32 {
    if world.generation().uses_fatigue_for_carry_capacity() {
        inputs.eqmac_capacity(world.nourishment().and_then(|n| n.fatigue))
    } else {
        inputs.capacity
    }
}

/// Automatic report identity and queue retry state for the current admission.
#[derive(Default)]
pub(crate) struct Reports {
    next_id: u64,
    current: Option<Report>,
    /// The inputs last reported in this admission, exact or bounded: what the
    /// session keeps in force while new ones are unproven.
    proven: Option<(u64, SpeedInputs)>,
}

/// One logical report, retained across queue retries. Its id also distinguishes
/// recovery to identical values from an earlier report still in flight.
struct Report {
    session_id: u64,
    revision: u64,
    id: u64,
    state: SpeedInputState,
    posted: bool,
}

impl Reports {
    fn prepare(&mut self, session_id: u64, revision: u64, state: SpeedInputState) -> &mut Report {
        if self.current.as_ref().is_none_or(|report| {
            (report.session_id, report.revision, report.state) != (session_id, revision, state)
        }) {
            self.next_id = self.next_id.wrapping_add(1);
            self.current = Some(Report {
                session_id,
                revision,
                id: self.next_id,
                state,
                posted: false,
            });
        }
        self.current.as_mut().expect("current report was prepared")
    }
}

impl Report {
    /// Whether the session has accepted this exact posted report, granted or
    /// not; the receipt's identity test is the one [`Self::granted`] applies.
    fn accepted(&self, world: &ClientWorld) -> bool {
        let (true, SpeedInputState::Ready(inputs), Some(accepted)) =
            (self.posted, self.state, world.accepted_speed_inputs())
        else {
            return false;
        };
        (
            accepted.session_id,
            accepted.revision,
            accepted.report_id,
            accepted.inputs,
        ) == (self.session_id, self.revision, self.id, inputs)
    }

    /// A receipt can clear only its exact posted ready report. An old cached or
    /// in-flight receipt cannot clear a replacement wait. Withdrawal invalidates
    /// the receipt even if its inputs are unchanged.
    fn granted(&self, world: &ClientWorld) -> bool {
        let (true, SpeedInputState::Ready(inputs), Some(accepted), Some(motion)) = (
            self.posted,
            self.state,
            world.accepted_speed_inputs(),
            world.motion(),
        ) else {
            return false;
        };
        accepted.granted
            && (
                accepted.session_id,
                accepted.revision,
                accepted.report_id,
                accepted.inputs,
            ) == (self.session_id, self.revision, self.id, inputs)
            && motion
                .units_per_second
                .is_some_and(|speed| speed.is_finite() && speed > 0.0)
    }
}

/// Recalculate, show the current prerequisite, and report through the normal
/// outbox. Only the session's exact accepted positive grant clears the wait.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn report(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    spells: Res<crate::spellbook::SpellNames>,
    mut notices: ResMut<crate::notices::Lines>,
    mut reports: Local<Reports>,
) {
    let world = online.world();
    let (Some(session_id), Some(revision)) = (world.session_id(), world.speed_input_revision())
    else {
        reports.current = None;
        notices.movement.set("");
        return;
    };
    if !online.enabled || !online.in_world() {
        reports.current = None;
        notices.movement.set("");
        return;
    }
    let last = reports
        .proven
        .filter(|(admission, _)| *admission == session_id)
        .map(|(_, inputs)| inputs);
    let Reported { state, remember } = reported(
        world,
        |id| spells.mechanics(id).map(|m| m.effects.to_vec()),
        last,
    );
    if let Some(inputs) = remember {
        reports.proven = Some((session_id, inputs));
    }
    let report = reports.prepare(session_id, revision, state);
    // The reason can change within one unavailable category (for example,
    // purse receipt leaves only cursor coins missing), even with no new report.
    // With a grant in force the session keeps it while it takes a new
    // report, or while the inputs are unproven, so there is nothing to say.
    let granted = moving_granted(world);
    notices.movement.set(match state {
        _ if granted => "",
        SpeedInputState::Unavailable(missing) | SpeedInputState::Bounded { missing, .. } => {
            readiness_reason(world, missing)
        }
        SpeedInputState::Ready(_) if report.granted(world) => "",
        // Say which step is outstanding: the session's acceptance of this
        // exact report, or a positive movement grant after it.
        SpeedInputState::Ready(_) if report.accepted(world) => {
            "Movement paused: the movement inputs were accepted; waiting for a movement grant."
        }
        SpeedInputState::Ready(_) => {
            "Movement paused: waiting for the session to accept the movement inputs."
        }
    });
    if !report.posted {
        report.posted = outbox.tell(
            world,
            ClientCommand::SpeedInputs {
                session_id,
                revision,
                report_id: report.id,
                state,
            },
        );
    }
}

#[cfg(test)]
mod tests;
