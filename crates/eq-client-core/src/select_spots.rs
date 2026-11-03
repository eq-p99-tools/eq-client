//! Where a character stands at character select: a spot picked ahead of time
//! for each zone (Adam's choice, 10-19), kept in `data/select-spots.txt`,
//! and the rule that proposes one from a zone's shape. The spots are this
//! client's own data, never a server's or a database's; the file's header
//! says the frame its numbers are in.
use crate::WorldPosition;
use crate::movement::CollisionWorld;
use glam::Vec3;
use std::collections::HashMap;
use std::sync::LazyLock;

/// The spots file, as built into the client.
const BUILT_IN: &str = include_str!("../data/select-spots.txt");

/// The camera's distance from the character where a spot names none it can
/// use, in world units.
pub const DISTANCE: f32 = 25.0;

/// The nearest distance the selection preview permits its camera.
pub const MIN_DISTANCE: f32 = 20.0;

/// How far the camera looks down at the character where a spot names
/// nothing else, in degrees.
pub const PITCH: f32 = 10.0;

/// One zone's spot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    /// Where the character stands, as `/loc` prints it: the server's y, x
    /// and z. The character stands on the first floor at or below z + 5.
    pub loc: [f32; 3],
    /// The way the character faces, in the server's 0-512 units.
    pub heading: f32,
    /// The camera's distance from the character, in world units.
    pub distance: f32,
    /// How far the camera looks down at the character, in degrees.
    pub pitch: f32,
    /// Whether a render of the spot has been looked at; false while it is
    /// the spot tool's untouched proposal.
    pub reviewed: bool,
}

impl Spot {
    /// The spot as a server position: the character's feet at the spot's
    /// height, facing its heading.
    #[must_use]
    pub const fn position(&self) -> WorldPosition {
        let [y, x, z] = self.loc;
        WorldPosition {
            x,
            y,
            z,
            heading: self.heading,
        }
    }

    /// The spot as a line of the spots file.
    #[must_use]
    pub fn line(&self, zone: &str) -> String {
        let [y, x, z] = self.loc;
        format!(
            "{zone:<12} {y:>9.2} {x:>9.2} {z:>9.2} {:>8.1} {:>9.1} {:>6.1}  {}",
            self.heading,
            self.distance,
            self.pitch,
            if self.reviewed {
                "reviewed"
            } else {
                "unreviewed"
            }
        )
    }
}

/// The spots, by zone short name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spots(HashMap<String, Spot>);

impl Spots {
    /// The spots built into the client.
    ///
    /// # Panics
    /// Never in a build whose tests pass: they read the built-in file.
    #[must_use]
    pub fn built_in() -> &'static Self {
        static SPOTS: LazyLock<Spots> =
            LazyLock::new(|| Spots::parse(BUILT_IN).expect("the built-in spots file reads"));
        &SPOTS
    }

    /// Reads a spots file: one zone a line, `#` starting a comment.
    ///
    /// # Errors
    /// Names the first line that isn't a short name, six numbers and a
    /// state, or that lists a zone a second time.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut spots = HashMap::new();
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            let words: Vec<&str> = line
                .split('#')
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .collect();
            let [zone, numbers @ .., state] = &words[..] else {
                continue;
            };
            let numbers: Vec<f32> = numbers
                .iter()
                .map(|word| word.parse::<f32>().ok().filter(|value| value.is_finite()))
                .collect::<Option<_>>()
                .ok_or_else(|| format!("line {number}: a value isn't a number"))?;
            let [y, x, z, heading, distance, pitch] = numbers[..] else {
                return Err(format!("line {number}: six numbers are needed"));
            };
            let reviewed = match *state {
                "reviewed" => true,
                "unreviewed" => false,
                _ => {
                    return Err(format!(
                        "line {number}: the state is reviewed or unreviewed"
                    ));
                }
            };
            let spot = Spot {
                loc: [y, x, z],
                heading,
                distance,
                pitch,
                reviewed,
            };
            if spots.insert(zone.to_ascii_lowercase(), spot).is_some() {
                return Err(format!("line {number}: {zone} is listed twice"));
            }
        }
        Ok(Self(spots))
    }

    /// The spot for a zone, by its short name.
    #[must_use]
    pub fn get(&self, zone: &str) -> Option<&Spot> {
        self.0.get(&zone.to_ascii_lowercase())
    }

    /// Every zone listed, with its spot.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Spot)> {
        self.0.iter().map(|(zone, spot)| (zone.as_str(), spot))
    }
}

/// How far below a spot's height its floor may be found, so a spot picked
/// by hand at a character's `/loc` stands on the floor under it.
pub const ABOVE_FLOOR: f32 = 5.0;

/// Directions the proposal looks along, around the compass.
const DIRECTIONS: usize = 16;

/// How far the proposal looks, in world units.
const VIEW: f32 = 300.0;

/// The headroom a floor needs, in world units: the tallest classic models
/// stand about this high.
const HEADROOM: f32 = 12.0;

/// The eye height the proposal looks from, above the floor.
const EYE: f32 = 5.0;

/// A floor the proposal weighs, in the renderer's frame.
#[derive(Clone, Copy, Debug)]
struct Floor {
    feet: Vec3,
    /// Whether something stands over it within the view.
    covered: bool,
}

/// Proposes a spot in a zone from its shape alone: the most open flat floor
/// near the middle of the zone's floors, with room for the camera in front
/// of the character and the longest view behind them; between views equally
/// long, the character faces away from the middle, so the camera in front
/// looks back across the zone. In a zone where at least
/// half the floors are covered (a dungeon) only covered floors count, so the outside
/// of its shell is never chosen; elsewhere only open ones. `bounds` is the
/// zone's in the renderer's frame. `surfaces` contains the drawn terrain
/// used to stand the preview, independently of `world`'s solid boundaries
/// and objects. A collision-only shell must never nominate a floor that
/// the preview then drops through onto a different, possibly wet, surface.
/// `floor_surfaces` contains only upward-authored drawn faces: a downward
/// ceiling cannot nominate a spot even though the collision ray hits it.
/// `wet` says whether a point is in water or lava. The standing floor,
/// camera orbit and some visible background must be dry. None when no
/// floor qualifies. A proposal remains unreviewed until visually inspected.
#[must_use]
pub fn propose(
    world: &CollisionWorld,
    surfaces: &CollisionWorld,
    floor_surfaces: &CollisionWorld,
    (min, max): (Vec3, Vec3),
    wet: impl Fn(Vec3) -> bool,
) -> Option<Spot> {
    let floors = floors(world, surfaces, floor_surfaces, (min, max), &wet);
    let covered = floors.iter().filter(|floor| floor.covered).count();
    // A dungeon's shell gives each column one open floor on top of at least
    // one covered floor inside, so covered floors at least match open ones.
    let indoor = covered * 2 >= floors.len();
    let floors: Vec<Floor> = floors
        .into_iter()
        .filter(|floor| floor.covered == indoor)
        .collect();
    if floors.is_empty() {
        return None;
    }
    #[allow(clippy::cast_precision_loss)] // A grid's count of floors is small.
    let middle = floors.iter().map(|floor| floor.feet).sum::<Vec3>() / floors.len() as f32;
    let reach = (max - min).length().max(1.0);
    floors
        .iter()
        .filter(|floor| flat(world, surfaces, floor_surfaces, floor.feet, &wet))
        .filter_map(|floor| {
            let views = views(world, floor.feet);
            #[allow(clippy::cast_precision_loss)] // Sixteen directions.
            let open = views.iter().sum::<f32>() / DIRECTIONS as f32;
            let outward = (floor.feet - middle).with_y(0.0).normalize_or_zero();
            let (front, room) = facing(&views, outward, |front, distance| {
                dry_orbit(surfaces, floor.feet, distance, &wet)
                    && dry_background(surfaces, floor.feet, front, &views, &wet)
            })?;
            let away = floor.feet.distance(middle) / reach;
            Some((open * (1.0 - away), floor.feet, front, room))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, feet, front, room)| {
            let direction = direction(front);
            let world_feet = eq_client_axes::to_world(feet);
            Spot {
                loc: [world_feet.y, world_feet.x, world_feet.z],
                heading: eq_client_axes::world_heading(direction.x.atan2(direction.z)),
                distance: camera_distance(room),
                pitch: PITCH,
                reviewed: false,
            }
        })
}

/// The floors under a grid over the zone: every upward surface in each
/// column with headroom over it, out of the water.
fn floors(
    world: &CollisionWorld,
    surfaces: &CollisionWorld,
    floor_surfaces: &CollisionWorld,
    (min, max): (Vec3, Vec3),
    wet: &impl Fn(Vec3) -> bool,
) -> Vec<Floor> {
    const CELLS: f32 = 48.0;
    const LAYERS: usize = 8;
    let step = ((max.x - min.x).max(max.z - min.z) / CELLS).max(8.0);
    let mut floors = Vec::new();
    let mut x = min.x + step * 0.5;
    while x < max.x {
        let mut z = min.z + step * 0.5;
        while z < max.z {
            let mut top = max.y + 1.0;
            for _ in 0..LAYERS {
                let origin = Vec3::new(x, top, z);
                let Some(hit) = floor_surfaces.ray_distance(origin, Vec3::NEG_Y, top - min.y + 1.0)
                else {
                    break;
                };
                let point = origin - Vec3::Y * hit;
                top = point.y - 0.05;
                // Resolve exactly the first drawn surface the preview picks
                // below authored z+5, including a nearby upper layer.
                let Some(floor) = surfaces.ground(point, ABOVE_FLOOR, ABOVE_FLOOR) else {
                    continue;
                };
                if (floor - point.y).abs() > 0.1 {
                    continue;
                }
                let feet = Vec3::new(x, floor, z);
                let lifted = feet + Vec3::Y * 0.5;
                let over = world.ray_distance(lifted, Vec3::Y, VIEW);
                if over.is_some_and(|over| over < HEADROOM) || !dry_floor(feet, wet) {
                    continue;
                }
                floors.push(Floor {
                    feet,
                    // Hidden collision ceilings constrain headroom, but do
                    // not make the outside of a dungeon's drawn roof inside.
                    covered: surfaces.ray_distance(lifted, Vec3::Y, VIEW).is_some(),
                });
            }
            z += step;
        }
        x += step;
    }
    floors
}

/// Whether the floor around the feet is level enough to stand a character
/// and nothing stands in its body.
fn flat(
    world: &CollisionWorld,
    surfaces: &CollisionWorld,
    floor_surfaces: &CollisionWorld,
    feet: Vec3,
    wet: &impl Fn(Vec3) -> bool,
) -> bool {
    const AROUND: f32 = 4.0;
    const STEP: f32 = 1.5;
    (0..8).all(|index| {
        #[allow(clippy::cast_precision_loss)] // Eight directions.
        let angle = index as f32 * std::f32::consts::TAU / 8.0;
        let point = feet + Vec3::new(angle.sin(), 0.0, angle.cos()) * AROUND;
        surfaces.ground(point, STEP, STEP).is_some_and(|height| {
            dry_floor(point.with_y(height), wet)
                && floor_surfaces
                    .ground(point.with_y(height), 0.1, 0.1)
                    .is_some()
                && world.ground(point.with_y(height), 0.1, 0.1).is_some()
        })
    }) && world
        .clearance(feet, HEADROOM, 1.0)
        .is_none_or(|gap| gap > 0.5)
}

/// How far the view runs clear from eye height in each direction.
fn views(world: &CollisionWorld, feet: Vec3) -> [f32; DIRECTIONS] {
    let eye = feet + Vec3::Y * EYE;
    std::array::from_fn(|index| {
        world
            .ray_distance(eye, direction(index), VIEW)
            .unwrap_or(VIEW)
    })
}

/// The way the character faces and the room in front for the camera: the
/// direction whose opposite view is longest, among those with room for the
/// camera in front, and between views equally long (in the open every view
/// reaches its limit) the one nearest `outward`, away from the zone's middle.
fn facing(
    views: &[f32; DIRECTIONS],
    outward: Vec3,
    suitable: impl Fn(usize, f32) -> bool,
) -> Option<(usize, f32)> {
    let behind = |front: usize| views[(front + DIRECTIONS / 2) % DIRECTIONS];
    (0..DIRECTIONS)
        .filter(|front| views[*front] >= MIN_DISTANCE + 2.0)
        .filter(|front| suitable(*front, camera_distance(views[*front])))
        .max_by(|a, b| {
            behind(*a).total_cmp(&behind(*b)).then_with(|| {
                direction(*a)
                    .dot(outward)
                    .total_cmp(&direction(*b).dot(outward))
            })
        })
        .map(|front| (front, views[front]))
}

fn camera_distance(room: f32) -> f32 {
    DISTANCE.min(room - 2.0)
}

/// A liquid's top can be a dry BSP boundary, or lie just below the floor
/// point. Testing only feet+1 misses that surface. A small margin on both
/// sides also avoids relying on an exact split-plane classification.
fn dry_floor(feet: Vec3, wet: &impl Fn(Vec3) -> bool) -> bool {
    [-1.0, -0.1, 0.0, 0.1, 1.0]
        .into_iter()
        .all(|rise| !wet(feet + Vec3::Y * rise))
}

/// The camera starts in front and then orbits: every sampled camera point
/// and the first visible floor below it must be dry. Looking down from dry
/// air above a harbour is not a dry camera placement.
fn dry_orbit(
    surfaces: &CollisionWorld,
    feet: Vec3,
    distance: f32,
    wet: &impl Fn(Vec3) -> bool,
) -> bool {
    let pitch = PITCH.to_radians();
    (0..DIRECTIONS).all(|index| {
        let camera = feet
            + direction(index) * (distance * pitch.cos())
            + Vec3::Y * (EYE + distance * pitch.sin());
        !wet(camera)
            && surfaces
                .ground(camera, 0.0, VIEW)
                .is_some_and(|floor| dry_floor(camera.with_y(floor), wet))
    })
}

/// At least one of three background rays must have visible dry ground
/// beyond the character. An unobstructed horizontal ray over nothing but
/// water is not a useful long view for character selection.
fn dry_background(
    surfaces: &CollisionWorld,
    feet: Vec3,
    front: usize,
    views: &[f32; DIRECTIONS],
    wet: &impl Fn(Vec3) -> bool,
) -> bool {
    let behind = (front + DIRECTIONS / 2) % DIRECTIONS;
    [
        (behind + DIRECTIONS - 1) % DIRECTIONS,
        behind,
        (behind + 1) % DIRECTIONS,
    ]
    .into_iter()
    .any(|index| {
        [0.25, 0.5, 0.75].into_iter().any(|share| {
            let sample = feet + direction(index) * (views[index] * share);
            surfaces
                .ground(sample, EYE, VIEW)
                .is_some_and(|floor| dry_floor(sample.with_y(floor), wet))
        })
    })
}

/// One of the compass directions, in the renderer's frame.
fn direction(index: usize) -> Vec3 {
    #[allow(clippy::cast_precision_loss)] // Sixteen directions.
    let angle = index as f32 * std::f32::consts::TAU / DIRECTIONS as f32;
    Vec3::new(angle.sin(), 0.0, angle.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_file_reads_and_names_only_known_zones() {
        let spots = Spots::built_in();
        for (zone, spot) in spots.iter() {
            assert!(crate::zones::number(zone).is_some(), "{zone}");
            assert!(spot.distance > 0.0, "{zone}");
        }
        assert!(
            BUILT_IN
                .lines()
                .any(|line| line.starts_with("# frame: y x z as /loc prints them")),
            "the header states the frame"
        );
    }

    #[test]
    fn lines_read_back_as_written_and_a_zone_listed_twice_is_refused() {
        let spot = Spot {
            loc: [-12.5, 340.25, 3.75],
            heading: 128.0,
            distance: 22.0,
            pitch: 12.0,
            reviewed: false,
        };
        let text = format!("# a comment\n\n{}  # and another\n", spot.line("ecommons"));
        let spots = Spots::parse(&text).unwrap();
        assert_eq!(spots.get("ECommons"), Some(&spot));
        assert_eq!(
            spot.position(),
            WorldPosition {
                x: 340.25,
                y: -12.5,
                z: 3.75,
                heading: 128.0
            }
        );
        let twice = format!("{0}\n{0}\n", spot.line("ecommons"));
        assert!(Spots::parse(&twice).unwrap_err().contains("twice"));
        assert!(Spots::parse("ecommons 1 2 3 4 5 6 maybe").is_err());
        assert!(Spots::parse("ecommons 1 2 three 4 5 6 reviewed").is_err());
        assert!(Spots::parse("ecommons 1 2 3 4 5 reviewed").is_err());
    }

    /// A square of floor `half` across each way at height zero, as two
    /// triangles each side up.
    fn floor(half: f32, height: f32) -> Vec<[[f32; 3]; 3]> {
        let (a, b, c, d) = (
            [-half, height, -half],
            [half, height, -half],
            [half, height, half],
            [-half, height, half],
        );
        vec![[a, b, c], [a, c, d]]
    }

    /// An upright wall from `from` to `to` across the floor, `high` tall.
    fn wall(from: [f32; 2], to: [f32; 2], high: f32) -> Vec<[[f32; 3]; 3]> {
        let (a, b, c, d) = (
            [from[0], 0.0, from[1]],
            [to[0], 0.0, to[1]],
            [to[0], high, to[1]],
            [from[0], high, from[1]],
        );
        vec![[a, b, c], [a, c, d]]
    }

    #[test]
    fn a_strip_walled_but_to_the_south_faces_the_character_toward_the_wall() {
        // The character faces the camera's room and the camera sees the
        // long view behind them: on a strip of floor walled to the north
        // (+Z), east and west and open to the south, the long view lies
        // south, so the character faces north, toward the wall, with room
        // left for the camera.
        let (a, b, c, d) = (
            [-80.0, 0.0, -200.0],
            [80.0, 0.0, -200.0],
            [80.0, 0.0, 60.0],
            [-80.0, 0.0, 60.0],
        );
        let mut triangles = vec![[a, b, c], [a, c, d]];
        triangles.extend(wall([-80.0, 60.0], [80.0, 60.0], 40.0));
        triangles.extend(wall([-80.0, -200.0], [-80.0, 60.0], 40.0));
        triangles.extend(wall([80.0, -200.0], [80.0, 60.0], 40.0));
        let world = CollisionWorld::new(triangles).unwrap();
        let spot = propose(
            &world,
            &world,
            &world,
            (Vec3::new(-80.0, 0.0, -200.0), Vec3::new(80.0, 40.0, 60.0)),
            |_| false,
        )
        .unwrap();
        let feet = eq_client_axes::from_world(Vec3::new(spot.loc[1], spot.loc[0], spot.loc[2]));
        assert!(feet.y.abs() < 0.01, "on the floor: {feet}");
        assert!(feet.z < 60.0, "on this side of the wall: {feet}");
        let yaw = eq_client_axes::render_heading(spot.heading);
        let toward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        assert!(toward.z > 0.5, "faces the wall: {toward}");
        assert!(spot.distance > 0.0 && spot.distance <= DISTANCE);
        assert!(!spot.reviewed);
    }

    #[test]
    fn in_the_open_the_character_faces_away_from_the_zones_middle() {
        // Every view reaches its limit, so only the way out from the middle
        // tells the directions apart: a quarter turn from the first.
        assert_eq!(
            facing(&[VIEW; DIRECTIONS], Vec3::X, |_, _| true),
            Some((4, VIEW))
        );
        assert_eq!(
            facing(&[VIEW; DIRECTIONS], Vec3::NEG_Z, |_, _| true),
            Some((8, VIEW))
        );
        // A longer view behind still comes first.
        let mut views = [100.0; DIRECTIONS];
        views[12] = VIEW;
        assert_eq!(facing(&views, Vec3::X, |_, _| true), Some((4, 100.0)));
        views[12] = 100.0;
        views[0] = VIEW;
        assert_eq!(
            facing(&views, Vec3::Z, |_, _| true).map(|(front, _)| front),
            Some(8)
        );
    }

    #[test]
    fn a_closed_room_puts_the_character_inside_not_on_its_roof() {
        let mut triangles = floor(50.0, 0.0);
        triangles.extend(floor(50.0, 30.0));
        for (from, to) in [
            ([-50.0, -50.0], [50.0, -50.0]),
            ([50.0, -50.0], [50.0, 50.0]),
            ([50.0, 50.0], [-50.0, 50.0]),
            ([-50.0, 50.0], [-50.0, -50.0]),
        ] {
            triangles.extend(wall(from, to, 30.0));
        }
        let world = CollisionWorld::new(triangles).unwrap();
        let spot = propose(
            &world,
            &world,
            &world,
            (Vec3::new(-50.0, 0.0, -50.0), Vec3::new(50.0, 30.0, 50.0)),
            |_| false,
        )
        .unwrap();
        assert!(spot.loc[2].abs() < 0.01, "on the room's floor: {spot:?}");
    }

    #[test]
    fn water_and_a_zone_with_no_floor_propose_nothing() {
        let world = CollisionWorld::new(floor(100.0, 0.0)).unwrap();
        let bounds = (
            Vec3::new(-100.0, 0.0, -100.0),
            Vec3::new(100.0, 10.0, 100.0),
        );
        assert!(propose(&world, &world, &world, bounds, |_| true).is_none());
        assert!(propose(&world, &world, &world, bounds, |_| false).is_some());
    }

    #[test]
    fn a_hidden_collision_shell_cannot_nominate_the_water_below_it() {
        // The proposer used to stand on a hidden collision shell, query dry
        // air there, and write its height. The preview has no drawn shell
        // and stands instead on the passable water surface below it.
        let mut solids = floor(100.0, 60.0);
        solids.extend(floor(100.0, -30.0));
        let world = CollisionWorld::new(solids).unwrap();
        let mut drawn = floor(100.0, 0.0);
        drawn.extend(floor(100.0, -30.0));
        let surfaces = CollisionWorld::new(drawn).unwrap();
        let bounds = (
            Vec3::new(-100.0, -30.0, -100.0),
            Vec3::new(100.0, 60.0, 100.0),
        );
        let wet = |point: Vec3| point.y <= 0.0 && point.y > -30.0;
        assert!(
            propose(&world, &world, &world, bounds, wet).is_some(),
            "the old geometry would choose the shell"
        );
        assert!(propose(&world, &surfaces, &surfaces, bounds, wet).is_none());
    }

    #[test]
    fn a_hidden_outer_ceiling_does_not_make_a_dungeons_roof_an_interior() {
        let mut drawn = floor(50.0, 0.0);
        drawn.extend(floor(50.0, 30.0));
        for (from, to) in [
            ([-50.0, -50.0], [50.0, -50.0]),
            ([50.0, -50.0], [50.0, 50.0]),
            ([50.0, 50.0], [-50.0, 50.0]),
            ([-50.0, 50.0], [-50.0, -50.0]),
        ] {
            drawn.extend(wall(from, to, 30.0));
        }
        let surfaces = CollisionWorld::new(drawn.clone()).unwrap();
        drawn.extend(floor(50.0, 60.0));
        let solids = CollisionWorld::new(drawn).unwrap();
        let spot = propose(
            &solids,
            &surfaces,
            &surfaces,
            (Vec3::new(-50.0, 0.0, -50.0), Vec3::new(50.0, 60.0, 50.0)),
            |_| false,
        )
        .unwrap();
        assert!(
            spot.loc[2].abs() < 0.01,
            "inside the room, not on its roof: {spot:?}"
        );
    }

    #[test]
    fn dry_air_above_the_liquid_surface_does_not_make_a_dry_floor() {
        let world = CollisionWorld::new(floor(100.0, 0.0)).unwrap();
        let bounds = (
            Vec3::new(-100.0, 0.0, -100.0),
            Vec3::new(100.0, 10.0, 100.0),
        );
        // Model an exact BSP plane that reports neither kind, with liquid
        // immediately under it. The old feet+1 check sees only dry air.
        assert!(propose(&world, &world, &world, bounds, |point| point.y < 0.0).is_none());
    }

    #[test]
    fn the_first_drawn_floor_below_z_plus_five_is_the_one_checked() {
        let mut triangles = floor(100.0, 0.0);
        triangles.extend(floor(100.0, 3.0));
        let world = CollisionWorld::new(triangles).unwrap();
        let bounds = (
            Vec3::new(-100.0, 0.0, -100.0),
            Vec3::new(100.0, 15.0, 100.0),
        );
        assert!(propose(&world, &world, &world, bounds, |point| point.y > 1.5).is_none());
    }

    #[test]
    fn a_dry_standing_patch_needs_a_dry_camera_orbit() {
        let world = CollisionWorld::new(floor(100.0, 0.0)).unwrap();
        let bounds = (
            Vec3::new(-100.0, 0.0, -100.0),
            Vec3::new(100.0, 10.0, 100.0),
        );
        let wet = |point: Vec3| point.with_y(0.0).length() > 12.0 && point.y <= 0.0;
        assert!(propose(&world, &world, &world, bounds, wet).is_none());
    }

    #[test]
    fn an_open_view_over_only_liquid_is_rejected() {
        let world = CollisionWorld::new(floor(200.0, 0.0)).unwrap();
        let wet = |point: Vec3| point.with_y(0.0).length() > 40.0 && point.y <= 0.0;
        assert!(dry_orbit(&world, Vec3::ZERO, DISTANCE, &wet));
        assert!(!dry_background(
            &world,
            Vec3::ZERO,
            0,
            &[VIEW; DIRECTIONS],
            &wet
        ));
        let bounds = (
            Vec3::new(-200.0, 0.0, -200.0),
            Vec3::new(200.0, 10.0, 200.0),
        );
        assert!(propose(&world, &world, &world, bounds, wet).is_none());
    }

    #[test]
    fn the_proposal_never_relies_on_a_distance_the_preview_clamps_outward() {
        assert!(facing(&[MIN_DISTANCE + 1.0; DIRECTIONS], Vec3::X, |_, _| true).is_none());
        let (_, room) = facing(&[MIN_DISTANCE + 2.0; DIRECTIONS], Vec3::X, |_, _| true).unwrap();
        assert!((camera_distance(room) - MIN_DISTANCE).abs() < f32::EPSILON);
    }
}
