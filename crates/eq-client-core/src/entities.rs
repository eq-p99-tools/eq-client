//! Distance-based entity selection, independent of the rendering engine.

use crate::{SpawnKind, SpawnState, WorldPosition};
use std::collections::{BTreeMap, BTreeSet};

/// Selects nearby visible entities, nearest first, with an exit margin and hard cap.
/// Radius is three-dimensional EQ world distance, measured from the player.
pub fn nearby(
    spawns: &BTreeMap<u16, SpawnState>,
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
        .values()
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

#[cfg(test)]
mod tests {
    use super::*;
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
            size: 0.0,
            invisible: false,
        }
    }
    #[test]
    fn dense_crowds_keep_only_the_nearest_two_hundred() {
        let all = (2u16..302)
            .map(|id| (id, spawn(id, f32::from(id) / 10.0)))
            .collect();
        let ids = nearby(
            &all,
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
                &all,
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
                &all,
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
        assert!(
            nearby(
                &all,
                1,
                WorldPosition::default(),
                &BTreeSet::new(),
                200.0,
                64
            )
            .is_empty()
        );
        assert_eq!(
            nearby(
                &all,
                1,
                WorldPosition::default(),
                &BTreeSet::from([2]),
                200.0,
                64
            ),
            vec![2]
        );
        all.get_mut(&2).unwrap().position.x = 241.0;
        assert!(
            nearby(
                &all,
                1,
                WorldPosition::default(),
                &BTreeSet::from([2]),
                200.0,
                64
            )
            .is_empty()
        );
    }
}
