//! Explicit installed-zone CPU diagnostic. No viewer, server or timing assertion.
use super::*;
use eq_client_core::movement::{SurfaceClock, SurfacePlan, SurfaceStroke};
use std::{hint::black_box, path::Path};

fn cost_stroke(pitch: f32, ms: u64, height: f32, heading: f32) -> SurfaceStroke {
    let h = eq_client_core::render_heading(heading);
    SurfaceStroke {
        set: eq_client_core::movement::water::LIQUID_5,
        input: eq_client_core::movement::water::WetInput {
            horizontal: [h.sin() * 1.7, h.cos() * 1.7],
            vertical: 1.7 * pitch.to_radians().sin(),
            jump: false,
        },
        seconds: Duration::from_millis(ms).as_secs_f32(),
        height,
    }
}
fn old_wet(world: &CollisionWorld, feet: Vec3, stroke: SurfaceStroke) -> Vec3 {
    let mut velocity = eq_client_core::movement::water::WetVelocity::default();
    let (mut left, mut delta) = (stroke.seconds, Vec3::ZERO);
    while left > 0. {
        let dt = left.min(eq_client_core::movement::water::WET_FRAME_SECONDS);
        let (h, y) = velocity.advance(stroke.set, stroke.input, dt);
        delta += Vec3::new(h[0], y, h[1]);
        left -= dt;
        if !world.origin_in_liquid(feet + delta, stroke.height) {
            break;
        }
    }
    world.wet_step(feet, delta, stroke.height)
}
fn attempt(
    world: &CollisionWorld,
    feet: Vec3,
    stroke: SurfaceStroke,
) -> Result<SurfacePlan, eq_client_core::movement::SurfaceDecline> {
    SurfaceClock::plan(
        None,
        world,
        feet,
        eq_client_core::movement::water::WetVelocity::default(),
        stroke,
    )
}
fn qualified_start(world: &CollisionWorld, origin: Vec3, height: f32) -> Vec3 {
    let mut fallback = None;
    for rise in [0_f32, 1., 2., 2.5] {
        for x in [0_f32, -2., 2., -4., 4.] {
            for z in [0_f32, -2., 2., -4., 4.] {
                let feet = origin + Vec3::new(x, rise - 3.75, z);
                if let Ok(p) = attempt(world, feet, cost_stroke(60., 250, height, 52.)) {
                    if p.active {
                        return feet;
                    }
                    fallback.get_or_insert(feet);
                }
            }
        }
    }
    fallback.expect("installed fixture has no qualified clear-water trial; do not time a fabricated replacement")
}

#[test]
#[ignore = "requires explicit installed assets and fixture origin; CPU-only cost diagnostic"]
fn installed_surface_owner_query_cost() {
    let directory = std::env::var("EQ_SURFACE_COST_DIR").expect("set installed assets directory");
    let zone_name = std::env::var("EQ_SURFACE_COST_ZONE").expect("set installed zone name");
    let values = std::env::var("EQ_SURFACE_COST_ORIGIN")
        .expect("set EQ x,y,z fixture origin")
        .split(',')
        .map(|v| v.parse::<f32>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 3);
    let zone = eq_client_assets::load_zone_collision(Path::new(&directory), &zone_name).unwrap();
    let base = crate::scene_collision::zone_collision(&zone)
        .unwrap()
        .with_swim_origin(3.75);
    let world = base.own_moves().unwrap_or(base);
    let character = eq_client_assets::characters::load_installed_character(
        Path::new(&directory),
        &zone_name,
        "HUM",
        &eq_client_assets::characters::DonorArchives::default(),
    )
    .unwrap();
    let height = character.height();
    let origin = Vec3::from_array(eq_client_core::render_position(
        eq_client_core::WorldPosition {
            x: values[0],
            y: values[1],
            z: values[2],
            heading: 0.,
        },
    ));
    let feet = qualified_start(&world, origin, height);
    let mut rows = Vec::new();
    for pitch in [15_f32, 60., 85.] {
        for ms in [100_u64, 150, 250] {
            let stroke = cost_stroke(pitch, ms, height, 52.);
            let proof = attempt(&world, feet, stroke);
            let Ok(proof) = proof else {
                rows.push(serde_json::json!({"pitch":pitch,"ms":ms,"declined":format!("{:?}",proof.err())}));
                continue;
            };
            let (mut old, mut new) = (Vec::new(), Vec::new());
            for i in 0..528 {
                let start = Instant::now();
                black_box(old_wet(&world, feet, stroke));
                let a = start.elapsed().as_nanos();
                let start = Instant::now();
                black_box(attempt(&world, feet, stroke).unwrap());
                let b = start.elapsed().as_nanos();
                if i >= 16 {
                    old.push(a);
                    new.push(b);
                }
            }
            old.sort_unstable();
            new.sort_unstable();
            rows.push(serde_json::json!({"pitch":pitch,"ms":ms,"samples":new.len(),"active":proof.active,
                "physical_pieces":proof.pieces,"numeric_tails":proof.tails,"owner_checks":proof.checks,
                "old_median_ns":old[256],"new_median_ns":new[256],"new_p95_ns":new[486],"new_max_ns":new[511]}));
        }
    }
    println!(
        "{}",
        serde_json::json!({"installed_zone":zone_name,"height":height,"feet_offset":3.75,
        "fixture_origin_eq":[values[0],values[1],values[2]],"qualified_feet":feet.to_array(),"rows":rows,
        "scope":"offline original terrain/object/BSP owner; fixed plain-forward input; no runtime or performance assertion"})
    );
}
