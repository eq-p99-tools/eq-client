//! Offline creation-form screenshot using a real, user-owned installation.
//! Usage: `cargo run -p eq-client-render --example character_creation -- INSTALL OUTPUT PAGE`
//! Optional arguments: classic|titanium|eqmac RACE GENDER last (last choices).
//! PAGE is 0 (identity), 1 (appearance), 2 (attributes), or 3 (home/review). No server is contacted.
use anyhow::{Context, Result, ensure};
use eq_client_core::{character_creation::Draft, creation::Rules};
use eq_client_render::{Preview, Source, ViewerConfig, script::Program};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let install = PathBuf::from(
        arguments
            .next()
            .context("expected installation directory")?,
    );
    let output = PathBuf::from(arguments.next().context("expected output PNG path")?);
    let page: usize = arguments
        .next()
        .context("expected page 0, 1, 2 or 3")?
        .parse()?;
    ensure!(page <= 3, "page must be 0, 1, 2 or 3");
    let policy = match arguments.next().as_deref().unwrap_or("titanium") {
        "classic" => Rules::Classic,
        "titanium" => Rules::Titanium,
        "eqmac" => Rules::EqMac,
        other => anyhow::bail!("unknown creation policy {other}"),
    };
    let race = arguments.next().unwrap_or_else(|| "1".into()).parse()?;
    let gender = arguments.next().unwrap_or_else(|| "0".into()).parse()?;
    ensure!(gender <= 1, "gender is 0 or 1");
    let mut draft = Draft::new(policy, None, 0);
    draft.name = "Testhero".into();
    draft.race(race);
    ensure!(
        draft.character.race == race,
        "race is unavailable in this policy"
    );
    draft.character.gender = gender;
    draft.reconcile();
    if arguments.next().as_deref() == Some("last") {
        let rules = policy.appearance(race, gender);
        for (feature, values) in rules.fields() {
            if let Some(value) = values.into_iter().rfind(|value| *value != 255) {
                feature.set(&mut draft.character, value);
            }
        }
    }
    let zone = eq_client_assets::load_zone(&install, "ecommons")?;
    let mut source = String::from("wait 1000\n");
    for _ in 0..page {
        source.push_str("press enter\n");
    }
    let script = Program::parse(&source, output.parent().context("output needs a parent")?)?;
    let status = eq_client_render::run(
        zone,
        None,
        ViewerConfig {
            eq_directory: Some(install),
            screenshot: Some(output),
            screenshot_after: Some(8.0),
            script: Some(script),
            window_size: Some((1024, 768)),
            max_fps: Some(60),
            ..Default::default()
        },
        Source::Offline(Preview {
            character_select: true,
            creation: Some(draft),
            ..Default::default()
        }),
    );
    ensure!(status == 0, "viewer exited with {status}");
    Ok(())
}
