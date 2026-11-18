//! Proposes a character select spot for each installed zone from its shape
//! (`eq_client_core::select_spots::propose`), printing unreviewed row snippets
//! by default, or with `--check` says whether each listed spot stands its
//! character on a floor a player can walk to. It reads the installation only,
//! opens no window and sends nothing.
//!
//! `cargo run --release -p eq-client-render --example propose_select_spots -- <EverQuest folder> [--spots <input file>] [--settings <folder>] [--check] [zone ...]`
//!
//! Without zones named, it tries every zone in the zone table whose files
//! the installation has (with `--check`, every listed zone the installation
//! has). Zones it can't load or place are said on stderr.
//! `--spots <input file>` opts into complete-document output: existing reviewed
//! rows and all comments, including audit identities, are retained exactly.
//! Send stdout to a separate output file, then inspect proposals before replacing
//! or reviewing rows. Generating a proposal never marks it reviewed.
//! `--settings <folder>` reads and keeps each zone's navigation mesh in that
//! folder's cache, as zone entry does; without it every mesh is built anew.
//!
//! Only floors a player can walk to are proposed: floors on the zone's main
//! island (the largest by dry floor area on its navigation mesh), or that
//! island's drops lead down to. That is our rule, not the program's.
//!
//! `--check` prints one line a listed zone: `reachable`, `NOT REACHABLE`,
//! `NO FLOOR` (the preview falls back), `kept` (the row's `# kept: reason`
//! comment keeps it whatever the check finds) or `unknown` (no mesh to
//! weigh; never a pass). A zone whose main island isn't clearly the main
//! one (under half the floor, or the next island at least half its size)
//! says `main island unclear` and wants a look even when it passes. It
//! exits 1 when any zone is flagged or unknown.
use anyhow::{Context, Result, bail};
use eq_client_core::select_spots::{BUILT_IN, Spots, rewrite_proposals};
use eq_client_render::SpotVerdict;
use std::io::Write;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let Some(directory) = arguments.next().map(PathBuf::from) else {
        eprintln!(
            "usage: propose_select_spots <EverQuest folder> [--spots <input file>] [--settings <folder>] [--check] [zone ...]"
        );
        std::process::exit(2);
    };
    let mut input = None;
    let mut settings = None;
    let mut check = false;
    let mut named = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--spots" => {
                if input.is_some() {
                    bail!("--spots may be supplied only once");
                }
                input = Some(PathBuf::from(
                    arguments.next().context("--spots needs an input file")?,
                ));
            }
            "--settings" => {
                if settings.is_some() {
                    bail!("--settings may be supplied only once");
                }
                settings = Some(PathBuf::from(
                    arguments.next().context("--settings needs a folder")?,
                ));
            }
            "--check" => check = true,
            _ if argument.starts_with('-') => bail!("unknown option: {argument}"),
            _ => named.push(argument),
        }
    }
    let full_document = input.is_some();
    let text = match input {
        Some(path) => std::fs::read_to_string(path).context("reading the input spots file")?,
        None => BUILT_IN.to_owned(),
    };
    let listed = Spots::parse(&text).map_err(anyhow::Error::msg)?;
    if check {
        let failed = check_spots(&directory, settings.as_deref(), &listed, named)?;
        std::process::exit(i32::from(failed));
    }
    let zones = if named.is_empty() {
        installed(&directory)
    } else {
        named
    };
    let mut proposals = Vec::new();
    let mut stdout = std::io::stdout().lock();
    for zone in zones {
        if listed.get(&zone).is_some_and(|spot| spot.reviewed) {
            eprintln!("{zone}: reviewed already, kept");
            continue;
        }
        let asset = match eq_client_assets::load_zone(&directory, &zone) {
            Ok(asset) => asset,
            Err(error) => {
                eprintln!("{zone}: not loaded: {error}");
                continue;
            }
        };
        let mesh = match eq_client_render::select_spot_mesh(settings.as_deref(), &zone, &asset) {
            Ok(mesh) => mesh,
            Err(error) => {
                eprintln!("{zone}: no navigation mesh, not proposed: {error}");
                continue;
            }
        };
        match eq_client_render::propose_select_spot(&asset, &mesh) {
            Some(spot) => {
                if full_document {
                    proposals.push((zone, spot));
                } else {
                    writeln!(stdout, "{}", spot.line(&zone))?;
                }
            }
            None => eprintln!("{zone}: no floor qualifies"),
        }
    }
    if full_document {
        let output = rewrite_proposals(
            &text,
            proposals.iter().map(|(zone, spot)| (zone.as_str(), *spot)),
        )
        .map_err(anyhow::Error::msg)?;
        stdout.write_all(output.as_bytes())?;
    }
    Ok(())
}

/// Checks each listed zone the installation has (or those named), printing
/// one line each; whether any was flagged or unknown.
fn check_spots(
    directory: &std::path::Path,
    settings: Option<&std::path::Path>,
    listed: &Spots,
    named: Vec<String>,
) -> Result<bool> {
    let zones: Vec<String> = if named.is_empty() {
        let mut zones: Vec<String> = listed
            .iter()
            .map(|(zone, _)| zone.to_owned())
            .filter(|zone| directory.join(format!("{zone}.s3d")).is_file())
            .collect();
        zones.sort();
        zones
    } else {
        named
    };
    let mut stdout = std::io::stdout().lock();
    let (mut flagged, mut unknown, mut unclear) = (Vec::new(), Vec::new(), Vec::new());
    for zone in zones {
        let started = std::time::Instant::now();
        let Some(spot) = listed.get(&zone) else {
            writeln!(stdout, "{zone}: unknown (not listed)")?;
            unknown.push(zone);
            continue;
        };
        let weighed = eq_client_assets::load_zone(directory, &zone)
            .map_err(|error| format!("not loaded: {error}"))
            .and_then(|asset| {
                let mesh = eq_client_render::select_spot_mesh(settings, &zone, &asset)?;
                Ok(eq_client_render::check_select_spot(&asset, spot, &mesh))
            });
        let seconds = started.elapsed().as_secs_f32();
        let (verdict, main) = match weighed {
            Ok(weighed) => weighed,
            Err(error) => {
                writeln!(stdout, "{zone}: unknown ({error}) [{seconds:.1} s]")?;
                unknown.push(zone);
                continue;
            }
        };
        let said = match verdict {
            SpotVerdict::Reachable => "reachable".to_owned(),
            SpotVerdict::Unreachable { z, island_area } => format!(
                "NOT REACHABLE (floor z {z:.2}, its island {})",
                island_area.map_or_else(|| "off the mesh".to_owned(), |area| format!("{area:.0}"))
            ),
            SpotVerdict::NoFloor => "NO FLOOR (the preview falls back)".to_owned(),
            SpotVerdict::NoMainIsland => "unknown (the mesh has no floor)".to_owned(),
        };
        let main_said = main.map_or_else(String::new, |main| {
            format!(
                "; main island {:.0} of {:.0}, next {:.0}{}{}",
                main.area,
                main.total,
                main.runner_up,
                if main.dry { "" } else { ", no dry floor" },
                if main.clear() {
                    ""
                } else {
                    ", main island unclear"
                }
            )
        });
        let kept = listed.kept(&zone);
        let line = match kept {
            Some(reason) => format!("kept ({reason}); check: {said}"),
            None => said,
        };
        writeln!(stdout, "{zone}: {line}{main_said} [{seconds:.1} s]")?;
        if main.is_some_and(|main| !main.clear()) {
            unclear.push(zone.clone());
        }
        match verdict {
            SpotVerdict::Reachable => {}
            SpotVerdict::NoMainIsland => unknown.push(zone),
            _ if kept.is_some() => {}
            _ => flagged.push(zone),
        }
    }
    writeln!(stdout, "flagged ({}): {}", flagged.len(), flagged.join(" "))?;
    writeln!(stdout, "unknown ({}): {}", unknown.len(), unknown.join(" "))?;
    writeln!(
        stdout,
        "main island unclear ({}): {}",
        unclear.len(),
        unclear.join(" ")
    )?;
    Ok(!flagged.is_empty() || !unknown.is_empty())
}

/// Every zone in the zone table whose main archive the installation has.
fn installed(directory: &std::path::Path) -> Vec<String> {
    let mut zones: Vec<String> = (0..=u32::from(u16::MAX))
        .filter_map(eq_client_core::zones::short_name)
        .filter(|zone| directory.join(format!("{zone}.s3d")).is_file())
        .map(str::to_owned)
        .collect();
    zones.sort();
    zones.dedup();
    zones
}
