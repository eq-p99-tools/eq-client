#![doc = "Engine-independent state shared by EQ client front ends and network adapters."]

pub mod buffs;
pub mod chat;
pub mod entities;
pub mod movement;
pub mod resources;
pub mod targeting;
pub use eq_network_game::characters::CharacterChoice;
pub use eq_network_game::chat::OutboundChat;
pub use eq_network_game::combat;
pub use eq_network_game::command::GameCommand as ClientCommand;
pub use eq_network_game::command::Posture;
pub use eq_network_game::doors;
pub use eq_network_game::inventory;
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
    BaseAttributes, PlayerState, Position as WorldPosition, PostureState, SpawnKind, SpawnState,
    WorldEvent,
};

/// Messages crossing the worker/presentation boundary. Queues are bounded by the host.
#[derive(Clone, Debug)]
pub enum WorldUpdate {
    /// A typed game-state change.
    Game(WorldEvent),
    /// Connection status; false disables local gameplay input immediately.
    Connection {
        /// Whether zone admission is complete.
        connected: bool,
        /// Whether this worker has ended and will not reconnect.
        terminal: bool,
        /// Credential-safe presentation label.
        label: String,
    },
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

/// Resolves classic playable race/gender identifiers without guessing NPC models.
pub fn classic_model(race: u32, gender: u32) -> Option<&'static str> {
    let pair = match race {
        1 => ["HUM", "HUF"],
        2 => ["BAM", "BAF"],
        3 => ["ERM", "ERF"],
        4 => ["ELM", "ELF"],
        5 => ["HIM", "HIF"],
        6 => ["DAM", "DAF"],
        7 => ["HAM", "HAF"],
        8 => ["DWM", "DWF"],
        9 => ["TRM", "TRF"],
        10 => ["OGM", "OGF"],
        11 => ["HOM", "HOF"],
        12 => ["GNM", "GNF"],
        128 => ["IKM", "IKF"],
        _ => return None,
    };
    pair.get(usize::try_from(gender).ok()?).copied()
}

/// Converts an EQ position to a right-handed, Y-up renderer position.
///
/// `EverQuest` reports `(north, east, up)` while common 3D engines use
/// `(right, up, forward)`.
pub const fn render_position(position: WorldPosition) -> [f32; 3] {
    [position.y, position.z, position.x]
}

/// Converts EQ heading to renderer yaw about Y, where the root faces +Z.
pub fn render_heading(heading: f32) -> f32 {
    // EQ horizontal direction is (sin(h), cos(h)); rendering swaps X/Y.
    std::f32::consts::FRAC_PI_2 - heading / 512.0 * std::f32::consts::TAU
}

/// Converts a rotation about renderer Y (+Z forward) to an EQ 0..512 heading.
pub fn world_heading(yaw: f32) -> f32 {
    ((std::f32::consts::FRAC_PI_2 - yaw) / std::f32::consts::TAU * 512.0).rem_euclid(512.0)
}

/// Converts a renderer position back to EQ world coordinates.
pub const fn world_position(position: [f32; 3], heading: f32) -> WorldPosition {
    WorldPosition {
        x: position[2],
        y: position[0],
        z: position[1],
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
    fn cardinal_headings_match_eq_axes_after_render_conversion() {
        for (heading, direction) in [
            (0.0, [1.0, 0.0, 0.0]),
            (128.0, [0.0, 0.0, 1.0]),
            (256.0, [-1.0, 0.0, 0.0]),
            (384.0, [0.0, 0.0, -1.0]),
        ] {
            let yaw = super::render_heading(heading);
            assert!((yaw.sin() - direction[0]).abs() < 0.00001);
            assert!((yaw.cos() - direction[2]).abs() < 0.00001);
            assert!((super::world_heading(yaw) - heading).abs() < 0.0001);
        }
        assert!(super::world_heading(super::render_heading(512.0)).abs() < 0.0001);
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
                .zip([20.0, 30.0, 10.0])
                .all(|(a, b)| (a - b).abs() < f32::EPSILON)
        );
        assert_eq!(world_position(actual, position.heading), position);
    }
}
