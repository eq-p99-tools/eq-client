//! The player's abilities: which they have, by the skills the server counts,
//! and the recovery timers the session started for them.
use super::{Changes, ClientWorld, Notice};
use crate::abilities::Ability;
use std::time::{Duration, Instant};

/// The compass point a heading faces, clockwise from north (0) to
/// north-west (7). EQ headings turn the other way, west at 128 of 512.
fn compass(heading: f32) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a heading rounded to one of eight points"
    )]
    let point = (heading.rem_euclid(512.0) / 64.0).round() as u8 % 8;
    (8 - point) % 8
}

impl ClientWorld {
    /// The abilities the player has, strikes and taunt first.
    #[must_use]
    pub fn abilities(&self) -> Vec<Ability> {
        let Some(player) = self.player.as_ref() else {
            return Vec::new();
        };
        let skills = player.skills.as_deref().unwrap_or_default();
        Ability::ALL
            .into_iter()
            .filter(|ability| ability.known(skills, player.race))
            .collect()
    }

    /// How long until the ability's recovery timer runs out, while it runs.
    #[must_use]
    pub fn ability_wait(&self, ability: Ability, now: Instant) -> Option<Duration> {
        let ready = self.ability_timers.get(&ability.recovery()?)?;
        Some(ready.saturating_duration_since(now)).filter(|wait| !wait.is_zero())
    }

    /// The session used an ability and started its timer.
    pub(super) fn ability_used(
        &mut self,
        (session_id, ability, ready_in): (u64, Ability, Duration),
        now: Instant,
        changes: &mut Changes,
    ) {
        if self.session_id != Some(session_id) {
            changes.ignored = true;
            return;
        }
        if let Some(recovery) = ability.recovery() {
            self.ability_timers.insert(recovery, now + ready_in);
        }
        // The client says where the player faces; servers say nothing.
        if ability == Ability::SenseHeading
            && let Some(player) = self.player.as_ref()
        {
            changes
                .notices
                .push(Notice::Heading(compass(player.position.heading)));
        }
    }

    /// The session refused an ability, and why.
    pub(super) fn ability_refused(&self, session_id: u64, reason: &str, changes: &mut Changes) {
        if self.session_id == Some(session_id) {
            changes
                .notices
                .push(Notice::AbilityRefused(reason.to_owned()));
        } else {
            changes.ignored = true;
        }
    }
}
