//! The latest spell that landed on each spawn in view, the player's own
//! included, so a front end can draw the spell's effect on its target. Like
//! the motions, they are the zone's, so they go with a spawn that leaves
//! and with the zone.
use super::ClientWorld;
use crate::SpellEffect;

/// A spell that landed on someone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Landing {
    /// The spell.
    pub spell_id: u16,
    /// Who cast it.
    pub caster_id: u16,
    /// The caster's level, as the landing says.
    pub caster_level: u16,
    /// Never the same for two landings, so a front end tells a new one from
    /// the one before it.
    pub count: u64,
}

impl ClientWorld {
    /// A spell action on a spawn in view or the player is a landing when its
    /// flag is the one the admission names for the server type; with none
    /// named, no action is.
    pub(super) fn landed_on(&mut self, effect: &SpellEffect) {
        let target = effect.target_id;
        if self.spell_landing_flag != Some(effect.effect_flag)
            || !(self.is_player(target) || self.zone.spawns.contains_key(&target))
        {
            return;
        }
        self.motion_count = self.motion_count.wrapping_add(1);
        self.zone.landings.insert(
            target,
            Landing {
                spell_id: effect.spell_id,
                caster_id: effect.caster_id,
                caster_level: effect.caster_level,
                count: self.motion_count,
            },
        );
    }
}
