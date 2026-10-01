//! Local spell-request feedback; server notifications remain authoritative.

use super::HudState;
use eq_client_core::{ClientCommand, PlayerState};
use std::time::Instant;

/// One user attempt to cast or forget a memorized gem.
#[derive(Clone, Copy)]
pub(super) struct Request {
    pub gem: u8,
    pub target_id: u16,
    pub forgetting: bool,
    /// Unmodified installation mana cost; None leaves the decision to the server.
    pub mana_cost: Option<u32>,
}

/// The gem's spell, if the request can go now; otherwise why not, in the
/// player's words. Like the official client, a cast refuses locally when the
/// server-reported mana is short (P99 otherwise answers with a generic
/// interruption); forgetting never needs mana.
pub(super) fn check(
    world: &eq_client_core::world::ClientWorld,
    player: &PlayerState,
    request: &Request,
    messages: &super::messages::Messages,
    now: Instant,
) -> Result<u32, String> {
    let Request {
        gem,
        forgetting,
        mana_cost,
        ..
    } = *request;
    let spell_id = player
        .memorized_spells
        .get(usize::from(gem))
        .copied()
        .flatten()
        .ok_or("Empty spell gem — open the spellbook [B] to memorize")?;
    if world.casting().cast.is_some() {
        return Err("Already casting — duck [C] to interrupt".into());
    }
    if world.casting().pending.is_some() {
        return Err("Waiting for the server to acknowledge the cast".into());
    }
    if forgetting {
        return Ok(spell_id);
    }
    if mana_cost
        .zip(world.vitals().mana)
        .is_some_and(|(cost, mana)| cost > mana)
    {
        return Err(messages.text(199, "Insufficient Mana to cast this spell!"));
    }
    let remaining = world.casting().cooldowns.remaining(spell_id, now);
    if !remaining.is_zero() {
        return Err(format!(
            "Spell available in {:.1}s",
            remaining.as_secs_f32()
        ));
    }
    Ok(spell_id)
}

/// Checks the request and sends it without predicting its result. A refusal
/// is worth a line; a request sent says nothing, as the cast bar shows the
/// cast once the server takes it, and the outbox shows its own refusals.
pub(super) fn spell(
    hud: &mut HudState,
    world: &eq_client_core::world::ClientWorld,
    player: &PlayerState,
    outbox: &crate::outbox::Outbox,
    request: &Request,
    messages: &super::messages::Messages,
) {
    let now = Instant::now();
    let Request {
        gem,
        target_id,
        forgetting,
        ..
    } = *request;
    match check(world, player, request, messages, now) {
        Err(refusal) => hud.action_feedback = Some((now, refusal)),
        Ok(spell_id) => {
            hud.action_feedback = None;
            let _ = outbox.post(world, |stamp| {
                if forgetting {
                    ClientCommand::ForgetSpell {
                        session_id: stamp.session_id,
                        gem,
                        spell_id,
                        created: stamp.created,
                    }
                } else {
                    ClientCommand::CastSpell {
                        session_id: stamp.session_id,
                        gem,
                        spell_id,
                        target_id,
                        created: stamp.created,
                    }
                }
            });
        }
    }
}
