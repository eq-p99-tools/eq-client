//! News about the player's spells: gems and the book, casts, buffs and
//! lasting effects, and spellbook changes.
use super::{CastNews, Changes, ClientWorld, InterruptPrint, Notice, OtherCaster, SpellCatalog};
use crate::{BookActionStatus, Buff, BuffUpdate, SpellEffect, SpellUpdate, buffs::Landing};
use std::time::Instant;

/// How a generation's program prints an interruption the server names,
/// the player's own when `own_bard` says the player is a Bard: the Titanium
/// program's `OP_InterruptCast` print tail (0x45e233, reads R49-51 and
/// R49-51b). The others are unread and keep this client's line.
const fn interrupt_print(dialect: crate::GameDialect, own_bard: bool) -> InterruptPrint {
    match dialect {
        crate::GameDialect::Titanium if own_bard => InterruptPrint::TitaniumBard,
        crate::GameDialect::Titanium => InterruptPrint::Titanium,
        crate::GameDialect::EqMac | crate::GameDialect::Rof2 => InterruptPrint::Unread,
    }
}

impl ClientWorld {
    /// A spell notice: the book and gems change, and the player's own casts
    /// move along while they are connected and alive. An interruption also
    /// tells the player why, in the server's words, and another caster's,
    /// which the server sends with the caster's name, says whose it was.
    /// Whose cast it was goes by spawn ID; the name only fills the line.
    pub(super) fn spell(&mut self, update: &SpellUpdate, now: Instant, changes: &mut Changes) {
        self.cast_news(update, now);
        if let Some(book) = self.spell_book.as_mut() {
            book.apply(update);
        }
        let active = self.connected && self.death.is_none();
        let other = match (self.generation, update) {
            (crate::GameDialect::Titanium, SpellUpdate::Interrupted { caster_id, .. }) => {
                InterruptPrint::TitaniumOther(self.other_caster(*caster_id))
            }
            _ => InterruptPrint::Unread,
        };
        let Some(player) = self.player.as_mut() else {
            return;
        };
        update.apply_gems(&mut player.memorized_spells);
        if !active {
            return;
        }
        if let SpellUpdate::Interrupted {
            caster_id,
            message_id,
            caster_name: Some(caster),
        } = update
            && *caster_id != u32::from(player.spawn_id)
            && !caster.is_empty()
        {
            changes.notices.push(Notice::OtherCastInterrupted {
                string_id: *message_id,
                caster: caster.clone(),
                print: other,
            });
        }
        changes.cast = self
            .casting
            .observe(player.spawn_id, &player.memorized_spells, update, now);
        let own_id = player.spawn_id;
        let print = interrupt_print(self.generation, player.class == Some(super::casting::BARD));
        self.own_cast_news(own_id, changes.cast);
        if let (Some(CastNews::Interrupted), Some((_, string_id))) =
            (changes.cast, self.casting.interrupted)
        {
            changes
                .notices
                .push(Notice::CastInterrupted { string_id, print });
        }
    }

    /// Who another caster is for the Titanium program's interruption
    /// filters (0x45e233, read R49-51b): a Bard by class whatever their
    /// kind, another player, or an NPC, and whether a player is in the
    /// player's group, by name as the program's group list holds them.
    fn other_caster(&self, caster_id: u32) -> OtherCaster {
        let Some(spawn) = u16::try_from(caster_id)
            .ok()
            .and_then(|id| self.zone.spawns.get(&id))
        else {
            return OtherCaster::Unknown;
        };
        let player = spawn.state.kind == crate::SpawnKind::Player;
        let grouped = player
            && self.group.as_ref().is_some_and(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.eq_ignore_ascii_case(&spawn.state.name))
            });
        if spawn.state.class == Some(8) {
            OtherCaster::Bard { grouped }
        } else if player {
            OtherCaster::Player { grouped }
        } else {
            OtherCaster::Npc
        }
    }

    /// Local transport submission or the ordered clear before its answer.
    pub(super) fn cast_pending(
        &mut self,
        session_id: u64,
        spell_id: Option<u32>,
        changes: &mut Changes,
    ) {
        if self.accepts_reply(session_id) && self.death.is_none() {
            self.casting.pend(spell_id);
        } else {
            changes.ignored = true;
        }
    }

    /// Ends the worker's local wait for the matching pending request.
    pub(super) fn cast_request_unconfirmed(
        &mut self,
        session_id: u64,
        spell_id: u32,
        changes: &mut Changes,
    ) {
        if self.accepts_reply(session_id)
            && self.death.is_none()
            && self.casting.pending == Some(spell_id)
        {
            self.casting.pending = None;
        } else {
            changes.ignored = true;
        }
    }

    /// Keeps the worker's request context for the following own start.
    pub(super) fn cast_late_begin(
        &mut self,
        session_id: u64,
        requested_spell: u32,
        changes: &mut Changes,
    ) {
        if self.accepts_reply(session_id) && self.death.is_none() && self.casting.pending.is_none()
        {
            self.casting.late_begin(requested_spell);
        } else {
            changes.ignored = true;
        }
    }

    /// Releases an identified local cast without inventing a spell result.
    pub(super) fn release_cast_hold(&mut self, session_id: u64, spell_id: u32) -> bool {
        let Some(own_id) = self.player.as_ref().map(|player| player.spawn_id) else {
            return false;
        };
        if !self.accepts_reply(session_id)
            || self.death.is_some()
            || !self.casting.release_hold(spell_id)
        {
            return false;
        }
        if self
            .zone
            .casts
            .get(&own_id)
            .is_some_and(|cast| u32::from(cast.spell_id) == spell_id)
        {
            self.zone.casts.remove(&own_id);
        }
        true
    }

    /// Applies the session's local release without changing buffs or cooldowns.
    pub(super) fn song_stop_unconfirmed(
        &mut self,
        session_id: u64,
        spell_id: Option<u32>,
        changes: &mut Changes,
    ) {
        let applies = match spell_id {
            Some(spell_id) => self.release_cast_hold(session_id, spell_id),
            None => self.accepts_reply(session_id) && self.death.is_none(),
        };
        if applies {
            changes.notices.push(Notice::SongStopUnconfirmed {
                released_cast: spell_id.is_some(),
            });
        } else {
            changes.ignored = true;
        }
    }

    /// The server refused the player's cast.
    pub(super) fn cast_refused(
        &self,
        (session_id, spell_id): (u64, u32),
        (reason, string_id): (&str, Option<u32>),
        changes: &mut Changes,
    ) {
        if self.accepts_reply(session_id) && self.death.is_none() {
            changes.notices.push(Notice::CastRefused {
                spell_id,
                reason: reason.to_owned(),
                string_id,
            });
        } else {
            changes.ignored = true;
        }
    }

    /// Every buff slot at once, as at admission.
    pub(super) fn buff_snapshot(
        &mut self,
        buffs: &[Option<Buff>],
        now: Instant,
        changes: &mut Changes,
    ) {
        self.buffs.replace_snapshot(
            buffs
                .iter()
                .enumerate()
                .filter_map(|(slot, buff)| Some((u32::try_from(slot).ok()?, buff.clone()?)))
                .collect(),
            u32::try_from(buffs.len()).unwrap_or(u32::MAX),
            now,
        );
        self.buff_disagreements(changes);
    }

    /// Reports each removal the slot finder predicted that the server's
    /// word contradicted (6.612 T1).
    fn buff_disagreements(&mut self, changes: &mut Changes) {
        changes.notices.extend(
            self.buffs
                .take_disagreements()
                .into_iter()
                .map(Notice::BuffPredictionContradicted),
        );
    }

    /// One of the player's buff slots changed.
    pub(super) fn buff(&mut self, update: &BuffUpdate, now: Instant, changes: &mut Changes) {
        if self
            .player
            .as_ref()
            .is_some_and(|player| u32::from(player.spawn_id) == update.entity_id)
        {
            if self.buffs.apply(update.clone(), now) {
                changes.notices.push(Notice::BuffSlotMismatch);
            }
            self.buff_disagreements(changes);
        } else {
            changes.ignored = true;
        }
    }

    /// A spell took effect on someone, which ends its caster's cast and
    /// completes it if it was under way; only a lasting effect on the player
    /// leaves a buff the server has not slotted.
    pub(super) fn spell_effect(
        &mut self,
        effect: &SpellEffect,
        (now, spells): (Instant, &dyn SpellCatalog),
        changes: &mut Changes,
    ) {
        self.landed(effect.caster_id, now);
        self.landed_on(effect);
        if self.is_player(effect.target_id) {
            // Titanium's landing lies the player down for effect 74 (read
            // R21), by the program's own landing test.
            changes.feign_landed = effect.took_hold()
                && self.generation.spell_feign_lies()
                && spells.feigns(u32::from(effect.spell_id));
            if effect.took_hold()
                && !matches!(effect.spell_id, 0 | u16::MAX)
                && !spells.instant_effect(u32::from(effect.spell_id))
            {
                let landing = self.landing(effect, spells);
                self.buffs.observe_effect(effect.clone(), &landing, now);
            }
        } else {
            changes.ignored = true;
        }
    }

    /// What the spell data and the world say about a landing on the player,
    /// for placing it as the Titanium program does.
    fn landing(&self, effect: &SpellEffect, spells: &dyn SpellCatalog) -> Landing {
        let spell = u32::from(effect.spell_id);
        let own = effect.caster_id == effect.target_id;
        let new = spells.effect_ids(spell);
        // Step 2's blocker test, only partly read: any held spell with
        // effect 148 leaves the landing unplaced. Native 0x40ea07 includes
        // the same spell only for 0x0abf; step 4 still excludes it.
        let blockers: Vec<u32> = self
            .buffs
            .spells()
            .filter(|held| *held != spell || spell == 0x0abf)
            .collect();
        let blocker = blockers.iter().try_fold(false, |found, held| {
            let ids = spells.effect_ids(*held)?;
            Some(found || ids.contains(&148))
        });
        let blocker = if blockers.iter().any(|held| {
            spells
                .effect_ids(*held)
                .is_some_and(|ids| ids.contains(&148))
        }) {
            Some(true)
        } else {
            blocker
        };
        // R20 (d): caster spawn +0x24c, plus effect 0x110's total for a Bard
        // song, which this client does not know: a Bard caster's is left
        // unknown.
        let caster_level = self
            .spawn(effect.caster_id)
            .filter(|caster| caster.state.class.is_some_and(|class| class != 8))
            .map(|caster| caster.state.level)
            .filter(|level| *level != 0);
        // What step 4 compares, from the spell data (read R49-22e); each
        // held spell's level comes from its record in the tracker.
        let facts = |id: u32, level: Option<u8>| crate::buffs::placement::Spell {
            id,
            band: spells.buff_band(id),
            bard: spells.bard_level(id),
            effects: spells.placement_effects(id),
            level,
        };
        Landing {
            finder: self.generation().buff_placement(),
            keeps_record: new.map(|ids| ids.iter().any(|id| matches!(id, 0x43 | 0x65 | 0x71))),
            spell: facts(spell, caster_level),
            held: self
                .buffs
                .spells()
                .map(|held| (held, facts(held, None)))
                .collect(),
            blocker,
            record_type: spells
                .target_type(spell)
                .map(|kind| if own && matches!(kind, 13 | 20) { 4 } else { 2 }),
            caster_level,
        }
    }

    /// The session's word on the spellbook change in flight.
    pub(super) fn book_action_news(&mut self, status: &BookActionStatus) {
        self.book_action = Some(status.clone());
        self.book_action_revision = self.book_action_revision.wrapping_add(1);
    }
}
