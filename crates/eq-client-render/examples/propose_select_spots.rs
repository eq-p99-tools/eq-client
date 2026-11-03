//! Proposes a character select spot for each installed zone from its shape
//! (`eq_client_core::select_spots::propose`), printing unreviewed row snippets
//! by default. It reads the installation only and opens no window.
//!
//! `cargo run --release -p eq-client-render --example propose_select_spots -- <EverQuest folder> [--spots <input file>] [zone ...]`
//!
//! Without zones named, it tries every zone in the zone table whose files
//! the installation has. Zones it can't load or place are said on stderr.
//! `--spots <input file>` opts into complete-document output: existing reviewed
//! rows and all comments, including audit identities, are retained exactly.
//! Send stdout to a separate output file, then inspect proposals before replacing
//! or reviewing rows. Generating a proposal never marks it reviewed.
use anyhow::{Context, Result, bail};
use eq_client_core::select_spots::{BUILT_IN, Spots, rewrite_proposals};
use std::io::Write;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let Some(directory) = arguments.next().map(PathBuf::from) else {
        eprintln!(
            "usage: propose_select_spots <EverQuest folder> [--spots <input file>] [zone ...]"
        );
        std::process::exit(2);
    };
    let mut input = None;
    let mut named = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--spots" {
            if input.is_some() {
                bail!("--spots may be supplied only once");
            }
            input = Some(PathBuf::from(
                arguments.next().context("--spots needs an input file")?,
            ));
        } else if argument.starts_with('-') {
            bail!("unknown option: {argument}");
        } else {
            named.push(argument);
        }
    }
    let full_document = input.is_some();
    let text = match input {
        Some(path) => std::fs::read_to_string(path).context("reading the input spots file")?,
        None => BUILT_IN.to_owned(),
    };
    let listed = Spots::parse(&text).map_err(anyhow::Error::msg)?;
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
        match eq_client_render::propose_select_spot(&asset) {
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
