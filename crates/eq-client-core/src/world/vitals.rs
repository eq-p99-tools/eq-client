//! The player's own vitals: HP, mana, endurance and experience as the server
//! last reported them, and the HP equipped items add, which the client works
//! out when a report leaves it out.
use crate::{
    ItemHitPoints, PlayerState,
    inventory::Inventory,
    resources::{eqemu_equipped_modifiers, eqemu_item_hit_points},
};

/// The player's own HP as the server last reported it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportedHp {
    /// Current HP.
    pub current: i32,
    /// Maximum HP.
    pub maximum: i32,
    /// Which values leave out what equipped items add, which the client
    /// adds back.
    pub items: ItemHitPoints,
}

/// The player's HP, mana, endurance and experience as last reported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vitals {
    /// Current mana.
    pub mana: Option<u32>,
    /// Current endurance.
    pub endurance: Option<u32>,
    /// Experience on the Titanium 0..330 scale.
    pub experience: Option<u32>,
    /// The last HP report.
    pub reported_hp: Option<ReportedHp>,
    /// HP the equipped items add, as last worked out.
    pub item_hp: Option<i64>,
}

impl Vitals {
    /// The full experience bar on the Titanium scale: 330 is a whole level.
    pub const EXPERIENCE_SCALE: u32 = 330;

    /// How far through the level the player is, from 0 to 1.
    #[must_use]
    pub fn experience_ratio(&self) -> Option<f64> {
        self.experience
            .map(|value| f64::from(value) / f64::from(Self::EXPERIENCE_SCALE))
    }

    /// The HP to show, current and maximum: the last report, adding back what
    /// equipped items give to whichever values leave it out (unknown item HP
    /// counts as none). A dead player has none left.
    pub(super) fn hit_points(&self, dead: bool) -> Option<(u32, u32)> {
        let report = self.reported_hp?;
        let items = self.item_hp.unwrap_or(0);
        let to_maximum = match report.items {
            ItemHitPoints::LeftOut => items,
            ItemHitPoints::LeftOutOfCurrent => 0,
        };
        let to_current = items;
        let current = if dead {
            0
        } else {
            i64::from(report.current) + to_current
        };
        let maximum = i64::from(report.maximum) + to_maximum;
        let shown = |value: i64| u32::try_from(value.max(0)).unwrap_or(u32::MAX);
        Some((shown(current), shown(maximum)))
    }

    /// Works out what equipped items add again once a report has come, as
    /// every report leaves it out of at least its current, keeping the last
    /// figure when the inventory cannot tell, as a gear change alone brings
    /// no new report. Returns whether it changed.
    pub(super) fn refresh_item_hp(&mut self, player: &PlayerState, inventory: &Inventory) -> bool {
        if self.reported_hp.is_none() {
            return false;
        }
        let Some(items) = item_hit_points(player, inventory) else {
            return false;
        };
        let changed = self.item_hp != Some(items);
        self.item_hp = Some(items);
        changed
    }
}

/// The share of `maximum` that `current` is, in percent, when there is a
/// maximum.
pub(super) fn percent((current, maximum): (u32, u32)) -> Option<u8> {
    (maximum > 0).then(|| {
        u8::try_from(u64::from(current.min(maximum)) * 100 / u64::from(maximum))
            .expect("percentage is bounded to 100")
    })
}

/// HP the player's equipped items add, when the inventory allows telling.
fn item_hit_points(player: &PlayerState, inventory: &Inventory) -> Option<i64> {
    let equipment = eqemu_equipped_modifiers(
        inventory,
        player.class?,
        player.race,
        u16::from(player.level),
    )
    .ok()?;
    Some(eqemu_item_hit_points(&equipment))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reported(current: i32, maximum: i32, items: ItemHitPoints) -> Vitals {
        Vitals {
            reported_hp: Some(ReportedHp {
                current,
                maximum,
                items,
            }),
            item_hp: Some(25),
            ..Vitals::default()
        }
    }

    #[test]
    fn item_hp_is_added_back_only_where_the_report_leaves_it_out() {
        use ItemHitPoints::{LeftOut, LeftOutOfCurrent};
        assert_eq!(
            reported(50, 100, LeftOut).hit_points(false),
            Some((75, 125))
        );
        // EQMac's own update counts item HP in the maximum already.
        assert_eq!(
            reported(50, 100, LeftOutOfCurrent).hit_points(false),
            Some((75, 100))
        );
        // Unknown item HP counts as none.
        let unknown = Vitals {
            item_hp: None,
            ..reported(50, 100, LeftOut)
        };
        assert_eq!(unknown.hit_points(false), Some((50, 100)));
        assert_eq!(Vitals::default().hit_points(false), None);
    }

    #[test]
    fn the_dead_have_no_hp_left_and_negative_reports_show_none() {
        use ItemHitPoints::{LeftOut, LeftOutOfCurrent};
        assert_eq!(reported(50, 100, LeftOut).hit_points(true), Some((0, 125)));
        // Below zero even with the items' 25 added back.
        assert_eq!(
            reported(-40, 100, LeftOutOfCurrent).hit_points(false),
            Some((0, 100))
        );
        assert_eq!(percent((0, 125)), Some(0));
        assert_eq!(percent((75, 125)), Some(60));
        assert_eq!(percent((130, 125)), Some(100));
        assert_eq!(percent((0, 0)), None);
    }
}
