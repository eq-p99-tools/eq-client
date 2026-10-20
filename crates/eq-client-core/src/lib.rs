#![doc = "Engine-independent state shared by EQ client front ends and network adapters."]

pub mod buffs;
pub mod chat;
pub mod classes;
pub mod daylight;
pub mod doors;
pub mod entities;
pub mod ground;
pub mod hotbar;
pub mod logs;
pub mod movement;
pub mod names;
pub mod options;
pub mod outfit;
pub mod pet;
pub mod qol;
pub mod races;
pub mod reading;
pub mod resources;
pub mod select_spots;
pub mod skills;
pub mod targeting;
pub mod whereabouts;
pub mod who;
pub mod world;
pub use eq_network_game::abilities;
pub use eq_network_game::bind_wound;
pub use eq_network_game::books;
pub use eq_network_game::characters::{CharacterChoice, CharacterLook};
pub use eq_network_game::chat::OutboundChat;
pub use eq_network_game::clock;
pub use eq_network_game::combat;
pub use eq_network_game::command::GameCommand as ClientCommand;
pub use eq_network_game::command::Posture;
pub use eq_network_game::corpses;
pub use eq_network_game::creation;
pub use eq_network_game::exchange;
pub use eq_network_game::food;
pub use eq_network_game::group;
pub use eq_network_game::hazards;
pub use eq_network_game::inventory;
pub use eq_network_game::listing;
pub use eq_network_game::loot;
pub use eq_network_game::merchant;
pub use eq_network_game::money;
pub use eq_network_game::movement::{
    BackwardCalibration, MotionCalibration, MovementMode, MovementRequest, StrafeCalibration,
    WalkCalibration,
};
pub use eq_network_game::pets;
pub use eq_network_game::raid;
pub use eq_network_game::resurrection;
pub use eq_network_game::servers;
pub use eq_network_game::socials;
pub use eq_network_game::spells::BookActionStatus;
pub use eq_network_game::spells::SpellBook;
pub use eq_network_game::spells::SpellUpdate;
pub use eq_network_game::tradeskills;
pub use eq_network_game::training;
pub use eq_network_game::zones;
pub use eq_network_game::zoning::ZoneLineDestination;
pub use eq_network_game::zoning::ZoneOffer;
pub use eq_network_game::zoning::ZoneRejection;

pub use eq_network_game::buffs::{Buff, BuffUpdate, SpellEffect};
pub use eq_network_game::world::{
    BaseAttributes, CampStatus, Capability, Coins, ItemHitPoints, PlayerState,
    Position as WorldPosition, PostureState, SpawnKind, SpawnState, WorldEvent,
};

/// Messages crossing the worker/presentation boundary. Queues are bounded by the host.
#[derive(Clone, Debug)]
pub enum WorldUpdate {
    /// A typed game-state change.
    Game(WorldEvent),
    /// Where the connection stands; anything but connected disables local
    /// gameplay input immediately.
    Connection(world::Link),
    /// A decoded line for the on-screen communication log.
    Chat(chat::ChatLine),
    /// A server string-table message; presentation resolves the ID locally.
    ServerMessage {
        /// Index into the user's installed `eqstr_us.txt`.
        string_id: u32,
        /// Ordered `%1`, `%2`, ... substitutions supplied by the server.
        arguments: Vec<chat::RichText>,
        /// The message type the server gave it, by which the official client
        /// colours its line; None where a generation's layout is not checked.
        message_type: Option<u32>,
    },
}

/// Converts an EQ position to the renderer's frame. `eq_client_axes` owns the
/// conversion, the reflection between EQ's frame and the renderer's included;
/// this and [`world_position`] adapt it to protocol positions.
pub fn render_position(position: WorldPosition) -> [f32; 3] {
    eq_client_axes::from_world(glam::Vec3::new(position.x, position.y, position.z)).to_array()
}

/// Converts an EQ direction or offset, such as a velocity, to the
/// renderer's frame, by the same axes as [`render_position`].
#[must_use]
pub fn render_vector(vector: [f32; 3]) -> [f32; 3] {
    eq_client_axes::from_world(glam::Vec3::from_array(vector)).to_array()
}

/// Heading conversions, derived from the same axes as [`render_position`].
pub use eq_client_axes::{FULL_TURN, model_yaw, render_heading, static_yaw, world_heading};

/// `EQEmu`'s per-model exceptions to its 3.125 z offset factor, in its order, with
/// the IDs of the `Race::` constants it names in the comments (common/races.h).
const Z_FACTORS: [(u32, f32); 46] = [
    (436, 0.577), // Basilisk
    (430, 0.5),   // Drake2
    (432, 1.9),   // Drake3
    (435, 0.93),  // Dragon
    (450, 0.938), // LavaSpider
    (479, 0.8),   // Alligator2
    (451, 0.816), // LavaSpiderQueen
    (437, 0.527), // Dragon2
    (439, 1.536), // Puma2
    (415, 1.0),   // Rat
    (438, 0.776), // Dragon3
    (452, 0.776), // Dragon4
    (441, 0.816), // SpiderQueen
    (440, 0.938), // Spider
    (46, 1.0),    // Imp
    (468, 1.0),   // Snake
    (459, 1.0),   // Corathus
    (462, 1.5),   // DrachnidCocoon
    (530, 1.2),   // Dragon5
    (549, 0.5),   // Goo4
    (548, 0.5),   // Goo3
    (547, 0.5),   // Goo2
    (604, 1.2),   // Dracolich
    (653, 5.9),   // Telmira
    (658, 4.0),   // MorellThule
    (323, 5.0),   // AnimatedArmor
    (663, 5.0),   // Amygdalan
    (147, 4.0),   // IksarSpirit
    (664, 4.0),   // Sandman
    (49, 9.0),    // LavaDragon
    (703, 9.0),   // AlaranSentryStone
    (668, 5.0),   // Rabbit
    (158, 7.0),   // Wurm
    (669, 7.0),   // BlindDreamer
    (187, 0.5),   // Siren
    (90, 0.5),    // HalasCitizen
    (190, 0.5),   // Othmir
    (183, 0.6),   // Coldain
    (14, 1.2),    // Werewolf
    (8, 0.7),     // Dwarf
    (216, 1.4),   // Horse
    (175, 1.75),  // EnchantedArmor
    (63, 1.75),   // Tiger
    (66, 1.0),    // StatueOfRallosZek
    (687, 2.0),   // Goral
    (686, 2.0),   // Selyrah
];

/// The size `EQEmu` uses for a spawn sent with size 0, as players are (common/
/// races.cpp `GetRaceGenderDefaultHeight`, the same for both genders of these
/// races). Only playable races are listed; other models use 6, as `EQEmu` does
/// past the end of its table.
fn default_size(race: u32) -> f32 {
    match race {
        2 | 130 => 7.0,     // Barbarian, Vah Shir
        4 | 6 | 330 => 5.0, // Wood Elf, Dark Elf, Froglok
        7 => 5.5,           // Half Elf
        8 => 4.0,           // Dwarf
        9 => 8.0,           // Troll
        10 => 9.0,          // Ogre
        11 => 3.5,          // Halfling
        12 => 3.0,          // Gnome
        _ => 6.0,           // Human, Erudite, High Elf, Iksar and other models
    }
}

/// How far above its feet a spawn's reported position sits: 0.2 × size × a
/// per-model factor, exactly as `EQEmu` places mobs it moves (zone/waypoints.cpp
/// `Mob::GetZOffset`), with the race's default size when none was sent. Used
/// only for the player's own body, whose z we send; remote models are drawn by
/// [`drawn_feet_offset`]. What a non-human player sends is recording item 1.11.
/// Unverified against a P99 capture.
#[must_use]
pub fn z_offset(race: u32, size: f32) -> f32 {
    let factor = Z_FACTORS
        .iter()
        .find(|(model, _)| *model == race)
        .map_or(3.125, |(_, factor)| *factor);
    0.2 * size_or_default(race, size) * factor
}

/// How far under a remote spawn's reported position its soles are drawn:
/// 0.625 × size for every model, with the race's default size when none was
/// sent. A local server's static spawns of five humanoid races in four cities,
/// short races included, all sat at floor + 0.625 × their own size, and the
/// classic meshes share one shape about their origin, so `EQEmu`'s per-model
/// factors (applied by the server only to mobs it moves) play no part here. A
/// mob the server has moved to its own lower height draws into the floor, as
/// the official client is expected to draw it. Inferred until a recording of
/// the official client: item 6.231.
#[must_use]
pub fn drawn_feet_offset(race: u32, size: f32) -> f32 {
    0.625 * size_or_default(race, size)
}

/// Where a remote model's soles are drawn, in renderer coordinates: on the
/// floor of `world` when one lies above `soles` (the reported position less
/// [`drawn_feet_offset`]) by up to 0.6 × size or below it by up to 0.5, else at
/// `soles`. `EQEmu` places a spawn it has moved at its own per-model height, up
/// to (0.625 − 0.2 × factor) × size under where a standing one sits, and
/// nothing in the spawn says which rule produced its z, so the floor decides.
/// The fix only ever raises a model: a standing spawn's soles already sit a
/// little over the floor, so the reach down is kept short, and a low flyer, a
/// mob on a lift or a door (doors are not in this geometry) or one over ground
/// this geometry lacks stays where the server says. Inferred; recording item
/// 6.231.
#[must_use]
pub fn grounded_soles(
    world: &movement::CollisionWorld,
    soles: glam::Vec3,
    race: u32,
    size: f32,
) -> f32 {
    let reach = 0.6 * size_or_default(race, size);
    world.ground(soles, reach, 0.5).unwrap_or(soles.y)
}

/// Where a remote model's soles are drawn, in renderer coordinates, as the
/// client carries it on between the server's reports: on the floor found
/// from where its soles were drawn the frame before (`previous`: their
/// height, and their spot across the ground), reaching up 0.6 × size as
/// [`grounded_soles`] does and down 0.5 plus the distance moved across the
/// ground since, so it follows a slope down as steep as 45° at any frame
/// time. Of the floors in that reach, the one nearest those soles: a spawn
/// on a bridge or a ledge stays on it while the ground runs on below. With no frame since the latest report, or no floor in that reach
/// (a ledge, a flyer, water), [`grounded_soles`] decides from `soles`, the
/// reported height carried on. `EQEmu` sends no vertical motion while its
/// own z follows the floor, so a spawn carried at its reported height would
/// float down a slope and sink into one going up until the next report.
/// Inferred; recording item 6.276.
#[must_use]
pub fn carried_soles(
    world: &movement::CollisionWorld,
    previous: Option<(f32, glam::Vec2)>,
    soles: glam::Vec3,
    race: u32,
    size: f32,
) -> f32 {
    previous
        .and_then(|(height, spot)| {
            let step = glam::Vec2::new(soles.x, soles.z).distance(spot);
            let from = glam::Vec3::new(soles.x, height, soles.z);
            world.nearest_ground(from, 0.6 * size_or_default(race, size), 0.5 + step)
        })
        .unwrap_or_else(|| grounded_soles(world, soles, race, size))
}

/// The size sent, or the race's default when it is missing or not positive.
fn size_or_default(race: u32, size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size
    } else {
        default_size(race)
    }
}

/// Converts a renderer position back to EQ world coordinates, undoing
/// [`render_position`].
pub fn world_position(position: [f32; 3], heading: f32) -> WorldPosition {
    let world = eq_client_axes::to_world(glam::Vec3::from_array(position));
    WorldPosition {
        x: world.x,
        y: world.y,
        z: world.z,
        heading,
    }
}

/// Server death notification retained during the bind-transfer lifecycle.
pub use eq_network_game::zoning::Death;
/// Preserved item link and server-supplied inspection result.
pub use eq_network_game::{
    chat::ItemLink,
    items::{EquipmentRules, ItemBonuses, ItemDetails, ItemStat},
};

#[cfg(test)]
mod tests {
    #[test]
    fn reported_positions_sit_a_size_and_model_scaled_offset_above_the_feet() {
        let offset = super::z_offset;
        assert!((offset(1, 6.0) - 3.75).abs() < 0.0001); // human
        assert!((offset(10, 9.0) - 5.625).abs() < 0.0001); // ogre
        assert!((offset(8, 4.0) - 0.56).abs() < 0.0001); // dwarf, race 8 in races.h
        assert!((offset(437, 10.0) - 1.054).abs() < 0.0001); // Dragon2
        // Players arrive with size 0, which means the race's default.
        assert!((offset(1, 0.0) - 3.75).abs() < 0.0001);
        assert!((offset(10, 0.0) - 5.625).abs() < 0.0001);
        assert!((offset(12, f32::NAN) - 1.875).abs() < 0.0001); // gnome
    }

    #[test]
    fn remote_models_hang_their_soles_by_size_alone() {
        let offset = super::drawn_feet_offset;
        assert!((offset(1, 6.0) - 3.75).abs() < 0.0001); // human, as z_offset
        assert!((offset(183, 4.8) - 3.0).abs() < 0.0001); // Coldain
        assert!((offset(8, 0.0) - 2.5).abs() < 0.0001); // dwarf, default size 4
        assert!((offset(12, f32::NAN) - 1.875).abs() < 0.0001); // gnome, default 3
    }

    /// A ramp rising `grade` units per unit along world x, in renderer
    /// coordinates, and the floor's height at a world x.
    fn ramp(grade: f32) -> crate::movement::CollisionWorld {
        let corner = |x: f32, y: f32| {
            glam::Vec3::from_array(super::render_position(super::WorldPosition {
                x,
                y,
                z: grade * x,
                heading: 0.0,
            }))
            .to_array()
        };
        crate::movement::CollisionWorld::new([
            [
                corner(-100.0, -20.0),
                corner(300.0, -20.0),
                corner(300.0, 20.0),
            ],
            [
                corner(-100.0, -20.0),
                corner(300.0, 20.0),
                corner(-100.0, 20.0),
            ],
        ])
        .unwrap()
    }

    #[test]
    fn a_carried_spawn_keeps_to_the_floor_up_and_down_a_slope() {
        // A size 6 skeleton (race 60) walking 11.8 units a second, reported
        // on the floor every 5 s with no vertical motion, as EQEmu reports a
        // walking NPC; drawn at 60 and at 10 frames a second.
        let (race, size, speed) = (60, 6.0, 11.8);
        let feet = super::drawn_feet_offset(race, size);
        for grade in [0.05_f32, -0.05, 0.19, -0.19, 0.45, -0.45, 0.99, -0.99] {
            let world = ramp(grade);
            for fps in [60_u16, 10] {
                let mut previous = None;
                for frame in 0..=u32::from(fps) * 10 {
                    let t = frame as f32 / f32::from(fps);
                    let report = (t / 5.0).floor() * 5.0;
                    if (t - report).abs() < 1e-6 {
                        previous = None;
                    }
                    let x = speed * t;
                    let reported_z = grade * speed * report + feet;
                    let soles =
                        glam::Vec3::from_array(super::render_position(super::WorldPosition {
                            x,
                            y: 0.0,
                            z: reported_z - feet,
                            heading: 0.0,
                        }));
                    let drawn = super::carried_soles(&world, previous, soles, race, size);
                    let floor = super::render_position(super::WorldPosition {
                        x,
                        y: 0.0,
                        z: grade * x,
                        heading: 0.0,
                    })[1];
                    assert!(
                        (drawn - floor).abs() < 0.01,
                        "grade {grade} at {fps} fps, {t:.2} s: drawn {drawn}, floor {floor}"
                    );
                    previous = Some((drawn, glam::Vec2::new(soles.x, soles.z)));
                }
            }
        }
        // Held at the reported height, as before, it loses the floor two
        // seconds into a 19% climb and is drawn 4.5 under the hill.
        let world = ramp(0.19);
        let soles = glam::Vec3::from_array(super::render_position(super::WorldPosition {
            x: speed * 2.0,
            ..Default::default()
        }));
        assert!((super::grounded_soles(&world, soles, race, size) - soles.y).abs() < 0.001);
    }

    #[test]
    fn a_carried_spawn_keeps_to_the_floor_nearest_its_soles() {
        // A deck two over the ground, and a slab three over the deck.
        let plate = |y: f32, half: f32| {
            [
                [[-half, y, -half], [half, y, -half], [half, y, half]],
                [[-half, y, -half], [half, y, half], [-half, y, half]],
            ]
        };
        let world = crate::movement::CollisionWorld::new(
            [plate(0.0, 50.0), plate(2.0, 20.0), plate(5.0, 10.0)].concat(),
        )
        .unwrap();
        // Two units a frame, a long frame: the reach down takes in the ground
        // under the deck and the reach up the slab, yet it stays on the deck.
        let on = super::carried_soles(
            &world,
            Some((2.0, glam::Vec2::ZERO)),
            glam::Vec3::new(2.0, 2.0, 0.0),
            60,
            6.0,
        );
        assert!((on - 2.0).abs() < 0.001, "{on}");
        // Off the deck's end, the ground under it is the floor in reach.
        let off = super::carried_soles(
            &world,
            Some((2.0, glam::Vec2::new(19.0, 0.0))),
            glam::Vec3::new(21.0, 2.0, 0.0),
            60,
            6.0,
        );
        assert!(off.abs() < 0.001, "{off}");
    }

    #[test]
    fn a_carried_flyer_stays_at_its_reported_height() {
        let world = ramp(0.0);
        let soles = glam::Vec3::Y * 9.0;
        let first = super::carried_soles(&world, None, soles, 60, 6.0);
        assert!((first - 9.0).abs() < 0.001);
        let on = glam::Vec3::new(1.0, 9.0, 0.0);
        let next = super::carried_soles(&world, Some((first, glam::Vec2::ZERO)), on, 60, 6.0);
        assert!((next - 9.0).abs() < 0.001);
    }

    #[test]
    fn remote_models_stand_on_a_floor_near_their_soles() {
        let floor = [
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ];
        let world = crate::movement::CollisionWorld::new(floor).unwrap();
        let soles = |y: f32| super::grounded_soles(&world, glam::Vec3::Y * y, 183, 4.0);
        // Reported two under the floor, as a walked mob's z puts it, it lands;
        // two over, as a flyer or a mob on a lift, it stays.
        assert!(soles(-2.0).abs() < 0.001);
        assert!((soles(2.0) - 2.0).abs() < 0.001);
        // It reaches up 0.6 × size to a floor above its soles, and no further.
        assert!(soles(-2.39).abs() < 0.001);
        assert!((soles(-2.41) + 2.41).abs() < 0.001);
        // It reaches down only 0.5, the measured excess of a standing spawn.
        assert!(soles(0.49).abs() < 0.001);
        assert!((soles(0.51) - 0.51).abs() < 0.001);
        assert!((soles(5.0) - 5.0).abs() < 0.001);
        // A size-less spawn reaches by its race's default size: 0.6 × 4 for a dwarf.
        assert!(super::grounded_soles(&world, glam::Vec3::Y * -2.3, 8, 0.0).abs() < 0.001);
    }

    use super::{WorldPosition, render_position, world_position};

    #[test]
    fn maps_world_coordinates_to_y_up_render_coordinates() {
        let position = WorldPosition {
            x: 10.0,
            y: 20.0,
            z: 30.0,
            heading: 0.0,
        };

        let actual = render_position(position);
        assert!(
            actual
                .iter()
                .zip([20.0, 30.0, -10.0])
                .all(|(a, b)| (a - b).abs() < f32::EPSILON)
        );
        assert_eq!(world_position(actual, position.heading), position);
    }
}
