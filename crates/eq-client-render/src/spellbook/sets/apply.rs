//! Applies a layout one confirmed gem change at a time.
use super::{AdmissionGems, Gems};
use crate::{
    hud::action_bar::{ActionRequests, BookChange},
    outbox::{BOOK_TIMEOUT as STEP_TIMEOUT, Outbox, Stamp},
};
use eq_client_core::{
    BookActionStatus, ClientCommand, PostureState, SpellBook, world::ClientWorld,
};
use std::time::Instant;

#[cfg(test)]
mod tests;

pub(super) struct Apply {
    pub(super) name: String,
    target: Gems,
    seen: Gems,
    session: Option<u64>,
    stun: Option<u64>,
    posture: Option<PostureState>,
    pending: Option<Pending>,
    stop: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Forget(u8, u32),
    Memorize(u8, u32),
}

impl Step {
    fn slot(self) -> usize {
        usize::from(match self {
            Self::Forget(gem, _) | Self::Memorize(gem, _) => gem,
        })
    }

    fn value(self) -> Option<u32> {
        match self {
            Self::Forget(..) => None,
            Self::Memorize(_, spell) => Some(spell),
        }
    }

    fn command(self, stamp: Stamp) -> ClientCommand {
        let Stamp {
            session_id,
            created,
        } = stamp;
        match self {
            Self::Forget(gem, spell_id) => ClientCommand::ForgetSpell {
                session_id,
                gem,
                spell_id,
                created,
            },
            Self::Memorize(gem, spell_id) => ClientCommand::MemorizeSpell {
                session_id,
                gem,
                spell_id,
                created,
            },
        }
    }
}

struct Pending {
    step: Step,
    revision: u64,
    since: Instant,
}

impl Apply {
    /// Validate the full layout before claiming the local book queue.
    pub(super) fn start(
        name: String,
        target: Gems,
        world: &ClientWorld,
        outbox: &Outbox,
    ) -> anyhow::Result<Self> {
        use anyhow::{Context, ensure};
        let seen = world.gems();
        ensure!(world.in_world(), "Enter the world to apply a spell set");
        let book = world.spell_book().context("Spellbook unavailable")?;
        check_learned(&target, &seen, book)?;
        ensure!(
            !world.is_stunned()
                && world.camp().is_none()
                && matches!(
                    posture(world),
                    None | Some(PostureState::Standing | PostureState::Sitting)
                ),
            "Wait until your character can act"
        );
        outbox.begin_spell_set(world, Instant::now())?;
        Ok(Self {
            name,
            target,
            seen,
            session: world.session_id(),
            stun: world.stun_revision(),
            posture: posture(world),
            pending: None,
            stop: None,
        })
    }

    pub(super) fn stop(&mut self, reason: impl Into<String>) {
        if self.stop.is_none() {
            self.stop = Some(reason.into());
        }
    }

    pub(super) fn invalidate(&mut self, old_gems: Option<AdmissionGems>) {
        if let Some((session, gems)) = old_gems
            && self.session == Some(session)
        {
            self.seen = gems;
        }
        self.session = None;
    }

    pub(super) fn stopping(&self) -> bool {
        self.stop.is_some()
    }

    pub(super) fn progress(&self) -> String {
        let count = self.matches(&self.seen);
        match (&self.stop, &self.pending) {
            (Some(_), Some(pending)) => format!(
                "Stopping · {count}/8 match · Waiting for gem {}",
                pending.step.slot() + 1
            ),
            (_, Some(pending)) => format!(
                "{count}/8 match · Waiting for gem {}",
                pending.step.slot() + 1
            ),
            _ => format!("{count}/8 match"),
        }
    }

    fn matches(&self, gems: &Gems) -> usize {
        self.target
            .iter()
            .zip(gems)
            .filter(|(target, actual)| target == actual)
            .count()
    }

    fn stopped(&self, reason: &str, gems: &Gems) -> String {
        format!(
            "{} stopped. {}/8 match. {reason}",
            self.name,
            self.matches(gems)
        )
    }

    /// Poll one authoritative outcome or submit one ordinary session command.
    /// Returns the final result after the sequence has settled.
    pub(super) fn tick(
        &mut self,
        world: &ClientWorld,
        outbox: &Outbox,
        requests: Option<&mut ActionRequests>,
        names: &super::super::SpellNames,
        now: Instant,
    ) -> Option<String> {
        if self.session.is_none() || world.session_id() != self.session || !world.in_world() {
            return Some(self.stopped("The admission ended.", &self.seen));
        }
        let actual = world.gems();
        let posture = posture(world);
        if outbox.spell_set_interrupted()
            || world.stun_revision() != self.stun
            || world.camp().is_some()
            || world.casting().cast.is_some()
            || world.casting().pending.is_some()
            || (posture != self.posture && posture != Some(PostureState::Sitting))
        {
            self.stop("Character actions were interrupted.");
        }
        self.posture = posture;
        if !self.settle(world, &actual, now) {
            return None;
        }
        if actual != self.seen {
            self.stop("The gem layout changed during apply.");
        }
        if let Some(reason) = &self.stop {
            return Some(self.stopped(reason, &actual));
        }
        let Some(step) = next_step(&self.target, &actual) else {
            return Some(format!("Applied {} · 8/8 gems match", self.name));
        };
        match submit(step, &self.target, &actual, world, outbox) {
            Ok(()) => {
                self.pending = Some(Pending {
                    step,
                    revision: world.book_action_revision(),
                    since: now,
                });
                if let Step::Memorize(gem, spell) = step {
                    super::super::note(
                        requests,
                        (BookChange::Memorize, spell),
                        format!("Memorizing {} into gem {}", names.label(spell), gem + 1),
                    );
                } else if let Some(requests) = requests {
                    requests.book = None;
                }
                None
            }
            Err(error) => Some(self.stopped(&error.to_string(), &actual)),
        }
    }

    /// A new status is necessary but insufficient: the exact gem must also
    /// have the requested value. An unrelated update never advances the set.
    fn settle(&mut self, world: &ClientWorld, actual: &Gems, now: Instant) -> bool {
        let Some(pending) = &self.pending else {
            return true;
        };
        let slot = pending.step.slot();
        let value = pending.step.value();
        let mut expected = self.seen;
        expected[slot] = value;
        let reply = (world.book_action_revision() != pending.revision).then(|| world.book_action());
        let failure = match reply {
            Some(Some(
                BookActionStatus::Cancelled(reason) | BookActionStatus::Rejected(reason),
            )) => Some(reason.clone()),
            _ if now.saturating_duration_since(pending.since) >= STEP_TIMEOUT => {
                Some(format!("No confirmation for gem {}.", slot + 1))
            }
            _ => None,
        };
        if reply == Some(None) && actual[slot] == value {
            self.seen = expected;
            self.pending = None;
            return true;
        }
        if let Some(reason) = failure {
            self.stop(reason);
            self.pending = None;
            return true;
        }
        if *actual != self.seen && *actual != expected {
            self.stop("The gem layout changed during apply.");
        }
        false
    }
}

fn posture(world: &ClientWorld) -> Option<PostureState> {
    world
        .player()
        .and_then(|player| world.posture(player.spawn_id))
}

fn check_learned(target: &Gems, current: &Gems, book: &SpellBook) -> anyhow::Result<()> {
    use anyhow::ensure;
    for (index, (&desired, &occupied)) in target.iter().zip(current).enumerate() {
        if let Some(spell) = desired.filter(|_| desired != occupied) {
            ensure!(
                book.slots().contains(&Some(spell)),
                "Spell #{spell} for gem {} is no longer learned",
                index + 1
            );
        }
    }
    Ok(())
}

fn submit(
    step: Step,
    target: &Gems,
    current: &Gems,
    world: &ClientWorld,
    outbox: &Outbox,
) -> anyhow::Result<()> {
    use anyhow::Context;
    check_learned(
        target,
        current,
        world.spell_book().context("Spellbook unavailable")?,
    )?;
    outbox.spell_set_step(world, |stamp| step.command(stamp))?;
    Ok(())
}

/// Preserve matching gems. Clear a spell's old, mismatched slot before
/// moving it, then clear the destination only when it needs replacement.
fn next_step(target: &Gems, current: &Gems) -> Option<Step> {
    let slot = target.iter().zip(current).position(|(a, b)| a != b)?;
    if let Some(spell) = target[slot]
        && let Some(source) = current
            .iter()
            .zip(target)
            .position(|(&value, &wanted)| value == Some(spell) && wanted != value)
    {
        return Some(Step::Forget(u8::try_from(source).ok()?, spell));
    }
    let gem = u8::try_from(slot).ok()?;
    current[slot].map_or_else(
        || target[slot].map(|spell| Step::Memorize(gem, spell)),
        |spell| Some(Step::Forget(gem, spell)),
    )
}
