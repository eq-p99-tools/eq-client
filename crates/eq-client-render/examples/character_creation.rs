//! Offline creation-form screenshot using a real, user-owned installation.
//! Usage: `cargo run -p eq-client-render --example character_creation -- INSTALL OUTPUT PAGE`
//! PAGE is 0 (identity), 1 (attributes), or 2 (home/review). No server is contacted.
use anyhow::{Context, Result, ensure};
use eq_client_render::{Preview, Source, ViewerConfig, script::Program};
use std::{fmt::Write, path::PathBuf};

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
        .context("expected page 0, 1 or 2")?
        .parse()?;
    ensure!(page <= 2, "page must be 0, 1 or 2");
    let zone = eq_client_assets::load_zone(&install, "ecommons")?;
    let mut source = String::from("wait 1000\npress c\n");
    for letter in "testhero".chars() {
        writeln!(source, "press {letter}")?;
    }
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
            ..Default::default()
        }),
    );
    ensure!(status == 0, "viewer exited with {status}");
    Ok(())
}
