//! Local spell-request feedback; server notifications remain authoritative.

use super::HudState;
use eq_client_core::{ClientCommand, PlayerState};
use std::{sync::mpsc::SyncSender, time::Instant};

/// One user attempt to cast or forget a memorized gem.
#[derive(Clone, Copy)]
pub(super) struct Request {
    pub session_id: u64,
    pub gem: u8,
    pub target_id: u16,
    pub forgetting: bool,
    /// Unmodified installation mana cost; None leaves the decision to the server.
    pub mana_cost: Option<u32>,
}

/// Validates local availability and queues a fresh request without predicting its result.
pub(super) fn spell(
    hud: &mut HudState,
    world: &eq_client_core::world::ClientWorld,
    player: &PlayerState,
    sender: &SyncSender<ClientCommand>,
    request: &Request,
    messages: &super::messages::Messages,
) {
    let Request {
        session_id,
        gem,
        target_id,
        forgetting,
        mana_cost,
    } = *request;
    let now = Instant::now();
    let message = match player
        .memorized_spells
        .get(usize::from(gem))
        .copied()
        .flatten()
    {
        None => "Empty spell gem — open the spellbook [B] to memorize".into(),
        Some(_) if world.casting().cast.is_some() => {
            "Already casting — duck [C] to interrupt".into()
        }
        Some(_) if world.casting().pending.is_some() => {
            "Waiting for the server to acknowledge the cast".into()
        }
        // Like the official client, refuse locally when server-reported mana is short;
        // P99 otherwise answers with a generic interruption.
        Some(_)
            if !forgetting
                && mana_cost
                    .zip(world.vitals().mana)
                    .is_some_and(|(cost, mana)| cost > mana) =>
        {
            messages.text(199, "Insufficient Mana to cast this spell!")
        }
        Some(spell_id) => {
            let remaining = world.casting().cooldowns.remaining(spell_id, now);
            if !forgetting && !remaining.is_zero() {
                format!("Spell available in {:.1}s", remaining.as_secs_f32())
            } else {
                let command = if forgetting {
                    ClientCommand::ForgetSpell {
                        session_id,
                        gem,
                        spell_id,
                        created: now,
                    }
                } else {
                    ClientCommand::CastSpell {
                        session_id,
                        gem,
                        spell_id,
                        target_id,
                        created: now,
                    }
                };
                match sender.try_send(command) {
                    Ok(()) if forgetting => {
                        "Forget request queued; waiting for server update".into()
                    }
                    Ok(()) => "Cast request queued; waiting for server response".into(),
                    Err(std::sync::mpsc::TrySendError::Full(_)) => {
                        "Request queue is full — try again shortly".into()
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                        "Connection worker stopped; request was not sent".into()
                    }
                }
            }
        }
    };
    hud.action_feedback = Some((now, message));
}
