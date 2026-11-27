//! Asset-free body/profile and long covered doorway regressions.
use super::*;
use crate::PlayerBody;
use eq_client_assets::{falls::Headroom, ui::InstalledClient};
use eq_client_core::movement::navmesh::{MeshNavigator, NavMesh};
use eq_client_core::movement::{
    CollisionMesh, Navigator, PathProgress, Planned, Scope, SearchRequest,
};

fn body() -> PlayerBody {
    PlayerBody {
        size: 6.0,
        height: 6.677_319_5,
        feet_offset: 3.750_000_2,
    }
}
fn profile() -> Settings {
    settings(
        InstalledClient::Titanium,
        eq_client_assets::scale::ZoneScale {
            classic_reduced: false,
        },
    )
}

#[test]
fn measured_collision_uses_a_profile_covering_both_native_room_and_capsule() {
    let settings = profile();
    assert_eq!(
        profile_size(body(), &settings, Some(Headroom::TITANIUM)),
        Some(6.0)
    );
    // Drawn size, native standing need, and capsule need are independent guards.
    for large in [
        PlayerBody {
            size: 9.0,
            ..body()
        },
        PlayerBody {
            feet_offset: 3.8,
            ..body()
        },
        PlayerBody {
            height: 7.2,
            ..body()
        },
    ] {
        assert_eq!(
            profile_size(large, &settings, Some(Headroom::TITANIUM)),
            Some(9.0)
        );
    }
    assert_eq!(
        profile_size(
            PlayerBody {
                height: 12.0,
                ..body()
            },
            &settings,
            Some(Headroom::TITANIUM)
        ),
        None
    );
    assert_eq!(
        profile_size(
            PlayerBody {
                size: 3.0,
                height: 6.8,
                feet_offset: 2.0
            },
            &settings,
            Some(Headroom::TITANIUM)
        ),
        Some(6.0)
    );
}

fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
    vec![[a, b, c], [a, c, d]]
}
fn block(low: [f32; 3], high: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
    let [x, y, z] = low;
    let [xx, yy, zz] = high;
    [
        quad([x, y, z], [x, yy, z], [xx, yy, z], [xx, y, z]),
        quad([x, y, zz], [xx, y, zz], [xx, yy, zz], [x, yy, zz]),
        quad([x, y, z], [x, y, zz], [x, yy, zz], [x, yy, z]),
        quad([xx, y, z], [xx, yy, z], [xx, yy, zz], [xx, y, zz]),
        quad([x, y, z], [xx, y, z], [xx, y, zz], [x, y, zz]),
        quad([x, yy, z], [x, yy, zz], [xx, yy, zz], [xx, yy, z]),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[test]
fn a_long_roofed_doorway_keeps_mesh_routing_for_the_body_that_fits() {
    // Synthetic long covered passage, not copied installed geometry. A tall
    // profile loses 18 units of floor, beyond a local door join's reach.
    let mut triangles = quad(
        [-30., 0., -10.],
        [-30., 0., 10.],
        [30., 0., 10.],
        [30., 0., -10.],
    );
    triangles.extend(quad(
        [-9., 10., -10.],
        [9., 10., -10.],
        [9., 10., 10.],
        [-9., 10., 10.],
    ));
    for (z, zz) in [(-10., -1.5), (1.5, 10.)] {
        triangles.extend(block([-0.05, 0., z], [0.05, 12., zz]));
    }
    let settings = profile();
    let mesh = NavMesh::build(&triangles, &settings, 1, &AtomicBool::new(false))
        .unwrap()
        .0
        .split(settings.sizes);
    let chosen = profile_size(body(), &settings, Some(Headroom::TITANIUM)).unwrap();
    let mut world = CollisionWorld::new(triangles).unwrap();
    world.set_door_obstacles(vec![(
        7,
        Arc::new(CollisionMesh::new(block([-0.05, 0., -1.5], [0.05, 12., 1.5])).unwrap()),
    )]);
    for direction in [-1., 1.] {
        for allowed in [true, false] {
            let mut search =
                MeshNavigator::new(mesh.for_height(chosen).unwrap()).search(SearchRequest {
                    start: Vec3::new(-20. * direction, 0., 0.),
                    goal: Vec3::new(20. * direction, 0., 0.),
                    reach: 0.5,
                    height: body().height,
                    doors: if allowed { vec![7] } else { vec![] },
                    drops: false,
                    scope: Scope::Whole,
                    goal_height: Some(0.5),
                });
            let result = loop {
                match search.advance(&world, 500) {
                    PathProgress::Searching => (),
                    done => break done,
                }
            };
            if allowed {
                assert!(matches!(result, PathProgress::Found(_)), "{result:?}");
                assert!(
                    matches!(search.planned(), Planned::Mesh { fallback: None, .. }),
                    "{:?}",
                    search.planned()
                );
                assert!(search.doors_along().contains(&Some(7)));
            } else {
                assert_eq!(result, PathProgress::Failed);
            }
        }
    }
}

#[test]
fn a_coordinate_beyond_the_authored_floor_stays_unreachable() {
    let floor = quad(
        [-10., 0., -10.],
        [-10., 0., 10.],
        [10., 0., 10.],
        [10., 0., -10.],
    );
    let mesh = NavMesh::build(&floor, &profile(), 1, &AtomicBool::new(false))
        .unwrap()
        .0;
    let world = CollisionWorld::new(floor).unwrap();
    let mut search = MeshNavigator::new(Arc::new(mesh)).search(SearchRequest {
        start: Vec3::ZERO,
        goal: Vec3::new(20., 0., 0.),
        reach: 0.5,
        height: body().height,
        doors: vec![],
        drops: false,
        scope: Scope::Whole,
        goal_height: Some(0.5),
    });
    let result = loop {
        match search.advance(&world, 500) {
            PathProgress::Searching => (),
            done => break done,
        }
    };
    assert_eq!(result, PathProgress::Failed);
    assert!(matches!(
        search.planned(),
        Planned::Mesh {
            fallback: Some(eq_client_core::movement::Fallback::OffMesh),
            ..
        }
    ));
}
