//! Local spell-request feedback; server notifications remain authoritative.

use eq_client_core::{ClientCommand, PlayerState};
use std::time::Instant;

/// One user attempt to cast or forget a memorized gem.
#[derive(Clone, Copy)]
pub(super) struct Request {
    pub gem: u8,
    pub target_id: u16,
    pub forgetting: bool,
    pub linked_timer: Option<u32>,
    /// What the program's cast start tests of the spell and the options.
    pub checks: CastChecks,
}

/// What the Titanium program's cast start (0x411f3d, read R49-34 e) tests
/// beyond the gem, from the installed spell data and the options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CastChecks {
    /// The level the player's class casts the spell at, from the installed
    /// spell data (255 for a class that can't); None where it isn't known,
    /// which leaves the test to the server.
    pub spell_level: Option<u8>,
    /// Whether the spell is a song: one a Bard has a level for ([I]: the
    /// program's own song test reads the spell's skill, not mapped here).
    pub song: bool,
    /// Allow Auto Stand: a sitting player stands to cast.
    pub auto_stand: bool,
}

/// The gem click's line while a cast is under way or the gem's refresh
/// runs (0x4ecb56, read R49-34 h), in colour 0x108. The cast start's own
/// "already casting" line (0x309a) is never reached from a gem, as the
/// click tests first.
const GEM_WAIT: u32 = 0x2efd;
const GEM_WAIT_COLOUR: u32 = 0x108;
/// The cast start's line when the player's level is below the spell's
/// (R49-34 e 10).
const LEVEL_TOO_LOW: u32 = 0x309b;
/// Its line when the player is stunned (R49-34 e 5).
const STUNNED: u32 = 0xae;
/// Its line for a non-song cast while not standing (R49-34 e 16, j): the
/// program tests for posture 'd' (100, standing), so the crouch ('o', 110
/// [I]) meets it, where sitting and lying meet the earlier stand test.
const DUCKING: u32 = 0xd5;
/// The colour of those lines.
const CAST_COLOUR: u32 = 0x121;

/// What a gem press that passes does: cast this spell, standing first where
/// the player sits and Allow Auto Stand is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Checked {
    pub spell_id: u32,
    pub stand_first: bool,
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

/// The gem's spell, if the request can go now; otherwise the line that says
/// why not, or None where the program says nothing. Mana is the server's to
/// judge: the Titanium program compares none before it sends the cast (read
/// R49-34 g), and the server's answer, in its own words, ends the cast.
pub(super) fn check(
    world: &eq_client_core::world::ClientWorld,
    player: &PlayerState,
    request: &Request,
    (messages, map): (&super::messages::Messages, &crate::keys::KeyMap),
    now: Instant,
) -> Result<Checked, Option<crate::chat::Said>> {
    use eq_client_core::PostureState;
    let Request {
        gem,
        forgetting,
        linked_timer,
        checks,
        ..
    } = *request;
    let line = |id, ours: &str| messages.said_text(id, ours).typed(CAST_COLOUR);
    let pass = |spell_id| Checked {
        spell_id,
        stand_first: false,
    };
    let spell_id = player
        .memorized_spells
        .get(usize::from(gem))
        .copied()
        .flatten()
        .ok_or_else(|| crate::chat::Said::own(super::empty_gem(gem, map)))?;
    // A right click forgets a filled gem at once, with no confirm, while no
    // request is pending, even mid-cast; while one is, it does nothing and
    // says nothing (0x4eccc6, read R49-34 d).
    if forgetting {
        if world.casting().pending.is_some() || world.book_request_pending() {
            return Err(None);
        }
        return Ok(pass(spell_id));
    }
    if world.casting().pending.is_some() || world.book_request_pending() {
        return Err(None);
    }
    // While a Bard sings and no other cast is under way, a gem press stops
    // the song whatever its timers, and casts nothing (0x4ecb56, read
    // R49-21c). The session decides it.
    let singing = world.casting().singing.is_some() && !world.casting().holds_actions();
    if singing {
        return Ok(pass(spell_id));
    }
    // The gem click (0x4ecb56, read R49-34 h): while a cast is under way or
    // the gem's refresh or shared recovery runs, its line and nothing sent.
    // Ours words, where the installation lacks the line, say which wait it is.
    let remaining = world
        .casting()
        .gem_cast_wait(gem, spell_id, linked_timer, now);
    if world.casting().cast.is_some() || remaining.is_none_or(|remaining| !remaining.is_zero()) {
        let ours = if world.casting().cast.is_some() {
            format!(
                "Already casting: {} to interrupt",
                map.named(crate::keys::Act::Duck, "duck")
            )
        } else {
            remaining.map_or_else(
                || "Spell timing is unavailable".into(),
                |remaining| format!("Spell available in {:.1}s", remaining.as_secs_f32()),
            )
        };
        return Err(Some(
            messages.said_text(GEM_WAIT, &ours).typed(GEM_WAIT_COLOUR),
        ));
    }
    // The program's cast start, in its order (0x411f3d, read R49-34 e).
    // 4: a sitting player stands first with Allow Auto Stand; lying is the
    // session's (0x3099), and sitting without it is left to the server.
    let posture = world.posture(player.spawn_id);
    let stand_first = checks.auto_stand && posture == Some(PostureState::Sitting);
    // 5: stunned.
    if world.is_stunned() {
        return Err(Some(line(STUNNED, "You are stunned.")));
    }
    // 9 (already casting) is the click's above.
    // 10: the player's level below the spell's for their class.
    if checks.spell_level.is_some_and(|level| player.level < level) {
        return Err(Some(line(
            LEVEL_TOO_LOW,
            "Your level is too low for that spell.",
        )));
    }
    // 16: a non-song cast while crouched. [I] Every non-song: the spell's
    // own "may cast unstanding" field (+0x1fc) isn't mapped here, so a spell
    // that has it is refused where the program would cast, never the other
    // way. A song while lying (0xd4) waits on that field too.
    if !checks.song && posture == Some(PostureState::Ducking) {
        return Err(Some(line(DUCKING, "You must be standing to cast that.")));
    }
    Ok(Checked {
        spell_id,
        stand_first,
    })
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
        Err(Some(refusal)) => chat.refuse(refusal),
        Err(None) => {}
        Ok(Checked {
            spell_id,
            stand_first,
        }) => {
            if stand_first {
                let _ = outbox.post(world, |stamp| ClientCommand::SetPosture {
                    session_id: stamp.session_id,
                    spawn_id: player.spawn_id,
                    posture: eq_client_core::Posture::Standing,
                    created: stamp.created,
                });
            }
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
