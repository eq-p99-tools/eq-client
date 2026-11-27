//! News about the player character: level, skills, hit points and health.
use super::{Changes, ClientWorld, ReportedHp, vitals};
use eq_network_game::world::LANGUAGE_SKILLS;

/// The current value in an existing skill or language slot.
pub(super) fn skill_value(player: &crate::PlayerState, skill_id: u32) -> Option<u32> {
    if let Some(language) = skill_id.checked_sub(LANGUAGE_SKILLS) {
        player
            .languages
            .as_ref()?
            .get(usize::try_from(language).ok()?)
            .copied()
            .map(u32::from)
    } else {
        player
            .skills
            .as_ref()?
            .get(usize::try_from(skill_id).ok()?)
            .copied()
    }
}

/// The sound the Titanium program plays for a level gained (read R49-6c).
const LEVEL_GAIN_SOUND: u16 = 0x8b;

/// Whether the own spawn level is distinct from the character's gate level.
pub(super) const fn separate_spawn_level(dialect: crate::GameDialect) -> bool {
    match dialect {
        crate::GameDialect::EqMac => true,
        crate::GameDialect::Titanium | crate::GameDialect::Rof2 => false,
    }
}

impl ClientWorld {
    /// The player's level and experience; what items give can depend on
    /// the level.
    pub(super) fn level_news(&mut self, current: u8, experience: u32, changes: &mut Changes) {
        let program = super::prints_r49_6_lines(self.generation);
        let separate = separate_spawn_level(self.generation);
        match self.player.as_mut() {
            Some(player) if self.connected => {
                // Mac 0x4e06ef only raises character +0x90. A lower/equal
                // update changes neither its experience nor its shown level.
                if separate && current <= player.level {
                    return;
                }
                if player.level != current {
                    self.skills_revision = self.skills_revision.wrapping_add(1);
                }
                // The Titanium program says a loss, and plays sound 0x8b for
                // a gain with no line, measuring from the level it holds
                // (read R49-6c). That 0x8b is the numbers file's is
                // inferred. Its music cue at level 10 is a gap (R49-7).
                if program && current < player.level {
                    changes.notices.push(super::Notice::LevelLost {
                        lost: player.level - current,
                        level: current,
                    });
                } else if program && current > player.level {
                    changes
                        .sounds
                        .push(crate::effects::Cue::Number(LEVEL_GAIN_SOUND));
                }
                player.level = current;
                self.displayed_level = Some(current);
                if separate && let Some(spawn) = self.zone.spawns.get_mut(&player.spawn_id) {
                    spawn.state.level = current;
                }
                self.vitals.experience = Some(experience);
                self.refresh_item_hp();
            }
            _ => changes.ignored = true,
        }
    }

    /// One of the player's skills; a rise is told, as the official client
    /// tells it.
    pub(super) fn skill_news(&mut self, skill_id: u32, value: u32, changes: &mut Changes) {
        match self.player.as_mut() {
            Some(player) if self.connected => {
                let before = skill_value(player, skill_id);
                player.apply_skill(skill_id, value);
                let after = skill_value(player, skill_id);
                if before != after {
                    self.skills_revision = self.skills_revision.wrapping_add(1);
                }
                if skill_id < LANGUAGE_SKILLS && before.is_some_and(|before| value > before) {
                    changes.notices.push(super::Notice::SkillUp {
                        skill: skill_id,
                        value,
                    });
                }
            }
            _ => changes.ignored = true,
        }
    }

    /// The disciplines the player knows, which Titanium's server sends
    /// whole after the profile and as one is learned.
    pub(super) fn disciplines_news(&mut self, list: &[u32], changes: &mut Changes) {
        match self.player.as_mut() {
            Some(player) if self.connected => player.disciplines = Some(list.to_vec()),
            _ => changes.ignored = true,
        }
    }

    /// Training at a guildmaster: the guildmaster's answer opens it, a
    /// practice raises a skill, and leaving closes it.
    pub(super) fn training_news(
        &mut self,
        update: &crate::training::TrainingUpdate,
        changes: &mut Changes,
    ) {
        use crate::training::{PracticeUpdate, TrainingUpdate};
        if !self.connected {
            changes.ignored = true;
            return;
        }
        match update {
            TrainingUpdate::Offered(offer) => {
                if self.zone.training.as_ref() != Some(offer) {
                    self.zone.training = Some(offer.clone());
                    self.training_revision = self.training_revision.wrapping_add(1);
                }
            }
            TrainingUpdate::Trained { skill, value, .. } => {
                self.skill_news(*skill, *value, changes);
                // The `EQMac` program says a language practice it resolves
                // itself (6.597); the Titanium program's language answers
                // come from the server.
                let says = match self.generation {
                    crate::GameDialect::EqMac => true,
                    crate::GameDialect::Titanium | crate::GameDialect::Rof2 => false,
                };
                let language = skill
                    .checked_sub(LANGUAGE_SKILLS)
                    .and_then(|language| u8::try_from(language).ok());
                if let (true, Some(language)) = (says, language) {
                    let trainer = self
                        .zone
                        .training
                        .as_ref()
                        .and_then(|offer| self.zone.spawns.get(&offer.trainer))
                        .map(|spawn| crate::entities::display_name(&spawn.state.name))
                        .unwrap_or_default();
                    changes.notices.push(super::Notice::LanguagePracticed {
                        trainer,
                        language,
                        first: *value == 1,
                        common_alternate: self
                            .language_zero_selector
                            .map(crate::rules::LanguageZeroSelector::alternate_name),
                    });
                }
            }
            TrainingUpdate::Practice(update) => {
                if matches!(
                    update,
                    PracticeUpdate::Requested { .. }
                        | PracticeUpdate::Unanswered { .. }
                        | PracticeUpdate::OutcomeUnknown { .. }
                        | PracticeUpdate::PointsUncertain
                ) {
                    self.zone.training_points_uncertain = true;
                }
                changes
                    .notices
                    .push(super::Notice::TrainingPractice(*update));
            }
            TrainingUpdate::Ended => {
                if self.zone.training.take().is_some() {
                    self.training_revision = self.training_revision.wrapping_add(1);
                }
            }
        }
        changes.trade = true;
    }

    /// The player's hit points, as the server reported them.
    pub(super) fn hp_news(&mut self, spawn_id: u16, report: ReportedHp, changes: &mut Changes) {
        if self.is_player(spawn_id) {
            self.vitals.reported_hp = Some(report);
            self.vitals.local_hp_damage = 0;
            self.own_hp_count_current = false;
            self.refresh_item_hp();
            self.show_hp();
            self.bleed_out_news();
        } else {
            changes.ignored = true;
        }
    }

    /// Whether the server's report says the player bled out, where the
    /// session takes the client's word for it (`Capability::BleedingOut`,
    /// and a threshold named, which the session names only where it counts
    /// what the player's items add): their HP, the report with what their
    /// items add counted afresh, is at or below the threshold, and no death
    /// was named for them. Only the server's own report decides, never a
    /// predicted tick. The session counts the items by the server type's own
    /// rule too, and refuses a report it rules out.
    fn bleed_out_news(&mut self) {
        let current = self.counted_hit_points();
        self.bled_out = current.filter(|current| {
            self.death.is_none()
                && self.can(crate::Capability::BleedingOut)
                && self
                    .death_threshold
                    .is_some_and(|threshold| *current <= i64::from(threshold))
        });
    }

    /// Notes the player's health.
    pub(super) fn own_health(&mut self, percent: u8) {
        if let Some(player) = self.player.as_mut() {
            player.hp_percent = Some(percent);
        }
    }

    /// The player's health follows the HP their last report and equipped
    /// items make.
    pub(super) fn show_hp(&mut self) {
        if let Some(percent) = self.hit_points().and_then(vitals::percent) {
            self.own_health(percent);
        }
    }

    /// Works out the HP equipped items add again, after the inventory, the
    /// player or the report changed.
    pub(super) fn refresh_item_hp(&mut self) {
        if let Some(player) = self.player.as_ref()
            && self.vitals.refresh_item_hp(player, &self.inventory)
        {
            self.show_hp();
        }
    }
}
