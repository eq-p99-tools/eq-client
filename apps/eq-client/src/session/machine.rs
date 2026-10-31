//! What this machine says about itself, for the weekly account a Titanium
//! world is owed when a player enters it on this machine.
//!
//! The official client asks its graphics engine and its Windows calls for
//! every word of that account. This client asks the system it runs on for
//! the same facts — the account is the player's own machine either way, and
//! a world stores what it is told. Where the system will not say something,
//! the field keeps the zero or the empty that a failed official read leaves
//! behind in the record; nothing here is invented. The reads name Linux
//! paths (`/proc`, `/sys`): on a host without them the account reports an
//! unreadable machine, and the report still goes out with what it has.

use eq_client_core::SysInfo;

/// The account for a world entered now, the last one having gone out at
/// `reported_at` — whole seconds since the epoch, 0 for none ever, as the
/// installed client's own `LastSysInfoDate` says.
#[must_use]
pub fn account(reported_at: u64) -> SysInfo {
    let (physical_mb, virtual_mb) = memory_totals(&read("/proc/meminfo"));
    let (width, height, refresh) = display_mode();
    SysInfo {
        cpu: cpu_brand(&read("/proc/cpuinfo")),
        gpu: adapter_name(),
        physical_mb,
        virtual_mb,
        // Every machine the report was watched from said 32; nothing on a
        // Linux host here contradicts it, and no cheaper read says
        // otherwise. Inferred.
        color_depth: 32,
        desktop_width: width,
        desktop_height: height,
        // The game is drawn on the desktop's mode until this client can
        // ask a window of its own what size it got. Inferred.
        screen_width: width,
        screen_height: height,
        refresh_hz: refresh,
        reported_at,
    }
}

/// A system file, or nothing at all when the host has none to give.
fn read<P: AsRef<std::path::Path>>(path: P) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The processor's brand, from `/proc/cpuinfo`'s first `model name` line;
/// empty when the host has no such line.
fn cpu_brand(cpuinfo: &str) -> String {
    cpuinfo
        .lines()
        .filter_map(|line| line.trim().split_once(':'))
        .find(|(field, _)| field.trim().eq_ignore_ascii_case("model name"))
        .map_or_else(String::new, |(_, brand)| brand.trim().to_owned())
}

/// The memory totals the official record carries: `dwTotalPhys`, and the
/// commit it adds to it. `/proc/meminfo` keeps both in kilobytes; the
/// record rounds each up to whole megabytes, as the official program does.
fn memory_totals(meminfo: &str) -> (u32, u32) {
    let kilobytes = |name: &str| {
        meminfo
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(field, _)| field.trim().eq_ignore_ascii_case(name))
            .and_then(|(_, value)| value.split_whitespace().next()?.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let physical = kilobytes("MemTotal");
    let commit = physical.saturating_add(kilobytes("SwapTotal"));
    (ceil_megabytes(physical), ceil_megabytes(commit))
}

/// Kilobytes rounded up to whole megabytes, the official record's rounding,
/// held inside the 32 bits the record gives it.
fn ceil_megabytes(kilobytes: u64) -> u32 {
    let megabytes = kilobytes.saturating_add(1023) / 1024;
    u32::try_from(megabytes).unwrap_or(u32::MAX)
}

/// The desktop's current mode: its width and height, and its refresh when
/// the mode names one. `/sys/class/drm/<connector>/modes` lists a
/// connector's modes with the one it shows now first; the first connector
/// that answers wins, as the one screen the player looks at. Zeros, as a
/// failed official read leaves them, when no connector will say.
fn display_mode() -> (u32, u32, u32) {
    for entry in drm_entries() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Connector folders are `card<N>-<connector>`; the bare cards name
        // no screen.
        if !name.starts_with("card") || !name.contains('-') {
            continue;
        }
        for line in read(entry.path().join("modes")).lines() {
            if let Some(mode) = parse_mode(line) {
                return mode;
            }
        }
    }
    (0, 0, 0)
}

/// One mode line, `WIDTHxHEIGHT` or `WIDTHxHEIGHT@hertz` (the hertz may be
/// fractional; the record keeps whole ones); None when the line is neither.
fn parse_mode(line: &str) -> Option<(u32, u32, u32)> {
    let line = line.trim();
    let (size, rate) = line.split_once('@').unwrap_or((line, ""));
    let (width, height) = size.split_once('x')?;
    let refresh = rate
        .split('.')
        .next()
        .and_then(|whole| whole.parse().ok())
        .unwrap_or(0);
    Some((width.parse().ok()?, height.parse().ok()?, refresh))
}

/// The display adapter's own name, where the firmware bothers to label it:
/// `PCI_LABEL` in a card's `device/uevent`. Empty when no card will say —
/// the official client's engine would have had nothing to report either,
/// and an empty name is the record's own answer.
fn adapter_name() -> String {
    for entry in drm_entries() {
        if !entry.file_name().to_string_lossy().starts_with("card") {
            continue;
        }
        let uevent = read(entry.path().join("device/uevent"));
        for line in uevent.lines() {
            if let Some(label) = line.trim().strip_prefix("PCI_LABEL=") {
                let label = label.trim();
                if !label.is_empty() {
                    return label.to_owned();
                }
            }
        }
    }
    String::new()
}

/// The entries of `/sys/class/drm`, or none when the host has no such
/// place.
fn drm_entries() -> impl Iterator<Item = std::fs::DirEntry> {
    std::fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_processor_line_names_the_processor_and_nothing_else_does() {
        assert_eq!(
            cpu_brand(
                "processor\t: 0\nmodel name\t: Test Processor 9000\nvendor_id\t: TestMaker\n"
            ),
            "Test Processor 9000"
        );
        // A host that says nothing about its processor gets an empty
        // brand, as a failed official read leaves one.
        assert_eq!(cpu_brand("vendor_id\t: TestMaker\n"), "");
        assert_eq!(cpu_brand(""), "");
    }

    #[test]
    fn memory_is_told_in_whole_megabytes_rounded_up() {
        // Exact multiples say exactly themselves; one kilobyte over a
        // multiple is rounded up, and the commit carries the swap with it.
        assert_eq!(
            memory_totals("MemTotal: 2097153 kB\nSwapTotal: 1048576 kB\n"),
            (2049, 3073)
        );
        assert_eq!(memory_totals("MemTotal: 1048576 kB\n"), (1024, 1024));
        assert_eq!(memory_totals(""), (0, 0));
    }

    #[test]
    fn a_mode_gives_its_size_and_whole_hertz_when_it_names_them() {
        assert_eq!(parse_mode("2560x1600"), Some((2560, 1600, 0)));
        assert_eq!(parse_mode("1920x1080@60"), Some((1920, 1080, 60)));
        assert_eq!(parse_mode("800x600@59.95"), Some((800, 600, 59)));
        assert_eq!(parse_mode("preferred"), None);
        assert_eq!(parse_mode("x"), None);
    }
}
