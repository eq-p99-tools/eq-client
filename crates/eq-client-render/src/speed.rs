//! Recalculate authoritative movement prerequisites; absence is never a zero load.
use crate::{online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_assets::spells::Effect;
use eq_client_core::{ClientCommand, world::ClientWorld};
use eq_client_core::{SpeedInputState, SpeedInputUnavailable as Missing, SpeedInputs};

/// Evaluated direct spell contributions, before combining active effects.
#[derive(Clone, Copy, Default)]
struct Modifiers {
    rate: i64,
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
        self.strength += other.strength;
        self.stamina += other.stamina;
        self.rooted |= other.rooted;
    }
}

/// Evaluate only effects that influence these inputs. Unread spell data and
/// unsupported affecting formulas never silently become zero. Reuse the asset
/// layer's level/cap evaluation instead of treating a scaling base as its value.
fn project(effects: &[Effect], level: u16, instrument: u32) -> Result<Modifiers, Missing> {
    let mut modifiers = Modifiers::default();
    for effect in effects {
        match effect.id {
            99 => modifiers.rooted = true,
            3 | 4 | 7 | 159 => {
                if level == 0 || !matches!(instrument, 0 | 10) || matches!(effect.formula, 60 | 70)
                {
                    return Err(Missing::Effects);
                }
                let value = effect.base_magnitude(level).ok_or(Missing::Effects)?;
                if effect.id == 3 {
                    modifiers.add(Modifiers {
                        rate: value,
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
            _ => return Err(Missing::Effects),
        }
    }
    Ok(modifiers)
}

/// First food and drink in general-slot order, including each bag's contents.
/// Pending changes to stat food cannot be mistaken for confirmed removal.
fn food(
    inventory: &eq_client_core::inventory::Inventory,
    level: u16,
) -> Result<Modifiers, Missing> {
    let mut modifiers = Modifiers::default();
    let mut meals = [false; 2];
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
        if bonus.worn.is_some() {
            return Err(Missing::Effects);
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
    Ok(modifiers)
}

/// Effective inputs from complete inventory, purse and buff membership.
/// Equipment uses the decoded eligibility/required/recommended-level rules.
/// Unsupported worn effects remain explicit; the base stat cap is 255
/// (higher-level/AA increases are deliberately not granted).
fn calculate(
    world: &ClientWorld,
    effects: impl Fn(u32) -> Option<Vec<Effect>>,
) -> Result<SpeedInputs, Missing> {
    let player = world.player().ok_or(Missing::Attributes)?;
    let weight =
        u16::try_from((world.movement_weight()? / 10).min(32_766)).expect("bounded weight");
    let base = player.base_attributes.ok_or(Missing::Attributes)?;
    let inventory = world.inventory();
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
    let mut modifiers = food(inventory, u16::from(player.level))?;
    for (_, item) in equipped {
        modifiers.strength += i64::from(item.bonuses.strength);
        modifiers.stamina += i64::from(item.bonuses.stamina);
        if let Some(worn) = item.worn {
            if worn.effect_type != 2 {
                return Err(Missing::Effects);
            }
            let level = if worn.level == 0 {
                u16::from(player.level)
            } else {
                u16::try_from(worn.level).map_err(|_| Missing::Effects)?
            };
            modifiers.add(project(
                &effects(worn.spell_id).ok_or(Missing::Effects)?,
                level,
                10,
            )?);
        }
    }
    let buffs = world.buffs();
    for buff in buffs.slots().ok_or(Missing::Effects)?.values() {
        modifiers.add(project(
            &effects(buff.spell_id).ok_or(Missing::Effects)?,
            u16::from(buff.caster_level),
            u32::from(buff.bard_modifier),
        )?);
    }
    for effect in buffs.effects().values() {
        let projected = project(
            &effects(u32::from(effect.spell_id)).ok_or(Missing::Effects)?,
            effect.caster_level,
            effect.instrument_modifier,
        )?;
        // An unslotted STR effect could replace an existing contributor. Speed
        // extremum/root selection is safe across the union; additive STR isn't.
        if projected.strength != 0 || projected.stamina != 0 {
            return Err(Missing::Effects);
        }
        modifiers.add(projected);
    }
    let fatigue_penalty = if world.generation() == eq_client_core::GameDialect::EqMac {
        let stamina = (i64::from(base.stamina) + modifiers.stamina).clamp(1, 255);
        // Until fatigue arrives, its source-defined maximum penalty is a sound
        // lower bound on STR. Do not turn the entire calculation into unknown.
        world
            .nourishment()
            .and_then(|n| n.fatigue)
            .map_or(10, |fatigue| (i64::from(fatigue) - stamina).clamp(0, 10))
    } else {
        0
    };
    Ok(SpeedInputs {
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
        capacity: u32::try_from(
            (i64::from(base.strength) + modifiers.strength - fatigue_penalty).clamp(1, 255),
        )
        .expect("bounded strength"),
    })
}

/// The same typed readiness result for movement reporting and script diagnostics.
pub(crate) fn inputs(
    world: &ClientWorld,
    spells: &crate::spellbook::SpellNames,
) -> SpeedInputState {
    match calculate(world, |id| spells.mechanics(id).map(|m| m.effects.to_vec())) {
        Ok(inputs) => SpeedInputState::Ready(inputs),
        Err(reason) => SpeedInputState::Unavailable(reason),
    }
}

/// Explain the actual calculation failure without changing its prerequisites.
/// A complete snapshot can still lack affecting item/spell details, which a
/// later server report does not necessarily repair. Cursor receipt matters;
/// knowledge of the bank balance does not.
fn readiness_reason(world: &ClientWorld, reason: Missing) -> &'static str {
    match reason {
        Missing::Attributes => "Movement paused: waiting for character information.",
        Missing::Inventory if !world.inventory().received() || world.inventory().stale() => {
            "Movement paused: waiting for inventory information."
        }
        Missing::Inventory => "Movement paused: item details are unavailable.",
        Missing::Coins if world.coins().is_none() => {
            "Movement paused: waiting for carried coin information."
        }
        Missing::Coins => "Movement paused: waiting for cursor coin information.",
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
        SpeedInputState::Ready(inputs) => Some((inputs.weight, Some(inputs.capacity))),
        SpeedInputState::Unavailable(_) => None,
    }
}

/// Automatic report identity and queue retry state for the current admission.
#[derive(Default)]
pub(crate) struct Reports {
    next_id: u64,
    current: Option<Report>,
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
    let state = inputs(world, &spells);
    let report = reports.prepare(session_id, revision, state);
    // The reason can change within one unavailable category (for example,
    // purse receipt leaves only cursor coins missing), even with no new report.
    notices.movement.set(match state {
        SpeedInputState::Unavailable(reason) => readiness_reason(world, reason),
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
