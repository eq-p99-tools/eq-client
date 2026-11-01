//! How far an installed client's camera may move, from the program. A
//! client generation's limits are its [`CameraLimits`], from
//! [`crate::ui::InstalledClient::camera_limits`].
//!
//! The Titanium program's third-person camera has no angle limit of its
//! own: it sits a height above the player's eyes and a distance behind
//! them, each held within its range on every update (0x57081f, 0x56fd78,
//! 0x570d81, 0x570fca, 0x5709db; the ranges at 0x65fb08-0x65fb14), and the
//! view tilts down to the player from wherever that puts it. So it never
//! drops below eye level: looking up is for the first-person view, whose
//! pitch is held within a quarter turn either way (0x4bfe0f). Program read
//! N2 of 11-01.

/// How far a client generation's camera may move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraLimits {
    /// The first-person view's pitch either side of level, in degrees:
    /// straight up to straight down.
    pub look_pitch_degrees: f32,
    /// The third-person camera's height above the player's eyes, least
    /// and most, in units.
    pub chase_height: (f32, f32),
    /// The third-person camera's distance behind the player, least and
    /// most, in units.
    pub chase_distance: (f32, f32),
}

/// The Titanium client's limits.
pub const TITANIUM: CameraLimits = CameraLimits {
    look_pitch_degrees: 90.0,
    chase_height: (0.0, 30.0),
    chase_distance: (2.0, 200.0),
};

/// The Mac-era client's limits: not yet read, so Titanium's (recording
/// checklist 6.349). They change only what the player sees.
pub const EQMAC: CameraLimits = TITANIUM;
