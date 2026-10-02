//! Distance-based entity selection, independent of the rendering engine.

use crate::{SpawnKind, SpawnState, WorldPosition};
use std::collections::BTreeSet;
use std::time::Duration;

/// Longest a spawn's reported motion is carried forward. Servers repeat a moving
/// spawn's position every few seconds (P99 about every 2.3 s, `EQEmu` at least
/// every 5 s), so a spawn with no newer report stops where it got to.
pub const MOTION_HORIZON: Duration = Duration::from_secs(6);

/// Where a spawn is `elapsed` after its last report: its reported velocity is
/// carried forward, up to [`MOTION_HORIZON`], as the official client moves
/// spawns between updates instead of leaving them at the last report.
#[must_use]
pub fn extrapolate(spawn: &SpawnState, elapsed: Duration) -> WorldPosition {
    let seconds = elapsed.min(MOTION_HORIZON).as_secs_f32();
    let [x, y, z] = spawn.velocity;
    let position = spawn.position;
    let moved = WorldPosition {
        x: position.x + x * seconds,
        y: position.y + y * seconds,
        z: position.z + z * seconds,
        heading: position.heading,
    };
    if [moved.x, moved.y, moved.z]
        .iter()
        .all(|value| value.is_finite())
    {
        moved
    } else {
        position
    }
}

/// Selects nearby visible entities, nearest first, with an exit margin and hard cap.
/// Radius is three-dimensional EQ world distance, measured from the player.
pub fn nearby<'a>(
    spawns: impl IntoIterator<Item = &'a SpawnState>,
    own_id: u16,
    position: WorldPosition,
    rendered: &BTreeSet<u16>,
    radius: f32,
    limit: usize,
) -> Vec<u16> {
    if !radius.is_finite() || radius <= 0.0 {
        return Vec::new();
    }
    let mut candidates: Vec<_> = spawns
        .into_iter()
        .filter_map(|spawn| {
            if spawn.spawn_id == own_id
                || spawn.invisible
                || matches!(spawn.kind, SpawnKind::Unknown(_))
            {
                return None;
            }
            let p = spawn.position;
            let distance = (p.x - position.x)
                .hypot(p.y - position.y)
                .hypot(p.z - position.z);
            let threshold = if rendered.contains(&spawn.spawn_id) {
                radius * 1.2
            } else {
                radius
            };
            (distance.is_finite() && distance <= threshold).then_some((spawn.spawn_id, distance))
        })
        .collect();
    candidates.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    candidates
        .into_iter()
        .take(limit)
        .map(|(id, _)| id)
        .collect()
}

/// A spawn's name as the official client shows it: the server's unique
/// numeric suffix dropped and underscores read as spaces, so `a_rat003`
/// shows as "a rat".
#[must_use]
pub fn display_name(raw: &str) -> String {
    raw.trim_end_matches(|c: char| c.is_ascii_digit())
        .replace('_', " ")
        .trim()
        .to_owned()
}

/// Whether `to` lies within `range` of `from`, in three-dimensional EQ world units.
#[must_use]
pub fn within(from: WorldPosition, to: WorldPosition, range: f32) -> bool {
    let distance = (to.x - from.x).hypot(to.y - from.y).hypot(to.z - from.z);
    distance.is_finite() && distance <= range
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn names_drop_server_suffixes() {
        assert_eq!(display_name("a_rat00"), "a rat");
        assert_eq!(display_name("Guard_Philips12"), "Guard Philips");
        assert_eq!(display_name("Marton_Sayer000"), "Marton Sayer");
    }

    #[test]
    #[allow(clippy::float_cmp)] // Exactly representable fixture values.
    fn moving_spawns_carry_their_reported_velocity_up_to_the_horizon() {
        let spawn = SpawnState {
            class: None,
            spawn_id: 9,
            name: "Synthetic walker".into(),
            kind: SpawnKind::Npc,
            race: 1,
            gender: 0,
            position: WorldPosition {
                x: 10.0,
                y: -4.0,
                z: 2.0,
                heading: 128.0,
            },
            velocity: [4.0, -2.0, 0.5],
            size: 0.0,
            invisible: false,
            appearance: crate::outfit::Appearance::default(),
            level: 0,
            listing: crate::listing::Listing::default(),
            pet_owner: None,
            hp_percent: None,
        };
        let at = extrapolate(&spawn, Duration::from_secs(2));
        assert_eq!((at.x, at.y, at.z, at.heading), (18.0, -8.0, 3.0, 128.0));
        let far = extrapolate(&spawn, Duration::from_mins(1));
        assert_eq!((far.x, far.y), (34.0, -16.0));
        let standing = SpawnState {
            velocity: [0.0; 3],
            ..spawn
        };
        assert_eq!(
            extrapolate(&standing, Duration::from_secs(3)),
            standing.position
        );
    }

    #[test]
    fn within_measures_three_dimensional_distance() {
        let at = |x, y, z| WorldPosition {
            x,
            y,
            z,
            heading: 0.0,
        };
        assert!(within(at(0.0, 0.0, 0.0), at(3.0, 4.0, 12.0), 13.0));
        assert!(!within(at(0.0, 0.0, 0.0), at(3.0, 4.0, 12.0), 12.9));
        assert!(!within(at(0.0, 0.0, 0.0), at(f32::NAN, 0.0, 0.0), 100.0));
    }
    fn spawn(id: u16, x: f32) -> SpawnState {
        SpawnState {
            class: None,
            spawn_id: id,
            name: String::new(),
            kind: SpawnKind::Npc,
            race: 1,
            gender: 0,
            position: WorldPosition {
                x,
                ..Default::default()
            },
            velocity: [0.0; 3],
            size: 0.0,
            invisible: false,
            appearance: crate::outfit::Appearance::default(),
            level: 0,
            listing: crate::listing::Listing::default(),
            pet_owner: None,
            hp_percent: None,
        }
    }
    #[test]
    fn dense_crowds_keep_only_the_nearest_two_hundred() {
        let all: BTreeMap<_, _> = (2u16..302)
            .map(|id| (id, spawn(id, f32::from(id) / 10.0)))
            .collect();
        let ids = nearby(
            all.values(),
            1,
            WorldPosition::default(),
            &BTreeSet::new(),
            200.0,
            200,
        );
        assert_eq!(ids.len(), 200);
        assert_eq!(ids.first(), Some(&2));
        assert_eq!(ids.last(), Some(&201));
    }

    #[test]
    fn culls_distance_height_invisibility_and_self_with_nearest_cap() {
        let mut all = BTreeMap::from([
            (1, spawn(1, 0.0)),
            (2, spawn(2, 25.0)),
            (3, spawn(3, 201.0)),
            (4, spawn(4, 10.0)),
            (5, spawn(5, 2.0)),
        ]);
        all.get_mut(&5).unwrap().invisible = true;
        assert_eq!(
            nearby(
                all.values(),
                1,
                WorldPosition::default(),
                &BTreeSet::new(),
                200.0,
                1
            ),
            vec![4]
        );
        all.get_mut(&4).unwrap().position.z = 300.0;
        assert_eq!(
            nearby(
                all.values(),
                1,
                WorldPosition::default(),
                &BTreeSet::new(),
                200.0,
                64
            ),
            vec![2]
        );
    }
    #[test]
    fn exit_margin_prevents_boundary_churn_but_never_admits_distant_new_entities() {
        let mut all = BTreeMap::from([(2, spawn(2, 210.0))]);
        assert_eq!(
            nearby(
                all.values(),
                1,
                WorldPosition::default(),
                &BTreeSet::new(),
                200.0,
                64
            ),
            []
        );
        assert_eq!(
            nearby(
                all.values(),
                1,
                WorldPosition::default(),
                &BTreeSet::from([2]),
                200.0,
                64
            ),
            vec![2]
        );
        all.get_mut(&2).unwrap().position.x = 241.0;
        assert_eq!(
            nearby(
                all.values(),
                1,
                WorldPosition::default(),
                &BTreeSet::from([2]),
                200.0,
                64
            ),
            []
        );
    }
}
