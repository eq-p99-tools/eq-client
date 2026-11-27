//! Script screenshots keep the resolved local directory out of automatic logs.
use super::Step;
use bevy::{prelude::*, render::view::screenshot::ScreenshotCaptured};
use std::path::PathBuf;

pub(super) fn log_step(step: &Step) {
    if matches!(step, Step::Screenshot(_)) {
        info!(step = "Screenshot", "Script step");
    } else {
        info!(?step, "Script step");
    }
}

pub(super) fn save(path: PathBuf) -> impl FnMut(On<ScreenshotCaptured>) {
    move |event| {
        let Ok(image) = event.image.clone().try_into_dynamic() else {
            error!("Could not convert script screenshot");
            return;
        };
        // HDR uses alpha for brightness. Preserve Bevy's RGB screenshot output.
        if image
            .to_rgb8()
            .save_with_format(&path, image::ImageFormat::Png)
            .is_ok()
        {
            info!("Saved script screenshot");
        } else {
            error!("Could not save script screenshot");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::log::{tracing, tracing_subscriber};
    use std::{fs, time::SystemTime};

    #[test]
    fn hotbutton_screenshot_saves_png_without_logging_the_script_directory() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let root = std::env::temp_dir().join(format!(
            "eq-private-script-{}-{}",
            std::process::id(),
            unique.as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let script = root.join("snap.eqscript");
        fs::write(&script, "screenshot(\"output.png\")\n").unwrap();
        let steps = crate::script_workbench::hotbutton_program(&script, &root)
            .unwrap()
            .into_steps()
            .unwrap();
        let [step @ Step::Screenshot(path)] = steps.as_slice() else {
            panic!("expected a single screenshot");
        };
        assert_eq!(path, &root.join("output.png"));
        let log = root.join("events.log");
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_writer(fs::File::create(&log).unwrap())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            log_step(step);
            let mut world = World::new();
            let entity = world.spawn_empty().observe(save(path.clone())).id();
            world.flush();
            world.trigger(ScreenshotCaptured {
                entity,
                image: crate::create_image(1, 1, vec![255; 4]),
            });
        });
        assert_eq!(image::open(path).unwrap().to_rgb8().dimensions(), (1, 1));
        let events = fs::read_to_string(&log).unwrap();
        assert!(events.contains("Script step") && events.contains("Saved script screenshot"));
        assert!(!events.contains("eq-private-script-") && !events.contains("output.png"));
        for file in [&script, path, &log] {
            fs::remove_file(file).unwrap();
        }
        fs::remove_dir(&root).unwrap();
    }
}
