#![doc = "Engine-independent state shared by EQ client front ends and network adapters."]

pub mod buffs;
pub mod chat;
pub mod doors;
pub mod entities;
pub mod ground;
pub mod movement;
pub mod outfit;
pub mod races;
pub mod resources;
pub mod targeting;
pub mod world;
pub use eq_network_game::characters::CharacterChoice;
pub use eq_network_game::chat::OutboundChat;
pub use eq_network_game::combat;
pub use eq_network_game::command::GameCommand as ClientCommand;
pub use eq_network_game::command::Posture;
pub use eq_network_game::creation;
pub use eq_network_game::inventory;
pub use eq_network_game::loot;
pub use eq_network_game::merchant;
pub use eq_network_game::movement::{
    BackwardCalibration, MotionCalibration, MovementMode, MovementRequest, StrafeCalibration,
    WalkCalibration,
};
pub use eq_network_game::spells::BookActionStatus;
pub use eq_network_game::spells::SpellBook;
pub use eq_network_game::spells::SpellUpdate;
pub use eq_network_game::zoning::ZoneLineDestination;
pub use eq_network_game::zoning::ZoneOffer;
pub use eq_network_game::zoning::ZoneRejection;

pub use eq_network_game::buffs::{Buff, BuffUpdate, SpellEffect};
pub use eq_network_game::world::{
    BaseAttributes, CampStatus, Capability, Coins, PlayerState, Position as WorldPosition,
    PostureState, SpawnKind, SpawnState, WorldEvent,
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
        arguments: Vec<String>,
    },
}

/// Converts an EQ position to the renderer's frame. `eq_client_axes` owns the
/// conversion, the reflection between EQ's frame and the renderer's included;
/// this and [`world_position`] adapt it to protocol positions.
pub fn render_position(position: WorldPosition) -> [f32; 3] {
    eq_client_axes::from_world(glam::Vec3::new(position.x, position.y, position.z)).to_array()
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
/// per-model factor, exactly as `EQEmu` places mobs (zone/waypoints.cpp
/// `Mob::GetZOffset`), with the race's default size when none was sent.
/// Unverified against a P99 capture.
#[must_use]
pub fn z_offset(race: u32, size: f32) -> f32 {
    let factor = Z_FACTORS
        .iter()
        .find(|(model, _)| *model == race)
        .map_or(3.125, |(_, factor)| *factor);
    let size = if size.is_finite() && size > 0.0 {
        size
    } else {
        default_size(race)
    };
    0.2 * size * factor
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
    items::{EquipmentRules, ItemBonuses, ItemDetails},
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
