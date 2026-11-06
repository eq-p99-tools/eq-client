//! Distance-based entity selection, independent of the rendering engine.

use crate::{
    GameDialect, SpawnKind, SpawnState, WorldPosition,
    movement::{Carried, carry},
};
use std::collections::BTreeSet;
use std::time::Duration;

/// One call of the programs' step when the client asks where a spawn is
/// rather than drawing it: one 50 ms span, the programs' unit of time.
const CALL: Duration = Duration::from_millis(50);

/// A spawn carried on `elapsed` from its last report, as the generation's
/// program carries it ([`crate::movement::carry`]): from its reported
/// velocity and forward input, one step each 50 ms span, as a client
/// drawing it all along would have. A spawn reported standing, with
/// neither, stays where it was reported.
#[must_use]
pub fn carried_since(dialect: GameDialect, spawn: &SpawnState, elapsed: Duration) -> Carried {
    let mut carried = Carried::reported(
        dialect,
        spawn.kind,
        spawn.race,
        spawn.position,
        spawn.velocity,
        spawn.forward,
    );
    // Exactly zero is what a standing report sends.
    #[allow(clippy::float_cmp)]
    let standing = spawn.velocity == [0.0; 3] && spawn.forward == 0.0;
    let mut left = if standing { Duration::ZERO } else { elapsed };
    while !left.is_zero() {
        let call = left.min(CALL);
        carry(dialect, &mut carried, call);
        left -= call;
    }
    let moved = carried.position;
    if ![moved.x, moved.y, moved.z]
        .iter()
        .all(|value| value.is_finite())
    {
        carried.position = spawn.position;
    }
    carried
}

/// Where a spawn is `elapsed` after its last report ([`carried_since`]).
#[must_use]
pub fn carried(dialect: GameDialect, spawn: &SpawnState, elapsed: Duration) -> WorldPosition {
    carried_since(dialect, spawn, elapsed).position
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
    nearby_with_invisible(spawns, own_id, position, rendered, radius, limit, false)
}

/// Selects nearby entities for local presentation, optionally including known
/// invisible spawns. This does not change their reported visibility or permit
/// targeting or other interaction with them.
pub fn nearby_with_invisible<'a>(
    spawns: impl IntoIterator<Item = &'a SpawnState>,
    own_id: u16,
    position: WorldPosition,
    rendered: &BTreeSet<u16>,
    radius: f32,
    limit: usize,
    show_invisible: bool,
) -> Vec<u16> {
    if !radius.is_finite() || radius <= 0.0 {
        return Vec::new();
    }
    let mut candidates: Vec<_> = spawns
        .into_iter()
        .filter_map(|spawn| {
            if spawn.spawn_id == own_id
                || (spawn.invisible && !show_invisible)
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
    fn moving_spawns_are_carried_on_as_the_generations_program_does() {
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
            velocity: [0.0; 3],
            forward: 0.5,
            walk_speed: 0.0,
            run_speed: 0.0,
            size: 0.0,
            invisible: false,
            appearance: crate::outfit::Appearance::default(),
            level: 0,
            listing: crate::listing::Listing::default(),
            name_parts: crate::names::NameParts::default(),
            pet_owner: None,
            hp_percent: None,
        };
        // An NPC with an input of 0.5, facing +X, settles at 11.64 a
        // second on either generation; it climbs to it within a few calls.
        for dialect in [GameDialect::Titanium, GameDialect::EqMac] {
            let at = carried(dialect, &spawn, Duration::from_secs(2));
            assert!((at.x - 10.0 - 2.0 * 11.636).abs() < 0.5, "{at:?}");
            assert!((at.y + 4.0).abs() < 0.001 && at.z == 2.0 && at.heading == 128.0);
        }
        // Titanium walks it on; EQMac holds a walker with no report for
        // over 12 s.
        let far = carried(GameDialect::Titanium, &spawn, Duration::from_secs(30));
        assert!((far.x - 10.0 - 30.0 * 11.636).abs() < 0.5, "{far:?}");
        let held = carried(GameDialect::EqMac, &spawn, Duration::from_secs(30));
        assert!((held.x - 10.0 - 12.0 * 11.636).abs() < 0.5, "{held:?}");
        // A reported velocity with no input dies away.
        let coasting = SpawnState {
            velocity: [20.0, 0.0, 0.0],
            forward: 0.0,
            ..spawn.clone()
        };
        let stop = carried(GameDialect::Titanium, &coasting, Duration::from_secs(5));
        assert!(stop.x > 10.0 && stop.x < 11.0, "{stop:?}");
        let standing = SpawnState {
            forward: 0.0,
            ..spawn
        };
        assert_eq!(
            carried(GameDialect::Titanium, &standing, Duration::from_secs(3)),
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
            forward: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            size: 0.0,
            invisible: false,
            appearance: crate::outfit::Appearance::default(),
            level: 0,
            listing: crate::listing::Listing::default(),
            name_parts: crate::names::NameParts::default(),
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
    fn showing_invisible_is_local_and_keeps_range_self_and_kind_filters() {
        let mut hidden = spawn(2, 5.0);
        hidden.invisible = true;
        let mut distant = spawn(3, 201.0);
        distant.invisible = true;
        let mut unknown = spawn(4, 2.0);
        unknown.kind = SpawnKind::Unknown(99);
        let all = [spawn(1, 0.0), hidden, distant, unknown];
        let select = |show| {
            nearby_with_invisible(
                &all,
                1,
                WorldPosition::default(),
                &BTreeSet::new(),
                200.0,
                64,
                show,
            )
        };
        assert_eq!(select(false), []);
        assert_eq!(select(true), [2]);
        assert!(all[1].invisible);
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
