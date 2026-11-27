//! Titanium kind-231 descriptions are installed spell text, not another
//! generic damage sentence. Numeric damage remains a separate received fact.
use super::{ClientWorld, Notice, SpellCatalog};
use crate::{
    SpawnKind, WorldPosition,
    combat::{Damage, DamageOutcome},
};

/// Installed text borrowed for one reduction; unknown and empty stay distinct.
#[derive(Clone, Copy, Debug)]
pub struct SpellImpact<'a> {
    /// The spell's target text (field 6).
    pub on_you: Option<&'a str>,
    /// The suffix for another target's name (field 7).
    pub on_other: Option<&'a str>,
    /// The installed discipline flag, when known.
    pub discipline: Option<bool>,
}

/// One native main-buffer description; the server's point line is independent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpellImpactLine {
    /// Target-side text, optionally wrapped by native singular/plural strings.
    Own {
        /// Installed text for the player; known empty text is preserved.
        text: String,
        /// Positive received damage when the native amount wrapper applies.
        amount: Option<u32>,
    },
    /// A known other target, substituted into its installed suffix.
    Other {
        /// Server name of the other target.
        target: String,
        /// Installed description following the target name.
        text: String,
    },
}

/// The main-buffer filter branch in Titanium 0x4a6bc9-0x4a6d03.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpellImpactFilter {
    /// Own non-Bard source or target bypasses the other-spell filters.
    Own,
    /// Own Bard: any `BardSongs` mode except `Hide`.
    OwnBard,
    /// Another Bard: the main-buffer branch accepts `Show` or `GroupOnly`.
    Bard,
    /// Either endpoint is grouped: the `PCSpells` group branch comes first.
    Grouped,
    /// An unrelated non-Bard player.
    Player,
    /// An unrelated non-Bard creature.
    Npc,
}

struct Actor<'a> {
    id: u16,
    name: &'a str,
    class: Option<u32>,
    kind: SpawnKind,
    position: WorldPosition,
}

impl ClientWorld {
    pub(super) fn spell_impact(
        &self,
        damage: &Damage,
        now: std::time::Instant,
        spells: &dyn SpellCatalog,
    ) -> Option<Notice> {
        let player = self.player.as_ref()?;
        let own_source = damage.source_id == player.spawn_id;
        let own_target = damage.target_id == player.spawn_id;
        let target = self.impact_actor(damage.target_id, now)?;
        let source = if damage.source_id == 0 {
            None
        } else {
            Some(self.impact_actor(damage.source_id, now)?)
        };
        let speaker = source.as_ref().unwrap_or(&target);
        if !own_source && !own_target && !within_impact_range(player.position, target.position) {
            return None;
        }
        let spell = u32::from(damage.spell_id?);
        if !(1..10_000).contains(&spell) {
            return None;
        }
        // Target type 13 cast on self has a separate native action/line path;
        // this description reducer must not substitute an ordinary impact.
        if own_source && own_target && matches!(spells.target_type(spell), None | Some(13)) {
            return None;
        }
        let installed = spells.impact(spell)?;
        let line = if own_target {
            SpellImpactLine::Own {
                text: installed.on_you?.to_owned(),
                amount: match damage.outcome {
                    DamageOutcome::Hit(amount) if amount > 0 => Some(amount),
                    _ => None,
                },
            }
        } else {
            let text = installed.on_other?;
            if text.is_empty() {
                return None;
            }
            SpellImpactLine::Other {
                target: target.name.to_owned(),
                text: text.to_owned(),
            }
        };
        let filter = self.impact_filter(speaker, &target, player.spawn_id)?;
        Some(Notice::SpellImpact {
            line,
            filter,
            message_type: installed
                .discipline
                .map(|flag| if flag { 0x10f } else { 0x108 }),
        })
    }

    fn impact_filter(
        &self,
        speaker: &Actor<'_>,
        target: &Actor<'_>,
        own: u16,
    ) -> Option<SpellImpactFilter> {
        Some(if speaker.class? == 8 {
            if speaker.id == own {
                SpellImpactFilter::OwnBard
            } else {
                SpellImpactFilter::Bard
            }
        } else if speaker.id == own || target.id == own {
            SpellImpactFilter::Own
        } else if self.impact_grouped(speaker) || self.impact_grouped(target) {
            SpellImpactFilter::Grouped
        } else if speaker.kind == SpawnKind::Player {
            SpellImpactFilter::Player
        } else {
            SpellImpactFilter::Npc
        })
    }

    fn impact_actor(&self, id: u16, now: std::time::Instant) -> Option<Actor<'_>> {
        if let Some(player) = self.player.as_ref().filter(|player| player.spawn_id == id) {
            return Some(Actor {
                id,
                name: &player.name,
                class: player.class,
                kind: SpawnKind::Player,
                position: player.position,
            });
        }
        let received = self.zone.spawns.get(&id)?;
        let spawn = &received.state;
        Some(Actor {
            id,
            name: &spawn.name,
            class: spawn.class.map(u32::from),
            kind: spawn.kind,
            position: received.carried(self.generation(), now),
        })
    }

    fn impact_grouped(&self, actor: &Actor<'_>) -> bool {
        actor.kind == SpawnKind::Player
            && self.group.as_ref().is_some_and(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.eq_ignore_ascii_case(actor.name))
            })
    }
}

// Independent native trace: 452c78 looks up target; 439ca3/439bd1 sum
// fabs(X/Y/Z) deltas at factor 1; 644804 is 75. Confirmed by read 416.
fn within_impact_range(own: WorldPosition, target: WorldPosition) -> bool {
    // Native stores each absolute delta as f32, then compares their x87
    // sum without rounding the total back to f32 at the 75-unit boundary.
    let distance = f64::from((target.x - own.x).abs())
        + f64::from((target.y - own.y).abs())
        + f64::from((target.z - own.z).abs());
    distance.is_finite() && distance <= 75.0
}
