#![doc = "Command-line entry point for the offline EQ zone viewer."]

mod logins;
mod presets;
mod session;

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use eq_client_assets::{ZoneAsset, ui::InstalledClient};
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

    /// Hide your own character's helm, as the official client's Show My Helm
    /// option does; other characters always show theirs. A character's own
    /// choice in the Options window's Display page wins over this.
    #[arg(long)]
    hide_own_helm: bool,

    /// The most frames a second the client draws; 0 leaves it to vsync, which
    /// is the monitor's refresh rate. Wins over `eqclient.ini`'s `MaxFPS`;
    /// the Options window's Max FPS wins over both once the character sets
    /// it. Without either, 60.
    #[arg(long)]
    max_fps: Option<u32>,

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
    /// Preview loot, merchant and trade windows with synthetic items and no network connection.
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

    /// Log in at once with `EQ_ACCOUNT` and `EQ_PASSWORD`, skipping the login
    /// screen, on the launch's login server: `--preset`, or else the first
    /// preset of the `EQ_PROTOCOL` type (P99 by default), at `EQ_LOGIN_HOST`
    /// and `EQ_LOGIN_PORT` where set.
    #[arg(long)]
    online: bool,

    /// Open the offline zone viewer instead of the login screen.
    #[arg(long, conflicts_with = "online")]
    offline: bool,

    /// The login server to start on, by its preset's name in
    /// `login-servers.txt` in the settings directory; without it, the one
    /// last played.
    #[arg(long, env = "EQ_PRESET")]
    preset: Option<String>,

    /// The world to play on, skipping the login server's list, for sessions
    /// on the launch's login server.
    #[arg(long, env = "EQ_SERVER")]
    server: Option<String>,

    /// The character to enter, skipping the world's list, for sessions on
    /// the launch's login server.
    #[arg(long, env = "EQ_CHARACTER")]
    character: Option<String>,

    /// JSON containing independently measured P99 movement calibration.
    #[arg(long)]
    movement_calibration: Option<PathBuf>,

    /// End each network session after this many seconds (including admission).
    #[arg(long)]
    session_seconds: Option<u64>,

    /// When hungry or thirsty, eat and drink whatever comes first, as the
    /// official client does. By default food and drink with modifiers are
    /// left to eat or drink by hand. A character's own choice on the Options
    /// window's quality-of-life page wins over this.
    #[arg(long)]
    auto_eat_anything: bool,

    /// Hide placed objects to inspect terrain and material transitions.
    #[arg(long)]
    terrain_only: bool,

    /// Attended key script (press/hold/wait/report/screenshot/quit), run only
    /// while the client window is focused, except offline or on a local
    /// `EQEmu` or TAKP server.
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

/// How a run starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The offline zone viewer.
    Offline,
    /// The login screen.
    Login,
    /// A session at once, with the environment's account and password.
    Online,
}

/// How a run starts: offline when asked, for a demo, or for a script or
/// screenshot that does not log in, as before the login screen; logged in
/// at once with `--online`; at the login screen otherwise, a script's
/// `login` step included.
fn mode(arguments: &Arguments, script: Option<&[Step]>) -> Result<Mode, &'static str> {
    let logs_in = script.is_some_and(|steps| steps.iter().any(|step| matches!(step, Step::Login)));
    let demo = arguments.demo_entities
        || arguments.demo_inventory
        || arguments.demo_spellbook
        || arguments.demo_character_select
        || arguments.demo_trade;
    if arguments.offline || arguments.inspect_only || demo {
        return if logs_in {
            Err("a script's login step needs the login screen, which this run leaves out")
        } else {
            Ok(Mode::Offline)
        };
    }
    if arguments.online {
        return if logs_in {
            Err("a script's login step types into the login screen, which --online skips")
        } else {
            Ok(Mode::Online)
        };
    }
    if !logs_in && (script.is_some() || arguments.screenshot.is_some()) {
        return Ok(Mode::Offline);
    }
    Ok(Mode::Login)
}

/// The preset a run begins on: the one named, else the first of the type
/// the environment names (P99 for `--online`, as before presets), else the
/// one last played, else the first.
fn launch_preset(
    presets: &mut presets::Presets,
    name: Option<&str>,
    environment: &presets::Endpoint,
    online: bool,
) -> Result<usize, String> {
    if let Some(name) = name {
        return presets.find(name).ok_or_else(|| {
            format!("--preset {name:?}: login-servers.txt has no login server of that name")
        });
    }
    let protocol = environment
        .protocol
        .or(online.then_some(ServerProtocol::Project1999));
    if let Some(protocol) = protocol {
        if let Some(index) = presets
            .list
            .iter()
            .position(|preset| preset.protocol == protocol)
        {
            return Ok(index);
        }
        let name = presets::seed_name(protocol).ok_or_else(|| {
            "EQ_PROTOCOL names a server type this client offers no login server of yet".to_owned()
        })?;
        presets.list.push(presets::Preset::new(name, protocol));
        return Ok(presets.list.len() - 1);
    }
    Ok(presets
        .last
        .as_deref()
        .and_then(|last| presets.find(last))
        .unwrap_or(0))
}

/// Refuses a script with local-only steps, `gm` and `chat`, unless the
/// session is local-only (see [`local_session`]), so `#` commands and a
/// script's chat can never reach P99, Quarm or a public server. Such a script
/// fails at launch rather than when it reaches the step.
fn check_local_steps(steps: Option<&[Step]>, local: bool) -> Result<(), &'static str> {
    let local_only = steps.is_some_and(|steps| {
        steps
            .iter()
            .any(|step| matches!(step, Step::Gm(_) | Step::Chat(_)))
    });
    if local_only && !local {
        return Err(concat!(
            "gm and chat script steps need --online with EQ_PROTOCOL=eqemu or takp ",
            "(a local EQEmu or TAKP server)"
        ));
    }
    Ok(())
}

/// A script on a stock server (`EQEmu` or TAKP) is a local test run: its
/// session refuses every server outside this machine's network, and only then
/// may it send `gm` steps and run without anyone watching the window. P99 and
/// Quarm scripts stay attended.
fn local_session(script: bool, protocol: Option<ServerProtocol>) -> bool {
    script && protocol.is_some_and(ServerProtocol::is_stock)
}

/// Startup problems print to stderr before the viewer exists; once it
/// runs, the session logs through `tracing` like the viewer.
fn main() {
    // A panic reaches the log, and with it the log file, as well as the
    // standard error stream.
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        tracing::error!("The client panicked: {panic}");
        report(panic);
    }));
    let mut arguments = Arguments::parse();
    let calibration = arguments
        .movement_calibration
        .as_deref()
        .map(load_calibration);
    // Validate every local input before the session logs in.
    let (script, script_follow) = script_input(&arguments);
    let mode = mode(&arguments, script.as_deref()).unwrap_or_else(|error| {
        eprintln!("error: {error}");
        std::process::exit(2);
    });
    let settings_directory = arguments
        .settings_dir
        .clone()
        .or_else(default_settings_directory);
    let start = (mode != Mode::Offline).then(|| {
        start_point(&arguments, mode, settings_directory.as_deref()).unwrap_or_else(|error| {
            eprintln!("error: {error}");
            std::process::exit(2);
        })
    });
    let protocol = start
        .as_ref()
        .map(|(presets, launch)| presets.list[launch.preset].protocol);
    let local = local_session(script.is_some(), protocol);
    if let Err(error) = check_local_steps(script.as_deref(), local) {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
    require_positive_distance(arguments.entity_distance);
    let launch_installation = start
        .as_ref()
        .and_then(|(presets, launch)| presets.list[launch.preset].installation.clone());
    let eq_directory = arguments
        .eq_dir
        .take()
        .or(launch_installation)
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
    let source = match start {
        None => Source::Offline(preview),
        Some(start) => online_source(
            start,
            mode,
            logins::Installation {
                directory: eq_directory.clone(),
                client: installed_client(protocol),
            },
            settings_directory.clone(),
            session_options(&arguments, &eq_directory, calibration, local),
        ),
    };
    println!("Controls: WASD moves; right-drag orbits; the wheel zooms.");
    let config = viewer_config(
        arguments,
        protocol,
        (eq_directory, settings_directory),
        (script, script_follow),
        local,
    );
    let exit = eq_client_render::run(zone, character, config, source);
    tracing::info!("The client ends with status {exit}");
    std::process::exit(exit);
}

/// The login screens, over the presets and the run's installation, and for
/// `--online` a session already logging in on the launch's preset.
fn online_source(
    (presets, launch): (presets::Presets, logins::Launch),
    mode: Mode,
    installation: logins::Installation,
    settings_directory: Option<PathBuf>,
    options: session::SessionOptions,
) -> Source {
    let preset = launch.preset;
    let launcher = logins::Launcher::new(
        presets,
        settings_directory,
        installation,
        options,
        (launch, std::env::args_os().skip(1).collect()),
    );
    let session = (mode == Mode::Online).then(|| launch_session(&launcher, preset));
    Source::Online {
        logins: Box::new(launcher),
        session,
    }
}

/// What every session of the run shares, whichever preset it logs in on.
fn session_options(
    arguments: &Arguments,
    install: &std::path::Path,
    calibration: Option<eq_client_core::MotionCalibration>,
    local_only: bool,
) -> session::SessionOptions {
    session::SessionOptions {
        install: install.to_path_buf(),
        seconds: arguments.session_seconds,
        calibration,
        local_only,
        auto_eat: if arguments.auto_eat_anything {
            eq_client_core::food::AutoEat::Anything
        } else {
            eq_client_core::food::AutoEat::Plain
        },
    }
}

/// The presets, and the launch's own preset, login server, world and
/// character.
fn start_point(
    arguments: &Arguments,
    mode: Mode,
    settings_directory: Option<&std::path::Path>,
) -> Result<(presets::Presets, logins::Launch), String> {
    let environment = presets::Endpoint::from_environment()?;
    let mut presets = presets::Presets::load(settings_directory, &environment);
    if presets.list.is_empty() {
        presets.list = presets::Presets::seeded(&environment).list;
    }
    let preset = launch_preset(
        &mut presets,
        arguments.preset.as_deref(),
        &environment,
        mode == Mode::Online,
    )?;
    let given = |value: &Option<String>| value.as_deref().unwrap_or_default().trim().to_owned();
    let launch = logins::Launch {
        preset,
        endpoint: environment,
        server: given(&arguments.server),
        character: given(&arguments.character),
    };
    Ok((presets, launch))
}

/// The session `--online` begins at launch, with the environment's account
/// and password; a run that cannot begin it ends before the window opens.
fn launch_session(launcher: &logins::Launcher, preset: usize) -> eq_client_render::Session {
    let variable = |name| {
        std::env::var(name).unwrap_or_else(|_| {
            eprintln!("error: --online needs {name} in the environment");
            std::process::exit(2);
        })
    };
    let (account, password) = (
        variable("EQ_ACCOUNT"),
        zeroize::Zeroizing::new(variable("EQ_PASSWORD")),
    );
    let Some(login) = launcher.login(preset, &account, &password) else {
        eprintln!("error: the launch's login server is missing");
        std::process::exit(2);
    };
    session::SessionWorker::start(login, launcher.options()).unwrap_or_else(|error| {
        eprintln!("Cannot start session: {error:#}");
        std::process::exit(1);
    })
}

/// Viewer settings from the command line and the validated script.
fn viewer_config(
    arguments: Arguments,
    protocol: Option<ServerProtocol>,
    (eq_directory, settings_directory): (PathBuf, Option<PathBuf>),
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
        installed_client: installed_client(protocol),
        entity_distance: Some(arguments.entity_distance),
        option_defaults: {
            let mut defaults = eq_client_core::options::Options {
                show_helm: !arguments.hide_own_helm,
                ..eq_client_core::options::Options::default()
            };
            defaults.qol.set(
                eq_client_core::qol::Fix::SkipModifiedFood,
                !arguments.auto_eat_anything,
            );
            defaults
        },
        max_fps: arguments
            .max_fps
            .map(|cap| u16::try_from(cap.min(1000)).unwrap_or(1000)),
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
        settings_directory,
        window_position: arguments.window_position,
        debug_overlay: arguments.debug_overlay,
    }
}

/// The official client a server's players install, whose own settings files
/// the viewer reads by that client's rules. Offline, the installation is
/// taken to be Titanium's, as the default `--eq-dir` is.
fn installed_client(protocol: Option<ServerProtocol>) -> InstalledClient {
    protocol.map_or(InstalledClient::Titanium, logins::installed_client)
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
        Arguments, InstalledClient, Mode, ServerProtocol, Step, check_local_steps, distance,
        finite, installed_client, launch_preset, load_script, local_session, mode,
        parse_window_position, presets, seconds,
    };
    use clap::Parser;

    #[test]
    fn titanium_servers_and_offline_runs_read_a_titanium_installation() {
        for protocol in [
            None,
            Some(ServerProtocol::EqEmu),
            Some(ServerProtocol::Project1999),
        ] {
            assert_eq!(installed_client(protocol), InstalledClient::Titanium);
        }
        for protocol in [ServerProtocol::Quarm, ServerProtocol::Takp] {
            assert_eq!(installed_client(Some(protocol)), InstalledClient::EqMac);
        }
    }

    #[test]
    fn a_run_opens_the_login_screen_unless_it_is_offline_online_or_an_older_script() {
        let run = |flags: &[&str], script: Option<&[Step]>| {
            let arguments = Arguments::try_parse_from(
                std::iter::once("eq-client").chain(flags.iter().copied()),
            )
            .unwrap();
            mode(&arguments, script)
        };
        let login = [Step::Login, Step::WaitServers];
        assert_eq!(run(&[], None), Ok(Mode::Login));
        assert_eq!(run(&[], Some(&login)), Ok(Mode::Login));
        assert_eq!(run(&["--online"], None), Ok(Mode::Online));
        for offline in [
            &["--offline"][..],
            &["--inspect-only"],
            &["--demo-inventory"],
            &["--screenshot", "a.png"],
        ] {
            assert_eq!(run(offline, None), Ok(Mode::Offline), "{offline:?}");
        }
        // Scripts written before the login screen still open the viewer.
        assert_eq!(run(&[], Some(&[Step::Face])), Ok(Mode::Offline));
        // A login step needs the screen it types into.
        assert!(run(&["--online"], Some(&login)).is_err());
        assert!(run(&["--offline"], Some(&login)).is_err());
        assert!(Arguments::try_parse_from(["eq-client", "--online", "--offline"]).is_err());
    }

    #[test]
    fn a_run_begins_on_the_named_preset_else_the_environments_type_else_the_last() {
        let none = presets::Endpoint::default();
        let mut presets = presets::Presets::seeded(&none);
        assert_eq!(
            launch_preset(&mut presets, Some("local takp"), &none, false),
            Ok(3)
        );
        assert!(launch_preset(&mut presets, Some("Nowhere"), &none, false).is_err());
        let eqemu = presets::Endpoint {
            protocol: Some(ServerProtocol::EqEmu),
            ..presets::Endpoint::default()
        };
        assert_eq!(launch_preset(&mut presets, None, &eqemu, false), Ok(2));
        // `--online` without a server type logs in on P99, as before presets.
        assert_eq!(launch_preset(&mut presets, None, &none, true), Ok(0));
        presets.last = Some("Project Quarm".into());
        assert_eq!(launch_preset(&mut presets, None, &none, false), Ok(1));
        presets.last = None;
        assert_eq!(launch_preset(&mut presets, None, &none, false), Ok(0));
        // A type the player removed comes back as its seed.
        presets
            .list
            .retain(|preset| preset.protocol != ServerProtocol::EqEmu);
        assert_eq!(launch_preset(&mut presets, None, &eqemu, false), Ok(3));
        assert_eq!(presets.list[3].protocol, ServerProtocol::EqEmu);
    }

    #[test]
    fn only_scripts_on_stock_servers_are_local_sessions() {
        assert!(local_session(true, Some(ServerProtocol::EqEmu)));
        assert!(local_session(true, Some(ServerProtocol::Takp)));
        assert!(!local_session(true, Some(ServerProtocol::Project1999)));
        assert!(!local_session(true, Some(ServerProtocol::Quarm)));
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
    fn gm_and_chat_steps_are_refused_outside_a_local_session() {
        let chat = Step::Chat(eq_client_core::OutboundChat::Say("Hail".into()));
        for steps in [[Step::Gm("summon".into())], [chat]] {
            assert!(check_local_steps(Some(&steps), false).is_err());
            assert!(check_local_steps(Some(&steps), true).is_ok());
        }
        assert!(check_local_steps(Some(&[Step::Face]), false).is_ok());
        assert!(check_local_steps(None, false).is_ok());
    }

    #[test]
    fn window_positions_accept_negative_multi_monitor_coordinates() {
        assert_eq!(parse_window_position("2592,-980"), Ok((2592, -980)));
        assert_eq!(parse_window_position(" -40 , 32 "), Ok((-40, 32)));
        assert!(parse_window_position("32").is_err());
        assert!(parse_window_position("x,1").is_err());
    }
}
