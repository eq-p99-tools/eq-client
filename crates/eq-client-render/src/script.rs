//! Opt-in scripted live checks that press keys through the ordinary input paths.
//!
//! A script never runs unattended: it pauses until the client window is focused,
//! stops if focus is lost while a key is held, and ends after a bounded time.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

const MAX_STEPS: usize = 500;
const MAX_HOLD: Duration = Duration::from_secs(10);
const MAX_WAIT: Duration = Duration::from_secs(120);
const MAX_ONLINE_WAIT: Duration = Duration::from_secs(180);
const MAX_RUNTIME: Duration = Duration::from_mins(15);

/// One scripted action.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Waits until the character selection list is shown.
    WaitSelect,
    /// Waits until the zone is admitted with a player present.
    WaitOnline,
    /// Presses keys together for one frame, modifiers first.
    Press(Vec<KeyCode>),
    /// Holds keys together for a bounded duration.
    Hold(Vec<KeyCode>, Duration),
    /// Pauses for a bounded duration.
    Wait(Duration),
    /// Logs a numeric summary of player, resource, cast and buff state.
    Report(String),
    /// Saves the primary window to a PNG beside the script.
    Screenshot(PathBuf),
    /// Closes the client.
    Quit,
}

/// Parses a script: one step per line, `#` starts a comment.
///
/// # Errors
/// Returns the first line that is unknown, malformed or outside the bounds.
pub fn parse(text: &str, base: &Path) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let fail = |reason: &str| format!("script line {}: {reason}", index + 1);
        let mut words = line.split_whitespace();
        let command = words.next().unwrap_or_default();
        let rest: Vec<&str> = words.collect();
        let millis = |value: Option<&&str>, limit: Duration| {
            value
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_millis)
                .filter(|duration| *duration <= limit)
                .ok_or_else(|| {
                    fail(&format!(
                        "expected milliseconds up to {}",
                        limit.as_millis()
                    ))
                })
        };
        let step = match (command, rest.as_slice()) {
            ("wait_select", []) => Step::WaitSelect,
            ("wait_online", []) => Step::WaitOnline,
            ("press", [keys]) => Step::Press(keys_from(keys).ok_or_else(|| fail("unknown key"))?),
            ("hold", [keys, _]) => Step::Hold(
                keys_from(keys).ok_or_else(|| fail("unknown key"))?,
                millis(rest.get(1), MAX_HOLD)?,
            ),
            ("wait", [_]) => Step::Wait(millis(rest.first(), MAX_WAIT)?),
            ("report", label) => Step::Report(label.join(" ")),
            ("screenshot", [name])
                if Path::new(name)
                    .file_name()
                    .is_some_and(|file| file == std::ffi::OsStr::new(name))
                    && Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("png")) =>
            {
                Step::Screenshot(base.join(name))
            }
            ("quit", []) => Step::Quit,
            _ => return Err(fail("unknown or malformed step")),
        };
        steps.push(step);
    }
    if steps.len() > MAX_STEPS {
        return Err(format!("script has more than {MAX_STEPS} steps"));
    }
    Ok(steps)
}

fn keys_from(text: &str) -> Option<Vec<KeyCode>> {
    text.split('+').map(key).collect()
}

fn key(name: &str) -> Option<KeyCode> {
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    const FUNCTIONS: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    match upper.as_str() {
        "SPACE" => Some(KeyCode::Space),
        "ESCAPE" => Some(KeyCode::Escape),
        "ENTER" => Some(KeyCode::Enter),
        "TAB" => Some(KeyCode::Tab),
        "UP" => Some(KeyCode::ArrowUp),
        "DOWN" => Some(KeyCode::ArrowDown),
        "LEFT" => Some(KeyCode::ArrowLeft),
        "RIGHT" => Some(KeyCode::ArrowRight),
        "SHIFT" => Some(KeyCode::ShiftLeft),
        "CTRL" => Some(KeyCode::ControlLeft),
        "ALT" => Some(KeyCode::AltLeft),
        _ if bytes.len() == 1 && bytes[0].is_ascii_uppercase() => {
            Some(LETTERS[usize::from(bytes[0] - b'A')])
        }
        _ if bytes.len() == 1 && bytes[0].is_ascii_digit() => {
            Some(DIGITS[usize::from(bytes[0] - b'0')])
        }
        _ => upper
            .strip_prefix('F')
            .and_then(|number| number.parse::<usize>().ok())
            .and_then(|number| FUNCTIONS.get(number.checked_sub(1)?).copied()),
    }
}

/// Remaining steps and the one in progress.
#[derive(Resource)]
pub struct Script {
    steps: VecDeque<Step>,
    current: Option<(Step, Duration)>,
    held: Vec<KeyCode>,
    started: Option<Duration>,
    paused: bool,
}

impl Script {
    /// Queues validated steps.
    #[must_use]
    pub fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: steps.into(),
            current: None,
            held: Vec::new(),
            started: None,
            paused: false,
        }
    }

    fn stop(&mut self, keys: &mut ButtonInput<KeyCode>, reason: &str) {
        for key in self.held.drain(..) {
            keys.release(key);
        }
        self.steps.clear();
        self.current = None;
        info!("Script stopped: {reason}");
    }
}

/// Runs after Bevy's keyboard update so injected presses are seen this frame.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn drive(
    mut commands: Commands,
    time: Res<Time<Real>>,
    script: Option<ResMut<Script>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    online: Res<super::online::OnlineState>,
    hud: Res<super::hud::HudState>,
    players: Query<&Transform, With<super::Player>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut script) = script else {
        return;
    };
    let now = time.elapsed();
    if script.current.is_none() && script.steps.is_empty() {
        commands.remove_resource::<Script>();
        info!("Script finished");
        return;
    }
    let started = *script.started.get_or_insert(now);
    if now.saturating_sub(started) > MAX_RUNTIME {
        script.stop(&mut keys, "maximum runtime reached");
        return;
    }
    if !windows.single().is_ok_and(|window| window.focused) {
        if !script.held.is_empty() {
            script.stop(&mut keys, "window lost focus while holding keys");
        } else if !script.paused {
            script.paused = true;
            info!("Script paused until the client window is focused");
        }
        return;
    }
    if std::mem::take(&mut script.paused) {
        info!("Script resumed");
    }
    // One-frame presses are released on the frame after they were pressed.
    if matches!(script.current, Some((Step::Press(_), _))) {
        for key in std::mem::take(&mut script.held) {
            keys.release(key);
        }
        script.current = None;
    }
    if let Some((step, since)) = script.current.clone() {
        let elapsed = now.saturating_sub(since);
        let done = match &step {
            Step::Hold(_, duration) | Step::Wait(duration) => elapsed >= *duration,
            Step::WaitSelect | Step::WaitOnline => {
                if elapsed > MAX_ONLINE_WAIT {
                    script.stop(&mut keys, "server state did not arrive");
                    return;
                }
                if step == Step::WaitSelect {
                    online.selection.is_some()
                } else {
                    online.connected && online.player.is_some()
                }
            }
            _ => true,
        };
        if !done {
            return;
        }
        for key in std::mem::take(&mut script.held) {
            keys.release(key);
        }
        script.current = None;
    }
    let Some(step) = script.steps.pop_front() else {
        return;
    };
    info!(?step, "Script step");
    match &step {
        Step::Press(chord) | Step::Hold(chord, _) => {
            for key in chord {
                keys.press(*key);
            }
            script.held.clone_from(chord);
        }
        Step::Report(label) => {
            report(label, &online, &hud, players.single().ok());
            return;
        }
        Step::Screenshot(path) => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            return;
        }
        Step::Quit => {
            script.stop(&mut keys, "quit requested");
            exit.write(AppExit::Success);
            return;
        }
        Step::WaitSelect | Step::WaitOnline | Step::Wait(_) => (),
    }
    script.current = Some((step, now));
}

fn report(
    label: &str,
    online: &super::online::OnlineState,
    hud: &super::hud::HudState,
    transform: Option<&Transform>,
) {
    let position = transform.map(|transform| {
        let world = super::world_position(transform.translation.to_array(), 0.0);
        let forward = transform.forward();
        (
            world.x,
            world.y,
            world.z,
            forward.x.atan2(forward.z).to_degrees(),
        )
    });
    let slots: Vec<u32> = hud
        .buff_state
        .slots()
        .map(|slots| slots.values().map(|buff| buff.spell_id).collect())
        .unwrap_or_default();
    let effects: Vec<u16> = hud.buff_state.effects().keys().copied().collect();
    let posture = online
        .player
        .as_ref()
        .and_then(|player| online.postures.get(&player.spawn_id));
    info!(
        label,
        zone = online.zone,
        connected = online.connected,
        ?position,
        ?posture,
        hp = ?hud.hp,
        mana = ?hud.mana,
        endurance = ?hud.endurance,
        estimate = ?hud.resource_estimate,
        casting = ?hud.casting.map(|(spell, _, _)| spell),
        pending = ?hud.pending_cast,
        interrupted = ?hud.interrupted.map(|(_, id)| id),
        feedback = ?hud.action_feedback.as_ref().map(|(_, text)| text),
        ?slots,
        ?effects,
        "Script report"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_steps_and_rejects_unsafe_input() {
        let base = Path::new("private");
        let steps = parse(
            "wait_select\nwait_online\npress F1 # self\npress alt+1\nhold W 1500\nwait 250\n\
             report after cast\nscreenshot a.png\nquit\n",
            base,
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                Step::WaitSelect,
                Step::WaitOnline,
                Step::Press(vec![KeyCode::F1]),
                Step::Press(vec![KeyCode::AltLeft, KeyCode::Digit1]),
                Step::Hold(vec![KeyCode::KeyW], Duration::from_millis(1500)),
                Step::Wait(Duration::from_millis(250)),
                Step::Report("after cast".into()),
                Step::Screenshot(base.join("a.png")),
                Step::Quit,
            ]
        );
        for bad in [
            "hold W 20000",
            "wait 999999",
            "press Nope",
            "screenshot ../a.png",
            "screenshot a.jpg",
            "teleport 1 2 3",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }
}
