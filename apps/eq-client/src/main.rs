#![doc = "Command-line entry point for the offline EQ zone viewer."]

mod session;

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use eq_client_assets::ZoneAsset;
use eq_client_core::WorldPosition;
use eq_client_render::{
    Preview, ProjectionStyle, Source, ValidationAction, ViewerConfig, script::Step,
};
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
    #[arg(long, requires = "screenshot", value_parser = seconds)]
    screenshot_after: Option<f32>,

    /// Initial EQ X coordinate; must be paired with `--start-y`.
    #[arg(long, requires = "start_y", allow_negative_numbers = true, value_parser = finite)]
    start_x: Option<f32>,

    /// Initial EQ Y coordinate; must be paired with `--start-x`.
    #[arg(long, requires = "start_x", allow_negative_numbers = true, value_parser = finite)]
    start_y: Option<f32>,

    /// Initial EQ Z coordinate, as `/loc` reports it: the viewer stands on the
    /// floor below it instead of on the highest surface at X and Y.
    #[arg(long, requires = "start_x", allow_negative_numbers = true, value_parser = finite)]
    start_z: Option<f32>,

    /// Initial camera distance from the character.
    #[arg(long, value_parser = distance)]
    camera_distance: Option<f32>,

    /// Radius in EQ units for nearby players and creatures (maximum 200 rendered).
    #[arg(long, default_value = "200")]
    entity_distance: f32,

    /// Hide your own character's helm, as the official client's show-helm
    /// option does; other characters always show theirs.
    #[arg(long)]
    hide_own_helm: bool,

    /// The most frames a second the client draws; 0 leaves it to vsync, which
    /// is the monitor's refresh rate.
    #[arg(long, default_value = "60")]
    max_fps: u32,

    /// Add coordinates, the movement mode and the nearby-entity count to the
    /// status box, for development and live checks.
    #[arg(long)]
    debug_overlay: bool,

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

    /// When hungry or thirsty, eat and drink whatever comes first, as the
    /// official client does. By default food and drink with modifiers are
    /// left to eat or drink by hand. A character's own choice in the Options
    /// window's Client page wins over this.
    #[arg(long, requires = "online")]
    auto_eat_anything: bool,

    /// Hide placed objects to inspect terrain and material transitions.
    #[arg(long)]
    terrain_only: bool,

    /// Attended key script (press/hold/wait/report/screenshot/quit), run only
    /// while the client window is focused, except offline or on a local
    /// `EQEmu` server.
    /// Screenshots are saved beside it.
    #[arg(long, conflicts_with = "screenshot")]
    script: Option<PathBuf>,

    /// Keep running steps appended to the script file until it quits or reaches
    /// the runtime limit.
    #[arg(long, requires = "script")]
    script_follow: bool,

    /// UI skin under the installation's `uifiles` whose window layouts to use,
    /// instead of the one the character last chose in the official client.
    #[arg(long)]
    ui_skin: Option<String>,

    /// Directory for this client's own settings, such as window positions.
    /// Defaults to `eq-client` in the per-user settings directory.
    #[arg(long, env = "EQ_CLIENT_SETTINGS_DIR")]
    settings_dir: Option<PathBuf>,

    /// Top-left window corner as `X,Y` in physical desktop pixels (either may be
    /// negative on multi-monitor desktops).
    #[arg(long, value_parser = parse_window_position, allow_hyphen_values = true)]
    window_position: Option<(i32, i32)>,
}

/// A finite number, such as a coordinate.
fn finite(value: &str) -> Result<f32, String> {
    value
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| "expected a finite number".into())
}

/// A finite positive distance.
fn distance(value: &str) -> Result<f32, String> {
    let distance = finite(value)?;
    if distance > 0.0 {
        Ok(distance)
    } else {
        Err("expected a positive distance".into())
    }
}

/// A delay of at most a day, in seconds.
fn seconds(value: &str) -> Result<f32, String> {
    let seconds = finite(value)?;
    if (0.0..=86_400.0).contains(&seconds) {
        Ok(seconds)
    } else {
        Err("expected 0 to 86400 seconds".into())
    }
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

/// The protocol an online session selects with `EQ_PROTOCOL` (P99 by
/// default), read once for the whole run.
fn online_protocol(online: bool) -> Result<Option<ServerProtocol>, String> {
    if !online {
        return Ok(None);
    }
    let value = std::env::var("EQ_PROTOCOL").unwrap_or_else(|_| "p99".into());
    value
        .parse::<ServerProtocol>()
        .map(Some)
        .map_err(|error| format!("EQ_PROTOCOL={value:?}: {error}"))
}

/// Refuses `gm` script steps unless the session is local-only (see
/// [`local_session`]), so `#` commands can never reach P99, Quarm or a public server.
fn check_gm_steps(steps: Option<&[Step]>, local: bool) -> Result<(), &'static str> {
    let gm = steps.is_some_and(|steps| steps.iter().any(|step| matches!(step, Step::Gm(_))));
    if gm && !local {
        return Err("gm script steps need --online with EQ_PROTOCOL=eqemu (a local EQEmu server)");
    }
    Ok(())
}

/// A script on `EQEmu` is a local test run: its session refuses every server
/// outside this machine's network, and only then may it send `gm` steps and run
/// without anyone watching the window. P99 and Quarm scripts stay attended.
fn local_session(script: bool, protocol: Option<ServerProtocol>) -> bool {
    script && protocol == Some(ServerProtocol::EqEmu)
}

/// Startup problems print to stderr before the viewer exists; once it
/// runs, the session logs through `tracing` like the viewer.
fn main() {
    let mut arguments = Arguments::parse();
    let calibration = arguments
        .movement_calibration
        .as_deref()
        .map(load_calibration);
    // Validate every local input before the session logs in.
    let (script, script_follow) = script_input(&arguments);
    let protocol = online_protocol(arguments.online).unwrap_or_else(|error| {
        eprintln!("error: {error}");
        std::process::exit(2);
    });
    let local = local_session(script.is_some(), protocol);
    if let Err(error) = check_gm_steps(script.as_deref(), local) {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
    require_positive_distance(arguments.entity_distance);
    let eq_directory = arguments
        .eq_dir
        .take()
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
    let preview = Preview {
        entities: arguments.demo_entities,
        inventory: arguments.demo_inventory,
        bank: arguments.demo_bank,
        spellbook: arguments.demo_spellbook,
        character_select: arguments.demo_character_select,
        trade: arguments.demo_trade,
    };
    let (worker, source) = if let Some(protocol) = protocol {
        match session::SessionWorker::start(
            &eq_directory,
            protocol,
            arguments.session_seconds,
            calibration,
            local,
            if arguments.auto_eat_anything {
                eq_client_core::food::AutoEat::Anything
            } else {
                eq_client_core::food::AutoEat::Plain
            },
        ) {
            Ok((worker, updates)) => {
                let commands = worker.commands();
                (Some(worker), Source::Online { updates, commands })
            }
            Err(error) => {
                eprintln!("Cannot start session: {error:#}");
                std::process::exit(1);
            }
        }
    } else {
        (None, Source::Offline(preview))
    };
    println!("Controls: WASD moves; right-drag orbits; the wheel zooms.");
    let config = viewer_config(
        arguments,
        protocol,
        eq_directory,
        (script, script_follow),
        local,
    );
    let exit = eq_client_render::run(zone, character, config, source);
    // Close the session before exiting with the viewer's status.
    drop(worker);
    std::process::exit(exit);
}

/// Viewer settings from the command line and the validated script.
fn viewer_config(
    arguments: Arguments,
    protocol: Option<ServerProtocol>,
    eq_directory: PathBuf,
    (script, script_follow): ScriptInput,
    local: bool,
) -> ViewerConfig {
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
                z: arguments.start_z.unwrap_or(0.0),
                heading: 0.0,
            }),
        start_height_known: arguments.start_z.is_some(),
        camera_distance: arguments.camera_distance,
        terrain_only: arguments.terrain_only,
        eq_directory: Some(eq_directory),
        entity_distance: Some(arguments.entity_distance),
        hide_own_helm: arguments.hide_own_helm,
        option_defaults: eq_client_core::options::Options {
            skip_modified_food: !arguments.auto_eat_anything,
            ..eq_client_core::options::Options::default()
        },
        frame_rate_cap: (arguments.max_fps > 0).then_some(arguments.max_fps),
        validation: if arguments.target_nearest_player_once {
            Some(ValidationAction::TargetNearestPlayer)
        } else if arguments.inspect_first_chat_item_once {
            Some(ValidationAction::InspectFirstItem)
        } else {
            None
        },
        script,
        script_follow,
        local_session: local,
        ui_skin: arguments.ui_skin,
        settings_directory: arguments.settings_dir.or_else(default_settings_directory),
        window_position: arguments.window_position,
        debug_overlay: arguments.debug_overlay,
    }
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
    let (steps, read) = load_script(path, arguments.script_follow);
    (
        Some(steps),
        arguments.script_follow.then(|| (path.to_path_buf(), read)),
    )
}

/// Reads and validates an attended key script, exiting on any invalid line;
/// also returns how many bytes were read, where following resumes. A followed
/// script is read only through its last complete line, since its writer may be
/// partway through the next.
fn load_script(
    path: &std::path::Path,
    follow: bool,
) -> (Vec<eq_client_render::script::Step>, usize) {
    std::fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            let read = if follow {
                text.rfind('\n').map_or(0, |end| end + 1)
            } else {
                text.len()
            };
            // Windows editors may save UTF-8 with a byte-order mark.
            let complete = &text[..read];
            eq_client_render::script::parse(
                complete.strip_prefix('\u{feff}').unwrap_or(complete),
                path.parent().unwrap_or_else(|| std::path::Path::new(".")),
            )
            .map(|steps| (steps, read))
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

/// `eq-client` in the platform's per-user settings directory.
fn default_settings_directory() -> Option<PathBuf> {
    let variable = |name| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        variable("APPDATA")
    } else if cfg!(target_os = "macos") {
        variable("HOME").map(|home| home.join("Library").join("Application Support"))
    } else {
        variable("XDG_CONFIG_HOME").or_else(|| variable("HOME").map(|home| home.join(".config")))
    };
    base.map(|base| base.join("eq-client"))
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
    use super::{
        ServerProtocol, Step, check_gm_steps, distance, finite, load_script, local_session,
        parse_window_position, seconds,
    };

    #[test]
    fn only_eqemu_scripts_are_local_sessions() {
        assert!(local_session(true, Some(ServerProtocol::EqEmu)));
        assert!(!local_session(true, Some(ServerProtocol::Project1999)));
        assert!(!local_session(false, Some(ServerProtocol::EqEmu)));
        assert!(!local_session(true, None));
    }

    #[test]
    fn numbers_from_the_command_line_must_be_finite_and_in_range() {
        assert_eq!(finite("-12.5"), Ok(-12.5));
        assert!(finite("inf").is_err() && finite("NaN").is_err());
        assert!(distance("0").is_err() && distance("40").is_ok());
        assert!(seconds("1e20").is_err() && seconds("-1").is_err());
        assert_eq!(seconds("2.5"), Ok(2.5));
    }

    #[test]
    fn scripts_skip_a_byte_order_mark_and_followed_ones_a_partial_last_line() {
        let path = std::env::temp_dir().join(format!("eq-client-load-{}.txt", std::process::id()));
        std::fs::write(&path, "\u{feff}wait 10\nhold W 15").unwrap();
        let (followed, read) = load_script(&path, true);
        std::fs::write(&path, "\u{feff}wait 10\n").unwrap();
        let (whole, _) = load_script(&path, false);
        std::fs::remove_file(&path).unwrap();
        assert_eq!((followed.len(), read), (1, "\u{feff}wait 10\n".len()));
        assert_eq!(whole.len(), 1);
    }

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
