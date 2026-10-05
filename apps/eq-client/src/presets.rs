//! The login servers the login screen offers: presets the player keeps in
//! `login-servers.txt` in the client's settings folder, each a login server
//! of one server type, with what the client remembers of playing there.
//! The file is the player's to edit; the client writes it back only to
//! remember the last account and world used and the installation each
//! needs, never a password.
use eq_network::client::ServerProtocol;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

/// The file's name in the settings folder.
const FILE: &str = "login-servers.txt";

/// The file's first line.
const HEADER: &str = "# eq-client login servers v1";

/// One login server the player can log in on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preset {
    /// What the login screen calls it, which no other preset shares.
    pub name: String,
    /// The server type, which decides how the session talks and which
    /// installation it needs: Titanium's for `EQEmu` and P99, the TAKP
    /// client's for TAKP and Quarm.
    pub protocol: ServerProtocol,
    /// The login server's address.
    pub host: String,
    /// The login server's port.
    pub port: u16,
    /// The installation of the official client this server type needs, once
    /// named: in the file, by a launch naming the preset with `--eq-dir`, or
    /// by the first session played here.
    pub installation: Option<PathBuf>,
    /// The account last logged in with here.
    pub account: Option<String>,
    /// The world last played on here.
    pub world: Option<String>,
}

impl Preset {
    /// A preset at the server type's usual login server, with nothing
    /// remembered.
    pub fn new(name: &str, protocol: ServerProtocol) -> Self {
        let (host, port) = protocol.default_endpoint();
        Self {
            name: name.to_owned(),
            protocol,
            host: host.to_owned(),
            port,
            installation: None,
            account: None,
            world: None,
        }
    }
}

/// Every preset, and which was played last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presets {
    /// The preset last played on, by name.
    pub last: Option<String>,
    /// The presets, in the file's order.
    pub list: Vec<Preset>,
}

impl Presets {
    /// One preset for each server type at its usual login server, which a
    /// first run writes. The server type and login server the environment
    /// names (`EQ_PROTOCOL`, `EQ_LOGIN_HOST`, `EQ_LOGIN_PORT`), as today's
    /// launchers set them, go into that type's preset.
    pub fn seeded(environment: &Endpoint) -> Self {
        let mut seeds: Vec<_> = [
            ServerProtocol::Project1999,
            ServerProtocol::Quarm,
            ServerProtocol::EqEmu,
            ServerProtocol::Takp,
        ]
        .into_iter()
        .filter_map(|protocol| Some(Preset::new(seed_name(protocol)?, protocol)))
        .collect();
        let mut last = None;
        if let Some(protocol) = environment.protocol
            && let Some(preset) = seeds.iter_mut().find(|preset| preset.protocol == protocol)
        {
            environment.apply(preset);
            last = Some(preset.name.clone());
        }
        Self { last, list: seeds }
    }

    /// Reads the file's text. Lines it does not understand are skipped, and
    /// so is a preset without a known server type or a valid port, or with
    /// another's name.
    pub fn read(text: &str) -> Self {
        let mut presets = Self {
            last: None,
            list: Vec::new(),
        };
        let mut current: Option<Section> = None;
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
            {
                presets.finish(current.take());
                current = Some(Section::new(name.trim()));
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match current.as_mut() {
                Some(section) => section.set(key, value),
                None if key == "last" && !value.is_empty() => {
                    presets.last = Some(value.to_owned());
                }
                None => (),
            }
        }
        presets.finish(current);
        presets
    }

    /// Adds a read section, if it makes a preset.
    fn finish(&mut self, section: Option<Section>) {
        let Some(preset) = section.and_then(Section::preset) else {
            return;
        };
        if self.find(&preset.name).is_none() {
            self.list.push(preset);
        }
    }

    /// The file's text.
    pub fn text(&self) -> String {
        let mut text = format!("{HEADER}\n");
        if let Some(last) = &self.last {
            let _ = writeln!(text, "last = {last}");
        }
        for preset in &self.list {
            // A server type this client cannot name is never written under
            // another's name; no preset of one is read or seeded.
            let Some(kind) = type_name(preset.protocol) else {
                continue;
            };
            let _ = writeln!(text, "\n[{}]", preset.name);
            let _ = writeln!(text, "type = {kind}");
            let _ = writeln!(text, "host = {}\nport = {}", preset.host, preset.port);
            let remembered = [
                (
                    "installation",
                    preset
                        .installation
                        .as_deref()
                        .map(|path| path.display().to_string()),
                ),
                ("account", preset.account.clone()),
                ("world", preset.world.clone()),
            ];
            for (key, value) in remembered {
                if let Some(value) = value {
                    let _ = writeln!(text, "{key} = {value}");
                }
            }
        }
        text
    }

    /// The presets in the settings folder: the file there, or, without one,
    /// the seeded presets, which are written there for the player to edit.
    /// Without a settings folder, the seeded presets, kept nowhere.
    pub fn load(directory: Option<&Path>, environment: &Endpoint) -> Self {
        let Some(directory) = directory else {
            return Self::seeded(environment);
        };
        match std::fs::read(directory.join(FILE)) {
            Ok(bytes) => Self::read(&String::from_utf8_lossy(&bytes)),
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("Cannot read the login servers' file: {error}");
                }
                let presets = Self::seeded(environment);
                presets.save(Some(directory));
                presets
            }
        }
    }

    /// Writes the presets to the settings folder, in one step, so a crash
    /// never leaves half a file. A failure is logged, and the presets live
    /// on for this run.
    pub fn save(&self, directory: Option<&Path>) {
        let Some(directory) = directory else {
            return;
        };
        let written = std::fs::create_dir_all(directory).and_then(|()| {
            let path = directory.join(FILE);
            let partial = path.with_extension("tmp");
            std::fs::write(&partial, self.text())?;
            std::fs::rename(&partial, &path)
        });
        if let Err(error) = written {
            tracing::warn!("Cannot save the login servers' file: {error}");
        }
    }

    /// A preset's place, by its name in any case, so that a name differing
    /// only in case is taken too.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.list
            .iter()
            .position(|preset| preset.name.eq_ignore_ascii_case(name))
    }

    /// Names the installation a preset needs, if it names none yet: the
    /// first one named stays, so only the file changes it. Says whether it
    /// named one.
    pub fn name_installation(&mut self, index: usize, directory: &Path) -> bool {
        match self.list.get_mut(index) {
            Some(preset) if preset.installation.is_none() => {
                preset.installation = Some(directory.to_path_buf());
                true
            }
            _ => false,
        }
    }
}

/// The login server today's environment variables name, for launchers and
/// scripts written before presets: each part only where it is set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Endpoint {
    /// `EQ_PROTOCOL`.
    pub protocol: Option<ServerProtocol>,
    /// `EQ_LOGIN_HOST`.
    pub host: Option<String>,
    /// `EQ_LOGIN_PORT`.
    pub port: Option<u16>,
}

impl Endpoint {
    /// Reads the environment.
    ///
    /// # Errors
    /// Says which variable holds something that is not a server type or a
    /// port.
    pub fn from_environment() -> Result<Self, String> {
        let variable = |name| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let protocol = variable("EQ_PROTOCOL")
            .map(|value| {
                value
                    .parse::<ServerProtocol>()
                    .map_err(|error| format!("EQ_PROTOCOL={value:?}: {error}"))
            })
            .transpose()?;
        let port = variable("EQ_LOGIN_PORT")
            .map(|value| {
                value
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or_else(|| format!("EQ_LOGIN_PORT={value:?} is not a port number"))
            })
            .transpose()?;
        Ok(Self {
            protocol,
            host: variable("EQ_LOGIN_HOST"),
            port,
        })
    }

    /// Puts the login server's address, where set, into a preset.
    pub fn apply(&self, preset: &mut Preset) {
        if let Some(host) = &self.host {
            preset.host.clone_from(host);
        }
        if let Some(port) = self.port {
            preset.port = port;
        }
    }
}

/// The name a first run gives a server type's preset; none for a type a
/// later eq-network adds, which this client offers no preset of until it is
/// checked here.
pub const fn seed_name(protocol: ServerProtocol) -> Option<&'static str> {
    match protocol {
        ServerProtocol::Project1999 => Some("Project 1999"),
        ServerProtocol::Quarm => Some("Project Quarm"),
        ServerProtocol::EqEmu => Some("Local EQEmu"),
        ServerProtocol::Takp => Some("Local TAKP"),
        _ => None,
    }
}

/// A server type's name in the file, which `EQ_PROTOCOL` also takes; none
/// for a type a later eq-network adds, which this client cannot name until
/// it is checked here.
const fn type_name(protocol: ServerProtocol) -> Option<&'static str> {
    match protocol {
        ServerProtocol::Project1999 => Some("p99"),
        ServerProtocol::Quarm => Some("quarm"),
        ServerProtocol::EqEmu => Some("eqemu"),
        ServerProtocol::Takp => Some("takp"),
        _ => None,
    }
}

/// A preset as its section of the file is read.
struct Section {
    name: String,
    protocol: Option<ServerProtocol>,
    host: Option<String>,
    port: Option<u16>,
    installation: Option<PathBuf>,
    account: Option<String>,
    world: Option<String>,
}

impl Section {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            protocol: None,
            host: None,
            port: None,
            installation: None,
            account: None,
            world: None,
        }
    }

    fn set(&mut self, key: &str, value: &str) {
        let text = (!value.is_empty()).then(|| value.to_owned());
        match key {
            "type" => self.protocol = value.parse().ok(),
            "host" => self.host = text,
            "port" => self.port = value.parse().ok().filter(|port| *port != 0),
            "installation" => self.installation = text.map(PathBuf::from),
            "account" => self.account = text,
            "world" => self.world = text,
            _ => (),
        }
    }

    /// The preset, when the section names a server type this client can
    /// name, and a port.
    fn preset(self) -> Option<Preset> {
        let protocol = self
            .protocol
            .filter(|protocol| type_name(*protocol).is_some())?;
        let (host, port) = protocol.default_endpoint();
        (!self.name.is_empty()).then_some(())?;
        Some(Preset {
            name: self.name,
            protocol,
            host: self.host.unwrap_or_else(|| host.to_owned()),
            port: self.port.unwrap_or(port),
            installation: self.installation,
            account: self.account,
            world: self.world,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_keep_to_their_file_and_never_a_password() {
        let mut presets = Presets::seeded(&Endpoint::default());
        assert_eq!(presets.last, None);
        assert_eq!(
            presets
                .list
                .iter()
                .map(|preset| (preset.protocol, preset.host.as_str(), preset.port))
                .collect::<Vec<_>>(),
            [
                (ServerProtocol::Project1999, "login.eqemulator.net", 5998),
                (ServerProtocol::Quarm, "loginserver.takproject.net", 6000),
                (ServerProtocol::EqEmu, "127.0.0.1", 5998),
                (ServerProtocol::Takp, "127.0.0.1", 6000),
            ]
        );
        presets.last = Some("Local EQEmu".into());
        let local = &mut presets.list[2];
        local.installation = Some(PathBuf::from("C:/EverQuest"));
        local.account = Some("example".into());
        local.world = Some("Example World".into());
        let text = presets.text();
        assert!(text.starts_with(HEADER));
        assert!(text.contains(
            "[Local EQEmu]\ntype = eqemu\nhost = 127.0.0.1\nport = 5998\n\
             installation = C:/EverQuest\naccount = example\nworld = Example World\n"
        ));
        assert!(!text.contains("password"));
        assert_eq!(Presets::read(&text), presets);
    }

    #[test]
    fn a_first_run_seeds_the_server_type_and_address_the_environment_names() {
        let environment = Endpoint {
            protocol: Some(ServerProtocol::Takp),
            host: Some("192.168.1.20".into()),
            port: None,
        };
        let presets = Presets::seeded(&environment);
        assert_eq!(presets.last.as_deref(), Some("Local TAKP"));
        let takp = &presets.list[presets.find("Local TAKP").unwrap()];
        assert_eq!((takp.host.as_str(), takp.port), ("192.168.1.20", 6000));
    }

    #[test]
    fn a_preset_keeps_the_first_installation_it_is_named_with() {
        let mut presets = Presets::seeded(&Endpoint::default());
        assert!(presets.name_installation(3, Path::new("C:/TAKP")));
        assert!(!presets.name_installation(3, Path::new("C:/EverQuest")));
        assert!(!presets.name_installation(9, Path::new("C:/TAKP")));
        assert_eq!(
            presets.list[3].installation.as_deref(),
            Some(Path::new("C:/TAKP"))
        );
    }

    #[test]
    fn a_hand_edited_file_keeps_what_makes_sense() {
        let presets = Presets::read(
            "last = Home\n\
             [Home]\ntype = eqemu\nhost = 10.0.0.5\nshiny = yes\n\
             [Nameless type]\nhost = 10.0.0.6\n\
             [Bad port]\ntype = takp\nport = none\n\
             [HOME]\ntype = takp\n\
             [Elsewhere]\ntype = project1999\nport = 5999\naccount =\n",
        );
        assert_eq!(presets.last.as_deref(), Some("Home"));
        let names: Vec<_> = presets
            .list
            .iter()
            .map(|preset| preset.name.as_str())
            .collect();
        // A section without a type is skipped, a bad port reads as the
        // type's own, and a name already taken, in any case, is skipped.
        assert_eq!(names, ["Home", "Bad port", "Elsewhere"]);
        assert_eq!(
            (presets.list[0].host.as_str(), presets.list[0].port),
            ("10.0.0.5", 5998)
        );
        assert_eq!(presets.list[1].port, 6000);
        assert_eq!(presets.list[2].port, 5999);
        assert_eq!(presets.list[2].account, None);
    }

    #[test]
    fn the_settings_folder_keeps_the_seeded_presets_until_edited() {
        let directory =
            std::env::temp_dir().join(format!("eq-client-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let seeded = Presets::load(Some(&directory), &Endpoint::default());
        assert!(directory.join(FILE).is_file());
        let mut changed = seeded.clone();
        changed.list.truncate(1);
        changed.save(Some(&directory));
        let reread = Presets::load(Some(&directory), &Endpoint::default());
        std::fs::remove_dir_all(&directory).unwrap();
        assert_eq!(seeded.list.len(), 4);
        assert_eq!(reread, changed);
    }
}
