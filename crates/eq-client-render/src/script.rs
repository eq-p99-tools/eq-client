//! Opt-in scripted live checks that press keys through the ordinary input paths.
//!
//! A script never runs unattended: it pauses until the client window is focused,
//! stops if focus is lost while a key is held, and ends after a bounded time.
//! Scripted clicks are only injected while the real pointer is outside the window,
//! so they cannot also press whatever the pointer happens to be over.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

const MAX_STEPS: usize = 500;
const MAX_HOLD: Duration = Duration::from_secs(10);
const MAX_WAIT: Duration = Duration::from_secs(120);
const MAX_TRACE: Duration = Duration::from_secs(10);
const MAX_ONLINE_WAIT: Duration = Duration::from_secs(180);
const MAX_RUNTIME: Duration = Duration::from_mins(15);

/// One scripted action.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Waits until the character selection list is shown.
    WaitSelect,
    /// Highlights a listed character by name and presses Enter.
    Select(String),
    /// Waits until the zone is admitted with a player present.
    WaitOnline,
    /// Waits until the named zone is admitted with a player present.
    WaitZone(String),
    /// Runs a game slash command such as `/camp`; chat text is refused.
    Slash(String),
    /// Presses keys together for one frame, modifiers first.
    Press(Vec<KeyCode>),
    /// Holds keys together for a bounded duration.
    Hold(Vec<KeyCode>, Duration),
    /// Pauses for a bounded duration.
    Wait(Duration),
    /// Orbits the camera so W walks along an EQ heading (0..512), at a pitch in degrees.
    Camera {
        /// EQ heading the camera looks along.
        heading: f32,
        /// Adds the player's current heading, for views relative to the model.
        relative: bool,
        /// Downward pitch in degrees, clamped like mouse orbiting.
        pitch: f32,
    },
    /// Logs the player's position and frame time every frame for a bounded duration.
    Trace(Duration),
    /// Left-clicks one UI control through Bevy's ordinary interaction state.
    Click(ClickTarget),
    /// Logs a numeric summary of player, resource, cast and buff state.
    Report(String),
    /// Saves the primary window to a PNG beside the script.
    Screenshot(PathBuf),
    /// Closes the client.
    Quit,
}

/// UI controls a script may click.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClickTarget {
    /// An inventory slot button by server slot number.
    Slot(i32),
    /// The spellbook's scribe-cursor-scroll button.
    Scribe,
    /// The inventory's auto-store-cursor button.
    Store,
    /// A spellbook row on the open page, from zero.
    BookRow(usize),
    /// The spellbook's memorize button for a gem, from zero.
    MemorizeGem(u8),
    /// A loot, merchant or window-closing button.
    Trade(TradeClick),
}

/// Loot and merchant window buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TradeClick {
    /// Take one corpse slot.
    Take(u16),
    /// Take everything.
    TakeAll,
    /// Close the loot window.
    EndLoot,
    /// Buy one unit from a merchant slot.
    Buy(u32),
    /// Sell an inventory slot.
    Sell(i32),
    /// Close the merchant window.
    EndShop,
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
        let step =
            parse_step(line).map_err(|reason| format!("script line {}: {reason}", index + 1))?;
        steps.push(match step {
            Step::Screenshot(name) => Step::Screenshot(base.join(name)),
            step => step,
        });
    }
    if steps.len() > MAX_STEPS {
        return Err(format!("script has more than {MAX_STEPS} steps"));
    }
    Ok(steps)
}

fn parse_step(line: &str) -> Result<Step, String> {
    let mut words = line.split_whitespace();
    let command = words.next().unwrap_or_default();
    let rest: Vec<&str> = words.collect();
    let millis = |value: &str, limit: Duration| {
        value
            .parse::<u64>()
            .ok()
            .map(Duration::from_millis)
            .filter(|duration| *duration <= limit)
            .ok_or_else(|| format!("expected milliseconds up to {}", limit.as_millis()))
    };
    let number = |value: &str, low: f32, high: f32| {
        value
            .parse::<f32>()
            .ok()
            .filter(|value| (low..=high).contains(value))
            .ok_or_else(|| format!("expected a number from {low} to {high}"))
    };
    let chord = |keys: &str| keys_from(keys).ok_or_else(|| String::from("unknown key"));
    Ok(match (command, rest.as_slice()) {
        ("wait_select", []) => Step::WaitSelect,
        ("select", [name]) => Step::Select((*name).to_owned()),
        ("wait_online", []) => Step::WaitOnline,
        ("wait_zone", [zone]) => Step::WaitZone(zone.to_ascii_lowercase()),
        ("slash", [command]) if matches!(*command, "camp" | "sit" | "stand") => {
            Step::Slash(format!("/{command}"))
        }
        ("press", [keys]) => Step::Press(chord(keys)?),
        ("hold", [keys, duration]) => Step::Hold(chord(keys)?, millis(duration, MAX_HOLD)?),
        ("wait", [duration]) => Step::Wait(millis(duration, MAX_WAIT)?),
        ("camera", [heading, pitch]) => Step::Camera {
            heading: number(heading, 0.0, 512.0)?,
            relative: false,
            pitch: number(pitch, -83.0, -8.0)?,
        },
        ("camera", ["player", offset, pitch]) => Step::Camera {
            heading: number(offset, 0.0, 512.0)?,
            relative: true,
            pitch: number(pitch, -83.0, -8.0)?,
        },
        ("trace", [duration]) => Step::Trace(millis(duration, MAX_TRACE)?),
        ("click", ["slot", slot]) => Step::Click(ClickTarget::Slot(
            slot.parse()
                .map_err(|_| String::from("expected a slot number"))?,
        )),
        ("click", ["scribe"]) => Step::Click(ClickTarget::Scribe),
        ("click", ["store"]) => Step::Click(ClickTarget::Store),
        ("click", ["book", row]) => Step::Click(ClickTarget::BookRow(
            row.parse()
                .map_err(|_| String::from("expected a book row number"))?,
        )),
        ("click", ["loot", slot]) => Step::Click(ClickTarget::Trade(TradeClick::Take(
            slot.parse()
                .map_err(|_| String::from("expected a corpse slot"))?,
        ))),
        ("click", ["loot_all"]) => Step::Click(ClickTarget::Trade(TradeClick::TakeAll)),
        ("click", ["loot_done"]) => Step::Click(ClickTarget::Trade(TradeClick::EndLoot)),
        ("click", ["buy", slot]) => Step::Click(ClickTarget::Trade(TradeClick::Buy(
            slot.parse()
                .map_err(|_| String::from("expected a merchant slot"))?,
        ))),
        ("click", ["sell", slot]) => Step::Click(ClickTarget::Trade(TradeClick::Sell(
            slot.parse()
                .map_err(|_| String::from("expected an inventory slot"))?,
        ))),
        ("click", ["shop_done"]) => Step::Click(ClickTarget::Trade(TradeClick::EndShop)),
        ("click", ["memorize", gem]) => Step::Click(ClickTarget::MemorizeGem(
            gem.parse::<u8>()
                .ok()
                .filter(|gem| (1..=8).contains(gem))
                .map(|gem| gem - 1)
                .ok_or_else(|| String::from("expected a gem from 1 to 8"))?,
        )),
        ("report", label) => Step::Report(label.join(" ")),
        ("screenshot", [name])
            if Path::new(name)
                .file_name()
                .is_some_and(|file| file == std::ffi::OsStr::new(name))
                && Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png")) =>
        {
            Step::Screenshot(PathBuf::from(name))
        }
        ("quit", []) => Step::Quit,
        _ => return Err("unknown or malformed step".into()),
    })
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
    clicked: bool,
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
            clicked: false,
            started: None,
            paused: false,
        }
    }

    fn release(&mut self, keys: &mut ButtonInput<KeyCode>, mouse: &mut ButtonInput<MouseButton>) {
        for key in self.held.drain(..) {
            keys.release(key);
        }
        if std::mem::take(&mut self.clicked) {
            mouse.release(MouseButton::Left);
        }
    }

    fn stop(
        &mut self,
        keys: &mut ButtonInput<KeyCode>,
        mouse: &mut ButtonInput<MouseButton>,
        reason: &str,
    ) {
        self.release(keys, mouse);
        self.steps.clear();
        self.current = None;
        info!("Script stopped: {reason}");
    }
}

type Buttons<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Interaction,
        &'static InheritedVisibility,
        Option<&'static super::inventory::SlotButton>,
        Has<super::spellbook::ScribeCursor>,
        Has<super::inventory::StoreCursor>,
        Option<&'static super::spellbook::BookEntry>,
        Option<&'static super::spellbook::GemChoice>,
        Option<&'static super::trade::Action>,
    ),
>;

type Observed<'w> = (
    Res<'w, super::hud::HudState>,
    Res<'w, super::inventory::InventoryState>,
    Res<'w, super::target::TargetState>,
    Res<'w, super::target::CommandsToServer>,
);

type Input<'w> = (
    ResMut<'w, ButtonInput<KeyCode>>,
    ResMut<'w, ButtonInput<MouseButton>>,
);

/// Runs after Bevy's keyboard and UI focus updates so injected input is seen this frame.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
pub(super) fn drive(
    mut commands: Commands,
    time: Res<Time<Real>>,
    script: Option<ResMut<Script>>,
    input: Input,
    mut online: ResMut<super::online::OnlineState>,
    observed: Observed,
    players: Query<&Transform, With<super::Player>>,
    mut cameras: Query<&mut super::OrbitCamera>,
    mut buttons: Buttons,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut script) = script else {
        return;
    };
    let (mut keys, mut mouse) = input;
    let now = time.elapsed();
    if script.current.is_none() && script.steps.is_empty() {
        commands.remove_resource::<Script>();
        info!("Script finished");
        return;
    }
    let started = *script.started.get_or_insert(now);
    if now.saturating_sub(started) > MAX_RUNTIME {
        script.stop(&mut keys, &mut mouse, "maximum runtime reached");
        return;
    }
    let window = windows.single().ok();
    if !window.is_some_and(|window| window.focused) {
        if !script.held.is_empty() {
            script.stop(
                &mut keys,
                &mut mouse,
                "window lost focus while holding keys",
            );
        } else if !script.paused {
            script.paused = true;
            info!("Script paused until the client window is focused");
        }
        return;
    }
    if std::mem::take(&mut script.paused) {
        info!("Script resumed");
    }
    // One-frame presses and clicks are released on the frame after they were pressed.
    if matches!(
        script.current,
        Some((Step::Press(_) | Step::Select(_) | Step::Click(_), _))
    ) && (!script.held.is_empty() || script.clicked)
    {
        script.release(&mut keys, &mut mouse);
        script.current = None;
    }
    if let Some((step, since)) = script.current.clone() {
        let elapsed = now.saturating_sub(since);
        let done = match &step {
            Step::Hold(_, duration) | Step::Wait(duration) => elapsed >= *duration,
            Step::Trace(duration) => {
                if let Ok(transform) = players.single() {
                    let (x, y, z, heading) = placement(transform);
                    debug!(
                        dt_ms = time.delta_secs() * 1000.0,
                        x, y, z, heading, "Script trace"
                    );
                }
                elapsed >= *duration
            }
            Step::WaitSelect | Step::WaitOnline | Step::WaitZone(_) => {
                if elapsed > MAX_ONLINE_WAIT {
                    script.stop(&mut keys, &mut mouse, "server state did not arrive");
                    return;
                }
                match &step {
                    Step::WaitSelect => online.selection.is_some(),
                    Step::WaitZone(zone) => {
                        online.connected && online.player.is_some() && online.zone == *zone
                    }
                    _ => online.connected && online.player.is_some(),
                }
            }
            Step::Click(target) => {
                if elapsed > MAX_WAIT {
                    script.stop(&mut keys, &mut mouse, "the pointer stayed over the window");
                    return;
                }
                if window.is_some_and(|window| window.cursor_position().is_some()) {
                    return;
                }
                if !click(*target, &mut buttons) {
                    script.stop(&mut keys, &mut mouse, "click target is not visible");
                    return;
                }
                mouse.press(MouseButton::Left);
                script.clicked = true;
                return;
            }
            _ => true,
        };
        if !done {
            return;
        }
        script.release(&mut keys, &mut mouse);
        script.current = None;
    }
    let Some(step) = script.steps.pop_front() else {
        return;
    };
    info!(?step, "Script step");
    match &step {
        Step::Slash(command) => {
            if let Err(error) = super::chat::submit_game_command(command, &online, &observed.3) {
                script.stop(&mut keys, &mut mouse, &error);
            }
            return;
        }
        Step::Press(chord) | Step::Hold(chord, _) => {
            for key in chord {
                keys.press(*key);
            }
            script.held.clone_from(chord);
        }
        Step::Select(name) => {
            if !online
                .selection
                .as_mut()
                .is_some_and(|selection| selection.choose_named(name))
            {
                script.stop(&mut keys, &mut mouse, "character is not listed");
                return;
            }
            keys.press(KeyCode::Enter);
            script.held = vec![KeyCode::Enter];
        }
        Step::Camera {
            heading,
            relative,
            pitch,
        } => {
            let base = if *relative {
                players
                    .single()
                    .map_or(0.0, |transform| placement(transform).3)
            } else {
                0.0
            };
            for mut camera in &mut cameras {
                // W walks along the camera's forward direction, opposite its orbit offset.
                camera.yaw = eq_client_core::render_heading(base + *heading) + std::f32::consts::PI;
                camera.pitch = pitch.to_radians().clamp(-1.45, -0.15);
            }
            return;
        }
        Step::Click(_) => {
            if window.is_some_and(|window| window.cursor_position().is_some()) {
                info!("Scripted click waits until the pointer leaves the client window");
            }
        }
        Step::Report(label) => {
            report(label, &online, &observed, players.single().ok());
            return;
        }
        Step::Screenshot(path) => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            return;
        }
        Step::Quit => {
            script.stop(&mut keys, &mut mouse, "quit requested");
            exit.write(AppExit::Success);
            return;
        }
        Step::WaitSelect
        | Step::WaitOnline
        | Step::WaitZone(_)
        | Step::Wait(_)
        | Step::Trace(_) => (),
    }
    script.current = Some((step, now));
}

/// Marks the first visible matching control pressed; the focus system clears it next frame.
fn click(target: ClickTarget, buttons: &mut Buttons) -> bool {
    for (mut interaction, visibility, slot, scribe, store, row, gem, trade) in buttons.iter_mut() {
        let matches = match target {
            ClickTarget::Slot(number) => slot.is_some_and(|slot| slot.0.0 == number),
            ClickTarget::Scribe => scribe,
            ClickTarget::Store => store,
            ClickTarget::BookRow(number) => row.is_some_and(|row| row.0 == number),
            ClickTarget::MemorizeGem(number) => gem.is_some_and(|gem| gem.0 == number),
            ClickTarget::Trade(click) => trade.is_some_and(|action| {
                use super::trade::Action;
                *action
                    == match click {
                        TradeClick::Take(slot) => Action::Take(slot),
                        TradeClick::TakeAll => Action::TakeAll,
                        TradeClick::EndLoot => Action::EndLoot,
                        TradeClick::Buy(slot) => Action::Buy(slot),
                        TradeClick::Sell(slot) => Action::Sell(slot),
                        TradeClick::EndShop => Action::EndShop,
                    }
            }),
        };
        if matches && visibility.get() {
            *interaction = Interaction::Pressed;
            return true;
        }
    }
    false
}

/// EQ coordinates and heading of the movement root.
fn placement(transform: &Transform) -> (f32, f32, f32, f32) {
    let world = super::world_position(transform.translation.to_array(), 0.0);
    let facing = transform.rotation * Vec3::Z;
    let heading = eq_client_core::world_heading(facing.x.atan2(facing.z));
    (world.x, world.y, world.z, heading)
}

fn report(
    label: &str,
    online: &super::online::OnlineState,
    (hud, inventory, target, _): &Observed,
    transform: Option<&Transform>,
) {
    let position = transform.map(placement);
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
    let items: Vec<(i32, u32, Option<u32>, Option<u32>)> = inventory
        .data
        .items()
        .values()
        .map(|item| {
            (
                item.slot.0,
                item.details.id,
                item.stack_count,
                item.scroll_spell,
            )
        })
        .collect();
    let book: Vec<(usize, u32)> = hud
        .spell_book
        .as_ref()
        .map(|book| {
            book.slots()
                .iter()
                .enumerate()
                .filter_map(|(slot, spell)| spell.map(|spell| (slot, spell)))
                .collect()
        })
        .unwrap_or_default();
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
        target = ?target.selected,
        gems = ?hud.spells,
        ?book,
        ?slots,
        ?effects,
        ?items,
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
            "wait_select\nselect Someone\nwait_online\nwait_zone TOX\nslash camp\npress F1 # self\n\
             press alt+1\nhold W 1500\nwait 250\ncamera 128 -20\ncamera player 256 -15\ntrace 2000\nclick slot 23\n\
             click scribe\nclick store\nclick book 0\nclick memorize 2\nclick loot 22\nclick loot_all\nclick buy 3\nclick sell 23\nclick shop_done\nreport after cast\nscreenshot a.png\nquit\n",
            base,
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                Step::WaitSelect,
                Step::Select("Someone".into()),
                Step::WaitOnline,
                Step::WaitZone("tox".into()),
                Step::Slash("/camp".into()),
                Step::Press(vec![KeyCode::F1]),
                Step::Press(vec![KeyCode::AltLeft, KeyCode::Digit1]),
                Step::Hold(vec![KeyCode::KeyW], Duration::from_millis(1500)),
                Step::Wait(Duration::from_millis(250)),
                Step::Camera {
                    heading: 128.0,
                    relative: false,
                    pitch: -20.0
                },
                Step::Camera {
                    heading: 256.0,
                    relative: true,
                    pitch: -15.0
                },
                Step::Trace(Duration::from_millis(2000)),
                Step::Click(ClickTarget::Slot(23)),
                Step::Click(ClickTarget::Scribe),
                Step::Click(ClickTarget::Store),
                Step::Click(ClickTarget::BookRow(0)),
                Step::Click(ClickTarget::MemorizeGem(1)),
                Step::Click(ClickTarget::Trade(TradeClick::Take(22))),
                Step::Click(ClickTarget::Trade(TradeClick::TakeAll)),
                Step::Click(ClickTarget::Trade(TradeClick::Buy(3))),
                Step::Click(ClickTarget::Trade(TradeClick::Sell(23))),
                Step::Click(ClickTarget::Trade(TradeClick::EndShop)),
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
            "camera 600 -20",
            "camera 10 -90",
            "trace 60000",
            "click slot x",
            "click anything",
            "click memorize 9",
            "click memorize 0",
            "slash say hello",
            "slash ooc",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn camera_heading_makes_w_walk_along_that_heading() {
        for heading in [0.0_f32, 100.0, 256.0, 400.0] {
            let yaw = eq_client_core::render_heading(heading) + std::f32::consts::PI;
            let walk = super::super::camera_relative_direction(0.0, 1.0, yaw);
            let expected = eq_client_core::world_heading(walk.x.atan2(walk.z));
            let error = (expected - heading).rem_euclid(512.0);
            assert!(error.min(512.0 - error) < 0.01, "{heading} -> {expected}");
        }
    }
}
