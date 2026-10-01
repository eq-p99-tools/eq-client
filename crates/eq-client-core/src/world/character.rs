//! News about the player character: level, skills, hit points and health.
use super::{Changes, ClientWorld, ReportedHp, vitals};

impl ClientWorld {
    /// The player's level and experience; what items give can depend on
    /// the level.
    pub(super) fn level_news(&mut self, current: u8, experience: u32, changes: &mut Changes) {
        match self.player.as_mut() {
            Some(player) if self.connected => {
                player.level = current;
                self.vitals.experience = Some(experience);
                self.refresh_item_hp();
            }
            _ => changes.ignored = true,
        }
    }

    /// One of the player's skills.
    pub(super) fn skill_news(&mut self, skill_id: u32, value: u32, changes: &mut Changes) {
        match self.player.as_mut() {
            Some(player) if self.connected => player.apply_skill(skill_id, value),
            _ => changes.ignored = true,
        }
    }

    /// The player's hit points, as the server reported them.
    pub(super) fn hp_news(&mut self, spawn_id: u16, report: ReportedHp, changes: &mut Changes) {
        if self.is_player(spawn_id) {
            self.vitals.reported_hp = Some(report);
            self.refresh_item_hp();
            self.show_hp();
        } else {
            changes.ignored = true;
        }
    }

    /// Notes the player's health.
    pub(super) fn own_health(&mut self, percent: u8) {
        if let Some(player) = self.player.as_mut() {
            player.hp_percent = Some(percent);
        }
    }

    /// The player's health follows the HP their last report and equipped
    /// items make.
    fn show_hp(&mut self) {
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
