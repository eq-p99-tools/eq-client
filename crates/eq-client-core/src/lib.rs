#![doc = "Engine-independent state shared by EQ client front ends and network adapters."]

pub mod breath;
pub mod buffs;
pub mod camp;
pub mod chat;
pub mod classes;
pub mod contacts;
pub mod daylight;
pub mod doors;
pub mod entities;
pub mod environment;
pub mod ground;
pub mod hotbar;
pub mod logs;
pub mod machine;
pub mod maps;
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
pub mod spell_tables;
pub mod targeting;
pub mod whereabouts;
pub mod who;
pub mod world;
pub use eq_network_game::GameDialect;
pub use eq_network_game::abilities;
pub use eq_network_game::bind_wound;
pub use eq_network_game::books;
pub use eq_network_game::characters::{CharacterChoice, CharacterLook};
pub use eq_network_game::chat::OutboundChat;
pub use eq_network_game::clock;
pub use eq_network_game::combat;
pub use eq_network_game::command::GameCommand as ClientCommand;
pub use eq_network_game::command::Posture;
pub use eq_network_game::confirmations;
pub use eq_network_game::control;
pub use eq_network_game::corpses;
pub use eq_network_game::creation;
pub use eq_network_game::deletion;
pub use eq_network_game::disciplines;
pub use eq_network_game::rules;
pub use eq_network_game::vision;
pub mod character_creation;
pub use eq_network_game::effects;
pub use eq_network_game::exchange;
pub use eq_network_game::food;
pub use eq_network_game::group;
pub use eq_network_game::guild;
pub use eq_network_game::hazards;
pub use eq_network_game::inventory;
pub use eq_network_game::knights;
pub use eq_network_game::listing;
pub use eq_network_game::loot;
pub use eq_network_game::merchant;
pub use eq_network_game::money;
pub use eq_network_game::movement::{
    BackwardCalibration, DiagonalAlong, DiagonalRates, EqMacFatigueInputs, GroundedDiagonal,
    HeadingUpdate, MotionCalibration, MovementMode, MovementRequest, SpeedInputAcceptance,
    SpeedInputState, SpeedInputUnavailable, SpeedInputs, StrafeCalibration, StrafeSide, Support,
    WalkCalibration,
};
pub use eq_network_game::pets;
pub use eq_network_game::popups::Marquee;
pub use eq_network_game::raid;
pub use eq_network_game::reach;
pub use eq_network_game::resurrection;
pub use eq_network_game::servers;
pub use eq_network_game::socials;
pub use eq_network_game::spells::BookActionStatus;
pub use eq_network_game::spells::SpellBook;
pub use eq_network_game::spells::SpellUpdate;
pub use eq_network_game::sysinfo::{Machine, SysInfo};
pub use eq_network_game::targets;
pub use eq_network_game::tracking;
pub use eq_network_game::tradeskills;
pub use eq_network_game::training;
pub use eq_network_game::zones;
pub use eq_network_game::zoning::ZoneOffer;
pub use eq_network_game::zoning::ZoneRejection;
pub use eq_network_game::zoning::{LocalPadCrossing, SameZoneRelocation, ZoneLineDestination};
pub use eq_network_game::{inspection, popups};

pub use eq_network_game::buffs::{Buff, BuffUpdate, InvisibilityStep, SpellEffect};
pub use eq_network_game::world::{
    BaseAttributes, CampEnd, CampStatus, Capability, Coins, ItemHitPoints, PlayerState,
    Position as WorldPosition, PostureState, SpawnKind, SpawnState, WorldEvent,
};

/// Messages crossing the worker/presentation boundary. Queues are bounded by the host.
#[derive(Clone, Debug)]
pub enum WorldUpdate {
    /// A producer-bound installed-spell provider fact, independent of admission.
    SpellTable(spell_tables::Envelope),
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

/// The height above its soles at which a model hangs from its spawn's
/// reported position, in the model's own units, before its drawn scale: the
/// Mac-era program starts a spawn's stand height at this and multiplies it
/// by the scale it draws the model at (program read N1, 0x51f79b). The
/// Titanium program takes the height from the model (0x4b6d42), not yet
/// read: this stands in for it there (recording checklist 6.347).
pub const STAND_HEIGHT: f32 = 3.125;

/// How far under a remote spawn's reported position its soles are drawn, its
/// stand height: [`STAND_HEIGHT`] × the scale its model is drawn at, which is
/// 0.625 × size for a classic model drawn at size × 0.2. A local server's
/// static spawns of five humanoid races in four cities, short races
/// included, all sat at floor + 0.625 × their own size, and the classic
/// meshes share one shape about their origin, so `EQEmu`'s per-model factors
/// (applied by the server only to mobs it moves) play no part here. A mob
/// the server has moved to its own lower height draws into the floor, as the
/// official client is expected to draw it (recording item 6.231).
#[must_use]
pub fn drawn_feet_offset(scale: f32) -> f32 {
    STAND_HEIGHT * scale
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
/// this geometry lacks stays where the server says. `size` is the size the
/// model is drawn by: the size sent, or its race's default, as the installed
/// client takes it. Inferred; recording item 6.231.
#[must_use]
pub fn grounded_soles(world: &movement::CollisionWorld, soles: glam::Vec3, size: f32) -> f32 {
    let reach = 0.6 * size;
    world.ground(soles, reach, 0.5).unwrap_or(soles.y)
}

/// How a carried spawn on the ground falls between the server's reports, in
/// the program's own units: its fall speed is in units a 50 ms step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarriedFall {
    /// What the fall speed gains each step: the zone header's gravity, 0.4
    /// by default.
    pub gravity: f32,
    /// The share of the fall speed each call keeps, the vertical D: 0.99.
    pub kept: f32,
    /// Where a spawn that falls under the zone is put back, once its zone's
    /// header has come: None leaves it falling.
    pub underworld: Option<UnderworldReturn>,
}

/// A zone's top and lowest heights, as its header gives them. The Titanium
/// program copies the header into its zone block at 0x9239e0 (the classic
/// model scale flag at +592 lands at 0x9239e0 + 0x250, read N1b), so the
/// globals it tests a fall against, 0x923bd8 and 0x923bdc, are the header's
/// +504 and +508, these two (inferred from the offsets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneHeights {
    /// The height the emulators' structs name `max_z`; the Titanium program
    /// keeps it at 0x923bd8.
    pub top: f32,
    /// The height below which a body is under the world; the Titanium
    /// program keeps it at 0x923bdc.
    pub underworld: f32,
}

/// Where a carried spawn that falls with no floor under it is put back. The
/// Titanium program finds no floor there (its floor finder gives -1e27), so
/// the spawn falls every step and nothing lands it, as an NPC carried over a
/// hole or off the collision's edge does, since only the player is refused
/// those (R-t, 0x48bb4e); once its feet pass the zone's lowest height, the
/// landing routine (0x48b6c5), for a spawn other than the player's, finds
/// the zone's top surface at the spawn's spot, straight down from 32,000,
/// and puts the spawn on it if that surface is more than 10 under the zone's
/// top, else leaves it be (R-s Titanium addendum, section 5: 0x923bdc,
/// 0x439f26, 0x923bd8, 0x478474). The program does so only in zones with a
/// flag at 0x923b88 whose meaning is unread, here taken as set in every
/// zone; `EQMac` is taken to do the same until read (checklist 6.362).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnderworldReturn {
    /// The zone's lowest height: the zone header's underworld, which the
    /// program's zone block keeps at 0x923bdc.
    pub underworld: f32,
    /// The zone's top surface must lie under this height for the spawn to be
    /// put on it: the header's `max_z` (the program's 0x923bd8), less 10.
    pub highest: f32,
    /// The height the search for the top surface starts from: 32,000.
    pub search_from: f32,
}

impl UnderworldReturn {
    /// The height a spawn fallen under the zone at `spot` is put back at: the
    /// first surface straight down from [`Self::search_from`], if it lies
    /// under [`Self::highest`]. None leaves it be, as where no surface lies
    /// under that spot at all (the program's search then gives -1e27, so a
    /// spawn it puts there is lost from view either way).
    #[must_use]
    pub fn put_back(&self, world: &movement::CollisionWorld, spot: glam::Vec2) -> Option<f32> {
        let from = glam::Vec3::new(spot.x, self.search_from, spot.y);
        world
            .ground(from, 0.0, 2.0 * self.search_from.abs())
            .filter(|&top| top < self.highest)
    }
}

/// Where a carried model's soles were drawn the frame before, since its
/// latest report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawnSoles {
    /// Their height, in renderer coordinates.
    pub height: f32,
    /// Their spot across the ground.
    pub spot: glam::Vec2,
    /// Its fall speed, in units a 50 ms step, downward negative: 0 while it
    /// stands on a floor. None until it has stood on one since the report.
    pub fall: Option<f32>,
}

/// Where a remote model's soles are drawn, in renderer coordinates, as the
/// client carries it on between the server's reports, `steps` 50 ms steps
/// after the frame before (`previous`), as both programs draw a spawn on the
/// ground: its height falls from where it was drawn under the zone's gravity
/// (the fall speed gains `fall.gravity` × steps, then keeps `fall.kept` of
/// itself, and the height moves by it × steps, steps held to 3), and where
/// its soles pass a floor, or one lies above them within its stand height
/// ([`z_offset`]), it stands on the floor nearest the height it was drawn
/// at, its fall stopped. So it rises to the floor at once, comes down a slope or off a
/// ledge under gravity, a little behind the floor going down, and with no
/// floor under it falls until the next report, or until it passes under the
/// zone and is put back on its top surface ([`UnderworldReturn`]). Read for Titanium (Decompile
/// R-z, 11-01: the gravity path 0x4850a0, the landing check 0x48b6c5 and
/// the floor finder 0x48a0fc, which takes a face at any angle) and in part
/// for `EQMac` (0x50ba07, 0x4b3a84, 0x4b479e). The reach up is read too:
/// both programs sweep for the floor from the spawn's origin, its stand
/// height above its feet (6.344): `stand`, its [`drawn_feet_offset`]
/// (Titanium's height inferred, 6.347), the `EQMac` sweep starting a further 0.5 up, which
/// is left out here; the sweep's radius is applied as `falls::TITANIUM` reads it
/// for Titanium and is otherwise left out (6.336). Neither `EQEmu` nor P99 sends vertical motion while
/// its own z follows the floor, so a model drawn at its reported height
/// would float down a slope and sink into one going up until the next
/// report.
///
/// It falls only once a floor has come within that stand height of its
/// carried soles, above or below, since its latest report: at the report it stands
/// on such a floor above them, or falls from them onto one below, as the
/// program falls from a reported height. One reported further off the floor
/// keeps its carried height (`soles`), as a flyer does, until a floor comes
/// within that reach. The program leaves a levitating or flying spawn out of
/// gravity by its movement mode, which this client doesn't read: until it
/// does (checklist 6.337), this is the only way a flyer and a spawn reported
/// in mid-air are told apart, and inferred, the stand height's reach down
/// included. A P99 player running downhill is reported up to 1.4 over our
/// floor at a 0.625 × size feet offset (pair 967 of the 11-01 sitting).
#[must_use]
pub fn carried_soles(
    world: &movement::CollisionWorld,
    previous: Option<DrawnSoles>,
    soles: glam::Vec3,
    stand: f32,
    fall: CarriedFall,
    steps: f32,
) -> DrawnSoles {
    let reach = stand;
    let spot = glam::Vec2::new(soles.x, soles.z);
    if let Some(DrawnSoles {
        height,
        fall: Some(speed),
        ..
    }) = previous
    {
        let steps = if steps.is_finite() {
            steps.clamp(0.0, 3.0)
        } else {
            0.0
        };
        let speed = (speed - fall.gravity * steps) * fall.kept;
        let below = (-speed * steps).max(0.0);
        let from = glam::Vec3::new(soles.x, height, soles.z);
        if let Some(floor) = world.nearest_ground(from, reach, below) {
            return DrawnSoles {
                height: floor,
                spot,
                fall: Some(0.0),
            };
        }
        let height = height - below;
        // Whether the program's move (0x478474) also stops the fall is
        // unread: the next step stands it on that surface either way.
        let height = fall
            .underworld
            .filter(|underworld| height < underworld.underworld)
            .and_then(|underworld| underworld.put_back(world, spot))
            .unwrap_or(height);
        return DrawnSoles {
            height,
            spot,
            fall: Some(speed),
        };
    }
    match world.ground(soles, reach, reach) {
        // A floor above the soles, or at them, it stands on at once.
        Some(floor) if floor >= soles.y => DrawnSoles {
            height: floor,
            spot,
            fall: Some(0.0),
        },
        // One under them it falls onto from there.
        Some(_) => DrawnSoles {
            height: soles.y,
            spot,
            fall: Some(0.0),
        },
        None => DrawnSoles {
            height: soles.y,
            spot,
            fall: None,
        },
    }
}

/// How long a drawn model takes to slide from where it was drawn onto a new
/// report's carried point: 0.1 s.
pub const CORRECTION_SECONDS: f32 = 0.1;

/// The part of a report's correction a drawn model still has to make. The
/// official client draws a spawn at its carried point, which each report
/// moves outright (Titanium read R2, `EQMac` R1); this client draws it at
/// that point plus what is left of the jump the report made, which shrinks
/// evenly to nothing over [`CORRECTION_SECONDS`], so a correction shows as a
/// short slide and steady motion is drawn where the program draws it, not
/// behind it. Display only, and ours (checklist 6.345).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Correction {
    /// The jump left to make: where the model was drawn less where the
    /// report put it.
    offset: glam::Vec3,
    /// The seconds of the slide left.
    left: f32,
}

impl Correction {
    /// No correction left to make.
    pub const NONE: Self = Self {
        offset: glam::Vec3::ZERO,
        left: 0.0,
    };

    /// Starts the slide from where the model was `drawn` onto a report's
    /// carried point, `target`.
    #[must_use]
    pub fn new(drawn: glam::Vec3, target: glam::Vec3) -> Self {
        let offset = drawn - target;
        if offset.is_finite() {
            Self {
                offset,
                left: CORRECTION_SECONDS,
            }
        } else {
            Self::default()
        }
    }

    /// Where the model is drawn `seconds` on, when its carried point is
    /// `target` then.
    pub fn advance(&mut self, target: glam::Vec3, seconds: f32) -> glam::Vec3 {
        if seconds.is_finite() && seconds > 0.0 {
            self.left = (self.left - seconds).max(0.0);
        }
        target + self.offset * (self.left / CORRECTION_SECONDS)
    }
}

/// How far a corpse leans to lie along the floor under it, each way, at
/// most: 35°.
pub const MOST_TILT: f32 = 35.0 * std::f32::consts::PI / 180.0;

/// How a corpse's body leans to lie along the floor under it, in the
/// renderer's frame, as turns about its root's own axes after its heading:
/// the root's rotation becomes heading × pitch × roll ([`Self::rotation`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tilt {
    /// The turn about the root's side-to-side axis (its X), in radians: a
    /// negative pitch raises the way it faces (its Z).
    pub pitch: f32,
    /// The turn about the way it faces (its Z), in radians: a positive roll
    /// raises its X side.
    pub roll: f32,
}

impl Tilt {
    /// The lean as a rotation in the root's own frame, pitch then roll, to
    /// follow its heading.
    #[must_use]
    pub fn rotation(self) -> glam::Quat {
        glam::Quat::from_rotation_x(self.pitch) * glam::Quat::from_rotation_z(self.roll)
    }

    /// This share of the lean, from none at 0 to all of it at 1, as a body
    /// falling into it.
    #[must_use]
    pub fn share(self, share: f32) -> Self {
        let share = share.clamp(0.0, 1.0);
        Self {
            pitch: self.pitch * share,
            roll: self.roll * share,
        }
    }

    /// Whether it leans not at all, as a body on flat ground.
    #[must_use]
    pub fn upright(self) -> bool {
        self.pitch == 0.0 && self.roll == 0.0
    }
}

/// How a corpse leans to lie along the floor of `world` under it: its soles
/// at `soles` (as [`carried_soles`] draws them), its root turned `yaw` about
/// the renderer's Y, and its body lying `length` from its soles along the
/// way it faces in its death clip's last frame (negative where it lies
/// behind them), in world units. The floor is sampled where the body is: at
/// the soles, at half the length and at the length along the side the body
/// lies, and at two points across the body from its middle, an eighth of
/// the length to each side (half of a side span a quarter of the length),
/// each with a reach of up 0.6 × size and down 0.5 plus the distance from
/// the soles. The pitch is the
/// least-squares slope of the floor along the body over the samples it has,
/// and the roll the slope between the two side samples, each clamped to
/// [`MOST_TILT`]. A sample with no floor in reach is skipped: with no floor
/// at the soles the body stays upright, with no other sample along the body
/// the pitch stays level, and with either side sample missing the roll
/// stays level. Adam's choice (10-20); what the official client does is
/// inferred, checklist item 6.282.
#[must_use]
pub fn corpse_tilt(
    world: &movement::CollisionWorld,
    soles: glam::Vec3,
    yaw: f32,
    length: f32,
    size: f32,
) -> Tilt {
    let reach = 0.6 * size;
    let floor =
        |offset: glam::Vec3| world.nearest_ground(soles + offset, reach, 0.5 + offset.length());
    if !length.is_finite() || length == 0.0 || floor(glam::Vec3::ZERO).is_none() {
        return Tilt::default();
    }
    let facing = glam::Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let across = glam::Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    // The way the body lies from its soles, as its death clip lays it.
    let body = facing * length.signum();
    let reach_along = length.abs();
    // The floor along the body, by distance from the soles, fitted by least
    // squares; a fit needs the soles and one more.
    let along: Vec<(f32, f32)> = [0.0, reach_along / 2.0, reach_along]
        .into_iter()
        .filter_map(|distance| Some((distance, floor(body * distance)?)))
        .collect();
    let rise = (along.len() >= 2).then(|| {
        #[allow(clippy::cast_precision_loss, reason = "at most three samples")]
        let count = along.len() as f32;
        let mean = |pick: fn(&(f32, f32)) -> f32| along.iter().map(pick).sum::<f32>() / count;
        let (x, y) = (mean(|sample| sample.0), mean(|sample| sample.1));
        let covariance: f32 = along.iter().map(|(at, h)| (at - x) * (h - y)).sum();
        let variance: f32 = along.iter().map(|(at, _)| (at - x).powi(2)).sum();
        covariance / variance
    });
    // The rise toward the root's Z is what the pitch lowers.
    let pitch = rise.map_or(0.0, |rise| {
        -(rise * length.signum()).atan().clamp(-MOST_TILT, MOST_TILT)
    });
    let middle = body * (reach_along / 2.0);
    let offset = reach_along / 8.0;
    let roll = floor(middle + across * offset)
        .zip(floor(middle - across * offset))
        .map_or(0.0, |(raised, lowered)| {
            (raised - lowered)
                .atan2(2.0 * offset)
                .clamp(-MOST_TILT, MOST_TILT)
        });
    Tilt { pitch, roll }
}

/// The size sent, or the race's default when it is missing or not positive:
/// the height a spawn of the race is drawn at, as `EQEmu` sizes players.
#[must_use]
pub fn size_or_default(race: u32, size: f32) -> f32 {
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
    fn remote_models_hang_their_soles_by_their_drawn_scale() {
        let offset = super::drawn_feet_offset;
        // A classic model drawn at size × 0.2 hangs 0.625 × size.
        assert!((offset(6.0 * 0.2) - 3.75).abs() < 0.0001); // human, as z_offset
        assert!((offset(4.8 * 0.2) - 3.0).abs() < 0.0001); // Coldain
        // A model drawn smaller than its size hangs by its scale.
        assert!((offset(0.54) - 1.6875).abs() < 0.0001);
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

    /// Both programs' fall for a spawn on the ground: the default gravity of
    /// 0.4 a step and the vertical D of 0.99 (Decompile R-z).
    const FALL: super::CarriedFall = super::CarriedFall {
        gravity: 0.4,
        kept: 0.99,
        underworld: None,
    };

    /// The most a carried spawn on a ramp of `grade` is drawn over the floor,
    /// walking 11.8 units a second, reported on the floor every 5 s with no
    /// vertical motion as `EQEmu` and P99 report a walking NPC, drawn at
    /// `fps`; it is never drawn under the floor.
    fn most_over_the_ramp(grade: f32, fps: u16) -> f32 {
        let (size, speed) = (6.0, 11.8);
        let feet = super::drawn_feet_offset(size * 0.2);
        let world = ramp(grade);
        let mut previous = None;
        let mut most = 0.0_f32;
        for frame in 0..=fps * 10 {
            let t = f32::from(frame) / f32::from(fps);
            let report = (t / 5.0).floor() * 5.0;
            if (t - report).abs() < 1e-6 {
                previous = None;
            }
            let x = speed * t;
            let reported_z = grade * speed * report + feet;
            let soles = glam::Vec3::from_array(super::render_position(super::WorldPosition {
                x,
                y: 0.0,
                z: reported_z - feet,
                heading: 0.0,
            }));
            let steps = 20.0 / f32::from(fps);
            let drawn = super::carried_soles(&world, previous, soles, feet, FALL, steps);
            let floor = super::render_position(super::WorldPosition {
                x,
                y: 0.0,
                z: grade * x,
                heading: 0.0,
            })[1];
            assert!(
                drawn.height >= floor - 0.01,
                "grade {grade} at {fps} fps, {t:.2} s: drawn {}, under the floor {floor}",
                drawn.height
            );
            most = most.max(drawn.height - floor);
            previous = Some(drawn);
        }
        most
    }

    #[test]
    fn a_carried_spawn_keeps_to_the_floor_up_a_slope_and_falls_down_one() {
        // Up to 55°, the steepest P99 walks an NPC on, which the floor test
        // of 45.6° once refused, leaving it at each report's height. Going
        // up it stands on the floor at once; going down it falls onto it
        // under gravity, as the programs' do, a little over it.
        let steepest = 55.0_f32.to_radians().tan();
        for grade in [0.05_f32, 0.19, 0.45, 0.99, steepest] {
            for fps in [60_u16, 10] {
                let over = most_over_the_ramp(grade, fps);
                assert!(over < 0.01, "up {grade} at {fps} fps: {over} over");
            }
        }
        // Going down, the most it is drawn over the floor is the lag the
        // programs' fall gives (6.335), measured at 60 frames a second, the
        // largest: none to 19%, 0.05 at 0.45, 0.34 at 45° and 0.78 at 55°
        // (0.64 and 0.10 at 30 and 10 frames a second).
        for (grade, lag) in [
            (0.05_f32, 0.01),
            (0.19, 0.01),
            (0.45, 0.05),
            (0.99, 0.35),
            (steepest, 0.8),
        ] {
            for fps in [60_u16, 30, 10] {
                let over = most_over_the_ramp(-grade, fps);
                assert!(
                    over < lag,
                    "down {grade} at {fps} fps: {over} over the floor, against the fall's lag of {lag}"
                );
            }
        }
        // Held at the reported height, as before, it loses the floor two
        // seconds into a 19% climb and is drawn 4.5 under the hill.
        let world = ramp(0.19);
        let soles = glam::Vec3::from_array(super::render_position(super::WorldPosition {
            x: 11.8 * 2.0,
            ..Default::default()
        }));
        assert!((super::grounded_soles(&world, soles, 6.0) - soles.y).abs() < 0.001);
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
        let standing = |x: f32| super::DrawnSoles {
            height: 2.0,
            spot: glam::Vec2::new(x, 0.0),
            fall: Some(0.0),
        };
        // A long frame: the reach up takes in the slab, yet it stays on the
        // deck.
        let on = super::carried_soles(
            &world,
            Some(standing(0.0)),
            glam::Vec3::new(2.0, 2.0, 0.0),
            3.75,
            FALL,
            3.0,
        );
        assert!(
            (on.height - 2.0).abs() < 0.001 && on.fall == Some(0.0),
            "{on:?}"
        );
        // Off the deck's end, it falls to the ground under it.
        let mut drawn = super::carried_soles(
            &world,
            Some(standing(19.0)),
            glam::Vec3::new(21.0, 2.0, 0.0),
            3.75,
            FALL,
            1.0 / 3.0,
        );
        assert!(
            drawn.height < 2.0 && drawn.fall.is_some_and(|speed| speed < 0.0),
            "{drawn:?}"
        );
        for _ in 0..30 {
            drawn = super::carried_soles(
                &world,
                Some(drawn),
                glam::Vec3::new(21.0, 2.0, 0.0),
                3.75,
                FALL,
                1.0 / 3.0,
            );
        }
        assert!(
            drawn.height.abs() < 0.001 && drawn.fall == Some(0.0),
            "{drawn:?}"
        );
    }

    #[test]
    fn a_carried_spawn_off_a_drop_reaches_the_floor_within_a_report_gap() {
        // Pair 967 of the 11-01 P99 sitting: on the floor at a report, then a
        // 16-unit drop. Under the zone's default gravity it lands well inside
        // the report's 1 s gap, at 60 and at 10 frames a second, where the
        // carried height stayed 16 over it.
        let plate = |y: f32, from: f32, to: f32| {
            [
                [[from, y, -20.0], [to, y, -20.0], [to, y, 20.0]],
                [[from, y, -20.0], [to, y, 20.0], [from, y, 20.0]],
            ]
        };
        let world = crate::movement::CollisionWorld::new(
            [plate(16.0, -20.0, 0.0), plate(0.0, 0.0, 40.0)].concat(),
        )
        .unwrap();
        for fps in [60_u16, 10] {
            let mut drawn = None;
            let mut landed = None;
            for frame in 0..=fps {
                let t = f32::from(frame) / f32::from(fps);
                // Running 30 units a second off the edge at x 0, carried at
                // the report's height.
                let soles = glam::Vec3::new(-5.0 + 30.0 * t, 16.0, 0.0);
                let next =
                    super::carried_soles(&world, drawn, soles, 3.75, FALL, 20.0 / f32::from(fps));
                if landed.is_none() && soles.x > 0.0 && next.height.abs() < 0.001 {
                    landed = Some(t);
                }
                drawn = Some(next);
            }
            assert!(
                landed.is_some_and(|t| t < 0.7),
                "{fps} fps: landed at {landed:?}"
            );
        }
    }

    #[test]
    fn a_carried_runner_reported_over_its_floor_falls_onto_it_and_keeps_to_it() {
        // Pair 967 of the 11-01 P99 sitting: a size 4.5 player running 30
        // units a second down a 27° slope, reported with its soles 1.4 over
        // our floor and carried at that height. It falls onto the floor and
        // follows it down, where the report's height held it 16 over it.
        let grade = -27.0_f32.to_radians().tan();
        let world = ramp(grade);
        let size = 4.5;
        for fps in [60_u16, 10] {
            let mut drawn = None;
            let mut over = 0.0_f32;
            for frame in 0..=fps {
                let t = f32::from(frame) / f32::from(fps);
                let x = 30.0 * t;
                let soles = glam::Vec3::from_array(super::render_position(super::WorldPosition {
                    x,
                    y: 0.0,
                    z: 1.4,
                    heading: 0.0,
                }));
                let next = super::carried_soles(
                    &world,
                    drawn,
                    soles,
                    super::drawn_feet_offset(size * 0.2),
                    FALL,
                    20.0 / f32::from(fps),
                );
                let floor = super::render_position(super::WorldPosition {
                    x,
                    y: 0.0,
                    z: grade * x,
                    heading: 0.0,
                })[1];
                assert!(
                    next.height >= floor - 0.01,
                    "{fps} fps at {t:.2} s: {next:?} under {floor}"
                );
                if t > 0.3 {
                    over = over.max(next.height - floor);
                }
                drawn = Some(next);
            }
            assert!(over < 1.5, "{fps} fps: {over} over the floor after 0.3 s");
        }
    }

    #[test]
    fn a_carried_flyer_stays_at_its_reported_height() {
        let world = ramp(0.0);
        let soles = glam::Vec3::Y * 9.0;
        let first = super::carried_soles(&world, None, soles, 3.75, FALL, 0.0);
        assert!(
            (first.height - 9.0).abs() < 0.001 && first.fall.is_none(),
            "{first:?}"
        );
        let on = glam::Vec3::new(1.0, 9.0, 0.0);
        let next = super::carried_soles(&world, Some(first), on, 3.75, FALL, 1.0 / 3.0);
        assert!(
            (next.height - 9.0).abs() < 0.001 && next.fall.is_none(),
            "{next:?}"
        );
        // Come down within reach of the floor, it falls onto it.
        let low = glam::Vec3::new(2.0, 3.0, 0.0);
        let mut drawn = super::carried_soles(&world, Some(next), low, 3.75, FALL, 1.0 / 3.0);
        assert!(
            (drawn.height - 3.0).abs() < 0.001 && drawn.fall == Some(0.0),
            "{drawn:?}"
        );
        for _ in 0..20 {
            drawn = super::carried_soles(&world, Some(drawn), low, 3.75, FALL, 1.0 / 3.0);
        }
        assert!(
            drawn.height.abs() < 0.001 && drawn.fall == Some(0.0),
            "{drawn:?}"
        );
    }

    /// Spawn 119 of the 11-01 local `EQEmu` look, carried from a report
    /// along its heading at its walk for one of its two long gaps, at 60
    /// frames a second, over the floor Live's floor run found under its
    /// carried spot (npc-motion/floor-run-eqemu-1316Z-119.txt): its height
    /// at each twentieth of the gap, up to where the collision ends, between
    /// the last twentieth with a floor and the first without. Past that edge
    /// is a surface `above` the last floor, if any, in a zone whose top is
    /// `top` and whose underworld is -500. Gives where its soles were drawn
    /// at the gap's end, the lowest they were drawn, and that last floor.
    fn spawn_119(gap: usize, above: Option<f32>, top: f32) -> (super::DrawnSoles, f32, f32) {
        // From the report, at the report's height, to where the walk took it
        // after the gap's seconds (EQ x, y, z).
        let (x0, y0, z0, x1, y1, seconds) = [
            (
                -989.0_f32,
                1213.1_f32,
                15.2_f32,
                -1415.7_f32,
                1499.7_f32,
                49.094_f32,
            ),
            (-1476.9, 1291.6, 2.4, -2108.0, 1295.5, 60.278),
        ][gap];
        let floors: &[f32] = [
            &[
                13.5, 12.3, 11.2, 11.1, 10.1, 9.1, 8.6, 9.0, 10.3, 12.3, 19.7, 33.5, 49.2, 62.7,
                75.2, 85.4, 95.2,
            ][..],
            &[-4.3; 14][..],
        ][gap];
        #[allow(clippy::cast_precision_loss)]
        let edge = (floors.len() as f32 - 0.6) / 20.0;
        let feet = 3.75;
        let render = |x: f32, y: f32, z: f32| {
            super::render_position(super::WorldPosition {
                x,
                y,
                z,
                heading: 0.0,
            })
        };
        let along = |share: f32| (x0 + (x1 - x0) * share, y0 + (y1 - y0) * share);
        // A strip 40 wide along the path, from `from` to `to` of the way,
        // rising from one height to another.
        let (dx, dy) = (x1 - x0, y1 - y0);
        let length = dx.hypot(dy);
        let (nx, ny) = (-dy / length * 20.0, dx / length * 20.0);
        let strip = |(from, low): (f32, f32), (to, high): (f32, f32)| {
            let ((ax, ay), (bx, by)) = (along(from), along(to));
            let (a, b, c, d) = (
                render(ax + nx, ay + ny, low),
                render(ax - nx, ay - ny, low),
                render(bx - nx, by - ny, high),
                render(bx + nx, by + ny, high),
            );
            [[a, b, c], [a, c, d]]
        };
        let last = floors[floors.len() - 1];
        #[allow(clippy::cast_precision_loss)]
        let mut points: Vec<(f32, f32)> = floors
            .iter()
            .enumerate()
            .map(|(at, &height)| (at as f32 / 20.0, height))
            .collect();
        points[0].0 = -0.05;
        points.push((edge, last));
        let mut triangles: Vec<_> = points
            .windows(2)
            .flat_map(|pair| strip(pair[0], pair[1]))
            .collect();
        if let Some(above) = above {
            triangles.extend(strip((edge, last + above), (1.05, last + above)));
        }
        let world = crate::movement::CollisionWorld::new(triangles).unwrap();
        let fall = super::CarriedFall {
            underworld: Some(super::UnderworldReturn {
                underworld: -500.0,
                highest: top - 10.0,
                search_from: 32_000.0,
            }),
            ..FALL
        };
        let fps = 60.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let frames = (seconds * fps) as u32;
        let soles_z = z0 - feet;
        let mut drawn = None;
        let mut lowest = f32::MAX;
        for frame in 0..=frames {
            #[allow(clippy::cast_precision_loss)]
            let (x, y) = along(frame as f32 / fps / seconds);
            let soles = glam::Vec3::from_array(render(x, y, soles_z));
            let next = super::carried_soles(&world, drawn, soles, feet, fall, 20.0 / fps);
            lowest = lowest.min(next.height);
            drawn = Some(next);
        }
        (drawn.unwrap(), lowest, last)
    }

    #[test]
    fn a_carried_spawn_fallen_under_the_zone_is_put_back_on_its_top_surface() {
        // Spawn 119's gaps, with a surface 60 over the floor past the edge:
        // it falls where its floor ends, comes to the zone's underworld well
        // before the next report, and is put on that surface, which it then
        // stands on.
        for gap in [0, 1] {
            let (drawn, lowest, last) = spawn_119(gap, Some(60.0), 10_000.0);
            assert!(lowest < -490.0, "gap {gap}: lowest {lowest}");
            assert!(
                (drawn.height - (last + 60.0)).abs() < 0.01 && drawn.fall == Some(0.0),
                "gap {gap}: {drawn:?}"
            );
            // Within 10 of the zone's top, that surface is too high: it is
            // left be, falling on.
            let (drawn, _, _) = spawn_119(gap, Some(60.0), last + 65.0);
            assert!(
                drawn.height < -500.0 && drawn.fall.is_some_and(|speed| speed < 0.0),
                "gap {gap}: {drawn:?}"
            );
        }
    }

    #[test]
    fn a_carried_spawn_walked_off_the_world_falls_on() {
        // Spawn 119 in the look: past the collision's edge there is no
        // surface over or under its carried spot, so nothing puts it back.
        // The program's search finds none either and puts it at -1e27, so
        // it is lost from view on both clients (6.362).
        for gap in [0, 1] {
            let (drawn, lowest, _) = spawn_119(gap, None, 10_000.0);
            assert!(
                drawn.height < -500.0 && (drawn.height - lowest).abs() < 0.001,
                "gap {gap}: {drawn:?}"
            );
        }
        // Without a zone header, nothing puts back a spawn under the zone.
        let world = ramp(0.0);
        let falling = super::DrawnSoles {
            height: -600.0,
            spot: glam::Vec2::ZERO,
            fall: Some(-8.0),
        };
        let soles = glam::Vec3::new(0.0, -600.0, 0.0);
        let drawn = super::carried_soles(&world, Some(falling), soles, 3.75, FALL, 1.0);
        assert!(drawn.height < -600.0, "{drawn:?}");
        // With one, it is put on the floor over it.
        let fall = super::CarriedFall {
            underworld: Some(super::UnderworldReturn {
                underworld: -500.0,
                highest: 990.0,
                search_from: 32_000.0,
            }),
            ..FALL
        };
        // Searched from 32,000, the height is good to a few thousandths;
        // the next step's landing stands it on the floor.
        let drawn = super::carried_soles(&world, Some(falling), soles, 3.75, fall, 1.0);
        assert!(drawn.height.abs() < 0.01, "{drawn:?}");
        let drawn = super::carried_soles(&world, Some(drawn), soles, 3.75, fall, 1.0);
        assert!(
            drawn.height.abs() < 0.001 && drawn.fall == Some(0.0),
            "{drawn:?}"
        );
    }

    #[test]
    fn a_correction_slides_evenly_onto_the_carried_point_within_a_tenth_of_a_second() {
        // A runner at 30 units a second, carried on with no report: drawn at
        // its carried point each frame, never behind it.
        let mut none = super::Correction::default();
        for frame in 0..60_u8 {
            let target = glam::Vec3::X * 30.0 * f32::from(frame) / 60.0;
            assert!(none.advance(target, 1.0 / 60.0).distance(target) < 1e-6);
        }
        // A report 4 units ahead of where it was drawn: drawn where it was at
        // first, half way at 0.05 s, on the carried point from 0.1 s.
        let drawn = glam::Vec3::new(10.0, 0.0, 0.0);
        let target = glam::Vec3::new(14.0, 0.0, 0.0);
        let mut correction = super::Correction::new(drawn, target);
        assert!(correction.advance(target, 0.0).distance(drawn) < 1e-6);
        let half = correction.advance(target, 0.05);
        assert!(
            half.distance(glam::Vec3::new(12.0, 0.0, 0.0)) < 1e-4,
            "{half}"
        );
        let moved = target + glam::Vec3::X * 1.5;
        assert!(correction.advance(moved, 0.05).distance(moved) < 1e-4);
        assert!(correction.advance(moved, 1.0).distance(moved) < 1e-6);
    }

    #[test]
    fn remote_models_stand_on_a_floor_near_their_soles() {
        let floor = [
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ];
        let world = crate::movement::CollisionWorld::new(floor).unwrap();
        let soles = |y: f32| super::grounded_soles(&world, glam::Vec3::Y * y, 4.0);
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
    }

    /// The soles on the ramp at a world x and y, in renderer coordinates.
    fn on_ramp(grade: f32, x: f32, y: f32) -> glam::Vec3 {
        glam::Vec3::from_array(super::render_position(super::WorldPosition {
            x,
            y,
            z: grade * x,
            heading: 0.0,
        }))
    }

    #[test]
    fn a_corpse_pitches_to_the_slope_it_lies_along() {
        // A size 6 skeleton lying 6 behind its soles, facing up the ramp
        // (world +x, the renderer's -Z), then down it.
        for grade in [0.1_f32, 0.3, 0.45, -0.45] {
            let world = ramp(grade);
            let slope = grade.atan();
            let soles = on_ramp(grade, 0.0, 0.0);
            for (yaw, raised) in [(std::f32::consts::PI, slope), (0.0, -slope)] {
                let tilt = super::corpse_tilt(&world, soles, yaw, -6.0, 6.0);
                // A negative pitch raises the way it faces.
                assert!(
                    (-tilt.pitch - raised).abs() < 1.0_f32.to_radians(),
                    "grade {grade}, yaw {yaw}: {tilt:?}"
                );
                assert!(tilt.roll.abs() < 1e-4, "{tilt:?}");
                // Turned by it, the way it faces runs up the slope as the
                // floor does.
                let facing = glam::Quat::from_rotation_y(yaw) * tilt.rotation() * glam::Vec3::Z;
                let along = glam::Vec2::new(facing.x, facing.z).length();
                assert!(((facing.y / along).atan() - raised).abs() < 1.0_f32.to_radians());
            }
            // Lying ahead of its soles instead, on an even slope, it leans
            // the same.
            let ahead = super::corpse_tilt(&world, soles, std::f32::consts::PI, 6.0, 6.0);
            assert!(
                (-ahead.pitch - slope).abs() < 1.0_f32.to_radians(),
                "grade {grade}: {ahead:?}"
            );
        }
        // A slope steeper than 35° is followed only so far.
        let world = ramp(0.9);
        let tilt = super::corpse_tilt(&world, glam::Vec3::ZERO, std::f32::consts::PI, -6.0, 6.0);
        assert!((tilt.pitch + super::MOST_TILT).abs() < 1e-5, "{tilt:?}");
    }

    #[test]
    fn a_corpse_samples_the_floor_where_its_body_lies() {
        // Level ground behind a crest at world x = 0, falling 0.3 a unit
        // past it; the corpse faces over the crest (world +x).
        let grade = 0.3_f32;
        let corner = |x: f32, y: f32| {
            super::render_position(super::WorldPosition {
                x,
                y,
                z: if x > 0.0 { -grade * x } else { 0.0 },
                heading: 0.0,
            })
        };
        let world = crate::movement::CollisionWorld::new([
            [corner(-50.0, -20.0), corner(0.0, -20.0), corner(0.0, 20.0)],
            [corner(-50.0, -20.0), corner(0.0, 20.0), corner(-50.0, 20.0)],
            [corner(0.0, -20.0), corner(50.0, -20.0), corner(50.0, 20.0)],
            [corner(0.0, -20.0), corner(50.0, 20.0), corner(0.0, 20.0)],
        ])
        .unwrap();
        let yaw = std::f32::consts::PI;
        // Lying behind, on the level, it lies level; lying ahead, over the
        // crest, it pitches down the fall.
        let behind = super::corpse_tilt(&world, glam::Vec3::ZERO, yaw, -6.0, 6.0);
        assert!(behind.upright(), "{behind:?}");
        let ahead = super::corpse_tilt(&world, glam::Vec3::ZERO, yaw, 6.0, 6.0);
        assert!(
            (ahead.pitch - grade.atan()).abs() < 1.0_f32.to_radians(),
            "{ahead:?}"
        );
        assert!(ahead.roll.abs() < 1e-4, "{ahead:?}");
    }

    #[test]
    fn a_corpse_rolls_to_a_slope_across_it() {
        // Facing world +y, the ramp rises to its X side.
        for grade in [0.2_f32, -0.4] {
            let world = ramp(grade);
            let yaw = std::f32::consts::FRAC_PI_2;
            let tilt = super::corpse_tilt(&world, on_ramp(grade, 0.0, 0.0), yaw, -6.0, 6.0);
            assert!(
                (tilt.roll - grade.atan()).abs() < 1.0_f32.to_radians(),
                "grade {grade}: {tilt:?}"
            );
            assert!(tilt.pitch.abs() < 1e-4, "{tilt:?}");
        }
    }

    #[test]
    fn a_corpse_on_flat_ground_lies_level() {
        let world = ramp(0.0);
        for yaw in [0.0_f32, 0.7, 2.0, -2.9] {
            let tilt = super::corpse_tilt(&world, glam::Vec3::ZERO, yaw, -5.0, 6.0);
            assert!(tilt.upright(), "yaw {yaw}: {tilt:?}");
            assert_eq!(tilt.rotation(), glam::Quat::IDENTITY);
        }
    }

    #[test]
    fn a_corpse_missing_a_floor_sample_stays_upright_on_that_axis() {
        let grade = 0.3;
        let world = ramp(grade);
        // Behind its soles, where its body lies, the ramp has ended: no
        // pitch, though it slopes; lying ahead instead, it has the floor.
        let edge = on_ramp(grade, -99.5, 0.0);
        let tilt = super::corpse_tilt(&world, edge, std::f32::consts::PI, -6.0, 6.0);
        assert!(tilt.upright(), "{tilt:?}");
        let tilt = super::corpse_tilt(&world, edge, std::f32::consts::PI, 6.0, 6.0);
        assert!(
            (-tilt.pitch - grade.atan()).abs() < 1.0_f32.to_radians(),
            "{tilt:?}"
        );
        // With only its middle on the floor it still pitches, fitted to
        // the soles and the middle.
        let tilt = super::corpse_tilt(
            &world,
            on_ramp(grade, -96.5, 0.0),
            std::f32::consts::PI,
            -6.0,
            6.0,
        );
        assert!(
            (-tilt.pitch - grade.atan()).abs() < 1.0_f32.to_radians(),
            "{tilt:?}"
        );
        // Facing along that edge, one side is off the ramp: no roll, though
        // the ramp rises to its side.
        let tilt = super::corpse_tilt(&world, edge, std::f32::consts::FRAC_PI_2, -6.0, 6.0);
        assert!(tilt.upright(), "{tilt:?}");
        // Facing up the ramp by its side edge, the pitch still follows it
        // with a side off the ramp.
        let along = on_ramp(grade, 0.0, 19.5);
        let tilt = super::corpse_tilt(&world, along, std::f32::consts::PI, -6.0, 6.0);
        assert!(
            (-tilt.pitch - grade.atan()).abs() < 1.0_f32.to_radians(),
            "{tilt:?}"
        );
        assert!(tilt.roll.abs() < f32::EPSILON, "{tilt:?}");
        // With no floor at its soles, it stays upright.
        let over = on_ramp(grade, 0.0, 0.0) + glam::Vec3::Y * 9.0;
        let tilt = super::corpse_tilt(&world, over, std::f32::consts::PI, -6.0, 6.0);
        assert_eq!(tilt, super::Tilt::default());
        // A model with no death clip to measure has no length to lie along.
        let tilt = super::corpse_tilt(&world, on_ramp(grade, 0.0, 0.0), 0.0, 0.0, 6.0);
        assert!(tilt.upright());
    }

    #[test]
    fn a_falling_corpse_takes_a_share_of_its_lean() {
        let tilt = super::Tilt {
            pitch: -0.4,
            roll: 0.2,
        };
        assert!(tilt.share(0.0).upright());
        assert_eq!(
            tilt.share(0.5),
            super::Tilt {
                pitch: -0.2,
                roll: 0.1
            }
        );
        assert_eq!(tilt.share(1.0), tilt);
        assert_eq!(tilt.share(3.0), tilt);
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
