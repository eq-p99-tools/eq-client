//! The app's side of the login screen: the presets it offers, the sessions
//! it starts on them, and what it remembers of each. A run uses one
//! installation of the official client, so a preset whose server type
//! needs another opens the client again with that one.
use crate::{
    presets::{Endpoint, Preset, Presets},
    session::{Login, SessionOptions, SessionWorker},
};
use eq_client_assets::ui::InstalledClient;
use eq_client_render::{Availability, Connection, LoginServer, Logins};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

/// The installation a run uses, and the official client it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    /// Where it is.
    pub directory: PathBuf,
    /// Which official client it holds.
    pub client: InstalledClient,
}

/// The official client a server type's players install.
pub fn installed_client(protocol: eq_network::client::ServerProtocol) -> InstalledClient {
    if protocol.is_titanium() {
        InstalledClient::Titanium
    } else {
        InstalledClient::EqMac
    }
}

/// The official client the installation in `directory` holds, for a run at
/// the login screen: that of the presets naming it, the launch's own first.
/// One no preset names is taken to be Titanium's, as the standard
/// installation is and as an offline run takes any, so a preset of the
/// other client never logs in on it until the preset names its own.
pub fn client_at(presets: &Presets, launch: usize, directory: &Path) -> InstalledClient {
    holder(presets, launch, directory).map_or(InstalledClient::Titanium, |preset| {
        installed_client(preset.protocol)
    })
}

/// The preset that names the installation in `directory`: the launch's own,
/// else the first in the file.
fn holder<'a>(presets: &'a Presets, launch: usize, directory: &Path) -> Option<&'a Preset> {
    let names = |preset: &&Preset| preset.installation.as_deref() == Some(directory);
    presets
        .list
        .get(launch)
        .filter(names)
        .or_else(|| presets.list.iter().find(names))
}

/// What the login screen calls a client's installation.
const fn client_name(client: InstalledClient) -> &'static str {
    match client {
        InstalledClient::Titanium => "Titanium",
        InstalledClient::EqMac => "TAKP client",
    }
}

/// What the launch says about its own preset: the login server today's
/// environment names, which wins over the preset's for this run alone, and
/// the world and character to go straight to.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    /// The preset the run began on, by its place.
    pub preset: usize,
    /// `EQ_LOGIN_HOST` and `EQ_LOGIN_PORT`, where set.
    pub endpoint: Endpoint,
    /// `--server` or `EQ_SERVER`: the world, skipping the login server's list.
    pub server: String,
    /// `--character` or `EQ_CHARACTER`: the character, skipping the world's list.
    pub character: String,
}

/// The presets, and how this run logs in on them.
pub struct Launcher {
    presets: Presets,
    /// The settings folder the presets are kept in; none keeps nothing.
    directory: Option<PathBuf>,
    installation: Installation,
    options: SessionOptions,
    launch: Launch,
    /// The arguments the client was started with, which it opens again
    /// with on another installation.
    arguments: Vec<OsString>,
}

impl Launcher {
    /// The launcher of a run.
    pub fn new(
        presets: Presets,
        directory: Option<PathBuf>,
        installation: Installation,
        options: SessionOptions,
        (launch, arguments): (Launch, Vec<OsString>),
    ) -> Self {
        Self {
            presets,
            directory,
            installation,
            options,
            launch,
            arguments,
        }
    }

    /// What every session of the run shares.
    pub const fn options(&self) -> &SessionOptions {
        &self.options
    }

    /// The login a session on a preset makes: the launch's own login server,
    /// world and character on the launch's preset.
    pub fn login(&self, index: usize, account: &str, password: &str) -> Option<Login> {
        let preset = self.presets.list.get(index)?;
        let mut preset = preset.clone();
        let launched = index == self.launch.preset;
        if launched {
            self.launch.endpoint.apply(&mut preset);
        }
        let given = |value: &String| {
            if launched {
                value.clone()
            } else {
                String::new()
            }
        };
        Some(Login {
            protocol: preset.protocol,
            host: preset.host,
            port: preset.port,
            account: account.to_owned(),
            password: Zeroizing::new(password.to_owned()),
            server: given(&self.launch.server),
            character: given(&self.launch.character),
        })
    }

    /// Whether this run logs in on a preset, or opens the client again with
    /// its installation, or cannot. Each but the first names the folders
    /// involved, so one named for the wrong client shows and can be put
    /// right in the file.
    fn availability(&self, preset: &Preset) -> Availability {
        let client = installed_client(preset.protocol);
        let here = &self.installation;
        let same_client = client == here.client;
        match &preset.installation {
            Some(directory) if same_client && *directory == here.directory => Availability::Here,
            Some(directory) => Availability::Reopens(directory.display().to_string()),
            None if same_client => Availability::Here,
            None => {
                let named = holder(&self.presets, self.launch.preset, &here.directory)
                    .map(|holder| format!(", as {} names it", holder.name))
                    .unwrap_or_default();
                Availability::Unavailable(format!(
                    "Needs a {} installation; this run's, {}, is read as {}'s{named}. \
                     Run eq-client --eq-dir <folder> --preset \"{}\" once",
                    client_name(client),
                    here.directory.display(),
                    client_name(here.client),
                    preset.name
                ))
            }
        }
    }

    /// Opens the client again on a preset with its installation, and keeps
    /// the preset as the last played.
    fn reopen(&mut self, index: usize) -> Result<Connection, String> {
        let preset = &self.presets.list[index];
        let installation = preset
            .installation
            .clone()
            .ok_or_else(|| "No installation for this server".to_owned())?;
        let arguments = reopen_arguments(&self.arguments, &preset.name, &installation);
        let executable = std::env::current_exe()
            .map_err(|error| format!("Cannot find eq-client to open again: {error}"))?;
        self.presets.last = Some(preset.name.clone());
        self.presets.save(self.directory.as_deref());
        let mut command = std::process::Command::new(executable);
        command.args(arguments);
        // The new run reads its login server from the preset alone.
        for variable in LAUNCH_VARIABLES {
            command.env_remove(variable);
        }
        command
            .spawn()
            .map_err(|error| format!("Cannot open eq-client again: {error}"))?;
        Ok(Connection::Reopened)
    }
}

/// The environment's login details, which a launch alone uses.
const LAUNCH_VARIABLES: [&str; 9] = [
    "EQ_PRESET",
    "EQ_PROTOCOL",
    "EQ_LOGIN_HOST",
    "EQ_LOGIN_PORT",
    "EQ_SERVER",
    "EQ_CHARACTER",
    "EQ_ACCOUNT",
    "EQ_PASSWORD",
    "EQ_CLIENT_DIR",
];

/// The options this run's arguments carry that a run on another preset
/// takes from it instead, with whether each takes a value.
const PER_PRESET: [(&str, bool); 6] = [
    ("--preset", true),
    ("--eq-dir", true),
    ("--server", true),
    ("--character", true),
    ("--online", false),
    ("--offline", false),
];

/// The arguments for opening the client again on a preset with its
/// installation: this run's, without what belongs to its own preset.
fn reopen_arguments(arguments: &[OsString], preset: &str, installation: &Path) -> Vec<OsString> {
    let mut kept = Vec::new();
    let mut skip_value = false;
    for argument in arguments {
        if std::mem::take(&mut skip_value) {
            continue;
        }
        let text = argument.to_string_lossy();
        let option = PER_PRESET.iter().find(|(name, _)| {
            text == *name
                || text
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with('='))
        });
        match option {
            Some((name, takes_value)) => skip_value = *takes_value && text == *name,
            None => kept.push(argument.clone()),
        }
    }
    kept.extend([
        "--preset".into(),
        preset.into(),
        "--eq-dir".into(),
        installation.as_os_str().to_owned(),
    ]);
    kept
}

impl Logins for Launcher {
    fn servers(&self) -> Vec<LoginServer> {
        // The official client remembers its own last account and world at
        // its login server; a preset there that remembers neither yet starts
        // from them.
        let official = self
            .installation
            .client
            .settings(&self.installation.directory)
            .login();
        self.presets
            .list
            .iter()
            .map(|preset| {
                let availability = self.availability(preset);
                let seeds = availability == Availability::Here
                    && official
                        .server
                        .as_deref()
                        .is_some_and(|server| same_login_server(server, preset));
                let seed = |remembered: &Option<String>, official: &Option<String>| {
                    remembered
                        .clone()
                        .or_else(|| official.clone().filter(|_| seeds))
                };
                LoginServer {
                    name: preset.name.clone(),
                    account: seed(&preset.account, &official.account).unwrap_or_default(),
                    world: seed(&preset.world, &official.world),
                    availability,
                }
            })
            .collect()
    }

    fn first(&self) -> usize {
        self.launch.preset
    }

    fn connect(
        &mut self,
        server: usize,
        account: &str,
        password: &str,
    ) -> Result<Connection, String> {
        let preset = self
            .presets
            .list
            .get(server)
            .ok_or_else(|| "No such login server".to_owned())?;
        match self.availability(preset) {
            Availability::Here => (),
            Availability::Reopens(_) => return self.reopen(server),
            Availability::Unavailable(reason) => return Err(reason),
        }
        let login = self
            .login(server, account, password)
            .ok_or_else(|| "No such login server".to_owned())?;
        SessionWorker::start(login, &self.options)
            .map(Connection::Session)
            .map_err(|error| format!("Cannot log in: {error}"))
    }

    fn played(&mut self, server: usize, account: &str, world: Option<&str>) {
        let Some(preset) = self.presets.list.get_mut(server) else {
            return;
        };
        preset.account = Some(account.to_owned());
        if let Some(world) = world {
            preset.world = Some(world.to_owned());
        }
        self.presets.last = Some(preset.name.clone());
        // The installation a preset first plays with is the one it needs.
        self.presets
            .name_installation(server, &self.installation.directory);
        self.presets.save(self.directory.as_deref());
    }

    fn scripted(&self) -> Option<(String, Zeroizing<String>)> {
        let account = std::env::var("EQ_ACCOUNT").ok()?;
        let password = Zeroizing::new(std::env::var("EQ_PASSWORD").ok()?);
        Some((account, password))
    }
}

/// Whether the official client's login server, `host:port` or a bare host,
/// is the preset's; `localhost` and `127.0.0.1` are the same.
fn same_login_server(official: &str, preset: &Preset) -> bool {
    let (host, port) = match official.trim().rsplit_once(':') {
        Some((host, port)) => (host.trim(), port.trim().parse::<u16>().ok()),
        None => (official.trim(), None),
    };
    let loopback = |host: &str| host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1";
    let same_host =
        host.eq_ignore_ascii_case(&preset.host) || (loopback(host) && loopback(&preset.host));
    same_host && port.is_none_or(|port| port == preset.port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_network::client::ServerProtocol;

    fn launcher(installation: Installation) -> Launcher {
        let mut presets = Presets::seeded(&Endpoint::default());
        presets.list[3].installation = Some(PathBuf::from("C:/TAKP"));
        let launch = Launch {
            preset: 2,
            endpoint: Endpoint {
                protocol: None,
                host: Some("192.168.1.20".into()),
                port: None,
            },
            server: "Example World".into(),
            character: String::new(),
        };
        run(presets, installation, launch)
    }

    fn run(presets: Presets, installation: Installation, launch: Launch) -> Launcher {
        Launcher::new(
            presets,
            None,
            installation,
            SessionOptions {
                install: installation_directory(),
                seconds: None,
                calibration: None,
                local_only: false,
                auto_eat: eq_client_core::food::AutoEat::default(),
            },
            (launch, Vec::new()),
        )
    }

    fn installation_directory() -> PathBuf {
        PathBuf::from("C:/EverQuest")
    }

    fn titanium() -> Installation {
        Installation {
            directory: installation_directory(),
            client: InstalledClient::Titanium,
        }
    }

    #[test]
    fn only_the_preset_at_the_official_clients_login_server_starts_from_its_account() {
        let directory =
            std::env::temp_dir().join(format!("eq-client-logins-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("eqhost.txt"),
            "[LoginServer]\nHost=localhost:5998\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("eqlsPlayerData.ini"),
            "[MISC]\nLastServerName=ExampleWorld\n[PLAYER]\nUsername=example\n",
        )
        .unwrap();
        let launcher = launcher(Installation {
            directory: directory.clone(),
            client: InstalledClient::Titanium,
        });
        let offered = launcher.servers();
        std::fs::remove_dir_all(&directory).unwrap();
        // Local EQEmu, at 127.0.0.1:5998, is where the official client logs in.
        assert_eq!(offered[2].account, "example");
        assert_eq!(offered[2].world.as_deref(), Some("ExampleWorld"));
        // P99 is another login server, whatever the official client used.
        assert_eq!(
            (offered[0].account.as_str(), offered[0].world.as_deref()),
            ("", None)
        );
        let preset = |host: &str, port| {
            let mut preset = Preset::new("Example", ServerProtocol::EqEmu);
            preset.host = host.into();
            preset.port = port;
            preset
        };
        assert!(same_login_server("LOCALHOST", &preset("127.0.0.1", 5998)));
        assert!(same_login_server(
            " 10.0.0.5:5999 ",
            &preset("10.0.0.5", 5999)
        ));
        assert!(!same_login_server(
            "10.0.0.5:5998",
            &preset("10.0.0.5", 5999)
        ));
        assert!(!same_login_server("10.0.0.6", &preset("10.0.0.5", 5998)));
    }

    #[test]
    fn presets_of_this_installations_client_log_in_here_and_others_open_their_own() {
        let launcher = launcher(titanium());
        let availability: Vec<_> = launcher
            .presets
            .list
            .iter()
            .map(|preset| launcher.availability(preset))
            .collect();
        assert_eq!(availability[0], Availability::Here);
        assert!(matches!(availability[1], Availability::Unavailable(_)));
        assert_eq!(availability[2], Availability::Here);
        assert_eq!(availability[3], Availability::Reopens("C:/TAKP".into()));
        assert_eq!(
            installed_client(ServerProtocol::Quarm),
            InstalledClient::EqMac
        );
    }

    #[test]
    fn a_login_screens_installation_holds_the_client_of_the_presets_naming_it() {
        let mut presets = Presets::seeded(&Endpoint::default());
        presets.list[2].installation = Some(installation_directory());
        presets.list[3].installation = Some(PathBuf::from("C:/TAKP"));
        let at = |presets: &Presets, launch, directory: &str| {
            client_at(presets, launch, Path::new(directory))
        };
        // Whichever preset the run begins on.
        assert_eq!(at(&presets, 3, "C:/EverQuest"), InstalledClient::Titanium);
        assert_eq!(at(&presets, 2, "C:/TAKP"), InstalledClient::EqMac);
        assert_eq!(at(&presets, 3, "C:/TAKP"), InstalledClient::EqMac);
        // One no preset names is taken to be Titanium's, even begun on TAKP.
        assert_eq!(at(&presets, 3, "D:/Elsewhere"), InstalledClient::Titanium);
        // Named for both clients, the launch's own preset decides, else the
        // first in the file.
        presets.list[1].installation = Some(installation_directory());
        assert_eq!(at(&presets, 1, "C:/EverQuest"), InstalledClient::EqMac);
        assert_eq!(at(&presets, 2, "C:/EverQuest"), InstalledClient::Titanium);
        assert_eq!(at(&presets, 0, "C:/EverQuest"), InstalledClient::EqMac);
    }

    #[test]
    fn a_titanium_run_begun_on_takp_logs_in_there_only_by_opening_its_installation() {
        // A run on a Titanium installation that begins on Local TAKP, the
        // last played, which names no installation of its own yet.
        let mut presets = Presets::seeded(&Endpoint::default());
        presets.last = Some("Local TAKP".into());
        let begun = |presets: &Presets| {
            let installation = Installation {
                directory: installation_directory(),
                client: client_at(presets, 3, &installation_directory()),
            };
            let launch = Launch {
                preset: 3,
                ..Launch::default()
            };
            run(presets.clone(), installation, launch)
        };
        let mut launcher = begun(&presets);
        assert_eq!(launcher.installation.client, InstalledClient::Titanium);
        let offered = launcher.servers();
        let Availability::Unavailable(reason) = &offered[3].availability else {
            panic!("{:?}", offered[3].availability);
        };
        // The hover names this run's folder and how it is read, and which
        // preset names that folder, where one does.
        assert!(reason.contains("this run's, C:/EverQuest, is read as Titanium's. Run"));
        assert!(launcher.connect(3, "someone", "secret").is_err());
        assert_eq!(offered[2].availability, Availability::Here);
        let mut named = presets.clone();
        named.name_installation(2, &installation_directory());
        let offered = begun(&named).servers();
        let Availability::Unavailable(reason) = &offered[3].availability else {
            panic!("{:?}", offered[3].availability);
        };
        assert!(reason.contains("is read as Titanium's, as Local EQEmu names it."));
        // Once it names its own, Connect opens the client again there.
        presets.name_installation(3, Path::new("D:/TAKP"));
        let launcher = begun(&presets);
        assert_eq!(
            launcher.servers()[3].availability,
            Availability::Reopens("D:/TAKP".into())
        );
    }

    #[test]
    fn only_the_launchs_preset_takes_its_login_server_world_and_character() {
        let launcher = launcher(titanium());
        let chosen = launcher.login(2, "someone", "secret").unwrap();
        assert_eq!(
            (chosen.host.as_str(), chosen.port, chosen.server.as_str()),
            ("192.168.1.20", 5998, "Example World")
        );
        assert_eq!(chosen.password.as_str(), "secret");
        let other = launcher.login(0, "someone", "secret").unwrap();
        assert_eq!(
            (other.host.as_str(), other.server.as_str()),
            ("login.eqemulator.net", "")
        );
    }

    #[test]
    fn a_preset_remembers_the_account_world_and_installation_it_played_with() {
        let mut launcher = launcher(titanium());
        launcher.played(2, "someone", Some("Example World"));
        let preset = &launcher.presets.list[2];
        assert_eq!(preset.account.as_deref(), Some("someone"));
        assert_eq!(preset.world.as_deref(), Some("Example World"));
        assert_eq!(
            preset.installation.as_deref(),
            Some(Path::new("C:/EverQuest"))
        );
        assert_eq!(launcher.presets.last.as_deref(), Some("Local EQEmu"));
        let offered = launcher.servers();
        assert_eq!(offered[2].account, "someone");
        assert_eq!(offered[2].world.as_deref(), Some("Example World"));
        // A world chosen for the character list alone keeps the last one.
        launcher.played(2, "someone", None);
        assert_eq!(
            launcher.presets.list[2].world.as_deref(),
            Some("Example World")
        );
    }

    #[test]
    fn opening_again_drops_what_belonged_to_this_runs_preset() {
        let arguments: Vec<OsString> = [
            "--preset",
            "Local EQEmu",
            "--eq-dir=C:/EverQuest",
            "--server",
            "Example World",
            "--debug-overlay",
            "--settings-dir",
            "D:/settings",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let reopened = reopen_arguments(&arguments, "Local TAKP", Path::new("C:/TAKP"));
        assert_eq!(
            reopened,
            [
                "--debug-overlay",
                "--settings-dir",
                "D:/settings",
                "--preset",
                "Local TAKP",
                "--eq-dir",
                "C:/TAKP"
            ]
            .map(OsString::from)
        );
    }
}
