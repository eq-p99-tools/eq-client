//! Audio output probe for the named cue path, using generated silence.
use super::*;
use bevy::audio::AudioSinkPlayback;
use eq_client_core::effects::Cue;
use std::time::{Duration, Instant};

fn wait_for<T>(app: &mut App, expected: &str, mut observe: impl FnMut(&mut App) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        app.update();
        assert!(
            !app.world().resource::<Sounds>().no_output,
            "Bevy did not attach an audio sink after device preflight"
        );
        if let Some(result) = observe(app) {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn active_sinks(app: &mut App) -> Vec<(Entity, f32)> {
    app.world_mut()
        .query::<(Entity, &Playing, &AudioSink)>()
        .iter(app.world())
        .filter(|(_, voice, sink)| voice.mp3 && !sink.empty())
        .map(|(entity, voice, _)| (entity, voice.until))
        .collect()
}

#[test]
#[ignore = "requires EQ_MP3_PROBE_DIR and a default audio device; generated silence, no window"]
fn named_mp3_attaches_finishes_and_resets() {
    let folder = PathBuf::from(std::env::var_os("EQ_MP3_PROBE_DIR").unwrap());
    assert!(
        folder.is_absolute(),
        "use a fresh absolute fixture directory"
    );
    match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(device) => drop(device),
        Err(error) => {
            eprintln!("SKIP_NO_AUDIO_OUTPUT: {error}");
            return;
        }
    }
    std::fs::create_dir(&folder).unwrap();
    // Roughly 2.6 seconds, long enough to observe a nonempty sink before finish.
    std::fs::write(folder.join("Probe.MP3"), mp3_silence(100)).unwrap();

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::audio::AudioPlugin::default(),
    ))
    .insert_resource(crate::ViewerSettings(crate::ViewerConfig {
        eq_directory: Some(folder),
        installed_client: eq_client_assets::ui::InstalledClient::Titanium,
        ..default()
    }))
    .init_resource::<Sounds>()
    .init_resource::<crate::options::OptionsState>()
    .add_systems(Update, (load, update).chain());

    app.update();
    let cue = Cue::File("PROBE.mp3".into());
    app.world_mut().resource_mut::<Sounds>().play_cue(&cue);
    let (first, until) = wait_for(&mut app, "a nonempty MP3 sink", |app| {
        active_sinks(app).first().copied()
    });
    wait_for(&mut app, "natural MP3 despawn", |app| {
        app.world().get_entity(first).is_err().then_some(())
    });
    let elapsed = app
        .world()
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs();
    assert!(
        elapsed <= until,
        "voice disappeared after its cleanup deadline"
    );

    let plays_before = app.world().resource::<Sounds>().plays;
    for _ in 0..=MP3_VOICES {
        app.world_mut().resource_mut::<Sounds>().play_cue(&cue);
    }
    let voices = wait_for(&mut app, "the capped MP3 voices", |app| {
        let sinks = active_sinks(app);
        (sinks.len() >= MP3_VOICES).then_some(sinks)
    });
    assert_eq!(voices.len(), MP3_VOICES);
    assert_eq!(
        app.world().resource::<Sounds>().plays - plays_before,
        MP3_VOICES as u64
    );

    let mut sounds = app.world_mut().resource_mut::<Sounds>();
    sounds.play_cue(&cue);
    assert_eq!(sounds.asked.len(), 1);
    sounds.reset();
    app.update();
    let sounds = app.world().resource::<Sounds>();
    assert!(sounds.asked.is_empty());
    assert_eq!(sounds.plays - plays_before, MP3_VOICES as u64);
    assert!(
        voices
            .iter()
            .all(|(entity, _)| app.world().get_entity(*entity).is_err())
    );
    eprintln!("MP3_SINK_LIFECYCLE_OK: generated silence attached, finished and reset");
}
