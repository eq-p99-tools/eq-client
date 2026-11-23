//! Local spell-request feedback; server notifications remain authoritative.

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
    pub linked_timer: Option<u32>,
}

/// Why a cast can't start now because one is under way: casting, or a
/// request the server hasn't answered. A gem press and a discipline meet
/// the same refusal, as both go to the program's one cast start (0x411f3d).
pub(crate) fn busy(
    world: &eq_client_core::world::ClientWorld,
    map: &crate::keys::KeyMap,
) -> Option<crate::chat::Said> {
    if world.casting().cast.is_some() {
        return Some(
            format!(
                "Already casting: {} to interrupt",
                map.named(crate::keys::Act::Duck, "duck")
            )
            .into(),
        );
    }
    world
        .casting()
        .pending
        .is_some()
        .then(|| "Waiting for the server to acknowledge the cast".into())
}

/// The gem's spell, if the request can go now; otherwise why not, in the
/// player's words. Like the official client, a cast refuses locally when the
/// server-reported mana is short (P99 otherwise answers with a generic
/// interruption); forgetting never needs mana.
pub(super) fn check(
    world: &eq_client_core::world::ClientWorld,
    player: &PlayerState,
    request: &Request,
    (messages, map): (&super::messages::Messages, &crate::keys::KeyMap),
    now: Instant,
) -> Result<u32, crate::chat::Said> {
    let Request {
        gem,
        forgetting,
        mana_cost,
        linked_timer,
        ..
    } = *request;
    let spell_id = player
        .memorized_spells
        .get(usize::from(gem))
        .copied()
        .flatten()
        .ok_or_else(|| crate::chat::Said::own(super::empty_gem(gem, map)))?;
    // While a Bard sings and no other cast is under way, a gem press stops
    // the song whatever its timers or the mana, and casts nothing
    // (0x4ecb56, read R49-21c); a forget goes too ([I], R49-48 a). The
    // session decides both.
    let singing = world.casting().singing.is_some() && !world.casting().holds_actions();
    if !singing && let Some(refusal) = busy(world, map) {
        return Err(refusal);
    }
    if forgetting || singing {
        return Ok(spell_id);
    }
    if mana_cost
        .zip(world.vitals().mana)
        .is_some_and(|(cost, mana)| cost > mana)
    {
        return Err(messages.said_text(199, "Not enough mana for that spell"));
    }
    let remaining = world.casting().spell_wait(spell_id, linked_timer, now);
    if !remaining.is_zero() {
        return Err(format!("Spell available in {:.1}s", remaining.as_secs_f32()).into());
    }
    Ok(spell_id)
}

/// Checks the request and sends it without predicting its result. A refusal
/// is said in the chat; a request sent says nothing, as the cast bar shows
/// the cast once the server takes it, and the outbox says its own refusals.
/// A cast from a gem that holds a spell first runs `before`, which may
/// send what the installed client sends before its own checks (the
/// Mac-era program's invisibility drop) and gives back the lines to say.
pub(super) fn spell(
    chat: &mut crate::chat::ChatState,
    world: &eq_client_core::world::ClientWorld,
    player: &PlayerState,
    outbox: &crate::outbox::Outbox,
    request: &Request,
    wording: (&super::messages::Messages, &crate::keys::KeyMap),
    before: impl FnOnce() -> Vec<eq_client_core::chat::ChatLine>,
) {
    let now = Instant::now();
    let Request {
        gem,
        target_id,
        forgetting,
        ..
    } = *request;
    if !forgetting
        && player
            .memorized_spells
            .get(usize::from(gem))
            .copied()
            .flatten()
            .is_some()
    {
        for line in before() {
            chat.history.push(line);
        }
    }
    match check(world, player, request, wording, now) {
        Err(refusal) => chat.refuse(refusal),
        Ok(spell_id) => {
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
