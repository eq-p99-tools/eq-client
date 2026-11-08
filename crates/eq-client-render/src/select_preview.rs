//! The character at character select: the highlighted character stands in
//! its own zone, at the zone's spot from the spots file (Adam's choice,
//! 10-19, over the official client's small themed arena), dressed from its
//! list's look by the same code that dresses everyone in the game, and the
//! camera orbits it. It shows only where the server type's list carries the
//! look; elsewhere the screen keeps its cover.
use super::{OrbitCamera, ViewerSettings, background::Background, online::OnlineState};
use bevy::prelude::*;
use eq_client_core::{outfit::Appearance, select_spots::Spots};

/// How fast the camera circles a character shown at select by itself, in
/// radians a second: one turn in about 20 s. Adam's choice (10-19 20:17Z,
/// and 23:47Z: the camera orbits, the character stands still): it circles
/// slowly until the player moves the camera, by right-drag or zoom, and
/// starts again when another character is highlighted.
const TURN: f32 = std::f32::consts::TAU / 20.0;

/// A serial offline screenshot queue. The one-shot screenshot path remains
/// unchanged unless an offline selection explicitly supplies further views.
#[derive(Resource)]
pub(super) struct AuditCaptures {
    remaining: std::collections::VecDeque<super::preview::SelectionCapture>,
    pending: Option<super::preview::SelectionCapture>,
    wait_frames: u8,
    orbit: f32,
}

impl AuditCaptures {
    pub(super) fn new(selection: &super::preview::SelectionPreview) -> Self {
        Self {
            remaining: selection.additional_captures.iter().cloned().collect(),
            pending: None,
            wait_frames: 0,
            orbit: selection.orbit,
        }
    }

    pub(super) const fn orbit(&self) -> f32 {
        self.orbit
    }
}

/// Advances only after Bevy has completed the preceding capture. A camera
/// frame is applied by the normal selection system, then at least two actual
/// updates and a second of collision-camera settling precede the next image.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn next_capture(
    mut commands: Commands,
    mut captured: RemovedComponents<super::Captured>,
    audit: Option<ResMut<AuditCaptures>>,
    mut preview: ResMut<Preview>,
    cameras: Query<&OrbitCamera>,
    request: Option<Res<super::CaptureRequest>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut audit) = audit else { return };
    if audit.pending.is_some() {
        if audit.wait_frames > 0 {
            audit.wait_frames -= 1;
            return;
        }
        if request.is_none()
            && let Some(view) = audit.pending.take()
        {
            commands.insert_resource(super::CaptureRequest {
                path: view.output,
                settle_timer: Timer::from_seconds(1.0, TimerMode::Once),
            });
        }
        return;
    }
    if captured.read().next().is_none() {
        return;
    }
    let Some(view) = audit.remaining.pop_front() else {
        exit.write(AppExit::Success);
        return;
    };
    let Ok(camera) = cameras.single() else { return };
    let base = camera.yaw - audit.orbit;
    preview.frame = Some((base + view.orbit, camera.pitch, camera.radius));
    audit.orbit = view.orbit;
    audit.pending = Some(view);
    audit.wait_frames = 2;
}

/// The look a character shown at select is dressed in, from its list.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(super) struct Look {
    /// Race/model identifier.
    pub race: u32,
    /// Worn gear and features.
    pub appearance: Appearance,
}

/// A listed character: the list's identity and the character's slot.
type Listed = (u64, u8);

/// The camera's yaw, pitch and distance.
type View = (f32, f32, f32);

/// What the preview shows, and what it is loading.
#[derive(Resource, Default)]
pub(super) struct Preview {
    /// The highlighted character the preview should show.
    wanted: Option<Listed>,
    /// The character standing on screen.
    shown: Option<Listed>,
    /// The latest load's number; an older load's result is dropped.
    job: u64,
    loading: Background<Result<(super::zone::PreparedEntry, Look, String), String>>,
    /// The view to give the camera once the character stands.
    frame: Option<View>,
    /// The view last seen, so a change the player makes stops the turning.
    seen: Option<View>,
    turning: bool,
}

impl Preview {
    /// Whether the highlighted character stands on screen, so the screen's
    /// cover gives way to it.
    pub(crate) fn showing(&self) -> bool {
        self.wanted.is_some() && self.wanted == self.shown
    }
}

/// Loads the highlighted character's zone and model off the frame when the
/// highlight changes, and stands the character in it once loaded. Leaving
/// the screen leaves the scene for the zone's admission to replace.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn show(
    mut commands: Commands,
    mut preview: ResMut<Preview>,
    mut online: ResMut<OnlineState>,
    (front, settings): (Res<super::login::FrontEnd>, Res<ViewerSettings>),
    offline: Option<Res<super::preview::Switches>>,
    mut scene: super::zone::Scene,
) {
    let wanted = wanted(&front, &online, settings.0.eq_directory.as_deref());
    let listed = wanted.as_ref().map(|(listed, ..)| *listed);
    if listed != preview.wanted {
        preview.wanted = listed;
        preview.loading.clear();
        preview.job += 1;
        if let Some((_, zone, look, directory)) = wanted {
            let job = preview.job;
            preview.loading.start(job, move || {
                super::zone::Entry::preview(&zone, (look.race, look.gender), &directory).map(
                    |entry| {
                        let appearance = Look {
                            race: look.race,
                            appearance: look.appearance,
                        };
                        (entry.prepare(false, false), appearance, zone)
                    },
                )
            });
        }
    }
    if preview.wanted.is_none() {
        preview.shown = None;
        return;
    }
    let job = preview.job;
    let Some(result) = preview.loading.poll(Some(job)) else {
        return;
    };
    match result {
        Ok((mut entry, look, zone)) => {
            let audit = offline
                .as_ref()
                .and_then(|switches| switches.0.selection.as_ref());
            let spot = audit
                .map(|audit| &audit.spot)
                .or_else(|| Spots::built_in().get(&zone));
            entry.stand(spot);
            let facing = entry.facing();
            scene.leave(&mut commands, &mut online.regions);
            let player = scene.enter(
                &mut commands,
                entry,
                settings.0.terrain_only,
                &mut online.regions,
            );
            commands.entity(player).insert(look);
            let (distance, pitch) = spot.map_or(
                (
                    eq_client_core::select_spots::DISTANCE,
                    eq_client_core::select_spots::PITCH,
                ),
                |spot| (spot.distance, spot.pitch),
            );
            // In front of the character, looking at its face.
            preview.frame = Some((
                facing + audit.map_or(0.0, |audit| audit.orbit),
                -pitch.to_radians(),
                distance,
            ));
            preview.shown = preview.wanted;
            preview.turning = audit.is_none();
        }
        Err(text) => warn!("Character select preview: {text}"),
    }
}

/// The highlighted character the preview can show: its list identity, zone
/// short name, look and the installation to load from; None off the
/// character list, while entering the world, or where the list or the
/// installation lacks what it needs.
fn wanted(
    front: &super::login::FrontEnd,
    online: &OnlineState,
    directory: Option<&std::path::Path>,
) -> Option<(
    Listed,
    String,
    eq_client_core::CharacterLook,
    std::path::PathBuf,
)> {
    if front.screen(online) != super::login::Screen::Characters {
        return None;
    }
    let selection = online.selection.as_ref()?;
    if selection.entering() {
        return None;
    }
    let slot = selection.chosen()?;
    let entry = selection.entry(slot)?;
    let zone = eq_client_core::zones::short_name(entry.zone_id?)?;
    Some((
        (selection.id(), slot),
        zone.to_owned(),
        entry.look?,
        directory?.to_owned(),
    ))
}

/// Frames the camera on a character as it comes to stand, then circles the
/// camera around it, the character keeping its facing, until the player
/// turns or zooms the camera.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn turn(
    time: Res<Time>,
    mut preview: ResMut<Preview>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    if let Some((yaw, pitch, distance)) = preview.frame.take() {
        camera.reset_view();
        // Within the orbit's own limits, so its clamps change nothing the
        // player didn't.
        camera.yaw = yaw;
        camera.pitch = pitch.clamp(-1.45, -0.15);
        camera.radius = distance.clamp(eq_client_core::select_spots::MIN_DISTANCE, 20_000.0);
        preview.seen = Some((camera.yaw, camera.pitch, camera.radius));
        return;
    }
    if !preview.showing() {
        return;
    }
    if preview
        .seen
        .is_some_and(|seen| seen != (camera.yaw, camera.pitch, camera.radius))
    {
        preview.turning = false;
    }
    if preview.turning {
        camera.yaw += TURN * time.delta_secs();
    }
    preview.seen = Some((camera.yaw, camera.pitch, camera.radius));
}

/// A UI root: its visibility, its layer and whether the preview hid it.
type UiRoot = (
    Entity,
    &'static mut Visibility,
    Option<&'static GlobalZIndex>,
    Option<&'static Veiled>,
);

/// A UI root hidden while a character shows at select, with the visibility
/// it had.
#[derive(Component)]
pub(super) struct Veiled(Visibility);

/// Hides the game's own windows, which sit under the screens' layer, while
/// a character shows at select, as the screen's cover hid them before;
/// they come back as they were once it doesn't.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn veil(
    mut commands: Commands,
    preview: Res<Preview>,
    mut roots: Query<UiRoot, (With<Node>, Without<ChildOf>)>,
) {
    let showing = preview.showing();
    for (entity, mut visibility, layer, veiled) in &mut roots {
        let under = layer.is_none_or(|layer| layer.0 < super::windows::Layer::Screen.base() - 1);
        match veiled {
            Some(Veiled(was)) if !showing => {
                if *visibility == Visibility::Hidden {
                    *visibility = *was;
                }
                commands.entity(entity).remove::<Veiled>();
            }
            None if showing && under => {
                commands.entity(entity).insert(Veiled(*visibility));
                *visibility = Visibility::Hidden;
            }
            Some(_) if *visibility != Visibility::Hidden => *visibility = Visibility::Hidden,
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Player;
    use eq_client_core::{CharacterChoice, CharacterLook};

    fn listed(look: Option<CharacterLook>) -> OnlineState {
        let mut online = OnlineState::new(true);
        online.selection = Some(super::super::character_select::Selection::new(
            7,
            vec![CharacterChoice {
                slot: 2,
                name: "Example".into(),
                level: Some(1),
                zone_id: Some(22),
                look,
            }],
        ));
        online
    }

    #[test]
    fn the_highlighted_character_shows_only_with_a_look_a_zone_and_an_installation() {
        let front = super::super::login::FrontEnd::default();
        let directory = std::path::Path::new("eq");
        let look = CharacterLook {
            race: 1,
            ..CharacterLook::default()
        };
        let online = listed(Some(look));
        let (key, zone, shown, _) = wanted(&front, &online, Some(directory)).unwrap();
        assert_eq!((key, zone.as_str(), shown), ((7, 2), "ecommons", look));
        assert!(wanted(&front, &online, None).is_none());
        // A server type whose list isn't checked to carry looks.
        assert!(wanted(&front, &listed(None), Some(directory)).is_none());
        let mut none_chosen = listed(Some(look));
        none_chosen.selection = Some(super::super::character_select::Selection::new(7, vec![]));
        assert!(wanted(&front, &none_chosen, Some(directory)).is_none());
    }

    #[test]
    fn a_new_highlight_drops_the_load_in_progress() {
        let mut preview = Preview {
            wanted: Some((7, 2)),
            job: 1,
            ..Preview::default()
        };
        preview.loading.start(1, || Err("first".to_owned()));
        preview.wanted = Some((7, 3));
        preview.loading.clear();
        preview.job += 1;
        assert!(preview.loading.poll(Some(preview.job)).is_none());
        assert!(!preview.showing());
    }

    fn turning_app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<Time>().insert_resource(Preview {
            wanted: Some((7, 2)),
            shown: Some((7, 2)),
            frame: Some((1.0, -0.2, 25.0)),
            turning: true,
            ..Preview::default()
        });
        app.add_systems(Update, turn);
        let camera = app
            .world_mut()
            .spawn(OrbitCamera {
                focus: Vec3::ZERO,
                radius: 120.0,
                yaw: 0.0,
                pitch: -0.5,
                pulled: None,
                view: default(),
            })
            .id();
        let player = app
            .world_mut()
            .spawn((
                Player,
                Look {
                    race: 1,
                    appearance: Appearance::default(),
                },
                Transform::default(),
            ))
            .id();
        (app, camera, player)
    }

    fn advance(app: &mut App) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(5));
        app.update();
    }

    fn camera_yaw(app: &App, camera: Entity) -> f32 {
        app.world().get::<OrbitCamera>(camera).unwrap().yaw
    }

    #[test]
    fn serial_captures_wait_for_completion_then_apply_the_real_camera_before_the_next_request() {
        let (mut app, camera, _) = turning_app();
        {
            let mut preview = app.world_mut().resource_mut::<Preview>();
            preview.frame = None;
            preview.turning = false;
        }
        {
            let mut orbit = app.world_mut().get_mut::<OrbitCamera>(camera).unwrap();
            orbit.yaw = 1.0;
            orbit.pitch = -0.2;
            orbit.radius = 25.0;
        }
        app.add_message::<AppExit>()
            .insert_resource(AuditCaptures {
                remaining: [super::super::preview::SelectionCapture {
                    orbit: std::f32::consts::FRAC_PI_2,
                    output: "next.png".into(),
                }]
                .into_iter()
                .collect(),
                pending: None,
                wait_frames: 0,
                orbit: 0.0,
            })
            .add_systems(Update, next_capture.after(turn));
        app.update();
        assert!(
            app.world()
                .get_resource::<super::super::CaptureRequest>()
                .is_none()
        );
        assert!((camera_yaw(&app, camera) - 1.0).abs() < f32::EPSILON);
        let captured = app.world_mut().spawn(super::super::Captured).id();
        app.world_mut()
            .entity_mut(captured)
            .remove::<super::super::Captured>();
        app.update();
        // Completion schedules a frame, not an immediate screenshot with
        // the old camera transform. The normal turn system applies it.
        assert!(
            app.world()
                .get_resource::<super::super::CaptureRequest>()
                .is_none()
        );
        for _ in 0..2 {
            app.update();
            assert!(
                app.world()
                    .get_resource::<super::super::CaptureRequest>()
                    .is_none()
            );
        }
        assert!((camera_yaw(&app, camera) - (1.0 + std::f32::consts::FRAC_PI_2)).abs() < 1e-5);
        app.update();
        let request = app.world().resource::<super::super::CaptureRequest>();
        assert_eq!(request.path, std::path::PathBuf::from("next.png"));
        assert_eq!(
            request.settle_timer.duration(),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 0);
        app.world_mut()
            .remove_resource::<super::super::CaptureRequest>();
        let captured = app.world_mut().spawn(super::super::Captured).id();
        app.world_mut()
            .entity_mut(captured)
            .remove::<super::super::Captured>();
        app.update();
        assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 1);
    }

    #[test]
    fn the_camera_circles_the_standing_character_until_the_player_moves_it() {
        let (mut app, camera, player) = turning_app();
        app.update();
        let framed = app.world().get::<OrbitCamera>(camera).unwrap();
        assert_eq!((framed.yaw, framed.pitch, framed.radius), (1.0, -0.2, 25.0));
        advance(&mut app);
        // A quarter turn in five seconds, around a character that keeps its
        // facing.
        assert!((camera_yaw(&app, camera) - (1.0 + std::f32::consts::FRAC_PI_2)).abs() < 1e-4);
        assert_eq!(
            app.world().get::<Transform>(player).unwrap().rotation,
            Quat::IDENTITY
        );
        // A zoom of the player's stops it where it is.
        app.world_mut()
            .get_mut::<OrbitCamera>(camera)
            .unwrap()
            .radius = 40.0;
        advance(&mut app);
        let stopped = camera_yaw(&app, camera);
        advance(&mut app);
        assert!((camera_yaw(&app, camera) - stopped).abs() < f32::EPSILON);
        assert!((stopped - (1.0 + std::f32::consts::FRAC_PI_2)).abs() < 1e-4);
        let view = app.world().get::<OrbitCamera>(camera).unwrap();
        assert!((view.radius - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_games_windows_hide_while_a_character_shows_and_come_back_as_they_were() {
        let mut app = App::new();
        app.init_resource::<Preview>().add_systems(Update, veil);
        let hud = app
            .world_mut()
            .spawn((Node::default(), GlobalZIndex(10)))
            .id();
        let closed = app
            .world_mut()
            .spawn((Node::default(), Visibility::Hidden))
            .id();
        let screen = app
            .world_mut()
            .spawn((
                Node::default(),
                GlobalZIndex(super::super::windows::Layer::Screen.base()),
            ))
            .id();
        let visibility = |app: &App, entity| *app.world().get::<Visibility>(entity).unwrap();
        app.update();
        assert_eq!(visibility(&app, hud), Visibility::Inherited);
        {
            let mut preview = app.world_mut().resource_mut::<Preview>();
            preview.wanted = Some((7, 2));
            preview.shown = Some((7, 2));
        }
        app.update();
        assert_eq!(visibility(&app, hud), Visibility::Hidden);
        assert_eq!(visibility(&app, screen), Visibility::Inherited);
        app.world_mut().resource_mut::<Preview>().shown = None;
        app.update();
        assert_eq!(visibility(&app, hud), Visibility::Inherited);
        assert_eq!(visibility(&app, closed), Visibility::Hidden);
        assert!(app.world().get::<Veiled>(hud).is_none());
    }
}
