//! What the spawns in view are doing that a front end shows as motion: each
//! one's latest swing or other one-shot motion the server sent, and the cast
//! each one is under, the player's own included. Both are the zone's, so
//! they go with a spawn that leaves and with the zone.
use super::{CastNews, Changes, ClientWorld};
use crate::{SpellUpdate, combat::Animation};
use std::time::{Duration, Instant};

/// How long past its duration a cast that nothing ended is still taken to
/// be under way (inferred): its landing or an interruption ends it sooner.
const CAST_MARGIN: Duration = Duration::from_secs(1);

/// A spawn's latest one-shot motion, as the server sent it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// The servers' animation number, such as 5 for a one-handed swing.
    pub action: u16,
    /// How fast to play it, as a multiple of normal; both servers send
    /// normal speed for every swing their code sends.
    pub speed: f32,
    /// Never the same for two motions, so a front end tells a new swing
    /// from the one before it when the two are alike.
    pub count: u64,
}

/// A cast under way, as its beginning said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cast {
    /// The spell being cast.
    pub(super) spell_id: u16,
    /// When it is over if nothing ends it sooner.
    pub(super) until: Instant,
}

impl ClientWorld {
    /// A spawn's one-shot motion, such as a swing, for a spawn in view or
    /// the player.
    pub(super) fn motion_news(&mut self, animation: Animation, changes: &mut Changes) {
        if !self.in_view(animation.spawn_id) {
            changes.ignored = true;
            return;
        }
        self.motion_count = self.motion_count.wrapping_add(1);
        self.zone.motions.insert(
            animation.spawn_id,
            Motion {
                action: animation.action,
                speed: animation.speed,
                count: self.motion_count,
            },
        );
    }

    /// Casts by anyone in view: a beginning starts one, replacing the
    /// caster's last, and an interruption ends it.
    pub(super) fn cast_news(&mut self, update: &SpellUpdate, now: Instant) {
        match *update {
            SpellUpdate::Began {
                caster_id,
                spell_id,
                duration_ms,
            } if self.in_view(caster_id) => {
                let duration = Duration::from_millis(u64::from(duration_ms));
                self.zone.casts.insert(
                    caster_id,
                    Cast {
                        spell_id,
                        until: now + duration + CAST_MARGIN,
                    },
                );
            }
            SpellUpdate::Interrupted { caster_id, .. } => {
                if let Ok(caster_id) = u16::try_from(caster_id) {
                    self.zone.casts.remove(&caster_id);
                }
            }
            _ => (),
        }
    }

    /// The player's own cast also ends where their cast bar's does: a gem's
    /// refresh or the spell's mana ends it, whether the spell landed or not.
    pub(super) fn own_cast_news(&mut self, own_id: u16, news: Option<CastNews>) {
        if matches!(news, Some(CastNews::Refreshed | CastNews::Ended)) {
            self.zone.casts.remove(&own_id);
        }
    }

    /// A spell landed: its caster's cast is over.
    pub(super) fn landed(&mut self, caster_id: u16) {
        self.zone.casts.remove(&caster_id);
    }

    /// Forgets the casts that ran past their time with nothing to end them.
    pub(super) fn expire_casts(&mut self, now: Instant) {
        self.zone.casts.retain(|_, cast| cast.until > now);
    }

    /// Whether a spawn ID is the player's or a spawn's in view.
    fn in_view(&self, id: u16) -> bool {
        self.is_player(id) || self.zone.spawns.contains_key(&id)
    }
}
