//! Offline selection audit using the normal selection loader, stand and camera.
//! Usage: `select_spot_preview INSTALL SPOTS ZONE OUTPUT [ORBIT_DEGREES]`
//! Or: `select_spot_preview INSTALL SPOTS ZONE OUTPUT_DIRECTORY batch`
//! Batch mode loads once and captures real0/90/180/270-degree camera views.
//! The hidden one-shot window sends no input or network traffic. Keep images
//! private; inspect them before independently marking any spot reviewed.
use anyhow::{Context, Result, ensure};
use eq_client_core::select_spots::Spots;
use eq_client_render::{Preview, SelectionCapture, SelectionPreview, Source, ViewerConfig};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let install = PathBuf::from(
        arguments
            .next()
            .context("expected installation directory")?,
    );
    let spots = PathBuf::from(arguments.next().context("expected spots file")?);
    let zone = arguments.next().context("expected zone short name")?;
    let mut output = PathBuf::from(arguments.next().context("expected output PNG path")?);
    let view = arguments.next().unwrap_or_else(|| "0".into());
    let batch = view == "batch";
    let orbit: f32 = if batch { 0.0 } else { view.parse()? };
    ensure!(orbit.is_finite(), "orbit angle must be finite");
    ensure!(arguments.next().is_none(), "unexpected argument");
    let spots = Spots::parse(&std::fs::read_to_string(spots)?).map_err(anyhow::Error::msg)?;
    let spot = *spots.get(&zone).context("zone is not in the spots file")?;
    let zone_id = eq_client_core::zones::number(&zone).context("unknown zone")?;
    let asset = eq_client_assets::load_zone(&install, &zone)?;
    let additional_captures = if batch {
        std::fs::create_dir_all(&output)?;
        let captures = [90.0_f32, 180.0, 270.0].map(|angle| SelectionCapture {
            orbit: angle.to_radians(),
            output: output.join(format!("{angle:03.0}.png")),
        });
        output = output.join("000.png");
        captures.to_vec()
    } else {
        Vec::new()
    };
    let status = eq_client_render::run(
        asset,
        None,
        ViewerConfig {
            eq_directory: Some(install),
            screenshot: Some(output),
            screenshot_after: Some(5.0),
            window_size: Some((1024, 768)),
            max_fps: Some(30),
            ..Default::default()
        },
        Source::Offline(Preview {
            selection: Some(SelectionPreview {
                zone_id,
                spot,
                orbit: orbit.to_radians(),
                additional_captures,
            }),
            ..Default::default()
        }),
    );
    ensure!(status == 0, "viewer exited with {status}");
    Ok(())
}
