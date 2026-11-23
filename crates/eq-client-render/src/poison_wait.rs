//! Effect 234's total for the Apply Poison wait, which this client sums and
//! tells the session ([`ClientCommand::PoisonWait`], checklist 6.493b): the
//! program's tick waits that many ms in place of 10 s when it is not 0 (read
//! R49-1f).
//!
//! The total is 0x4106a3(0xea)'s: the effect's value from every active buff
//! at its caster's level, a type-4 record negated (0x41002f), and from every
//! AA the player's class may use at their level (0x40feaa). Worn items do
//! not count: neither 0x41002f nor 0x40feaa reads them. Where something
//! that could carry the effect is unknown (a buff's spell, its formula, the
//! AA definitions of a player who holds AAs), the total is None, and the
//! session keeps its 10 s.
//!
//! This is the host's own report: no script, slash line or social can post
//! it or choose its value (reviewer H1).
use bevy::prelude::*;
use eq_client_assets::spells::Effect;
use eq_client_core::{ClientCommand, PlayerState, world::ClientWorld};

use super::{online::OnlineState, outbox::Outbox};

/// The poison-apply-time effect (read R49-1f).
const POISON_APPLY_TIME: u32 = 234;

/// One buff's effect 234 at its caster's level; None when its spell is
/// unknown, or when it carries the effect and its formula or instrument
/// provenance is one this client does not evaluate.
fn spell_total(effects: Option<Vec<Effect>>, caster_level: u16, instrument: u32) -> Option<i64> {
    let mut total = 0;
    for effect in effects?
        .iter()
        .filter(|effect| effect.id == POISON_APPLY_TIME)
    {
        // As the breath path, only an ordinary instrument modifier and a
        // formula this client evaluates at the given level are summed.
        if !matches!(instrument, 0 | 10)
            || (caster_level == 0 && !matches!(effect.formula, 0 | 100))
            || matches!(effect.formula, 60 | 70)
        {
            return None;
        }
        total += effect.base_magnitude(caster_level)?;
    }
    Some(total)
}

/// A slotted buff's share: its spell's effect 234 at the record's caster
/// level, negated for a type-4 record as 0x41002f does; a type-0 record
/// that carries the effect is not one this client can place, so None.
fn buff_total(buff: &eq_client_core::Buff, mechanics: &impl Fn(u32) -> Option<Vec<Effect>>) -> Option<i64> {
    let total = spell_total(
        mechanics(buff.spell_id),
        u16::from(buff.caster_level),
        u32::from(buff.bard_modifier),
    )?;
    match buff.effect_type {
        _ if total == 0 => Some(0),
        0 => None,
        4 => Some(-total),
        _ => Some(total),
    }
}

/// Effect 234's total for the player in ms, as the program sums it; None
/// where something that could carry it is unknown (reviewer H2).
pub(super) fn total(
    world: &ClientWorld,
    player: &PlayerState,
    mechanics: impl Fn(u32) -> Option<Vec<Effect>>,
) -> Option<i64> {
    let buffs = world.buffs();
    let slots = buffs.slots()?;
    let mut total = 0;
    for buff in slots.values() {
        total += buff_total(buff, &mechanics)?;
    }
    // A landing the server gave no slot may replace a slotted buff, so one
    // that carries the effect leaves the total unknown, as the breath path
    // does for its additive effects.
    for effect in buffs.effects().values() {
        let spell_id = u32::from(effect.spell_id);
        if slots.values().any(|buff| buff.spell_id == spell_id) {
            continue;
        }
        let share = match buffs.modelled_effect_record(effect.spell_id) {
            Some(record) => buff_total(record, &mechanics)?,
            None => spell_total(
                mechanics(spell_id),
                effect.caster_level,
                effect.instrument_modifier,
            )?,
        };
        if share != 0 {
            return None;
        }
    }
    // A player whose profile holds no AA, or on a server that offers none,
    // has nothing there to add; otherwise every definition must be known.
    let holds_none = match player.titanium_environment {
        Some(profile) => !profile.has_alternate_advancement,
        None => !world.can(eq_client_core::Capability::AlternateAdvancement),
    };
    if !holds_none {
        let class = player.class?;
        for effect in world
            .alternate_advancement()
            .effects(class, player.level)?
            .into_iter()
            .filter(|effect| effect.id == POISON_APPLY_TIME)
        {
            total += i64::from(effect.value);
        }
    }
    Some(total)
}

/// The total last told, for the admission it was told in.
#[derive(Default)]
pub(super) struct Told(Option<(u64, Option<i64>)>);

/// Tells the session effect 234's total whenever it changes, and again in
/// each new admission, while the session offers Apply Poison.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn report(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    spells: Res<crate::spellbook::SpellNames>,
    mut told: Local<Told>,
) {
    let world = online.world();
    let (Some(session_id), Some(player)) = (world.session_id(), world.player()) else {
        told.0 = None;
        return;
    };
    if !online.enabled
        || !online.in_world()
        || !world.can(eq_client_core::Capability::ApplyingPoison)
    {
        return;
    }
    let total_ms = total(world, player, |id| {
        spells.mechanics(id).map(|mechanics| mechanics.effects.to_vec())
    });
    if told.0 == Some((session_id, total_ms)) {
        return;
    }
    if outbox.tell(
        world,
        ClientCommand::PoisonWait {
            session_id,
            total_ms,
        },
    ) {
        told.0 = Some((session_id, total_ms));
    }
}

#[cfg(test)]
mod tests;
