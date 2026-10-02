//! News about the player's spells: gems and the book, casts, buffs and
//! lasting effects, and spellbook changes.
use super::{CastNews, Changes, ClientWorld, Notice, SpellCatalog};
use crate::{BookActionStatus, Buff, BuffUpdate, SpellEffect, SpellUpdate};
use std::time::Instant;

impl ClientWorld {
    /// A spell notice: the book and gems change, and the player's own casts
    /// move along while they are connected and alive.
    pub(super) fn spell(&mut self, update: &SpellUpdate, now: Instant) -> Option<CastNews> {
        // Forgetting a gem is answered only by the gem emptying.
        if matches!(update, SpellUpdate::Slot { mode: 2, .. })
            && matches!(self.book_action, Some(BookActionStatus::Submitted))
        {
            self.book_action = None;
        }
        if let Some(book) = self.spell_book.as_mut() {
            book.apply(update);
        }
        let active = self.connected && self.death.is_none();
        let player = self.player.as_mut()?;
        update.apply_gems(&mut player.memorized_spells);
        if !active {
            return None;
        }
        self.casting
            .observe(player.spawn_id, &player.memorized_spells, update, now)
    }

    /// The server took the player's cast request and has not begun it yet.
    pub(super) fn cast_pending(
        &mut self,
        session_id: u64,
        spell_id: Option<u32>,
        changes: &mut Changes,
    ) {
        if self.accepts_reply(session_id) && self.death.is_none() {
            self.casting.pending = spell_id;
        } else {
            changes.ignored = true;
        }
    }

    /// The server refused the player's cast.
    pub(super) fn cast_refused(
        &self,
        (session_id, spell_id): (u64, u32),
        reason: &str,
        changes: &mut Changes,
    ) {
        if self.accepts_reply(session_id) && self.death.is_none() {
            changes.notices.push(Notice::CastRefused {
                spell_id,
                reason: reason.to_owned(),
            });
        } else {
            changes.ignored = true;
        }
    }

    /// Every buff slot at once, as at admission.
    pub(super) fn buff_snapshot(&mut self, buffs: &[Option<Buff>]) {
        self.buffs.replace_snapshot(
            buffs
                .iter()
                .enumerate()
                .filter_map(|(slot, buff)| Some((u32::try_from(slot).ok()?, buff.clone()?)))
                .collect(),
            u32::try_from(buffs.len()).unwrap_or(u32::MAX),
        );
    }

    /// One of the player's buff slots changed.
    pub(super) fn buff(&mut self, update: &BuffUpdate, changes: &mut Changes) {
        if self
            .player
            .as_ref()
            .is_some_and(|player| u32::from(player.spawn_id) == update.entity_id)
        {
            self.buffs.apply(update.clone());
        } else {
            changes.ignored = true;
        }
    }

    /// A spell took effect on someone; only a lasting effect on the player
    /// leaves a buff the server has not slotted.
    pub(super) fn spell_effect(
        &mut self,
        effect: &SpellEffect,
        spells: &dyn SpellCatalog,
        changes: &mut Changes,
    ) {
        if self.is_player(effect.target_id) {
            if effect.effect_flag == 4
                && !matches!(effect.spell_id, 0 | u16::MAX)
                && !spells.instant_effect(u32::from(effect.spell_id))
            {
                self.buffs.observe_effect(effect.clone());
            }
        } else {
            changes.ignored = true;
        }
    }

    /// The session's word on the spellbook change in flight.
    pub(super) fn book_action_news(&mut self, status: &BookActionStatus) {
        self.book_action = (!matches!(status, BookActionStatus::Confirmed)).then(|| status.clone());
        self.book_action_revision = self.book_action_revision.wrapping_add(1);
    }
}
