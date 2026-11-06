//! Read the selected Titanium installation's numeric movement and posture bindings.
use super::{Act, Chord, KeyMap, scan_key};
use bevy::log::warn;
use std::{collections::BTreeMap, path::Path};

/// Movement command names in Titanium's numeric key mapping table.
const COMMANDS: [(Act, &str); 11] = [
    (Act::Forward, "FORWARD"),
    (Act::Back, "BACK"),
    (Act::TurnLeft, "LEFT"),
    (Act::TurnRight, "RIGHT"),
    (Act::StrafeLeft, "STRAFE_LEFT"),
    (Act::StrafeRight, "STRAFE_RIGHT"),
    (Act::Duck, "DUCK"),
    (Act::SitStand, "SIT_STAND"),
    (Act::Walk, "RUN_WALK"),
    (Act::Jump, "JUMP"),
    (Act::Autorun, "AUTORUN"),
];

impl KeyMap {
    pub(super) fn import_titanium_movement(&mut self, directory: &Path) {
        let defaults = read(directory, "defaults.ini");
        let saved = read(directory, "eqclient.ini");
        let defaults = keymaps(&defaults);
        let saved = keymaps(&saved);
        let value = |key: &str| saved.get(key).or_else(|| defaults.get(key)).copied();
        let mut skipped_legacy = 0;
        let mut skipped_modified = 0;
        let mut skipped_scan = 0;

        for (act, command) in COMMANDS {
            let chords = self.0.entry(act).or_default();
            *chords = std::array::from_fn(|slot| {
                let legacy = if slot == 0 {
                    format!("KEYMAP_{command}")
                } else {
                    format!("KEYMAP_{command}_2")
                };
                // Legacy values shadow numeric values. Honor a legacy
                // clear and leave other legacy slots unbound.
                if let Some(text) = value(&legacy).filter(|text| *text != "*NONE*") {
                    if !text.is_empty() && !text.eq_ignore_ascii_case("NOKEY") {
                        skipped_legacy += 1;
                    }
                    return None;
                }
                let numeric = format!("KEYMAPPING_{command}_{}", slot + 1);
                let Some(packed) = value(&numeric).and_then(decimal_prefix) else {
                    return chords[slot];
                };
                if packed == -1 {
                    return chords[slot];
                }
                let ctrl = packed & 0x2000_0000 != 0;
                let shift = packed & 0x4000_0000 != 0;
                let alt = packed & 0x1000_0000 != 0;
                if (ctrl || shift || alt)
                    && matches!(
                        act,
                        Act::Forward
                            | Act::Back
                            | Act::TurnLeft
                            | Act::TurnRight
                            | Act::StrafeLeft
                            | Act::StrafeRight
                    )
                {
                    // Held movement cannot yet latch native modified chords.
                    skipped_modified += 1;
                    return None;
                }
                let scan = packed.to_le_bytes()[0];
                let Some(key) = scan_key(scan) else {
                    skipped_scan += usize::from(scan != 0);
                    return None;
                };
                Some(Chord {
                    key,
                    ctrl,
                    shift,
                    alt,
                })
            });
        }
        if skipped_legacy + skipped_modified + skipped_scan > 0 {
            warn!(
                "Left Titanium movement/posture slots unbound ({skipped_legacy} legacy, {skipped_modified} modified movement, {skipped_scan} unsupported scan codes)"
            );
        }
    }
}

fn read(directory: &Path, name: &str) -> String {
    std::fs::read(directory.join(name)).map_or_else(
        |_| String::new(),
        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
    )
}

/// First `KeyMaps` section and first occurrence of a key, matching Titanium.
fn keymaps(text: &str) -> BTreeMap<String, &str> {
    let lines = text
        .lines()
        .skip_while(|line| !line.eq_ignore_ascii_case("[KeyMaps]"))
        .skip(1)
        .take_while(|line| !line.starts_with('['));
    let mut values = BTreeMap::new();
    for (key, value) in lines.filter_map(|line| line.split_once('=')) {
        values
            .entry(
                key.trim_end_matches(|c: char| c.is_ascii_whitespace())
                    .to_ascii_uppercase(),
            )
            .or_insert(value);
    }
    values
}

/// The numeric reader consumes a signed decimal prefix. Empty or nonnumeric
/// values have the same effect as zero (clear the slot).
fn decimal_prefix(text: &str) -> Option<i32> {
    let text = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits = text
        .strip_prefix(['-', '+'])
        .unwrap_or(text)
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if digits == 0 {
        return Some(0);
    }
    text[..digits + usize::from(text.starts_with(['-', '+']))]
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use eq_client_assets::ui::InstalledClient;

    #[test]
    fn titanium_layers_movement_and_posture_slots_without_stealing_chords() {
        let directory = std::env::temp_dir().join(format!(
            "eq-client-installed-movement-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let defaults = "[KeyMaps]\nKEYMAPPING_FORWARD_1=18\nKEYMAPPING_LEFT_2=0\n\
            KEYMAPPING_BACK_1=44\nKEYMAPPING_RIGHT_2=20\nKEYMAPPING_STRAFE_LEFT_1=30\n";
        let saved = "[Other]\nKEYMAPPING_RIGHT_1=20\n[KeyMaps]\n\
            keymapping_forward_1=17suffix\nKEYMAPPING_FORWARD_1=31\n\
            KEYMAPPING_BACK_1=31\nKEYMAPPING_LEFT_1=30\nKEYMAPPING_RIGHT_1=32\nKEYMAPPING_RIGHT_2=-1\n\
            KEYMAPPING_DUCK_1=45\nKEYMAPPING_SIT_STAND_1=536870930\n\
            KEYMAPPING_STRAFE_RIGHT_1=1073741841\nKEYMAP_FORWARD_2=CONTROL+w\n\
            KEYMAPPING_FORWARD_2=44\nKEYMAP_BACK_2=NOKEY\n[KeyMaps]\nKEYMAPPING_RIGHT_1=20\n";
        std::fs::write(directory.join("defaults.ini"), defaults).unwrap();
        std::fs::write(directory.join("eqclient.ini"), saved).unwrap();

        let mut settings = crate::ViewerConfig {
            eq_directory: Some(directory.clone()),
            installed_client: InstalledClient::Titanium,
            ..default()
        };
        let map = KeyMap::for_settings(&settings);
        assert_eq!(map.chords(Act::Forward), [Chord::key(KeyCode::KeyW)]);
        assert_eq!(map.chords(Act::Back), [Chord::key(KeyCode::KeyS)]);
        assert_eq!(map.chords(Act::TurnLeft), [Chord::key(KeyCode::KeyA)]);
        assert_eq!(
            map.chords(Act::TurnRight),
            [Chord::key(KeyCode::KeyD), Chord::key(KeyCode::ArrowRight)]
        );
        assert_eq!(map.chords(Act::StrafeLeft), [Chord::key(KeyCode::KeyA)]);
        assert_eq!(map.chords(Act::StrafeRight), [] as [Chord; 0]);
        assert_eq!(map.chords(Act::Duck), [Chord::key(KeyCode::KeyX)]);
        assert_eq!(map.chords(Act::SitStand), [Chord::ctrl(KeyCode::KeyE)]);
        let mut keyboard = ButtonInput::default();
        for (key, act) in [
            (KeyCode::KeyW, Act::Forward),
            (KeyCode::KeyS, Act::Back),
            (KeyCode::KeyA, Act::TurnLeft),
            (KeyCode::KeyD, Act::TurnRight),
        ] {
            keyboard.reset_all();
            keyboard.press(key);
            assert!(map.holding(&keyboard, &[]).held(act));
        }
        assert!(!map.just_pressed(
            &keyboard,
            Act::Duck,
            &super::super::presses::Presses::default()
        ));

        settings.installed_client = InstalledClient::EqMac;
        assert_eq!(
            KeyMap::for_settings(&settings).chords(Act::Duck),
            [Chord::key(KeyCode::KeyD)]
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("eqclient.ini")).unwrap(),
            saved
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("defaults.ini")).unwrap(),
            defaults
        );
        std::fs::remove_file(directory.join("defaults.ini")).unwrap();
        std::fs::remove_file(directory.join("eqclient.ini")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
