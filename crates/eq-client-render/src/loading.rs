//! The loading screen. While the player moves between zones, a cover hides
//! the scene and every window: from the moment the server offers a transfer
//! (Gate, a zone line, the trip back to the bind after a death) until the
//! new zone is drawn with the spawns around the player in it. At Enter World,
//! character select's own cover waits for the admission and this one takes
//! over until the zone has settled. The zone's scene loads in the
//! background after its admission, and the cover waits for it before it
//! waits for the spawns. Offline there is nothing to wait for.
//!
//! The one wait: a transfer that begins as the player's own cast completes,
//! as Gate's does, lets the casting gesture finish before the cover is
//! drawn, at most [`GESTURE_WAIT`] seconds. Only the drawing waits: the
//! cover is up for everything else, the pointer included (inferred, recording
//! checklist item 6.230). A new zone admitting the player during the
//! gesture doesn't end it: the admission's news waits until the gesture has
//! played ([`Loading::waiting`]), so the old zone and its windows stay as
//! they were under the clear cover, while the session takes the new zone as
//! it always does.
//!
//! So the player never sees the old zone hang on while the transfer runs,
//! the frame freeze while the new zone loads, or the new zone's windows and
//! spawns pop in.
//!
//! The cover is the installed client's own loading screen where its
//! generation names one ([`InstalledClient::loading_screen`]): its picture
//! at its own size in the screen's pixels, centred on black, with a bar
//! along its foot that fills as the move comes on and says, on one line,
//! the first sentence of the world's message of the day, or the string
//! table's line until that comes. The
//! picture's size and place, the bar's size, place and colours, and the
//! line's colour are as the official client draws them in a screenshot of
//! it at 2560 by 1600 (10-19). The bar's fill for each step of the move
//! is inferred (recording checklist item 6.239). That the words are the
//! server's message of the day is from the program: the official client
//! holds no welcome of its own to fill in, and its bar's welcome matches
//! the message's opening. Cutting the message at its first sentence is
//! inferred (6.222). The official bar also cycles through loading lines
//! the program holds as it fills; this client's bar shows its one line
//! throughout (6.222). Character select draws the same screen
//! once Enter World is asked for, so the two read as one (inferred, 6.222).
//! Without the picture, the cover is this client's own: a dark screen with
//! one word on it.
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_assets::ui::InstalledClient;
use std::path::Path;

/// The least time the cover stays once the zone is admitted, so its windows
/// and the chat have laid out before they show.
const SETTLE: f32 = 0.25;
/// The most it stays once the zone's scene is drawn, whatever spawns are
/// still to come.
const MOST: f32 = 4.0;
/// The most it stays while a transfer runs, or while an admitted zone's
/// scene loads. A transfer that hangs this long is stuck, and the player
/// sees the old zone again, with the status box saying where the connection
/// stands; a load that hangs this long shows the zone as far as it got.
const STUCK: f32 = 30.0;
/// The most the cover's drawing waits for the player's casting gesture when
/// a transfer begins as it plays. Inferred (6.230).
const GESTURE_WAIT: f32 = 1.5;
/// How long after a transfer begins a casting gesture that started then still
/// counts as having started with it: the landing and the transfer come
/// together, and the gesture follows the landing within a frame.
const GESTURE_WITH_TRANSFER: f32 = 0.1;
/// Over every window and screen. The cover takes the pointer, so nothing
/// under it reacts or shows a tooltip.
pub(crate) const LAYER: i32 = 900;

/// This client's word on its own cover, and on the picture where the
/// installation's string table lacks the line.
const WORD: &str = "Loading";

/// The bar along the picture's foot, in the screen's pixels: its width and
/// height, and where its words begin from its left edge. As the official
/// client draws them (10-19).
const BAR: (f32, f32) = (292.0, 14.0);
const BAR_WORDS: f32 = 32.0;
/// The bar's ground, its one-pixel edge, its fill and its words, as the
/// official client draws them (10-19, read off screenshots).
const BAR_GROUND: Color = Color::BLACK;
const BAR_EDGE: Color = Color::srgb(0.78, 0.78, 0.0);
const BAR_FILL: Color = Color::srgb(0.17, 0.15, 0.78);
const BAR_INK: Color = Color::srgb(1.0, 1.0, 0.0);
/// How much of the bar is filled at each step of the move: the transfer or
/// Enter World asked for, the zone's admission, its scene drawn, the spawns
/// around the player drawn. Inferred (6.239).
const FILLED: [f32; 4] = [0.1, 0.4, 0.7, 1.0];

/// What the covers draw: the installed client's loading screen, read once,
/// or nothing of it where the installation has none.
#[derive(Resource, Default)]
pub(crate) struct Screen {
    /// The picture, and its width and height in pixels.
    picture: Option<(Handle<Image>, Vec2)>,
    /// The bar's line until the world's message of the day comes, in the
    /// installed client's words.
    line: Option<String>,
}

impl Screen {
    /// Whether the cover draws the installed client's picture.
    pub(crate) const fn pictured(&self) -> bool {
        self.picture.is_some()
    }

    /// A screen with a picture of 640 by 480 that was never read.
    #[cfg(test)]
    pub(crate) fn pictured_for_tests() -> Self {
        Self {
            picture: Some((Handle::default(), Vec2::new(640.0, 480.0))),
            line: None,
        }
    }
}

/// Where the cover is shown or hidden for the frame, and whether its drawing
/// waits: what follows reads [`Loading`] as the frame draws it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Shown;

/// The bar's fill.
#[derive(Component)]
struct BarFill;

/// The bar's words.
#[derive(Component)]
struct BarWords;

/// Reads the installed client's loading screen: its picture, and its line
/// where the string table has it. A missing or unreadable picture leaves
/// the client's own cover, said once in the log.
fn read(
    directory: Option<&Path>,
    client: InstalledClient,
    messages: Option<&crate::hud::messages::Messages>,
) -> Option<(image::RgbaImage, Option<String>)> {
    let screen = client.loading_screen()?;
    let directory = directory?;
    match screen.read_picture(directory) {
        Ok(pixels) => {
            let line = messages.and_then(|messages| messages.argument_free(screen.status));
            Some((pixels, line.map(str::to_owned)))
        }
        Err(error) => {
            warn!("Loading screen picture unavailable, so the cover is plain: {error}");
            None
        }
    }
}

/// Reads the loading screen at startup, before the cover is drawn.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn prepare(
    settings: Res<crate::ViewerSettings>,
    messages: Option<Res<crate::hud::messages::Messages>>,
    mut images: ResMut<Assets<Image>>,
    mut screen: ResMut<Screen>,
) {
    let Some((pixels, line)) = read(
        settings.0.eq_directory.as_deref(),
        settings.0.installed_client,
        messages.as_deref(),
    ) else {
        return;
    };
    let (width, height) = pixels.dimensions();
    if width == 0 || height == 0 {
        return;
    }
    #[allow(clippy::cast_precision_loss)] // A picture's sides are far below f32's exact range.
    let size = Vec2::new(width as f32, height as f32);
    *screen = Screen {
        picture: Some((images.add(crate::sheets::image(pixels)), size)),
        line,
    };
}

/// Spawns a full-window cover drawing the loading screen, with `marker`,
/// shown as `display`: the installed client's picture and line, or this
/// client's own dark cover with its word.
pub(crate) fn cover<'a>(
    commands: &'a mut Commands,
    screen: &Screen,
    marker: impl Bundle,
    display: Display,
) -> EntityCommands<'a> {
    let mut root = commands.spawn((
        marker,
        Button,
        GlobalZIndex(LAYER),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            display,
            ..default()
        },
        BackgroundColor(screen.shade()),
    ));
    root.with_children(|root| {
        let Some((image, size)) = &screen.picture else {
            theme::label(root, WORD, Size::Large);
            return;
        };
        root.spawn((
            ImageNode {
                image: image.clone(),
                image_mode: NodeImageMode::Stretch,
                ..default()
            },
            Node {
                width: px(size.x),
                height: px(size.y),
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .with_children(|picture| bar(picture, size.x, screen.line.as_deref()));
    });
    root
}

/// The bar along the picture's foot, empty, with the line in it.
fn bar(picture: &mut ChildSpawnerCommands, width: f32, line: Option<&str>) {
    picture
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px((width - BAR.0) / 2.0),
                bottom: px(0),
                width: px(BAR.0),
                height: px(BAR.1),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                // Words longer than the bar end at its edge.
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(BAR_GROUND),
            BorderColor::all(BAR_EDGE),
        ))
        .with_children(|bar| {
            bar.spawn((
                BarFill,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: percent(0),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(BAR_FILL),
            ));
            bar.spawn((
                BarWords,
                theme::text(line.unwrap_or(WORD), Size::Small, BAR_INK),
                TextLayout::new(Justify::Left, LineBreak::NoWrap),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(BAR_WORDS),
                    ..default()
                },
            ));
        });
}

/// Where the player stands between zones.
#[derive(Resource, Default)]
pub(crate) struct Loading {
    phase: Phase,
    /// The admission last seen, to tell a new one.
    admission: Option<u64>,
    /// Whether the cover's drawing waited for the player's casting gesture
    /// in the last frame drawn.
    waiting: bool,
    /// Whether the spawns around the player were all drawn at the last look.
    settled: bool,
}

/// How far a move between zones has come.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Phase {
    /// In a zone, at character select or offline: nothing to cover.
    #[default]
    Clear,
    /// The server offered a transfer this many seconds into the run, and no
    /// zone has admitted the player since.
    Moving(f32),
    /// The transfer ran longer than [`STUCK`]: the cover is down until a zone
    /// admits the player.
    Stuck,
    /// A zone admitted the player at the first time into the run. Its scene
    /// was drawn at the second, or is still loading while there is none,
    /// and then the spawns around the player are coming in.
    Arriving(f32, Option<f32>),
}

impl Loading {
    /// Whether the cover is up.
    pub(crate) fn covered(&self) -> bool {
        matches!(self.phase, Phase::Moving(_) | Phase::Arriving(..))
    }

    /// Whether the cover's drawing waited for the player's casting gesture
    /// in the last frame drawn. A new zone's admission waits with it.
    pub(crate) const fn waiting(&self) -> bool {
        self.waiting
    }

    /// How much of the bar is filled: by the step the move has reached
    /// ([`FILLED`]); before any, as at Enter World, its first.
    fn filled(&self) -> f32 {
        match self.phase {
            Phase::Arriving(_, None) => FILLED[1],
            Phase::Arriving(_, Some(_)) if self.settled => FILLED[3],
            Phase::Arriving(_, Some(_)) => FILLED[2],
            Phase::Clear | Phase::Moving(_) | Phase::Stuck => FILLED[0],
        }
    }
}

#[cfg(test)]
impl Loading {
    /// Loading with the cover up over a transfer, drawn.
    pub(crate) fn covering_for_tests() -> Self {
        Self {
            phase: Phase::Moving(0.0),
            ..Self::default()
        }
    }

    /// Loading as the last frame drawn left it, its drawing waiting for the
    /// player's casting gesture or not.
    pub(crate) fn waiting_for_gesture(waiting: bool) -> Self {
        Self {
            waiting,
            ..Self::default()
        }
    }
}

/// The loading screen's cover.
#[derive(Component)]
struct Cover;

/// The colour behind the cover's picture or word, kept while its drawing
/// waits.
#[derive(Component)]
struct Shade(Color);

impl Screen {
    /// The colour behind the picture, or the client's own cover colour.
    fn shade(&self) -> Color {
        if self.picture.is_some() {
            Color::BLACK
        } else {
            theme::COVER
        }
    }
}

/// Whether the cover's drawing waits for the player's casting gesture: a
/// transfer began `waited` seconds ago, as `gesture` (seconds since it began,
/// seconds left) was starting, and it is still playing, for at most
/// [`GESTURE_WAIT`] seconds.
fn waits_for_gesture(waited: f32, gesture: Option<(f32, f32)>) -> bool {
    gesture.is_some_and(|(since, left)| {
        waited < GESTURE_WAIT && left > 0.0 && since + GESTURE_WITH_TRANSFER >= waited
    })
}

/// Spawns the cover, hidden, and keeps it up while the player moves between
/// zones.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<Loading>()
        .init_resource::<Screen>()
        .add_systems(Startup, (prepare, spawn).chain())
        .add_systems(
            Update,
            (
                track
                    .after(crate::Stage::Scene)
                    .before(crate::Stage::Typing),
                // After the models are posed, so a casting gesture that
                // began this frame is seen before the cover is drawn.
                show.in_set(crate::Stage::Present)
                    .in_set(Shown)
                    .after(crate::character::animate),
                fill.in_set(crate::Stage::Present),
            ),
        );
}

/// Fills the bar of every cover drawing it to the step the move has
/// reached, and writes in it the first sentence of the world's latest
/// message of the day once one has come, else the string table's line.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn fill(
    loading: Res<Loading>,
    screen: Res<Screen>,
    chat: Option<Res<crate::chat::ChatState>>,
    mut fills: Query<&mut Node, With<BarFill>>,
    mut words: Query<(&mut Text, Ref<BarWords>)>,
) {
    let width = percent(loading.filled() * 100.0);
    for mut node in &mut fills {
        if node.width != width {
            node.width = width;
        }
    }
    // The words change only with the chat, or on a bar just drawn.
    let fresh = words.iter().any(|(_, marker)| marker.is_added());
    if !fresh && chat.as_ref().is_some_and(|chat| !chat.is_changed()) {
        return;
    }
    let line = chat
        .as_deref()
        .and_then(|chat| motd(&chat.history))
        .map(first_sentence)
        .filter(|sentence| !sentence.is_empty())
        .or(screen.line.as_deref())
        .unwrap_or(WORD);
    for (mut text, _) in &mut words {
        if text.0 != line {
            line.clone_into(&mut text.0);
        }
    }
}

/// The world's latest message of the day, as the chat holds it.
fn motd(history: &eq_client_core::chat::ChatHistory) -> Option<&str> {
    use eq_client_core::chat::{ChannelName, ChatTab};
    history
        .lines(ChatTab::for_channel(ChannelName::Motd))
        .into_iter()
        .rev()
        .find(|(_, line)| line.channel == ChannelName::Motd)
        .map(|(_, line)| line.message.text.as_str())
}

/// A message's first sentence: its words before the first mark that ends
/// one, or all of them where none does (inferred, 6.222).
fn first_sentence(message: &str) -> &str {
    message
        .split(['!', '.', '?'])
        .next()
        .unwrap_or(message)
        .trim()
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn spawn(mut commands: Commands, screen: Res<Screen>) {
    let shade = screen.shade();
    cover(&mut commands, &screen, (Cover, Shade(shade)), Display::None);
}

/// Follows the move between zones: a transfer the server offered puts the
/// cover up for up to [`STUCK`] seconds, and a new admission keeps it up
/// while its scene loads, for up to [`STUCK`] seconds, then until the spawns
/// around the player are drawn, at least [`SETTLE`] and at most [`MOST`]
/// seconds after the scene.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn track(
    time: Res<Time>,
    online: Res<crate::online::OnlineState>,
    nearby: Option<Res<crate::entities::NearbyEntities>>,
    mut loading: ResMut<Loading>,
) {
    let world = online.world();
    let now = time.elapsed_secs();
    let admission = world.session_id();
    let settled = world.connected() && nearby.is_none_or(|nearby| nearby.settled());
    let phase = if !online.enabled || world.ended() {
        // Offline, or the session is over and the screen says why.
        Phase::Clear
    } else if admission != loading.admission {
        // A new admission brings its zone. None means the player camped or
        // the connection dropped, and character select has its own cover.
        let drawn = (!online.scene_loading()).then_some(now);
        admission.map_or(Phase::Clear, |_| Phase::Arriving(now, drawn))
    } else if world.pending_transfer().is_some() {
        match loading.phase {
            Phase::Moving(since) if now - since >= STUCK => Phase::Stuck,
            Phase::Moving(since) => Phase::Moving(since),
            Phase::Stuck => Phase::Stuck,
            Phase::Clear | Phase::Arriving(..) => Phase::Moving(now),
        }
    } else if let Phase::Arriving(admitted, drawn) = loading.phase {
        if online.scene_loading() {
            if now - admitted < STUCK {
                Phase::Arriving(admitted, None)
            } else {
                Phase::Clear
            }
        } else {
            let since = drawn.unwrap_or(now);
            let waited = now - since;
            if waited < MOST && (waited < SETTLE || !settled) {
                Phase::Arriving(admitted, Some(since))
            } else {
                Phase::Clear
            }
        }
    } else {
        // In the zone, or the server refused the transfer.
        Phase::Clear
    };
    if loading.phase != phase {
        report(loading.phase, phase, settled);
    }
    if loading.phase != phase || loading.admission != admission || loading.settled != settled {
        loading.phase = phase;
        loading.admission = admission;
        loading.settled = settled;
    }
}

/// Logs the cover going up and down, so a session's log says when each zone
/// change kept the player waiting and whether the spawns around them were
/// all drawn when it lifted.
fn report(before: Phase, after: Phase, settled: bool) {
    match (before, after) {
        (_, Phase::Moving(_)) => info!("Loading screen up: a transfer began"),
        (Phase::Arriving(_, None), Phase::Arriving(_, Some(_))) => {
            info!("Loading screen held: the zone's scene is drawn");
        }
        (Phase::Moving(_), Phase::Arriving(..)) => {
            info!("Loading screen held: a zone admitted the player");
        }
        (_, Phase::Arriving(..)) => info!("Loading screen up: a zone admitted the player"),
        (_, Phase::Stuck) => {
            warn!("Loading screen lifted: no zone admitted the player in {STUCK} s");
        }
        (Phase::Arriving(_, None), Phase::Clear) => {
            warn!("Loading screen down before the zone's scene loaded");
        }
        (Phase::Arriving(..), Phase::Clear) => info!(settled, "Loading screen down: in the zone"),
        (Phase::Moving(_), Phase::Clear) => {
            info!("Loading screen down: the transfer ended without a new zone");
        }
        _ => {}
    }
}

/// Shows the cover while the player moves between zones, and hides it
/// otherwise. While its drawing waits for the player's casting gesture, it
/// is there but clear, so it still takes the pointer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn show(
    time: Res<Time>,
    mut loading: ResMut<Loading>,
    player: Query<&crate::character::AnimatedCharacter, With<crate::Player>>,
    mut covers: Query<(&mut Node, &mut BackgroundColor, &Shade, &Children), With<Cover>>,
    mut parts: Query<&mut Visibility>,
) {
    let display = if loading.covered() {
        Display::Flex
    } else {
        Display::None
    };
    let clear = match loading.phase {
        Phase::Moving(since) => waits_for_gesture(
            time.elapsed_secs() - since,
            player
                .iter()
                .next()
                .and_then(crate::character::AnimatedCharacter::casting_gesture),
        ),
        _ => false,
    };
    if loading.waiting != clear {
        loading.waiting = clear;
    }
    for (mut node, mut background, shade, children) in &mut covers {
        if node.display != display {
            node.display = display;
        }
        let color = if clear { Color::NONE } else { shade.0 };
        if background.0 != color {
            background.0 = color;
        }
        let shown = if clear {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for child in children {
            if let Ok(mut visibility) = parts.get_mut(*child) {
                visibility.set_if_neq(shown);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::WorldEvent;
    use std::time::Duration;

    fn app(online: bool) -> App {
        let mut app = App::new();
        app.insert_resource(OnlineState::new(online))
            .init_resource::<Time>()
            .init_resource::<Loading>()
            .add_systems(Update, track);
        app
    }

    /// Runs a frame this many seconds after the last, and says whether the
    /// cover is up.
    fn after(app: &mut App, seconds: f32) -> bool {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(seconds));
        app.update();
        app.world().resource::<Loading>().covered()
    }

    fn online(app: &mut App) -> Mut<'_, OnlineState> {
        app.world_mut().resource_mut::<OnlineState>()
    }

    fn transfer(app: &mut App) {
        testing::news(
            &mut online(app),
            [WorldEvent::ZoneTransfer(eq_client_core::ZoneOffer {
                zone_id: 2,
                instance_id: 0,
                position: eq_client_core::WorldPosition::default(),
                reason: 0,
                to_bind: true,
                solicited: true,
            })],
        );
    }

    #[test]
    fn a_transfer_covers_the_old_zone_until_the_new_one_has_settled() {
        let mut app = app(true);
        assert!(!after(&mut app, 0.0));
        // Enter World: the zone's first moments stay covered.
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, 0.1));
        assert!(!after(&mut app, SETTLE));
        // Gate: the old zone is covered for the whole transfer.
        transfer(&mut app);
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, MOST + 10.0));
        // One that hangs shows the old zone again until a zone admits the
        // player.
        assert!(!after(&mut app, STUCK));
        assert!(!after(&mut app, 60.0));
        // The new zone's admission keeps it up until it has settled.
        testing::admit(&mut online(&mut app), 2, testing::player(7));
        assert!(after(&mut app, 0.1));
        assert!(!after(&mut app, SETTLE));
        assert!(!after(&mut app, 1.0));
    }

    #[test]
    fn the_cover_waits_for_the_spawns_around_the_player_but_not_for_ever() {
        let mut app = app(true);
        app.init_resource::<crate::entities::NearbyEntities>();
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, SETTLE));
        assert!(after(&mut app, 1.0));
        app.world_mut()
            .resource_mut::<crate::entities::NearbyEntities>()
            .set_settled(true);
        assert!(!after(&mut app, 0.0));
        // Spawns that never all come in lift it once it has waited its most.
        app.world_mut()
            .resource_mut::<crate::entities::NearbyEntities>()
            .set_settled(false);
        transfer(&mut app);
        testing::admit(&mut online(&mut app), 2, testing::player(7));
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, MOST - 0.5));
        assert!(!after(&mut app, 0.5));
    }

    /// Loads the admitted zone's scene in the background until `release`
    /// is used, and takes it once it is done, as the next batch does.
    fn load_scene(app: &mut App, release: &std::sync::mpsc::Sender<()>) {
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !testing::take_scene(&mut online(app)) {
            assert!(std::time::Instant::now() < deadline, "the load never ended");
            std::thread::yield_now();
        }
    }

    #[test]
    fn the_cover_waits_for_the_zone_to_load_but_not_for_ever() {
        let mut app = app(true);
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        let release = testing::hold_scene(&mut online(&mut app));
        assert!(after(&mut app, 0.0));
        // The spawns' limit counts from the scene, not the admission.
        assert!(after(&mut app, MOST + 1.0));
        load_scene(&mut app, &release);
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, 0.1));
        assert!(!after(&mut app, SETTLE));
        // A load that hangs lifts it once it has waited as long as a stuck
        // transfer.
        testing::admit(&mut online(&mut app), 2, testing::player(7));
        let _held = testing::hold_scene(&mut online(&mut app));
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, STUCK - 1.0));
        assert!(!after(&mut app, 2.0));
    }

    #[test]
    fn spawns_that_never_all_come_in_hold_the_cover_at_most_after_the_scene() {
        let mut app = app(true);
        app.init_resource::<crate::entities::NearbyEntities>();
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        let release = testing::hold_scene(&mut online(&mut app));
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, 2.0));
        load_scene(&mut app, &release);
        // A model that never loads leaves the spawns unsettled, and the
        // cover still lifts once it has waited its most after the scene.
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, MOST - 0.5));
        assert!(!after(&mut app, 1.0));
    }

    #[test]
    fn a_refused_transfer_a_camp_or_an_ended_session_lifts_the_cover() {
        let mut app = app(true);
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        after(&mut app, 0.0);
        assert!(!after(&mut app, SETTLE));
        transfer(&mut app);
        assert!(after(&mut app, 0.0));
        testing::news(
            &mut online(&mut app),
            [WorldEvent::ZoneTransferRejected {
                session_id: 1,
                reason: eq_client_core::ZoneRejection::Server(0),
            }],
        );
        assert!(!after(&mut app, 0.0));
        // Camped, character select has its own cover.
        transfer(&mut app);
        assert!(after(&mut app, 0.0));
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Camp(eq_client_core::CampStatus::Camped)],
        );
        assert!(!after(&mut app, 0.0));
        // An ended session shows why it ended.
        testing::admit(&mut online(&mut app), 3, testing::player(7));
        transfer(&mut app);
        assert!(after(&mut app, 0.0));
        testing::link(&mut online(&mut app), eq_client_core::world::Link::Ended);
        assert!(!after(&mut app, 0.0));
    }

    #[test]
    fn offline_nothing_is_covered() {
        let mut app = app(false);
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        assert!(!after(&mut app, 0.0));
        transfer(&mut app);
        assert!(!after(&mut app, 0.0));
    }

    #[test]
    fn the_installed_clients_picture_and_line_make_the_cover_or_it_stays_plain() {
        let install =
            std::env::temp_dir().join(format!("eq-render-loading-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&install);
        std::fs::create_dir_all(install.join("Resources")).unwrap();
        image::RgbImage::new(64, 48)
            .save(install.join("Resources/loadscreen.jpg"))
            .unwrap();
        let strings = crate::hud::messages::Messages::parse("EQST0002\n0 1\n12387 Example line\n");
        let titanium = read(Some(&install), InstalledClient::Titanium, Some(&strings));
        let no_line = read(Some(&install), InstalledClient::Titanium, None);
        let mac = read(Some(&install), InstalledClient::EqMac, Some(&strings));
        std::fs::remove_file(install.join("Resources/loadscreen.jpg")).unwrap();
        let missing = read(Some(&install), InstalledClient::Titanium, Some(&strings));
        std::fs::remove_dir_all(&install).unwrap();
        let (pixels, line) = titanium.unwrap();
        assert_eq!(pixels.dimensions(), (64, 48));
        assert_eq!(line.as_deref(), Some("Example line"));
        assert_eq!(no_line.unwrap().1, None);
        assert!(mac.is_none());
        assert!(missing.is_none());
        assert!(read(None, InstalledClient::Titanium, Some(&strings)).is_none());
    }

    /// The words on the cover, and whether it draws a picture.
    fn drawn(screen: Screen) -> (Vec<String>, bool) {
        let mut app = App::new();
        app.insert_resource(screen).add_systems(Startup, spawn);
        app.update();
        let words = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        let pictured = app
            .world_mut()
            .query::<&ImageNode>()
            .iter(app.world())
            .next()
            .is_some();
        (words, pictured)
    }

    #[test]
    fn the_cover_draws_the_picture_with_its_line_or_the_word() {
        assert_eq!(drawn(Screen::default()), (vec![WORD.to_owned()], false));
        let picture = Some((Handle::default(), Vec2::new(640.0, 480.0)));
        assert_eq!(
            drawn(Screen {
                picture: picture.clone(),
                line: Some("Example line".into()),
            }),
            (vec!["Example line".to_owned()], true)
        );
        assert_eq!(
            drawn(Screen {
                picture,
                line: None
            }),
            (vec![WORD.to_owned()], true)
        );
    }

    #[test]
    fn the_bar_fills_with_the_move_and_says_the_messages_first_sentence() {
        use eq_client_core::chat::{ChannelName, ChatLine, Message, Source};
        let mut app = App::new();
        app.init_resource::<Loading>()
            .insert_resource(Screen {
                picture: Some((Handle::default(), Vec2::new(640.0, 480.0))),
                line: Some("Example line".into()),
            })
            .add_systems(Startup, spawn)
            .add_systems(Update, fill);
        app.update();
        let bar = |app: &mut App| {
            let width = app
                .world_mut()
                .query_filtered::<&Node, With<BarFill>>()
                .single(app.world())
                .unwrap()
                .width;
            let words = app
                .world_mut()
                .query_filtered::<&Text, With<BarWords>>()
                .single(app.world())
                .unwrap()
                .0
                .clone();
            (width, words)
        };
        assert_eq!(bar(&mut app), (percent(10), "Example line".to_owned()));
        let steps = [
            (Phase::Arriving(0.0, None), false, 40),
            (Phase::Arriving(0.0, Some(1.0)), false, 70),
            (Phase::Arriving(0.0, Some(1.0)), true, 100),
        ];
        for (phase, settled, filled) in steps {
            let mut loading = app.world_mut().resource_mut::<Loading>();
            loading.phase = phase;
            loading.settled = settled;
            app.update();
            assert_eq!(bar(&mut app).0, percent(filled));
        }
        app.init_resource::<crate::chat::ChatState>();
        app.update();
        // A chat with no message of the day yet keeps the string table's line.
        assert_eq!(bar(&mut app).1, "Example line");
        let said = |channel, text: &str| ChatLine {
            channel,
            message_type: None,
            sender: None,
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: text.into(),
                item_links: vec![],
            },
            source: Source::Server,
        };
        let mut chat = app.world_mut().resource_mut::<crate::chat::ChatState>();
        chat.history.push(said(
            ChannelName::Motd,
            "Welcome to Example World! Please read the rules. See example.org.",
        ));
        chat.history.push(said(ChannelName::Say, "Example say"));
        app.update();
        // The message's first sentence, on one line the bar ends.
        assert_eq!(bar(&mut app).1, "Welcome to Example World");
        let layout = app
            .world_mut()
            .query_filtered::<&TextLayout, With<BarWords>>()
            .single(app.world())
            .unwrap()
            .linebreak;
        assert_eq!(layout, LineBreak::NoWrap);
        let clipped = app
            .world_mut()
            .query_filtered::<&Node, (Without<BarFill>, Without<BarWords>)>()
            .iter(app.world())
            .any(|node| node.width == px(BAR.0) && node.overflow == Overflow::clip());
        assert!(clipped, "the bar clips its words");
    }

    #[test]
    fn the_bars_words_are_the_messages_first_sentence() {
        assert_eq!(first_sentence("Welcome to X! Be nice."), "Welcome to X");
        assert_eq!(first_sentence("Hello there. Bye"), "Hello there");
        assert_eq!(first_sentence("Ready? Go"), "Ready");
        assert_eq!(first_sentence("No mark at all"), "No mark at all");
        assert_eq!(first_sentence("! starts with one"), "");
    }

    #[test]
    fn the_cover_shows_while_the_player_moves_between_zones() {
        let mut app = App::new();
        app.init_resource::<Loading>()
            .init_resource::<Screen>()
            .init_resource::<Time>()
            .add_systems(Startup, spawn)
            .add_systems(Update, show);
        app.update();
        let display = |app: &mut App| {
            app.world_mut()
                .query_filtered::<&Node, With<Cover>>()
                .single(app.world())
                .unwrap()
                .display
        };
        assert_eq!(display(&mut app), Display::None);
        app.world_mut().resource_mut::<Loading>().phase = Phase::Moving(0.0);
        app.update();
        assert_eq!(display(&mut app), Display::Flex);
        app.world_mut().resource_mut::<Loading>().phase = Phase::Clear;
        app.update();
        assert_eq!(display(&mut app), Display::None);
    }

    #[test]
    fn the_cover_waits_for_a_casting_gesture_that_began_with_the_transfer() {
        // Gate's gesture, a second long, began with the transfer: the
        // drawing waits until it ends.
        assert!(waits_for_gesture(0.0, Some((0.0, 1.0))));
        assert!(waits_for_gesture(0.6, Some((0.6, 0.4))));
        assert!(!waits_for_gesture(1.0, Some((1.0, 0.0))));
        // Never longer than its cap, however long the clip.
        assert!(waits_for_gesture(1.4, Some((1.4, 2.0))));
        assert!(!waits_for_gesture(1.5, Some((1.5, 2.0))));
        // A gesture already under way when the transfer began counts too.
        assert!(waits_for_gesture(0.2, Some((0.5, 0.3))));
        // No gesture, or one that began after the transfer, waits for
        // nothing.
        assert!(!waits_for_gesture(0.0, None));
        assert!(!waits_for_gesture(0.5, Some((0.1, 0.9))));
    }
}
