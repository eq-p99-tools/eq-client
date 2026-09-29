#![doc = "Command-line entry point for the offline EQ zone viewer."]

mod session;

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use eq_client_assets::ZoneAsset;
use eq_client_core::WorldPosition;
use eq_client_render::{ProjectionStyle, ValidationAction, ViewerConfig, script::Step};
use eq_network::client::ServerProtocol;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CameraStyle {
    Perspective,
    Orthographic,
}

#[derive(Debug, Parser)]
#[command(version, about)]
#[allow(clippy::struct_excessive_bools)] // Independent CLI switches, with clap enforcing incompatible modes.
struct Arguments {
    /// Path to a locally installed `EverQuest` client.
    #[arg(long, env = "EQ_CLIENT_DIR")]
    eq_dir: Option<PathBuf>,

    /// Zone short name to load from `<zone>.s3d`.
    #[arg(long, default_value = "ecommons")]
    zone: String,

    /// Camera projection used by the offline viewer.
    #[arg(long, value_enum, default_value_t = CameraStyle::Perspective)]
    camera: CameraStyle,

    /// Validate and summarize the zone without opening a window.
    #[arg(long)]
    inspect_only: bool,

    /// Save a rendered frame to this PNG path, then exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,

    /// Wait this many seconds after readiness before capturing a screenshot.
    #[arg(long, requires = "screenshot")]
    screenshot_after: Option<f32>,

    /// Initial EQ X coordinate; must be paired with `--start-y`.
    #[arg(long, requires = "start_y", allow_negative_numbers = true)]
    start_x: Option<f32>,

    /// Initial EQ Y coordinate; must be paired with `--start-x`.
    #[arg(long, requires = "start_x", allow_negative_numbers = true)]
    start_y: Option<f32>,

    /// Initial camera distance from the character.
    #[arg(long)]
    camera_distance: Option<f32>,

    /// Radius in EQ units for nearby players and creatures (maximum 200 rendered).
    #[arg(long, default_value = "200")]
    entity_distance: f32,

    /// Show synthetic moving entities offline, without connecting to a server.
    #[arg(long, conflicts_with = "online")]
    demo_entities: bool,
    /// Open a synthetic inventory preview without connecting to a server.
    #[arg(long, conflicts_with = "online")]
    demo_inventory: bool,
    /// Include personal bank storage in the offline inventory preview.
    #[arg(long, requires = "demo_inventory", conflicts_with = "online")]
    demo_bank: bool,
    /// Open a synthetic spellbook preview without connecting to a server.
    #[arg(long, conflicts_with = "online")]
    demo_spellbook: bool,
    /// Preview character selection with synthetic names and no network connection.
    #[arg(long, conflicts_with = "online")]
    demo_character_select: bool,
    /// Preview loot and merchant windows with synthetic items and no network connection.
    #[arg(long, conflicts_with = "online")]
    demo_trade: bool,

    /// Select the nearest rendered player once, without moving or attacking.
    #[arg(long)]
    target_nearest_player_once: bool,

    /// Inspect the first genuine item link received in chat once, for live validation.
    #[arg(
        long,
        requires = "online",
        conflicts_with = "target_nearest_player_once"
    )]
    inspect_first_chat_item_once: bool,

    /// Classic character model code for the offline preview (for example HUM or HUF).
    #[arg(long, default_value = "HUM")]
    character_model: String,

    /// Connect using `EQ_ACCOUNT`, `EQ_PASSWORD`, `EQ_SERVER`, and `EQ_CHARACTER`.
    /// Select P99 (default) or Quarm with `EQ_PROTOCOL`.
    #[arg(long)]
    online: bool,

    /// JSON containing independently measured P99 movement calibration.
    #[arg(long, requires = "online")]
    movement_calibration: Option<PathBuf>,

    /// End the network session after this many seconds (including admission).
    #[arg(long, requires = "online")]
    session_seconds: Option<u64>,

    /// Hide placed objects to inspect terrain and material transitions.
    #[arg(long)]
    terrain_only: bool,

    /// Attended key script (press/hold/wait/report/screenshot/quit), run only
    /// while the client window is focused. Screenshots are saved beside it.
    #[arg(long)]
    script: Option<PathBuf>,

    /// Keep running steps appended to the script file until it quits or reaches
    /// the runtime limit.
    #[arg(long, requires = "script")]
    script_follow: bool,

    /// Top-left window corner as `X,Y` in physical desktop pixels (either may be
    /// negative on multi-monitor desktops).
    #[arg(long, value_parser = parse_window_position, allow_hyphen_values = true)]
    window_position: Option<(i32, i32)>,
}

fn parse_window_position(value: &str) -> Result<(i32, i32), String> {
    let (x, y) = value
        .split_once(',')
        .ok_or_else(|| "expected X,Y".to_owned())?;
    let coordinate = |text: &str| {
        text.trim()
            .parse::<i32>()
            .map_err(|error| format!("{text:?}: {error}"))
    };
    Ok((coordinate(x)?, coordinate(y)?))
}

/// The protocol an online session selects with `EQ_PROTOCOL` (P99 by default).
fn online_protocol(online: bool) -> Option<ServerProtocol> {
    online
        .then(|| {
            std::env::var("EQ_PROTOCOL")
                .unwrap_or_else(|_| "p99".into())
                .parse::<ServerProtocol>()
                .ok()
        })
        .flatten()
}

/// Refuses `gm` script steps unless the session is on a local `EQEmu` server, so
/// `#` commands can never reach P99 or Quarm.
fn check_gm_steps(steps: Option<&[Step]>, local_eqemu: bool) -> Result<(), &'static str> {
    let gm = steps.is_some_and(|steps| steps.iter().any(|step| matches!(step, Step::Gm(_))));
    if gm && !local_eqemu {
        return Err("gm script steps need --online with EQ_PROTOCOL=eqemu (a local EQEmu server)");
    }
    Ok(())
}

fn main() {
    let arguments = Arguments::parse();
    let calibration = arguments
        .movement_calibration
        .as_deref()
        .map(load_calibration);
    // Validate every local input before the session logs in.
    let (script, script_follow) = script_input(&arguments);
    let protocol = online_protocol(arguments.online);
    let local_eqemu = protocol == Some(ServerProtocol::EqEmu);
    if let Err(error) = check_gm_steps(script.as_deref(), local_eqemu) {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
    require_positive_distance(arguments.entity_distance);
    let eq_directory = arguments
        .eq_dir
        .or_else(default_eq_directory)
        .unwrap_or_else(|| {
            eprintln!("error: pass --eq-dir or set EQ_CLIENT_DIR");
            std::process::exit(2);
        });
    let zone = match eq_client_assets::load_zone(&eq_directory, &arguments.zone) {
        Ok(zone) => zone,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    };

    print_summary(&zone);
    if arguments.inspect_only {
        return;
    }

    let character = match eq_client_assets::characters::load_character(
        &eq_directory.join("global_chr.s3d"),
        &arguments.character_model,
    ) {
        Ok(character) => Some(character),
        Err(error) => {
            eprintln!("Character preview unavailable: {error}");
            None
        }
    };
    let (worker, updates) = if arguments.online {
        match session::SessionWorker::start(&eq_directory, arguments.session_seconds, calibration) {
            Ok((worker, receiver)) => (Some(worker), Some(receiver)),
            Err(error) => {
                eprintln!("Cannot start session: {error:#}");
                std::process::exit(1);
            }
        }
    } else {
        (None, None)
    };
    println!("Controls: WASD moves; right-drag orbits; the wheel zooms.");
    eq_client_render::run(
        zone,
        character,
        ViewerConfig {
            estimate_titanium_resources: protocol.is_some_and(ServerProtocol::is_titanium),
            projection: match arguments.camera {
                CameraStyle::Perspective => ProjectionStyle::Perspective,
                CameraStyle::Orthographic => ProjectionStyle::Orthographic,
            },
            screenshot: arguments.screenshot,
            screenshot_after: arguments.screenshot_after,
            start_position: arguments
                .start_x
                .zip(arguments.start_y)
                .map(|(x, y)| WorldPosition {
                    x,
                    y,
                    z: 0.0,
                    heading: 0.0,
                }),
            camera_distance: arguments.camera_distance,
            terrain_only: arguments.terrain_only,
            eq_directory: Some(eq_directory),
            entity_distance: Some(arguments.entity_distance),
            demo_entities: arguments.demo_entities,
            demo_inventory: arguments.demo_inventory,
            demo_bank: arguments.demo_bank,
            demo_spellbook: arguments.demo_spellbook,
            demo_character_select: arguments.demo_character_select,
            demo_trade: arguments.demo_trade,
            validation: if arguments.target_nearest_player_once {
                Some(ValidationAction::TargetNearestPlayer)
            } else if arguments.inspect_first_chat_item_once {
                Some(ValidationAction::InspectFirstItem)
            } else {
                None
            },
            script,
            script_follow,
            local_gm_commands: local_eqemu,
            window_position: arguments.window_position,
        },
        updates,
        worker.as_ref().map(session::SessionWorker::commands),
    );
}

fn require_positive_distance(distance: f32) {
    if !distance.is_finite() || distance <= 0.0 {
        eprintln!("error: --entity-distance must be a finite positive number");
        std::process::exit(2);
    }
}

/// Reads independently measured movement calibration, exiting when it is invalid.
fn load_calibration(path: &std::path::Path) -> eq_client_core::MotionCalibration {
    (|| {
        let bytes = std::fs::read(path)?;
        let calibration: eq_client_core::MotionCalibration = serde_json::from_slice(&bytes)?;
        calibration.validate()?;
        Ok::<_, anyhow::Error>(calibration)
    })()
    .unwrap_or_else(|error| {
        eprintln!("Invalid movement calibration: {error}");
        std::process::exit(2);
    })
}

/// The validated script and, when following, where to keep reading it.
type ScriptInput = (
    Option<Vec<eq_client_render::script::Step>>,
    Option<(PathBuf, usize)>,
);

fn script_input(arguments: &Arguments) -> ScriptInput {
    let Some(path) = arguments.script.as_deref() else {
        return (None, None);
    };
    let (steps, read) = load_script(path);
    (
        Some(steps),
        arguments.script_follow.then(|| (path.to_path_buf(), read)),
    )
}

/// Reads and validates an attended key script, exiting on any invalid line;
/// also returns how many bytes were read, where following resumes.
fn load_script(path: &std::path::Path) -> (Vec<eq_client_render::script::Step>, usize) {
    std::fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            eq_client_render::script::parse(
                &text,
                path.parent().unwrap_or_else(|| std::path::Path::new(".")),
            )
            .map(|steps| (steps, text.len()))
        })
        .unwrap_or_else(|error| {
            eprintln!("Invalid script: {error}");
            std::process::exit(2);
        })
}

fn default_eq_directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("PROGRAMFILES(X86)")
            .map(PathBuf::from)
            .map(|directory| directory.join("Sony").join("EverQuest"))
            .filter(|directory| directory.is_dir())
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

fn print_summary(zone: &ZoneAsset) {
    println!("Zone: {}", zone.short_name);
    println!("Primitives: {}", zone.primitives.len());
    println!("Triangles: {}", zone.triangle_count());
    println!("Textures: {}", zone.textures.len());
    println!("Object models: {}", zone.models.len());
    println!("Placed objects: {}", zone.objects.len());
    if let Some((min, max)) = zone.bounds() {
        println!("Bounds: {min:?} to {max:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::{Step, check_gm_steps, parse_window_position};

    #[test]
    fn gm_steps_are_refused_outside_a_local_eqemu_session() {
        let gm = [Step::Gm("summon".into())];
        assert!(check_gm_steps(Some(&gm), false).is_err());
        assert!(check_gm_steps(Some(&gm), true).is_ok());
        assert!(check_gm_steps(Some(&[Step::Face]), false).is_ok());
        assert!(check_gm_steps(None, false).is_ok());
    }

    #[test]
    fn window_positions_accept_negative_multi_monitor_coordinates() {
        assert_eq!(parse_window_position("2592,-980"), Ok((2592, -980)));
        assert_eq!(parse_window_position(" -40 , 32 "), Ok((-40, 32)));
        assert!(parse_window_position("32").is_err());
        assert!(parse_window_position("x,1").is_err());
    }
}
