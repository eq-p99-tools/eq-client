//! The player's abilities: which they have, by the skills the server counts,
//! and the recovery timers the session started for them.
use super::{Changes, ClientWorld, Notice};
use crate::{
    abilities::{Ability, AskAnswer, AskOutcome},
    bind_wound::BindWoundUpdate,
    money::Coin,
};
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

/// A kind of coin in the client's words.
const fn coin_words(coin: Coin) -> &'static str {
    match coin {
        Coin::Platinum => "platinum",
        Coin::Gold => "gold",
        Coin::Silver => "silver",
        Coin::Copper => "copper",
    }
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
            .filter(|ability| match self.generation {
                crate::GameDialect::EqMac => {
                    crate::abilities::eqmac::known(*ability, skills, player.race, player.class)
                }
                _ => ability.known(skills, player.race),
            })
            .collect()
    }

    /// Whether the server type offers the ability; one it does not is shown
    /// greyed, and the session refuses it. Before the session says, all are.
    #[must_use]
    pub fn ability_offered(&self, ability: Ability) -> bool {
        self.offered_abilities
            .as_ref()
            .is_none_or(|offered| offered.contains(&ability))
    }

    /// How long until the ability's recovery timer runs out, while it runs.
    #[must_use]
    pub fn ability_wait(&self, ability: Ability, now: Instant) -> Option<Duration> {
        let ready = self.ability_timers.get(&ability.recovery()?)?;
        Some(ready.saturating_duration_since(now)).filter(|wait| !wait.is_zero())
    }

    /// The server released the named skill's recovery group. Unknown skills
    /// cannot release unrelated abilities. A reset does not imply success.
    pub(super) fn skill_ready(&mut self, skill: u32) {
        if let Some(recovery) = Ability::from_skill(skill)
            .filter(|ability| {
                self.generation != crate::GameDialect::EqMac
                    || crate::abilities::eqmac::SUPPORTED.contains(ability)
            })
            .and_then(Ability::recovery)
        {
            self.ability_timers.remove(&recovery);
        }
    }

    /// The session used an ability and started its timer, with the
    /// official client's line for it where it has one.
    pub(super) fn ability_used(
        &mut self,
        (session_id, ability, ready_in, string_id): (u64, Ability, Duration, Option<u32>),
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
        if let Some(string_id) = string_id {
            changes.notices.push(Notice::ProgramLine {
                string_id,
                arguments: Vec::new(),
                lines: Vec::new(),
                said: format!("{}: used", ability.name()),
            });
        }
        if ability == Ability::Tracking {
            self.tracking_pressed();
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

    /// The answer to a beg or a pick, in the official client's line; the
    /// session tells the coins it brought separately.
    pub(super) fn ask_answered(answer: &AskAnswer, changes: &mut Changes) {
        let (string_id, arguments) = answer.line();
        let ability = answer.ability.name();
        let said = match &answer.outcome {
            AskOutcome::Failed => format!("{ability}: nothing"),
            AskOutcome::Coins { coin, amount } => {
                format!("{ability}: {amount} {}", coin_words(*coin))
            }
            AskOutcome::Item(name) => format!("{ability}: {name}"),
        };
        changes.notices.push(Notice::ProgramLine {
            string_id,
            arguments,
            lines: Vec::new(),
            said,
        });
    }

    /// The session refused an ability, and why: in its words, and the
    /// official client's string with what it names where it has one.
    pub(super) fn ability_refused(
        &self,
        session_id: u64,
        (reason, string_id, arguments): (&str, Option<u32>, &[String]),
        changes: &mut Changes,
    ) {
        if self.session_id == Some(session_id) {
            changes.notices.push(Notice::AbilityRefused {
                reason: reason.to_owned(),
                string_id,
                arguments: arguments.to_vec(),
            });
        } else {
            changes.ignored = true;
        }
    }

    /// Bandaging outcomes leave the session's button recovery running.
    pub(super) fn bind_wound(update: &BindWoundUpdate, changes: &mut Changes) {
        if !matches!(update, BindWoundUpdate::Unlocked) {
            changes.notices.push(Notice::BindWound(update.clone()));
        }
    }
}
