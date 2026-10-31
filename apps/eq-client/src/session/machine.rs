//! This machine's weekly account for a Titanium world that is owed one
//! (eq-network's `sysinfo`), as the official client gives it on Windows.
//!
//! The session asks for it from its own thread, only when a report is due:
//! the processor and memory are read then, and the display and adapter come
//! from the snapshot the window keeps ([`DisplaySnapshot`]), so nothing here
//! calls the windowing or graphics libraries. The record is meant to look
//! like a standard Windows client's on every platform (Adam, 2026-10-31):
//! fields that describe the official program rather than the machine are
//! sent as the official client sent them, each labelled below, and the
//! fields Linux cannot give the Windows way are marked TODO.
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{
    Machine, SysInfo,
    machine::{Display, DisplaySnapshot},
};
use std::path::PathBuf;

#[cfg(windows)]
mod platform {
    //! Windows: the monitor as Windows names it to this process.

    /// The monitor's device name as Windows names it (`\\.\DISPLAY1`),
    /// which winit reports as the monitor's name.
    pub(super) fn device_name(display: &super::Display) -> String {
        display
            .monitor_name
            .clone()
            .unwrap_or_else(|| super::display_name(display.monitor))
    }
}

#[cfg(not(windows))]
mod platform {
    //! Other systems, which name monitors their own way: the name Windows
    //! would give, built.

    /// The monitor's device name, built the way Windows names it
    /// (`\\.\DISPLAY<n>`, n counting monitors from 1, left to right), since
    /// this system names monitors differently (a Windows look by Adam's
    /// choice, 2026-10-31). TODO: make it match what a standard Windows
    /// client sends on the same layout; Windows numbers its displays in its
    /// own order, which need not be left to right.
    pub(super) fn device_name(display: &super::Display) -> String {
        super::display_name(display.monitor)
    }
}

/// The official 32-bit client's Direct3D 9 figure for available texture
/// memory, as it sent it on 2026-10-31 (`0x3f60_0000`, about 1 GB, well
/// under the card's own). It describes that program's Direct3D 9 process,
/// not the machine, and this client cannot measure it: a constant on every
/// platform (recording checklist item 6.292).
const TEXTURE_MEMORY: u32 = 0x3f60_0000;

/// The three format checks (A4R4G4B4, DXT1, DXT3), the official engine's
/// Direct3D 9 capability answers, which every adapter of the last twenty
/// years gives; not a fact this client measures (6.293).
const FORMATS: [u8; 3] = [1, 1, 1];

/// The colour depth, 32 in every record watched (both captures).
const COLOR_DEPTH: u32 = 32;

/// The memory the official client reports at most: it sent 1,024 MB
/// physical and 2,048 MB virtual on a machine with many times that, so the record is
/// capped, and the real totals would stand out (one capture; the cap is
/// inferred, recording checklist item 6.291).
const MEMORY_CAP_MB: (u32, u32) = (1024, 2048);

/// The host's machine, for the session's weekly report.
#[derive(Debug)]
pub struct HostMachine {
    /// The installed client, whose own files hold the official stamp.
    client: InstalledClient,
    install: PathBuf,
    /// This client's settings directory, where its own stamp is kept.
    settings: Option<PathBuf>,
    display: DisplaySnapshot,
}

impl HostMachine {
    /// The machine for a session of `client`'s generation on `install`,
    /// keeping its stamp in `settings`, with the window's `display`.
    pub const fn new(
        client: InstalledClient,
        install: PathBuf,
        settings: Option<PathBuf>,
        display: DisplaySnapshot,
    ) -> Self {
        Self {
            client,
            install,
            settings,
            display,
        }
    }
}

impl Machine for HostMachine {
    /// This client's own stamp where it has one, else the installed
    /// client's (`eqclient.ini`, then `defaults.ini`), else none.
    fn last_reported(&self) -> u64 {
        self.settings
            .as_deref()
            .and_then(eq_client_assets::ui::own_last_sysinfo_date)
            .or_else(|| self.client.settings(&self.install).last_sysinfo_date())
            .unwrap_or(0)
    }

    fn measure(&self) -> SysInfo {
        let (cpu, total_mb) = processor_and_memory();
        account(&cpu, total_mb, &self.display.get(), platform::device_name)
    }
}

/// The record for a machine with this processor brand, total memory and
/// display, naming its monitor with `device`.
fn account(
    cpu: &str,
    total_mb: u32,
    display: &Display,
    device: impl Fn(&Display) -> String,
) -> SysInfo {
    let (desktop_width, desktop_height, refresh_hz) = display.desktop;
    let (screen_width, screen_height) = display.screen;
    SysInfo {
        cpu: cpu.to_owned(),
        physical_mb: total_mb.min(MEMORY_CAP_MB.0),
        virtual_mb: total_mb.min(MEMORY_CAP_MB.1),
        color_depth: COLOR_DEPTH,
        desktop_width,
        desktop_height,
        screen_width,
        screen_height,
        refresh_hz,
        texture_memory: TEXTURE_MEMORY,
        formats: FORMATS,
        // The renderer's adapter name, cut to its field by the record. On
        // Windows it is the driver's own string, as the official client's;
        // TODO: other systems' drivers may word the same card differently
        // (Mesa adds its own details), which a standard Windows client
        // would not send.
        adapter: display.adapter.clone(),
        device: device(display),
    }
}

/// `\\.\DISPLAY<n>` for the monitor at `index`, counting from 1.
fn display_name(index: usize) -> String {
    format!(r"\\.\DISPLAY{}", index.saturating_add(1))
}

/// The processor's brand and the total physical memory in megabytes,
/// rounded up, as the system reports them (on Windows, `cpuid`'s brand and
/// `GlobalMemoryStatusEx`'s total, the official client's own sources); an
/// empty brand and none where the system will not say.
fn processor_and_memory() -> (String, u32) {
    let Ok(mut system) = sysinfo::System::new() else {
        return (String::new(), 0);
    };
    system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
    system.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    let brand = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_owned())
        .unwrap_or_default();
    (brand, ceil_megabytes(system.total_memory() / 1024))
}

/// Kilobytes rounded up to whole megabytes, held inside 32 bits.
fn ceil_megabytes(kilobytes: u64) -> u32 {
    let megabytes = kilobytes.saturating_add(1023) / 1024;
    u32::try_from(megabytes).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display() -> Display {
        Display {
            desktop: (1920, 1080, 60),
            screen: (1280, 720),
            monitor: 1,
            monitor_name: None,
            adapter: "Test Adapter".into(),
        }
    }

    #[test]
    fn the_record_looks_like_a_windows_clients_with_this_machines_measures() {
        let record = account("Test CPU", 65_536, &display(), |display| {
            display_name(display.monitor)
        });
        assert_eq!(record.cpu, "Test CPU");
        // Memory is capped as the official record caps it.
        assert_eq!((record.physical_mb, record.virtual_mb), (1024, 2048));
        assert_eq!(record.color_depth, 32);
        assert_eq!((record.desktop_width, record.desktop_height), (1920, 1080));
        assert_eq!((record.screen_width, record.screen_height), (1280, 720));
        assert_eq!(record.refresh_hz, 60);
        assert_eq!(record.texture_memory, 0x3f60_0000);
        assert_eq!(record.formats, [1, 1, 1]);
        assert_eq!(record.adapter, "Test Adapter");
        assert_eq!(record.device, r"\\.\DISPLAY2");
        // A small machine reports its own totals, under the caps.
        let small = account("", 1500, &display(), |_| String::new());
        assert_eq!((small.physical_mb, small.virtual_mb), (1024, 1500));
    }

    #[test]
    fn memory_is_whole_megabytes_rounded_up() {
        assert_eq!(ceil_megabytes(2_097_153), 2049);
        assert_eq!(ceil_megabytes(1_048_576), 1024);
        assert_eq!(ceil_megabytes(0), 0);
        assert_eq!(ceil_megabytes(u64::MAX), u32::MAX);
    }

    #[test]
    fn our_own_stamp_wins_over_the_installed_clients() {
        let root = std::env::temp_dir().join(format!("eq-machine-{}", std::process::id()));
        let (install, ours) = (root.join("install"), root.join("settings"));
        std::fs::create_dir_all(&install).expect("install");
        let machine = |settings: Option<PathBuf>| {
            HostMachine::new(
                InstalledClient::Titanium,
                install.clone(),
                settings,
                DisplaySnapshot::default(),
            )
        };
        assert_eq!(machine(Some(ours.clone())).last_reported(), 0);
        std::fs::write(
            install.join("eqclient.ini"),
            "[Defaults]\nLastSysInfoDate=5\n",
        )
        .expect("ini");
        assert_eq!(machine(Some(ours.clone())).last_reported(), 5);
        eq_client_assets::ui::keep_last_sysinfo_date(&ours, 9).expect("ours");
        assert_eq!(machine(Some(ours)).last_reported(), 9);
        assert_eq!(machine(None).last_reported(), 5);
        std::fs::remove_dir_all(&root).expect("cleaned");
    }
}
