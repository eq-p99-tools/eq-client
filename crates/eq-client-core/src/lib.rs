#![doc = "Engine-independent state shared by EQ client front ends and network adapters."]

/// A position in `EverQuest` world coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldPosition {
    /// North/south coordinate reported by the world server.
    pub x: f32,
    /// East/west coordinate reported by the world server.
    pub y: f32,
    /// Height above the zone origin.
    pub z: f32,
    /// Heading in `EverQuest`'s 0-512 representation.
    pub heading: f32,
}

/// Stable state changes that a network adapter can deliver to a presentation layer.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum WorldUpdate {
    /// The active character entered a zone.
    ZoneChanged {
        /// Server short name for the new zone.
        short_name: String,
    },
    /// The server accepted a new position for the active character.
    PlayerPosition(WorldPosition),
}

/// Converts an EQ position to a right-handed, Y-up renderer position.
///
/// `EverQuest` reports `(north, east, up)` while common 3D engines use
/// `(right, up, forward)`.
pub const fn render_position(position: WorldPosition) -> [f32; 3] {
    [position.y, position.z, position.x]
}

#[cfg(test)]
mod tests {
    use super::{WorldPosition, render_position};

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
    }
}
