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

/// Titanium's own-HP gauge value, truncated to per mille and clamped to
/// -110..1000 (0x5598bc). This display value establishes no life state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HpGauge(i16);

impl HpGauge {
    /// The native signed per-mille value, including its -2 draw sentinel.
    #[must_use]
    pub const fn value(self) -> i16 {
        self.0
    }

    /// The drawn fraction (0x5049fe): negative HP uses the purple reserve
    /// scale below -9; -9..-1 draws empty except for the -2 sentinel.
    #[must_use]
    pub fn ratio(self) -> f64 {
        match self.0 {
            -2 => 1.0,
            -110..=-10 => f64::from(1100 + 10 * self.0) / 1000.0,
            -9..=-1 => 0.0,
            value => f64::from(value) / 1000.0,
        }
    }
}

/// The player's HP, mana, endurance and experience as last reported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vitals {
    /// Current mana.
    pub mana: Option<u32>,
    /// Current endurance, as last reported less a jump's cost the client
    /// took off since ([`crate::world::ClientWorld::spend_endurance`]).
    pub endurance: Option<u32>,
    /// Experience on the Titanium 0..330 scale.
    pub experience: Option<u32>,
    /// The last HP report.
    pub reported_hp: Option<ReportedHp>,
    /// HP the equipped items add, as last worked out.
    pub item_hp: Option<i64>,
    /// Successfully submitted Titanium environmental damage since the last
    /// own-HP report. This local counter is separate from received HP facts.
    pub local_hp_damage: u64,
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
        let (current, maximum) = self.signed_hit_points(dead)?;
        let shown = |value: i64| u32::try_from(value.max(0)).unwrap_or(u32::MAX);
        Some((shown(current), shown(maximum)))
    }

    fn signed_hit_points(&self, dead: bool) -> Option<(i64, i64)> {
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
            self.local_current(report.current) + to_current
        };
        let maximum = i64::from(report.maximum) + to_maximum;
        Some((current, maximum))
    }

    fn local_current(&self, reported: i32) -> i64 {
        if self.local_hp_damage == 0 {
            i64::from(reported)
        } else {
            i64::from(reported)
                .saturating_sub(i64::try_from(self.local_hp_damage).unwrap_or(i64::MAX))
                .clamp(-32768, 10_000_000)
        }
    }

    /// The native signed gauge, with no division when maximum is missing
    /// or invalid. Text HP retains its existing nonnegative presentation.
    pub(super) fn gauge(&self, dead: bool) -> Option<HpGauge> {
        let (current, maximum) = self.signed_hit_points(dead)?;
        if maximum <= 0 {
            return None;
        }
        let value = (current.saturating_mul(1000) / maximum).clamp(-110, 1000);
        Some(HpGauge(
            i16::try_from(value).expect("native gauge range fits i16"),
        ))
    }

    /// The player's current HP, the last report with what equipped items add
    /// counted afresh; None without a report, or while the inventory cannot
    /// tell what the items add (the shown HP keeps the last figure then).
    pub(super) fn counted_hp(&self, player: &PlayerState, inventory: &Inventory) -> Option<i64> {
        let report = self.reported_hp?;
        Some(i64::from(report.current) + item_hit_points(player, inventory)?)
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

    #[test]
    fn titanium_signed_hp_gauge_has_a_separate_purple_scale_and_sentinel() {
        use ItemHitPoints::LeftOutOfCurrent;
        let gauge = |current| {
            Vitals {
                item_hp: Some(0),
                ..reported(current, 1000, LeftOutOfCurrent)
            }
            .gauge(false)
            .unwrap()
        };
        for (current, fill) in [
            (1000, 1.0),
            (1, 0.001),
            (0, 0.0),
            (-1, 0.0),
            (-2, 1.0),
            (-9, 0.0),
            (-10, 1.0),
            (-50, 0.6),
            (-110, 0.0),
            (-200, 0.0),
        ] {
            let gauge = gauge(current);
            assert_eq!(
                gauge.value(),
                i16::try_from(current.clamp(-110, 1000)).unwrap()
            );
            assert!((gauge.ratio() - fill).abs() < f64::EPSILON);
        }
        // The signed report is adjusted for items before the native ratio.
        assert_eq!(
            reported(-30, 100, LeftOutOfCurrent).gauge(false),
            Some(HpGauge(-50))
        );
        assert_eq!(
            reported(-30, 100, LeftOutOfCurrent).gauge(true),
            Some(HpGauge(0))
        );
        assert_eq!(reported(1, 0, LeftOutOfCurrent).gauge(false), None);
    }

    #[test]
    fn local_hp_delta_bounds_apply_to_base_before_equipment() {
        let vitals = Vitals {
            item_hp: Some(-20),
            local_hp_damage: 1,
            ..reported(10_000_010, 10_000_020, ItemHitPoints::LeftOut)
        };
        assert_eq!(
            vitals.signed_hit_points(false),
            Some((9_999_980, 10_000_000))
        );
        let lower = Vitals {
            item_hp: Some(-50),
            local_hp_damage: 100,
            ..reported(-32760, 100, ItemHitPoints::LeftOut)
        };
        assert_eq!(lower.signed_hit_points(false), Some((-32818, 50)));
    }
}
