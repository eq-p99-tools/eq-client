//! The script language: one bounded step per line, `#` starts a comment.
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::input::keyboard::{Key, NativeKey};
use bevy::prelude::KeyCode;
use eq_client_core::inventory::InventorySlot;

/// Most steps one script may queue, counting steps appended while following.
pub(super) const MAX_STEPS: usize = 500;
const MAX_HOLD: Duration = Duration::from_secs(10);
/// Longest wait, also the longest a scripted click waits for the pointer to leave.
pub(super) const MAX_WAIT: Duration = Duration::from_mins(2);
const MAX_TRACE: Duration = Duration::from_secs(10);
const MAX_WALK: Duration = Duration::from_mins(1);
/// A script's AA request (checklist 6.569). The session alone decides
/// whether it goes; the client checks nothing about the rank or the points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AaRequest {
    /// `train_aa(RANK_ID)`: buy the rank with this definition id; on `EQMac`
    /// the Mac AA id. Spends points, which nothing gives back.
    Train(u32),
    /// `activate_aa(RANK_ID)`: use an owned rank's ability, with no target.
    Activate(u32),
    /// `aa_split(PERCENT)`: the share of experience that goes to AAs, 0
    /// (off) to 100.
    Split(u8),
}

/// A spellbook ordering or deletion button: move the chosen spell one slot
/// earlier or later, ask to delete it, or answer that question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookEdit {
    /// Move earlier.
    Earlier,
    /// Move later.
    Later,
    /// Delete spell, which asks first.
    Delete,
    /// Confirm the deletion.
    Confirm,
    /// Cancel the deletion.
    Cancel,
}

/// One scripted action.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Types the account and password the environment holds (`EQ_ACCOUNT`,
    /// `EQ_PASSWORD`) into the login screen and connects.
    Login,
    /// Waits until the login server's list of worlds is shown.
    WaitServers,
    /// Highlights a listed world by name and plays on it.
    Server(String),
    /// Waits until the character selection list is shown.
    WaitSelect,
    /// Highlights a listed character by name and presses Enter.
    Select(String),
    /// Delete one exact listed name after repeating it explicitly. Completion is
    /// only an absence observation on a refreshed list, never a deletion ACK.
    DeleteCharacter {
        /// Exact name from the received character list.
        name: String,
        /// Explicit repeated name required before submitting the request.
        confirmation: String,
    },
    /// Creates a character from the selection screen (local test servers).
    Create(eq_client_core::creation::NewCharacter),
    /// Asks for a character the server is expected to refuse, a taken name
    /// for one: the step passes on the refusal and fails on a creation.
    CreateRefused(eq_client_core::creation::NewCharacter),
    /// Waits until the zone is admitted with a player present.
    WaitOnline,
    /// Waits until the named zone is admitted with a player present.
    WaitZone(String),
    /// Waits for a fresh, received movement instruction within this admission.
    WaitRelocation(Duration),
    /// Waits, for a bounded duration, until the spawn targeted as the step
    /// starts is an NPC's corpse or has left the zone: a fight's end, however
    /// long it lasts.
    WaitDead(Duration),
    /// Runs a game slash command such as `/camp`; chat text is refused.
    Slash(String),
    /// Asks the session for one AA request, in this client's own script words
    /// (checklist 6.569): no official slash form is read yet (R49-40b).
    AlternateAdvancement(AaRequest),
    /// Edit or run a social via the ordinary chat controls in a local session.
    Social(String),
    /// Sends an allowed `#` command, such as `summon`, to a local `EQEmu` server;
    /// the step stops the script on any other server.
    Gm(String),
    /// Says a line on a chat channel as a player types it, on a local `EQEmu`
    /// or TAKP server only, as `gm` steps are.
    Chat(eq_client_core::OutboundChat),
    /// Moves a window, by its saved name, as a drag moves it: its top left
    /// corner to this place on screen, in logical pixels.
    Drag(String, bevy::math::Vec2),
    /// Presses keys together for one frame, modifiers first.
    Press(Vec<KeyCode>),
    /// Holds keys together for a bounded duration.
    Hold(Vec<KeyCode>, Duration),
    /// Holds keys across later steps until explicitly released or the run ends.
    KeyDown(Vec<KeyCode>),
    /// Releases only the named explicit holds.
    KeyUp(Vec<KeyCode>),
    /// Turns the wheel over visible world space, upward positive, in lines.
    WorldWheel(f32),
    /// Holds right over visible world space and distributes a bounded mouse
    /// displacement, in physical pixels, over the duration before releasing.
    WorldLook {
        /// Total relative displacement; zero holds the look still.
        delta: bevy::math::Vec2,
        /// Longest hold, excluding the final input frame before release.
        duration: Duration,
    },
    /// Pauses for a bounded duration.
    Wait(Duration),
    /// Orbits the camera to look along an EQ heading (0..512), so the
    /// camera-relative forward walks that way, at a pitch in degrees and,
    /// where given, at a distance from the player; the view then holds both
    /// within its own ranges, as it holds the mouse's.
    Camera {
        /// EQ heading the camera looks along.
        heading: f32,
        /// Adds the player's current heading, for views relative to the model.
        relative: bool,
        /// Pitch in degrees, upward positive, from straight down (-89) to
        /// straight up (89); the view holds it within its own range, so with
        /// the program's chase tilt it never looks up.
        pitch: f32,
        /// Distance from the player, or the one it has.
        radius: Option<f32>,
    },
    /// Logs the player's position and frame time every frame for a bounded duration.
    Trace(Duration),
    /// Turns the camera so the camera-relative forward walks toward the
    /// current target.
    Face,
    /// Turns the player to an EQ heading (0..512), whatever the camera does,
    /// as moving with the camera-relative keys turns them: the heading sent
    /// turns at the turn keys' rate. It ends once the server has the heading.
    FaceHeading(f32),
    /// Holds a movement command for a bounded duration, as holding its key
    /// does, whatever keys the key table gives it; traced as `hold` is.
    Move(MoveCommand, Duration),
    /// Moves forward the way the camera looks toward the current target until it is within a distance, for a
    /// bounded duration.
    Approach(f32, Duration),
    /// Chases the current target as a player's hands would: every frame it
    /// faces where the target is drawn now (its last reported position
    /// carried on as the generation's program carries it) and moves forward while the target
    /// is beyond the distance, standing within it. It ends only when the
    /// target dies or leaves the zone, or when the time runs out, never
    /// merely on reaching the target; auto-attack is the script's to turn on
    /// first.
    Chase(f32, Duration),
    /// Moves forward the way the camera looks toward the nearest world container of one tradeskill kind
    /// (its container type, such as a pottery wheel's), chosen as the step
    /// starts, until it is within a distance, for a bounded duration.
    ApproachObject {
        /// The container type the object must have.
        kind: u8,
        /// Flat distance at which the step ends.
        range: f32,
        /// Longest the step may walk.
        duration: Duration,
    },
    /// Walks a searched path around walls to within a distance of the current
    /// target, for a bounded duration.
    Walk(f32, Duration),
    /// Walks a searched path around walls toward the nearest world container
    /// of one tradeskill kind, chosen as the step starts, until it is within a
    /// distance, for a bounded duration.
    WalkObject {
        /// The container type the object must have.
        kind: u8,
        /// Flat distance at which the step ends.
        range: f32,
        /// Longest the step may walk.
        duration: Duration,
    },
    /// Asks the current target to take the item on the cursor, as clicking
    /// it with the item does.
    Give,
    /// Opens an ordinary service on the exact selected spawn.
    Interact {
        /// Admission in which the spawn was observed.
        admission: u64,
        /// Current selected spawn ID.
        spawn: u16,
        /// Current selected spawn revision.
        revision: u64,
    },
    /// Responds to an explicitly observed server dialog.
    RespondToPopup {
        /// Admission from the dialog report.
        admission: u64,
        /// Receipt serial from the dialog report.
        serial: u64,
        /// The displayed button to choose.
        action: eq_client_core::popups::PopupAction,
    },
    /// Requests an inspection for a confirmed selection.
    InspectPlayer {
        /// Current admission.
        admission: u64,
        /// Selected player spawn.
        spawn: u16,
        /// The worker-confirmed target revision from a report.
        revision: u64,
    },
    /// Inspects a received item link from the current admission.
    InspectLink {
        /// Admission in which the message arrived.
        admission: u64,
        /// Arrival ID of the message.
        message: u64,
        /// Zero-based link ordinal in that message.
        ordinal: usize,
    },
    /// Inserts a received item link into the editable chat draft.
    ComposeLink {
        /// Admission in which the message arrived.
        admission: u64,
        /// Arrival ID of the message.
        message: u64,
        /// Zero-based link ordinal in that message.
        ordinal: usize,
    },
    /// Inserts a link for a worn or carried item into the editable chat draft.
    ComposeOwnedLink {
        /// Current admission in which the inventory was observed.
        admission: u64,
        /// Protocol slot of the owned item.
        slot: InventorySlot,
    },
    /// Left-clicks one UI control through Bevy's ordinary interaction state.
    Click(ClickTarget),
    /// Right-clicks one UI control the same way, as a bag is opened or an
    /// item inspected.
    RightClick(ClickTarget),
    /// Clicks one UI control with modifier keys held, as a person holds
    /// Shift, Ctrl or Alt while clicking (checklist 6.586): the keys go down
    /// with the button's press and up the frame after its release, so every
    /// window reads them as it reads a person's.
    ClickWith {
        /// The modifiers held: Shift, Ctrl, Alt or a chord of them.
        keys: Vec<KeyCode>,
        /// The control clicked.
        target: ClickTarget,
        /// Whether it is the right button.
        right: bool,
    },
    /// Rests the pointer on one UI control, found as a click finds it, so
    /// its tooltip shows; it stays hovered until the next click or hover.
    Hover(ClickTarget),
    /// Holds the left button on one UI control, found as a click finds it,
    /// for a bounded duration, then lets it go there: a hold long enough
    /// picks a hotkey up onto the cursor.
    HoldClick(ClickTarget, Duration),
    /// Logs a numeric summary of player, resource, cast and buff state.
    Report(String),
    /// Plays one of the installation's sound files by its name, on the
    /// listener, as a check that sounds are heard.
    Sound(String),
    /// Saves the primary window to a PNG beside the script.
    Screenshot(PathBuf),
    /// Closes the client.
    Quit,
}

/// The movement commands a `move` step holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveCommand {
    Forward,
    Back,
    CameraForward,
    CameraBack,
    StrafeLeft,
    StrafeRight,
    TurnLeft,
    TurnRight,
}

impl MoveCommand {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "forward" => Self::Forward,
            "back" => Self::Back,
            "camera_forward" => Self::CameraForward,
            "camera_back" => Self::CameraBack,
            "strafe_left" => Self::StrafeLeft,
            "strafe_right" => Self::StrafeRight,
            "turn_left" => Self::TurnLeft,
            "turn_right" => Self::TurnRight,
            _ => return None,
        })
    }
}

/// UI controls a script may click.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClickTarget {
    /// A normal scene press/release at logical window coordinates.
    World {
        /// The admission whose world the script clicked.
        admission: u64,
        /// Horizontal logical window coordinate.
        x: u16,
        /// Vertical logical window coordinate.
        y: u16,
    },
    /// An inventory slot button by server slot number.
    Slot(i32),
    /// The spellbook's scribe-cursor-scroll button.
    Scribe,
    /// The inventory's auto-store-cursor button.
    Store,
    /// Explicit cursor-item destroy selection, followed by ordinary Yes/No.
    Destroy,
    /// Explicit cursor-item drop selection, followed by ordinary Yes/No.
    Drop,
    /// A spellbook row on the open page, from zero.
    BookRow(usize),
    /// The spellbook's memorize button for a gem, from zero.
    MemorizeGem(u8),
    /// A loot, merchant or window-closing button.
    Trade(TradeClick),
    /// A bag's tint swatch by storage slot, or one of its palette colors.
    Tint(i32, Option<usize>),
    /// The give window's Give button.
    Give,
    /// A button of the skin's character list: a character's, by its slot
    /// from zero, Enter World or Quit.
    CharacterList(CharacterClick),
    /// One of the chat window's channel tabs.
    ChatTab(eq_client_core::chat::ChatTab),
    /// A coin box: the purse's, the bank's or the give window's.
    Coins(
        eq_client_core::money::CoinPlace,
        eq_client_core::money::Coin,
    ),
    /// A button of the quantity picker.
    Pick(PickButton),
    /// The selector's button for the skin's Actions window.
    ActionsWindow,
    /// A button that opens and closes a window, by the window's key, such
    /// as the inventory's Skills button (`skills`).
    Toggle(&'static str),
    /// A box on a skinned window's title bar, by the window's key: its
    /// close box (true) or its minimize box.
    TitleBox(&'static str, bool),
    /// The button in a skinned window's own body that closes it, by the
    /// window's key: its Done or Close, as a player presses it.
    Done(&'static str),
    /// Any button of a skinned window, by the window's key and what the
    /// installed skin's file calls the button (`ScreenID`), spelled exactly.
    Button(&'static str, &'static str),
    /// An arrow of a skinned window's scrollbar, by the window's key: the
    /// one that scrolls up (true) or down.
    Scroll(&'static str, bool),
    /// A tab of the open tabbed window, from zero.
    Tab(usize),
    /// An ability button of the Actions window: its page and place, from
    /// zero.
    Ability(AbilityPage, usize),
    /// The Actions window's melee attack button.
    Attack,
    /// A button by the slash command it runs: a Pet Info window button, by
    /// the `/pet` line it gives, or a group window button.
    Slash(&'static str),
    /// The group window's Follow button, which joins the group the player
    /// was invited to.
    GroupJoin,
    /// An Options window checkbox, by the option's name in a file.
    Option(eq_client_core::options::Toggle),
    /// A Filters page row's button, by its kind's stable name (6.494, K3).
    Filter(eq_client_core::filters::FilterCategory),
    /// A slider, pressed this far along it, in percent.
    Slider(SliderClick, u8),
    /// The Keyboard page's filter drop-down, which opens or closes its list.
    KeyFilter,
    /// The bug report window's category drop-down.
    BugType,
    /// The bug report or feedback window's text box, which takes the
    /// keyboard as a player's click does.
    ReportBox(crate::report_windows::Report),
    /// A choice in the open drop-down's list, from zero.
    Choice(usize),
    /// A row of the Training window's list, its Train button or its Done
    /// button.
    Training(TrainingClick),
    /// A row of the track window's list, from zero.
    TrackRow(usize),
    /// A row of the skills picker's list, from zero.
    SkillRow(usize),
    /// The track window's Track button, the skin's or the client's own.
    Track,
    /// A Raid window button that acts for the member chosen, or for the
    /// raid.
    Raid(RaidClick),
    /// A row of a Raid window list: the list of members in a raid group
    /// (true) or of the rest, and the row from zero.
    RaidRow(bool, usize),
    /// A button of the skin's effects windows: the lasting one (true) or the
    /// short one, and the button from zero.
    Buff(bool, u32),
    /// The confirmation dialog's Yes (true) or No.
    Answer(bool),
    /// The book window's arrow: forward (true) or back.
    Page(bool),
    /// Combine on the window of the tradeskill container in this pack slot.
    Combine(i32),
    /// A button of the map's toolbar.
    Map(crate::map::MapButton),
    /// One of the map's search, watch, inspector, landmark or presentation
    /// controls (checklist 6.589).
    MapControl(crate::map::MapControl),
    /// A place on the skin's spellbook's open pages, from zero
    /// (`SBW_Spell0` to `SBW_Spell15`).
    BookPlace(u8),
    /// The skin's spellbook's arrow: forward (true) or back.
    BookPage(bool),
    /// One of the spellbook's ordering and deletion buttons, for the row
    /// chosen with `book` (checklist 6.588).
    BookEdit(BookEdit),
    /// A spawn in the world, clicked where it shows (checklist 6.587).
    Spawn {
        /// The admission it was reported in.
        admission: u64,
        /// Its spawn id.
        id: u16,
        /// Its revision, so a reused id is refused.
        revision: u64,
    },
    /// An object on the ground, clicked where it shows.
    Object {
        /// The admission it was reported in.
        admission: u64,
        /// Its drop id.
        drop_id: u32,
    },
    /// A door, double-clicked where it shows.
    Door {
        /// The admission it was reported in.
        admission: u64,
        /// Its door id.
        id: u8,
    },
    /// A spell gem of the skin's spell bar, from zero.
    SpellGem(u8),
    /// A hotbutton of the action bar, the client's own or the skin's, from
    /// zero.
    HotButton(usize),
}

/// The skin's character list's buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharacterClick {
    /// A character's, by its slot from zero.
    Slot(u8),
    /// Enter World.
    Enter,
    /// Quit.
    Quit,
}

/// The Training window's controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrainingClick {
    /// A row of the list, from zero.
    Row(usize),
    /// The offered row for this language ID (not its sorted row number).
    Language(u8),
    /// The offered ordinary skill, independent of sorted row order.
    Skill(u32),
    /// Train.
    Train,
    /// Done.
    Done,
}

/// The Raid window's buttons that act for the member chosen, or for the
/// raid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RaidClick {
    /// Disband: removes the member chosen, or else leaves.
    Disband,
    /// Lock (true) or Unlock.
    Lock(bool),
    /// A group button, from 0, or No Group.
    Move(Option<u8>),
    /// Make Leader.
    MakeLeader,
}

/// The Actions window's pages that hold ability buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbilityPage {
    /// The Combat page's four.
    Combat,
    /// The Abilities page's six.
    Abilities,
}

/// The sliders a script presses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SliderClick {
    /// An Options window slider, by its setting.
    Level(eq_client_core::options::Level),
    /// The quantity window's slider.
    Quantity,
}

/// The quantity picker's buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PickButton {
    /// One fewer.
    Less,
    /// One more.
    More,
    /// The least.
    Min,
    /// All of them.
    Max,
    /// Take the amount shown.
    Confirm,
    /// Take nothing.
    Cancel,
    /// The skin's quantity window's number box, which takes typing once
    /// clicked.
    Amount,
}

/// Loot and merchant window buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TradeClick {
    /// Take the item at a place on the corpse, from 0.
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
    /// Choose a ware in the skin's merchant window, by its merchant slot.
    Choose(u32),
    /// The skin's merchant window's Buy, for the ware chosen.
    BuyChosen,
    /// The skin's merchant window's Sell, for the carried item chosen.
    SellChosen,
}

/// Accept an installed cue basename regardless of the host's path separators.
fn sound_name(name: &str) -> bool {
    !name.contains(['/', '\\', ':'])
        && name.rsplit_once('.').is_some_and(|(stem, extension)| {
            !stem.is_empty()
                && (extension.eq_ignore_ascii_case("wav") || extension.eq_ignore_ascii_case("mp3"))
        })
}

/// Parses a linear action list. Use [`super::Program`] for scripts with control flow.
///
/// # Errors
/// Returns the first line that is unknown, malformed or outside the bounds.
pub fn parse(text: &str, base: &Path) -> Result<Vec<Step>, String> {
    super::Program::parse(text, base)
        .and_then(super::Program::into_steps)
        .map_err(|error| error.to_string())
}

/// A camera step's heading (or `player` and an offset from the player's
/// heading), pitch and, where given, distance.
fn parse_camera(arguments: &[&str]) -> Result<Step, String> {
    let number = |value: &str, low: f32, high: f32| {
        value
            .parse::<f32>()
            .ok()
            .filter(|value| (low..=high).contains(value))
            .ok_or_else(|| format!("expected a number from {low} to {high}"))
    };
    let (relative, rest) = match arguments {
        ["player", rest @ ..] => (true, rest),
        rest => (false, rest),
    };
    let [heading, pitch, radius @ ..] = rest else {
        return Err("expected a heading and a pitch".into());
    };
    if radius.len() > 1 {
        return Err("expected at most a distance after the pitch".into());
    }
    Ok(Step::Camera {
        heading: number(heading, 0.0, 512.0)?,
        relative,
        pitch: number(pitch, -89.0, 89.0)?,
        radius: radius
            .first()
            .map(|radius| number(radius, 1.0, 20_000.0))
            .transpose()?,
    })
}

/// Every action command `parse_words` accepts, for the editor's highlighter and
/// the unknown-command diagnostic. A test keeps it in step with the grammar.
pub(crate) const COMMANDS: &[&str] = &[
    "login",
    "wait_servers",
    "server",
    "wait_select",
    "select",
    "delete",
    "create",
    "create_refused",
    "wait_online",
    "wait_zone",
    "wait_relocation",
    "wait_dead",
    "slash",
    "boat",
    "train_aa",
    "activate_aa",
    "aa_split",
    "social",
    "gm",
    "chat",
    "drag",
    "press",
    "hold",
    "key_down",
    "key_up",
    "wheel",
    "look",
    "wait",
    "camera",
    "trace",
    "face",
    "move",
    "approach",
    "chase",
    "walk",
    "give",
    "interact",
    "popup",
    "inspect_player",
    "inspect_link",
    "compose_link",
    "compose_owned_link",
    "click",
    "right_click",
    "click_with",
    "right_click_with",
    "hover",
    "hold_click",
    "report",
    "sound",
    "screenshot",
    "quit",
];

/// Commands whose argument is free text: a call takes it as one string,
/// split into words for the grammar.
pub(crate) const FREE_TEXT: &[&str] = &["server", "slash", "social", "gm", "chat", "report"];

/// One action from its command and argument words: the grammar every call
/// lowers to.
#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive command grammar keeps unknown-command rejection explicit"
)]
pub(super) fn parse_words(command: &str, rest: &[&str]) -> Result<Step, String> {
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
    // An AA definition id; 0 names none.
    let rank_id = |id: &str| {
        id.parse::<u32>()
            .ok()
            .filter(|id| *id != 0)
            .ok_or("expected an AA rank id from 1")
    };
    // A world container that combines: a station, not a bag that only holds things.
    let station = |kind: &str| {
        kind.parse::<u8>()
            .ok()
            .filter(|kind| eq_client_core::tradeskills::combines(*kind))
            .ok_or("expected a tradeskill container type")
    };
    Ok(match (command, rest) {
        ("login", []) => Step::Login,
        ("wait_servers", []) => Step::WaitServers,
        ("server", name) if !name.is_empty() => Step::Server(name.join(" ")),
        ("wait_select", []) => Step::WaitSelect,
        ("select", [name]) => Step::Select((*name).to_owned()),
        ("delete", [name, "confirm", confirmation])
            if name == confirmation
                && !name.is_empty()
                && name.len() < 64
                && name.is_ascii()
                && !name.as_bytes().contains(&0) =>
        {
            Step::DeleteCharacter {
                name: (*name).into(),
                confirmation: (*confirmation).into(),
            }
        }
        ("create", arguments) => Step::Create(parse_create(arguments)?),
        ("create_refused", arguments) => Step::CreateRefused(parse_create(arguments)?),
        ("wait_online", []) => Step::WaitOnline,
        ("wait_zone", [zone]) => Step::WaitZone(zone.to_ascii_lowercase()),
        ("wait_relocation", [duration]) => Step::WaitRelocation(millis(duration, MAX_WAIT)?),
        ("wait_dead", [duration]) => Step::WaitDead(millis(duration, MAX_WAIT)?),
        ("slash", words) => parse_slash(words)?,
        ("boat", [intent @ ("acquire" | "release")]) => Step::Slash(format!("/boat {intent}")),
        ("train_aa", [id]) => Step::AlternateAdvancement(AaRequest::Train(rank_id(id)?)),
        ("activate_aa", [id]) => Step::AlternateAdvancement(AaRequest::Activate(rank_id(id)?)),
        ("aa_split", [percent]) => Step::AlternateAdvancement(AaRequest::Split(
            percent
                .parse::<u8>()
                .ok()
                .filter(|percent| *percent <= 100)
                .ok_or("aa_split takes a percent from 0 (off) to 100")?,
        )),
        ("social", operation) if !operation.is_empty() => {
            Step::Social(format!("/social {}", operation.join(" ")))
        }
        ("gm", words) => parse_gm(words)?,
        ("chat", words) => parse_chat(words)?,
        ("drag", [window, x, y]) => Step::Drag(
            crate::windows::WindowId::named(window)
                .map(|_| (*window).to_owned())
                .ok_or_else(|| format!("no window is named {window}"))?,
            bevy::math::Vec2::new(number(x, 0.0, 10_000.0)?, number(y, 0.0, 10_000.0)?),
        ),
        ("press", [keys]) => Step::Press(chord(keys)?),
        ("hold", [keys, duration]) => Step::Hold(chord(keys)?, millis(duration, MAX_HOLD)?),
        ("key_down", [keys]) => Step::KeyDown(chord(keys)?),
        ("key_up", [keys]) => Step::KeyUp(chord(keys)?),
        ("wheel", ["world", lines]) => Step::WorldWheel(number(lines, -64.0, 64.0)?),
        ("look", ["world", x, y, duration]) => {
            let duration = millis(duration, MAX_HOLD)?;
            if duration.is_zero() {
                return Err("world look needs a positive duration".into());
            }
            Step::WorldLook {
                delta: bevy::math::Vec2::new(
                    number(x, -4096.0, 4096.0)?,
                    number(y, -4096.0, 4096.0)?,
                ),
                duration,
            }
        }
        ("wait", [duration]) => Step::Wait(millis(duration, MAX_WAIT)?),
        ("camera", arguments) => parse_camera(arguments)?,
        ("trace", [duration]) => Step::Trace(millis(duration, MAX_TRACE)?),
        ("face", []) => Step::Face,
        ("face", [heading]) => Step::FaceHeading(number(heading, 0.0, 512.0)?),
        ("move", [command, duration]) => Step::Move(
            MoveCommand::named(command).ok_or("expected a movement command")?,
            millis(duration, MAX_WALK)?,
        ),
        ("approach", ["object", kind, range, duration]) => Step::ApproachObject {
            kind: station(kind)?,
            range: number(range, 1.0, 200.0)?,
            duration: millis(duration, MAX_HOLD)?,
        },
        ("approach", [range, duration]) => {
            Step::Approach(number(range, 1.0, 200.0)?, millis(duration, MAX_HOLD)?)
        }
        ("chase", [range, limit]) => {
            Step::Chase(number(range, 1.0, 200.0)?, millis(limit, MAX_WAIT)?)
        }
        ("walk", ["object", kind, range, duration]) => Step::WalkObject {
            kind: station(kind)?,
            range: number(range, 1.0, 200.0)?,
            duration: millis(duration, MAX_WALK)?,
        },
        ("walk", [range, duration]) => {
            Step::Walk(number(range, 1.0, 200.0)?, millis(duration, MAX_WALK)?)
        }
        ("give", []) => Step::Give,
        ("interact", [admission, spawn, revision]) => Step::Interact {
            admission: admission.parse().map_err(|_| "expected an admission ID")?,
            spawn: spawn.parse().map_err(|_| "expected a spawn ID")?,
            revision: revision.parse().map_err(|_| "expected a spawn revision")?,
        },
        ("popup", [admission, serial, button]) => Step::RespondToPopup {
            admission: admission.parse().map_err(|_| "expected an admission ID")?,
            serial: serial.parse().map_err(|_| "expected a dialog serial")?,
            action: match *button {
                "ok" | "yes" => eq_client_core::popups::PopupAction::Positive,
                "no" => eq_client_core::popups::PopupAction::Negative,
                "close" => eq_client_core::popups::PopupAction::Dismiss,
                _ => return Err("popup button must be ok, yes, no or close".into()),
            },
        },
        ("inspect_player", [admission, spawn, revision]) => Step::InspectPlayer {
            admission: admission.parse().map_err(|_| "expected an admission ID")?,
            spawn: spawn.parse().map_err(|_| "expected a spawn ID")?,
            revision: revision.parse().map_err(|_| "expected a target revision")?,
        },
        ("inspect_link", [admission, message, ordinal]) => Step::InspectLink {
            admission: admission.parse().map_err(|_| "expected an admission ID")?,
            message: message.parse().map_err(|_| "expected a chat arrival ID")?,
            ordinal: ordinal.parse().map_err(|_| "expected a link ordinal")?,
        },
        ("compose_link", [admission, message, ordinal]) => Step::ComposeLink {
            admission: admission.parse().map_err(|_| "expected an admission ID")?,
            message: message.parse().map_err(|_| "expected a chat arrival ID")?,
            ordinal: ordinal.parse().map_err(|_| "expected a link ordinal")?,
        },
        ("compose_owned_link", [admission, slot]) => Step::ComposeOwnedLink {
            admission: admission.parse().map_err(|_| "expected an admission ID")?,
            slot: InventorySlot(slot.parse().map_err(|_| "expected an inventory slot")?),
        },
        ("click", target) => Step::Click(parse_click(target)?),
        ("right_click", target) => Step::RightClick(right_clicked(parse_click(target)?)?),
        ("click_with", [keys, target @ ..]) => Step::ClickWith {
            keys: modifiers(keys)?,
            target: parse_click(target)?,
            right: false,
        },
        ("right_click_with", [keys, target @ ..]) => Step::ClickWith {
            keys: modifiers(keys)?,
            target: right_clicked(parse_click(target)?)?,
            right: true,
        },
        ("hover", target) => Step::Hover(window_control(parse_click(target)?)?),
        ("hold_click", [duration, target @ ..]) => Step::HoldClick(
            window_control(parse_click(target)?)?,
            millis(duration, MAX_HOLD)?,
        ),
        ("report", label) => Step::Report(label.join(" ")),
        ("sound", [name]) if sound_name(name) => Step::Sound((*name).to_owned()),
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
        _ if COMMANDS.contains(&command) => {
            return Err(format!(
                "malformed '{command}' step; see the command reference"
            ));
        }
        _ => return Err(format!("unknown command '{command}'")),
    })
}

/// `slash <words>`: any line the player could type after a `/`, run as
/// typing it would, since a script may do whatever the player can (Adam,
/// 11-23). The typed-line handlers refuse what they would refuse from the
/// keyboard; only the chat line's own limits apply here.
/// A Filters page row by its kind's stable name; a kind with no row is none.
fn filter_row(name: &str) -> Result<eq_client_core::filters::FilterCategory, &'static str> {
    eq_client_core::filters::FilterCategory::from_key(name)
        .filter(|category| eq_client_core::chat_filters::has_row(*category))
        .ok_or("expected a filter row, by its kind's name, such as ooc")
}

fn parse_slash(words: &[&str]) -> Result<Step, String> {
    let Some(first) = words.first() else {
        return Err("slash takes a command".into());
    };
    if first.starts_with('/') {
        return Err("slash takes a command without its leading slash".into());
    }
    let line = format!("/{}", words.join(" "));
    let longest = crate::chat::MAX_TEXT;
    if line.len() > longest {
        return Err(format!(
            "a slash line is at most {longest} bytes, as the chat line is"
        ));
    }
    if line.chars().any(char::is_control) {
        return Err("a slash line takes no control characters".into());
    }
    Ok(Step::Slash(line))
}

/// A button by the slash command it runs: a Pet Info window button, by its
/// `/pet` words, or a group or Raid window button, by its name. The group
/// window's Decline is in Disband's place and runs the same command, so
/// either name reaches the one showing.
fn slash_click(window: &str, words: &[&str]) -> Result<ClickTarget, String> {
    if window == "raid" {
        return Ok(match words {
            ["invite"] => ClickTarget::Slash("/raidinvite"),
            ["accept"] => ClickTarget::Slash("/raidaccept"),
            ["decline"] => ClickTarget::Slash("/raiddecline"),
            ["disband"] => ClickTarget::Raid(RaidClick::Disband),
            ["lock"] => ClickTarget::Raid(RaidClick::Lock(true)),
            ["unlock"] => ClickTarget::Raid(RaidClick::Lock(false)),
            ["nogroup"] => ClickTarget::Raid(RaidClick::Move(None)),
            ["makeleader"] => ClickTarget::Raid(RaidClick::MakeLeader),
            ["group", number] => ClickTarget::Raid(RaidClick::Move(Some(
                number
                    .parse::<u8>()
                    .ok()
                    .filter(|number| (1..=12).contains(number))
                    .ok_or("expected a raid group from 1 to 12")?
                    - 1,
            ))),
            _ => {
                return Err(concat!(
                    "expected invite, accept, decline, disband, lock, unlock, ",
                    "nogroup, makeleader or group and its number"
                )
                .into());
            }
        });
    }
    if window == "pet" {
        let line = format!("/pet {}", words.join(" "));
        return crate::skinned::PET_COMMANDS
            .iter()
            .map(|(_, command)| *command)
            .find(|command| *command == line)
            .map(ClickTarget::Slash)
            .ok_or_else(|| "expected a Pet Info window button, by its /pet words".into());
    }
    Ok(match words {
        ["invite"] => ClickTarget::Slash("/invite"),
        ["follow"] => ClickTarget::GroupJoin,
        ["disband" | "decline"] => ClickTarget::Slash("/disband"),
        _ => return Err("expected invite, follow, disband or decline".into()),
    })
}

/// A numeric click argument, with the existing target-specific parse error.
fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, String> {
    text.parse().map_err(|_| format!("expected {what}"))
}

/// `click <target>` or `right_click <target>`: a slot, spellbook, trade or
/// bag tint button.
/// A target a right click takes: a door or a ground object is only left
/// clicked, as the scene acts on nothing else for them (checklist 6.587).
fn right_clicked(target: ClickTarget) -> Result<ClickTarget, String> {
    if matches!(
        target,
        ClickTarget::Object { .. } | ClickTarget::Door { .. }
    ) {
        Err("an object or door takes click; a right click acts only on a spawn".into())
    } else {
        Ok(target)
    }
}

/// A target that `hover` and `hold_click` take: a window's control, never
/// a thing in the scene, which a person only clicks (checklist 6.587).
fn window_control(target: ClickTarget) -> Result<ClickTarget, String> {
    if matches!(
        target,
        ClickTarget::World { .. }
            | ClickTarget::Spawn { .. }
            | ClickTarget::Object { .. }
            | ClickTarget::Door { .. }
    ) {
        Err("a spawn, object or door takes click, right_click or click_with".into())
    } else {
        Ok(target)
    }
}

/// A drop-down, or a report window's text box.
fn form_click(kind: &str, which: &str) -> Result<ClickTarget, String> {
    use crate::report_windows::Report;
    Ok(match (kind, which) {
        ("dropdown", "key_filter") => ClickTarget::KeyFilter,
        ("dropdown", "bug_type") => ClickTarget::BugType,
        ("report", "bug_text") => ClickTarget::ReportBox(Report::Bug),
        ("report", "feedback_text") => ClickTarget::ReportBox(Report::Feedback),
        ("dropdown", _) => return Err("expected key_filter or bug_type".into()),
        _ => return Err("expected bug_text or feedback_text".into()),
    })
}

fn parse_click(words: &[&str]) -> Result<ClickTarget, String> {
    if let Some(training) = training_click(words)? {
        return Ok(ClickTarget::Training(training));
    }
    Ok(match words {
        ["slot", slot] => ClickTarget::Slot(number(slot, "a slot number")?),
        ["scribe"] => ClickTarget::Scribe,
        ["store"] => ClickTarget::Store,
        ["destroy"] => ClickTarget::Destroy,
        ["drop"] => ClickTarget::Drop,
        ["book", row] => ClickTarget::BookRow(number(row, "a book row number")?),
        ["loot", place] => {
            ClickTarget::Trade(TradeClick::Take(number(place, "a place on the corpse")?))
        }
        ["loot_all"] => ClickTarget::Trade(TradeClick::TakeAll),
        ["loot_done"] => ClickTarget::Trade(TradeClick::EndLoot),
        ["buy", slot] => ClickTarget::Trade(TradeClick::Buy(number(slot, "a merchant slot")?)),
        ["sell", slot] => ClickTarget::Trade(TradeClick::Sell(number(slot, "an inventory slot")?)),
        ["shop_done"] => ClickTarget::Trade(TradeClick::EndShop),
        ["merchant_row", slot] => {
            ClickTarget::Trade(TradeClick::Choose(number(slot, "a merchant slot")?))
        }
        ["buy_chosen"] => ClickTarget::Trade(TradeClick::BuyChosen),
        ["sell_chosen"] => ClickTarget::Trade(TradeClick::SellChosen),
        ["give"] => ClickTarget::Give,
        ["character", number] => character(number)?,
        ["enter_world"] => ClickTarget::CharacterList(CharacterClick::Enter),
        ["chat_tab", tab] => chat_tab(tab)?,
        ["quit_game"] => ClickTarget::CharacterList(CharacterClick::Quit),
        ["actions"] => ClickTarget::ActionsWindow,
        ["window", key] => ClickTarget::Toggle(window_key(key)?),
        ["close_box", key] => ClickTarget::TitleBox(window_key(key)?, true),
        ["done", key] => ClickTarget::Done(window_key(key)?),
        ["button", key, id] => ClickTarget::Button(window_key(key)?, screen_id(id)?),
        ["minimize_box", key] => ClickTarget::TitleBox(window_key(key)?, false),
        ["scroll_up", key] => ClickTarget::Scroll(window_key(key)?, true),
        ["scroll_down", key] => ClickTarget::Scroll(window_key(key)?, false),
        [window @ ("pet" | "group" | "raid"), words @ ..] => slash_click(window, words)?,
        ["raid_row", list @ ("grouped" | "ungrouped"), row] => {
            ClickTarget::RaidRow(*list == "grouped", ordinal(row, "a row")?)
        }
        [window @ ("buff" | "song"), button] => ClickTarget::Buff(
            *window == "buff",
            u32::try_from(ordinal(button, "a button")?).map_err(|_| "button out of range")?,
        ),
        ["filter", name] => ClickTarget::Filter(filter_row(name)?),
        ["option", name] => ClickTarget::Option(
            eq_client_core::options::Toggle::all()
                .find(|toggle| toggle.key() == *name)
                .ok_or("expected an option, by its name in a file")?,
        ),
        ["attack"] => ClickTarget::Attack,
        ["tracking", row] => ClickTarget::TrackRow(ordinal(row, "a row")?),
        ["skill_select", row] => ClickTarget::SkillRow(ordinal(row, "a row")?),
        ["track"] => ClickTarget::Track,
        ["answer", answer @ ("yes" | "no")] => ClickTarget::Answer(*answer == "yes"),
        ["page", way @ ("next" | "back")] => ClickTarget::Page(*way == "next"),
        ["combine", slot] => ClickTarget::Combine(number(slot, "a pack slot number")?),
        ["map", action] => ClickTarget::Map(map_button(action)?),
        [kind, ..] if kind.starts_with("map_") => ClickTarget::MapControl(map_control(words)?),
        ["slider", name, percent] => ClickTarget::Slider(
            slider(name)?,
            percent
                .parse::<u8>()
                .ok()
                .filter(|percent| *percent <= 100)
                .ok_or_else(|| String::from("expected a percentage from 0 to 100"))?,
        ),
        ["dropdown" | "report", which] => form_click(words[0], which)?,
        ["choice", choice] => ClickTarget::Choice(ordinal(choice, "a choice")?),
        ["tab", tab] => ClickTarget::Tab(ordinal(tab, "a tab")?),
        ["ability", page, place] => ClickTarget::Ability(
            match *page {
                "combat" => AbilityPage::Combat,
                "abilities" => AbilityPage::Abilities,
                _ => return Err("expected combat or abilities".into()),
            },
            ordinal(place, "an ability button")?,
        ),
        ["coins", place, coin] => ClickTarget::Coins(coin_place(place)?, coin_kind(coin)?),
        ["pick", button] => ClickTarget::Pick(pick_button(button)?),
        ["tint", slot] => ClickTarget::Tint(number(slot, "a bag slot")?, None),
        ["tint", slot, color] => ClickTarget::Tint(
            number(slot, "a bag slot")?,
            Some(number(color, "a palette color")?),
        ),
        ["memorize", gem] => ClickTarget::MemorizeGem(gem_number(gem)?),
        ["spell_gem", gem] => ClickTarget::SpellGem(gem_number(gem)?),
        ["hot_button", number] => ClickTarget::HotButton(hot_button(number)?),
        ["book_place", place] => ClickTarget::BookPlace(
            place
                .parse::<u8>()
                .ok()
                .filter(|place| usize::from(*place) < crate::spellbook::PLACES)
                .ok_or_else(|| String::from("expected a book place from 0 to 15"))?,
        ),
        ["book_page", way @ ("next" | "back")] => ClickTarget::BookPage(*way == "next"),
        ["book_edit", action] => ClickTarget::BookEdit(book_edit(action)?),
        ["world" | "spawn" | "object" | "door", ..] => world_target(words)?,
        _ => return Err("unknown or malformed step".into()),
    })
}

/// A thing in the world by the ids `report()` gives (checklist 6.587).
fn world_target(words: &[&str]) -> Result<ClickTarget, String> {
    let admission = |word: &str| word.parse().map_err(|_| "expected an admission ID");
    Ok(match words {
        ["world", reported, x, y] => ClickTarget::World {
            admission: admission(reported)?,
            x: x.parse().map_err(|_| "expected logical window x")?,
            y: y.parse().map_err(|_| "expected logical window y")?,
        },
        ["spawn", reported, id, revision] => ClickTarget::Spawn {
            admission: admission(reported)?,
            id: id.parse().map_err(|_| "expected a spawn ID")?,
            revision: revision.parse().map_err(|_| "expected a spawn revision")?,
        },
        ["object", reported, drop_id] => ClickTarget::Object {
            admission: admission(reported)?,
            drop_id: drop_id.parse().map_err(|_| "expected a drop ID")?,
        },
        ["door", reported, id] => ClickTarget::Door {
            admission: admission(reported)?,
            id: id.parse().map_err(|_| "expected a door ID")?,
        },
        _ => return Err("unknown or malformed step".into()),
    })
}

/// A spellbook ordering or deletion button by its word.
fn book_edit(word: &str) -> Result<BookEdit, String> {
    Ok(match word {
        "earlier" => BookEdit::Earlier,
        "later" => BookEdit::Later,
        "delete" => BookEdit::Delete,
        "confirm" => BookEdit::Confirm,
        "cancel" => BookEdit::Cancel,
        _ => return Err("book_edit takes earlier, later, delete, confirm or cancel".into()),
    })
}

/// Training row/subject selection and the ordinary Train/Done buttons.
/// Unrecognized shapes retain the general click parser's existing fallback.
fn training_click(words: &[&str]) -> Result<Option<TrainingClick>, String> {
    Ok(Some(match words {
        ["training", row] => TrainingClick::Row(ordinal(row, "a row")?),
        ["training_skill", skill] => {
            let id = skill
                .parse::<u32>()
                .ok()
                .filter(|id| *id < eq_client_core::training::SKILLS)
                .or_else(|| {
                    (0..eq_client_core::training::SKILLS).find(|id| {
                        eq_client_core::skills::name(*id)
                            .is_some_and(|name| name.eq_ignore_ascii_case(skill))
                    })
                });
            TrainingClick::Skill(
                id.ok_or("expected an ordinary skill ID (0..99) or canonical skill name")?,
            )
        }
        ["training_language", id] => {
            let id: u8 = id.parse().map_err(|_| "expected a language ID (0..25)")?;
            if id >= eq_client_core::training::LANGUAGES {
                return Err("expected a language ID (0..25)".into());
            }
            TrainingClick::Language(id)
        }
        ["train"] => TrainingClick::Train,
        ["training_done"] => TrainingClick::Done,
        _ => return Ok(None),
    }))
}

/// One of the chat window's tabs, by its label in any case.
fn chat_tab(label: &str) -> Result<ClickTarget, String> {
    eq_client_core::chat::ChatTab::ALL
        .into_iter()
        .find(|tab| tab.label().eq_ignore_ascii_case(label))
        .map(ClickTarget::ChatTab)
        .ok_or_else(|| String::from("expected a chat tab by its label"))
}

/// A character's button of the skin's character list, from 1 to 8.
fn character(number: &str) -> Result<ClickTarget, String> {
    number
        .parse::<u8>()
        .ok()
        .filter(|number| (1..=8).contains(number))
        .map(|number| ClickTarget::CharacterList(CharacterClick::Slot(number - 1)))
        .ok_or_else(|| String::from("expected a character from 1 to 8"))
}

/// A spell gem a script names, from 1 to 8, as a gem from zero.
fn gem_number(gem: &str) -> Result<u8, String> {
    gem.parse::<u8>()
        .ok()
        .filter(|gem| (1..=8).contains(gem))
        .map(|gem| gem - 1)
        .ok_or_else(|| String::from("expected a gem from 1 to 8"))
}

/// A hotbutton a script names, from 1 to 10 as its keys are, as a slot from
/// zero.
fn hot_button(number: &str) -> Result<usize, String> {
    number
        .parse::<usize>()
        .ok()
        .filter(|number| (1..=eq_client_core::hotbar::SLOTS).contains(number))
        .map(|number| number - 1)
        .ok_or_else(|| String::from("expected a hotbutton from 1 to 10"))
}

/// The slider a script names: an Options window setting's, or the
/// quantity window's.
fn slider(name: &str) -> Result<SliderClick, String> {
    if name == "quantity" {
        return Ok(SliderClick::Quantity);
    }
    eq_client_core::options::Level::ALL
        .into_iter()
        .find(|level| level.key() == name)
        .map(SliderClick::Level)
        .ok_or_else(|| "expected an Options slider key or quantity".into())
}

/// The quantity picker's control a script names.
fn pick_button(name: &str) -> Result<PickButton, String> {
    Ok(match name {
        "less" => PickButton::Less,
        "more" => PickButton::More,
        "min" => PickButton::Min,
        "max" => PickButton::Max,
        "confirm" => PickButton::Confirm,
        "cancel" => PickButton::Cancel,
        "amount" => PickButton::Amount,
        _ => return Err("expected less, more, min, max, confirm, cancel or amount".into()),
    })
}

/// A coin box's place: `purse`, `bank` or `give`.
fn coin_place(word: &str) -> Result<eq_client_core::money::CoinPlace, String> {
    use eq_client_core::money::CoinPlace;
    Ok(match word {
        "purse" => CoinPlace::Purse,
        "bank" => CoinPlace::Bank,
        "give" => CoinPlace::Trade,
        _ => return Err("expected purse, bank or give".into()),
    })
}

/// A place counted from one, as a script names it, from zero.
/// The map toolbar's button a script names.
fn map_button(action: &str) -> Result<crate::map::MapButton, String> {
    use crate::map::MapButton;
    Ok(match action {
        "zoom_in" => MapButton::ZoomIn,
        "zoom_out" => MapButton::ZoomOut,
        "reset" => MapButton::Reset,
        "labels" => MapButton::Labels,
        "up" => MapButton::Pan(0, -1),
        "down" => MapButton::Pan(0, 1),
        "left" => MapButton::Pan(-1, 0),
        "right" => MapButton::Pan(1, 0),
        _ => {
            return Err(
                "expected zoom_in, zoom_out, reset, labels, up, down, left or right".into(),
            );
        }
    })
}

fn ordinal(word: &str, what: &str) -> Result<usize, String> {
    word.parse::<usize>()
        .ok()
        .and_then(|place| place.checked_sub(1))
        .ok_or_else(|| format!("expected {what} counted from 1"))
}

/// A coin box's kind: `platinum`, `gold`, `silver` or `copper`.
fn coin_kind(word: &str) -> Result<eq_client_core::money::Coin, String> {
    use eq_client_core::money::Coin;
    Ok(match word {
        "platinum" => Coin::Platinum,
        "gold" => Coin::Gold,
        "silver" => Coin::Silver,
        "copper" => Coin::Copper,
        _ => return Err("expected platinum, gold, silver or copper".into()),
    })
}

/// `gm <line>`: a `#` line without its `#`, as a user types it.
fn parse_gm(words: &[&str]) -> Result<Step, String> {
    gm_command(words).map(Step::Gm)
}

/// The `gm` line whose local-test helper runs on a local test session: a
/// bare `depop` removes the reconciled NPC selection through the session's
/// fixture path. Anywhere else, and every other line, goes as a typed `#`
/// line would (approved plan 6.584).
pub(crate) fn gm_fixture(command: &str) -> bool {
    command.trim() == "depop"
}

/// Any line a user could type after `#`, with the chat line's own limits as
/// a slash line has them: no control characters, and no longer than the chat
/// line takes with its `#`. The server decides what the line does (6.584).
pub(super) fn gm_command(words: &[&str]) -> Result<String, String> {
    let Some(first) = words.first() else {
        return Err("gm takes a command".into());
    };
    if first.starts_with('#') {
        return Err("gm takes a command without its leading #".into());
    }
    let text = words.join(" ");
    let longest = crate::chat::MAX_TEXT;
    if text.len() + 1 > longest {
        return Err(format!(
            "a gm line is at most {longest} bytes with its #, as the chat line is"
        ));
    }
    if text.chars().any(char::is_control) {
        return Err("a gm line takes no control characters".into());
    }
    Ok(text)
}

/// A window's key, such as `skills`, as the window registry spells it.
fn window_key(key: &str) -> Result<&'static str, String> {
    crate::windows::WindowId::ALL
        .into_iter()
        .find_map(|id| match id.key() {
            std::borrow::Cow::Borrowed(name) if name == key => Some(name),
            _ => None,
        })
        .ok_or_else(|| "expected a window, by its key such as skills".into())
}

/// One of the map's own controls by its words (checklist 6.589).
fn map_control(words: &[&str]) -> Result<crate::map::MapControl, String> {
    use crate::map::MapControl;
    Ok(match words {
        ["map_search", "names"] => MapControl::Search(false),
        ["map_search", "landmark"] => MapControl::Search(true),
        ["map_watch", "clear"] => MapControl::ClearSearch,
        ["map_watch", "track"] => MapControl::Track,
        ["map_watch", "track_selected"] => MapControl::TrackSelected,
        ["map_watch", "list"] => MapControl::WatchList,
        ["map_watch", "remove", name @ ..] => MapControl::RemoveWatch(shown_name(name)?),
        ["map_marker", admission, id, revision] => MapControl::Marker {
            admission: admission.parse().map_err(|_| "expected an admission ID")?,
            id: id.parse().map_err(|_| "expected a spawn ID")?,
            revision: revision.parse().map_err(|_| "expected a spawn revision")?,
        },
        ["map_inspector", "clear"] => MapControl::ClearInspector,
        ["map_target"] => MapControl::Target,
        ["map_landmarks", "toggle"] => MapControl::Landmarks,
        ["map_landmarks", "save"] => MapControl::SaveLandmark,
        ["map_landmarks", "remove"] => MapControl::RemoveLandmark,
        ["map_landmarks", "locate", name @ ..] => MapControl::Locate(shown_name(name)?),
        ["map_style", "less"] => MapControl::Less,
        ["map_style", "more"] => MapControl::More,
        ["map_style", "style"] => MapControl::Style,
        ["map_style", "lock"] => MapControl::Lock,
        ["map_style", "outlines"] => MapControl::Outlines,
        _ => return Err(concat!(
            "expected map_search names|landmark, map_watch clear|track|track_selected|list|remove NAME, ",
            "map_marker ADMISSION ID REVISION, map_inspector clear, map_target, ",
            "map_landmarks toggle|save|remove|locate NAME, or map_style less|more|style|lock|outlines"
        ).into()),
    })
}

/// A name a map row shows, as a script gives it in words joined by single
/// spaces: printable, 1 to 64 bytes. Kept for the run, once per name, as
/// [`screen_id`] keeps its names.
fn shown_name(words: &[&str]) -> Result<&'static str, String> {
    static NAMES: std::sync::Mutex<std::collections::BTreeSet<&'static str>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    let name = words.join(" ");
    if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
        return Err("expected a name as the map shows it, up to 64 bytes".into());
    }
    let mut names = NAMES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(kept) = names.get(name.as_str()) {
        return Ok(kept);
    }
    let kept: &'static str = Box::leak(name.into_boxed_str());
    names.insert(kept);
    Ok(kept)
}

/// A skin button's `ScreenID` as a script names it: letters, digits and
/// underscores. Kept for the run, once per name; a script holds at most
/// `MAX_STEPS` steps, so few are ever kept.
fn screen_id(id: &str) -> Result<&'static str, String> {
    static NAMES: std::sync::Mutex<std::collections::BTreeSet<&'static str>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("expected a button, by its ScreenID in the skin's file".into());
    }
    let mut names = NAMES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(name) = names.get(id) {
        return Ok(name);
    }
    let name: &'static str = Box::leak(id.into());
    names.insert(name);
    Ok(name)
}

/// `chat <say|ooc|shout|auction|group|guild|raid> <words>` or `chat tell
/// <Name> <words>`: a line as a player types it, in printable ASCII. Words
/// starting with `#` go as typing them would (6.584).
fn parse_chat(words: &[&str]) -> Result<Step, String> {
    use eq_client_core::OutboundChat;
    let usage = || {
        String::from(concat!(
            "chat takes say, ooc, shout, auction, group, guild or raid and words, ",
            "or tell, a name and words"
        ))
    };
    let text = |words: &[&str]| {
        let text = words.join(" ");
        (!text.is_empty() && text.chars().all(|c| c.is_ascii_graphic() || c == ' '))
            .then_some(text)
            .ok_or_else(usage)
    };
    let [channel, rest @ ..] = words else {
        return Err(usage());
    };
    Ok(Step::Chat(match *channel {
        "say" => OutboundChat::Say(text(rest)?),
        "ooc" => OutboundChat::Ooc(text(rest)?),
        "shout" => OutboundChat::Shout(text(rest)?),
        "auction" => OutboundChat::Auction(text(rest)?),
        "group" => OutboundChat::Group(text(rest)?),
        "guild" => OutboundChat::Guild(text(rest)?),
        "raid" => OutboundChat::Raid(text(rest)?),
        "tell" => match rest {
            [name, words @ ..] if name.chars().all(|c| c.is_ascii_alphabetic()) => {
                OutboundChat::Tell {
                    recipient: (*name).to_owned(),
                    message: text(words)?,
                }
            }
            _ => return Err(usage()),
        },
        _ => return Err(usage()),
    }))
}

/// `create <Name> <race> <class> <gender> <deity> <start zone> <stat for free points>`,
/// and `create_refused` with the same words.
fn parse_create(arguments: &[&str]) -> Result<eq_client_core::creation::NewCharacter, String> {
    let [name, race, class, gender, deity, zone, primary] = arguments else {
        return Err("create takes a name and six numbers".into());
    };
    let value = |text: &str| {
        text.parse::<u32>()
            .map_err(|_| String::from("expected a number"))
    };
    let character = eq_client_core::creation::NewCharacter::with_points_in(
        *name,
        (value(race)?, value(class)?, value(gender)?),
        (value(deity)?, value(zone)?),
        usize::try_from(value(primary)?).map_err(|_| String::from("bad stat"))?,
    )
    .map_err(|error| error.to_string())?;
    character.validate().map_err(|error| error.to_string())?;
    Ok(character)
}

/// The modifier keys a modified click holds: Shift, Ctrl and Alt only, each
/// at most once.
fn modifiers(text: &str) -> Result<Vec<KeyCode>, String> {
    let keys = keys_from(text).ok_or("unknown key")?;
    let modifier = |key: &KeyCode| {
        matches!(
            key,
            KeyCode::ShiftLeft | KeyCode::ControlLeft | KeyCode::AltLeft
        )
    };
    let mut seen = Vec::new();
    for key in keys {
        if !modifier(&key) || seen.contains(&key) {
            return Err("click_with keys must be shift, ctrl or alt, each once".into());
        }
        seen.push(key);
    }
    Ok(seen)
}

fn keys_from(text: &str) -> Option<Vec<KeyCode>> {
    text.split('+').map(key).collect()
}

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
/// The keypad's digits, from 0, as Num Lock on types them.
const NUMPAD: [KeyCode; 10] = [
    KeyCode::Numpad0,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
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
const FUNCTION_KEYS: [Key; 12] = [
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
];

fn key(name: &str) -> Option<KeyCode> {
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    match upper.as_str() {
        "SPACE" => Some(KeyCode::Space),
        "ESCAPE" => Some(KeyCode::Escape),
        "ENTER" => Some(KeyCode::Enter),
        "BACKSPACE" => Some(KeyCode::Backspace),
        "DELETE" => Some(KeyCode::Delete),
        "TAB" => Some(KeyCode::Tab),
        "SLASH" => Some(KeyCode::Slash),
        "UP" => Some(KeyCode::ArrowUp),
        "DOWN" => Some(KeyCode::ArrowDown),
        "LEFT" => Some(KeyCode::ArrowLeft),
        "RIGHT" => Some(KeyCode::ArrowRight),
        "SHIFT" => Some(KeyCode::ShiftLeft),
        "NUMLOCK" => Some(KeyCode::NumLock),
        "INSERT" => Some(KeyCode::Insert),
        "HOME" => Some(KeyCode::Home),
        "END" => Some(KeyCode::End),
        "PAGEUP" => Some(KeyCode::PageUp),
        "PAGEDOWN" => Some(KeyCode::PageDown),
        "NUMPAD_ADD" => Some(KeyCode::NumpadAdd),
        "NUMPAD_SUBTRACT" => Some(KeyCode::NumpadSubtract),
        "NUMPAD_MULTIPLY" => Some(KeyCode::NumpadMultiply),
        "NUMPAD_DIVIDE" => Some(KeyCode::NumpadDivide),
        "NUMPAD_DECIMAL" => Some(KeyCode::NumpadDecimal),
        "NUMPAD_ENTER" => Some(KeyCode::NumpadEnter),
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
            .and_then(|number| FUNCTIONS.get(number.checked_sub(1)?).copied())
            .or_else(|| match upper.strip_prefix("NUMPAD")?.as_bytes() {
                [digit @ b'0'..=b'9'] => Some(NUMPAD[usize::from(digit - b'0')]),
                _ => None,
            }),
    }
}

/// What a key on a US keyboard means and types, as the window reports it
/// with the key's press: the logical key, and the text it types, which
/// Shift makes upper case and Ctrl or Alt makes none.
pub(super) fn typed(key: KeyCode, shift: bool, chorded: bool) -> (Key, Option<String>) {
    let character = |text: String| {
        (
            Key::Character(text.as_str().into()),
            (!chorded).then_some(text),
        )
    };
    if let Some(index) = LETTERS.iter().position(|letter| *letter == key) {
        let letter = char::from(b'a' + u8::try_from(index).unwrap_or_default());
        let letter = if shift {
            letter.to_ascii_uppercase()
        } else {
            letter
        };
        return character(letter.to_string());
    }
    if let Some(index) = DIGITS.iter().position(|digit| *digit == key) {
        return character(index.to_string());
    }
    if let Some(index) = NUMPAD.iter().position(|digit| *digit == key) {
        return character(index.to_string());
    }
    if let Some(index) = FUNCTIONS.iter().position(|function| *function == key) {
        return (FUNCTION_KEYS[index].clone(), None);
    }
    match key {
        KeyCode::Slash if shift => character("?".to_owned()),
        KeyCode::Slash | KeyCode::NumpadDivide => character("/".to_owned()),
        KeyCode::Space => (Key::Space, (!chorded).then(|| " ".to_owned())),
        KeyCode::Escape => (Key::Escape, None),
        KeyCode::Enter | KeyCode::NumpadEnter => (Key::Enter, None),
        KeyCode::Backspace => (Key::Backspace, None),
        KeyCode::Delete => (Key::Delete, None),
        KeyCode::Tab => (Key::Tab, None),
        KeyCode::ArrowUp => (Key::ArrowUp, None),
        KeyCode::ArrowDown => (Key::ArrowDown, None),
        KeyCode::ArrowLeft => (Key::ArrowLeft, None),
        KeyCode::ArrowRight => (Key::ArrowRight, None),
        KeyCode::NumpadAdd => character("+".to_owned()),
        KeyCode::NumpadSubtract => character("-".to_owned()),
        KeyCode::NumpadMultiply => character("*".to_owned()),
        KeyCode::NumpadDecimal => character(".".to_owned()),
        KeyCode::NumLock => (Key::NumLock, None),
        KeyCode::Insert => (Key::Insert, None),
        KeyCode::Home => (Key::Home, None),
        KeyCode::End => (Key::End, None),
        KeyCode::PageUp => (Key::PageUp, None),
        KeyCode::PageDown => (Key::PageDown, None),
        KeyCode::ShiftLeft => (Key::Shift, None),
        KeyCode::ControlLeft => (Key::Control, None),
        KeyCode::AltLeft => (Key::Alt, None),
        _ => (Key::Unidentified(NativeKey::Unidentified), None),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn listed_commands_are_known_and_every_other_word_is_unknown() {
        for command in super::COMMANDS {
            // A call without arguments is valid or malformed, never unknown.
            if let Err(error) = super::parse_words(command, &[]) {
                assert!(!error.starts_with("unknown command"), "{command}: {error}");
            }
        }
        for word in ["pres", "Press", "let", "fn", "wait_", "x"] {
            assert_eq!(
                super::parse_words(word, &["1"]).unwrap_err(),
                format!("unknown command '{word}'")
            );
        }
        // The Vim plugin colours the same commands.
        let vim = include_str!("../../../../contrib/vim/syntax/eqscript.vim");
        let list = vim
            .lines()
            .find_map(|line| line.strip_prefix(r"syntax match eqscriptCommand /^\s*\zs\%("))
            .and_then(|line| line.split_once(r"\)\>"))
            .map(|(list, _)| list.split(r"\|").collect::<Vec<_>>())
            .unwrap();
        assert_eq!(list, super::COMMANDS);
    }

    #[test]
    fn aa_steps_are_our_own_words_with_bounded_arguments() {
        use super::AaRequest;
        let base = Path::new("private");
        assert_eq!(
            parse(
                "train_aa(42)\nactivate_aa(4294967295)\naa_split(0)\naa_split(100)\n",
                base
            )
            .unwrap(),
            [
                Step::AlternateAdvancement(AaRequest::Train(42)),
                Step::AlternateAdvancement(AaRequest::Activate(u32::MAX)),
                Step::AlternateAdvancement(AaRequest::Split(0)),
                Step::AlternateAdvancement(AaRequest::Split(100)),
            ]
        );
        for bad in [
            "train_aa()",
            "train_aa(0)",
            "train_aa(-1)",
            "train_aa(42, 1)",
            "activate_aa(42, 7)",
            "activate_aa(\"Fleet\")",
            "aa_split(101)",
            "aa_split()",
        ] {
            let source = format!("report(\"before\")\n{bad}\nreport(\"after\")\n");
            let error = super::super::Program::parse(&source, base).unwrap_err();
            assert_eq!(error.line, Some(2), "{bad}");
        }
    }

    #[test]
    fn documented_boat_steps_use_the_slash_path_and_reject_other_actions() {
        let base = Path::new("private");
        assert_eq!(
            parse("boat(\"acquire\")\nboat(\"release\")\n", base).unwrap(),
            parse("slash(\"boat acquire\")\nslash(\"boat release\")\n", base).unwrap()
        );
        for bad in [
            "boat()",
            "boat(\"steer\")",
            "boat(\"acquire\", \"extra\")",
            "boat(\"release\", \"extra\")",
        ] {
            let source = format!("report(\"before\")\n{bad}\nreport(\"after\")\n");
            let error = super::super::Program::parse(&source, base).unwrap_err();
            assert_eq!(error.line, Some(2), "{bad}");
        }
    }

    #[test]
    fn trainer_skills_parse_by_id_or_name_without_depending_on_row_order() {
        let base = Path::new("private");
        for (source, id) in [
            ("click(\"training_skill\", 53)", 53),
            ("click(\"training_skill\", \"Tracking\")", 53),
            ("click(\"training_skill\", \"tRaCkInG\")", 53),
            ("click(\"training_skill\", \"1H Blunt\")", 0),
        ] {
            assert_eq!(
                parse(source, base).unwrap(),
                [Step::Click(ClickTarget::Training(TrainingClick::Skill(id)))]
            );
        }
        for bad in [
            "click(\"training_skill\")",
            "click(\"training_skill\", 100)",
            "click(\"training_skill\", -1)",
            "click(\"training_skill\", 1.5)",
            "click(\"training_skill\", \"not a skill\")",
            "click(\"training_skill\", \"\")",
            "click(\"slot\", \"2 3\")",
            "click(\"training_skill\", 53, 1)",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn training_language_selects_a_bounded_offered_row_without_sending_a_numeric_request() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click(\"training\", 1)\nclick(\"training_language\", 0)\nclick(\"training_language\", 25)\nclick(\"train\")\nclick(\"training_done\")",
                base
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::Training(TrainingClick::Row(0))),
                Step::Click(ClickTarget::Training(TrainingClick::Language(0))),
                Step::Click(ClickTarget::Training(TrainingClick::Language(25))),
                Step::Click(ClickTarget::Training(TrainingClick::Train)),
                Step::Click(ClickTarget::Training(TrainingClick::Done)),
            ]
        );
        for bad in [
            "click(\"training\")",
            "click(\"training\", 0)",
            "click(\"training\", 1, 2)",
            "click(\"train\", 1)",
            "click(\"training_done\", 1)",
            "click(\"training_language\")",
            "click(\"training_language\", 26)",
            "click(\"training_language\", -1)",
            "click(\"training_language\", 256)",
            "click(\"training_language\", 1, 2)",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn deletion_requires_exact_repeated_name_without_position_fallback() {
        assert_eq!(
            parse(
                "delete(\"Disposable\", \"confirm\", \"Disposable\")",
                std::path::Path::new(".")
            )
            .unwrap(),
            vec![Step::DeleteCharacter {
                name: "Disposable".into(),
                confirmation: "Disposable".into()
            }]
        );
        for source in [
            "delete(\"Disposable\")",
            "delete(\"Disposable\", \"confirm\", \"Other\")",
            "delete(\"Disposable\", \"confirm\", \"disposable\")",
            "delete(0)",
            "delete(\"Disposable\", \"yes\")",
        ] {
            assert!(
                parse(source, std::path::Path::new(".")).is_err(),
                "{source}"
            );
        }
    }

    use super::*;

    #[test]
    fn semantic_actions_require_exact_reported_identities() {
        let base = Path::new(".");
        assert_eq!(
            parse(
                "interact(6, 42, 12)\ninspect_link(6, 70, 0)\ncompose_owned_link(6, 23)",
                base
            )
            .unwrap(),
            [
                Step::Interact {
                    admission: 6,
                    spawn: 42,
                    revision: 12
                },
                Step::InspectLink {
                    admission: 6,
                    message: 70,
                    ordinal: 0
                },
                Step::ComposeOwnedLink {
                    admission: 6,
                    slot: InventorySlot(23),
                },
            ],
        );
        for malformed in [
            "interact(42)",
            "interact(6, 65536, 12)",
            "interact(6, 42, -1)",
            "inspect_link(70, 0)",
            "inspect_link(6, 70, -1)",
            "inspect_link(6, 70, 0, \"raw\")",
            "compose_owned_link(23)",
            "compose_owned_link(-1, 23)",
            "compose_owned_link(6, \"worn\")",
            "compose_owned_link(6, 23, \"raw\")",
        ] {
            assert!(parse(malformed, base).is_err(), "{malformed}");
        }
    }

    #[test]
    fn dialog_and_player_inspection_steps_require_scoped_choices() {
        let base = Path::new(".");
        assert_eq!(
            parse("popup(6, 14, \"no\")\ninspect_player(6, 42, 12)", base).unwrap(),
            [
                Step::RespondToPopup {
                    admission: 6,
                    serial: 14,
                    action: eq_client_core::popups::PopupAction::Negative
                },
                Step::InspectPlayer {
                    admission: 6,
                    spawn: 42,
                    revision: 12
                },
            ],
        );
        for text in [
            "popup(\"yes\")",
            "popup(6, 14, \"auto\")",
            "inspect_player(42)",
            "inspect_player(6, 65536, 12)",
        ] {
            assert!(parse(text, base).is_err(), "{text}");
        }
    }

    #[test]
    fn a_scrollbar_arrow_is_reached_by_its_windows_key() {
        let steps = parse(
            "click(\"scroll_up\", \"chat\")\nclick(\"scroll_down\", \"skills\")\n",
            Path::new("private"),
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                Step::Click(ClickTarget::Scroll("chat", true)),
                Step::Click(ClickTarget::Scroll("skills", false)),
            ]
        );
        assert!(parse("click(\"scroll_up\", \"nowhere\")\n", Path::new("private")).is_err());
    }

    #[test]
    fn parses_the_login_steps() {
        let base = Path::new("private");
        assert_eq!(
            parse("login()\nwait_servers()\nserver(\"Example World\")\n", base).unwrap(),
            [
                Step::Login,
                Step::WaitServers,
                Step::Server("Example World".into()),
            ]
        );
        // The account and password come only from the environment.
        for bad in [
            "login(\"someone\")",
            "wait_servers(\"now\")",
            "server(\"\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "one table of step bounds")]
    fn parses_bounded_steps_and_rejects_unsafe_input() {
        let base = Path::new("private");
        let steps = parse(
            "wait_select()\n\
             select(\"Someone\")\n\
             create(\"Testcleric\", 1, 2, 0, 212, 1, 4)\n\
             create_refused(\"Testcleric\", 1, 2, 0, 212, 1, 4)\n\
             wait_online()\n\
             wait_zone(\"TOX\")\n\
             slash(\"camp\")\n\
             slash(\"target a cave rat\")\n\
             gm(\"summon\")\n\
             gm(\"damage 10000\")\n\
             gm(\"givemoney 0 0 5 0\")\n\
             press(\"F1\") # self\n\
             press(\"alt+1\")\n\
             hold(\"W\", 1500)\n\
             wait(250)\n\
             trace(2000)\n\
             face()\n\
             approach(12, 5000)\n\
             walk(8, 30000)\n\
             click(\"slot\", 23)\n\
             right_click(\"slot\", 22)\n\
             hover(\"close_box\", \"chat\")\n\
             click(\"scribe\")\n\
             click(\"store\")\n\
             click(\"book\", 0)\n\
             click(\"memorize\", 2)\n\
             click(\"loot\", 0)\n\
             click(\"loot_all\")\n\
             click(\"buy\", 3)\n\
             click(\"sell\", 23)\n\
             click(\"shop_done\")\n\
             give()\n\
             click(\"give\")\n\
             report(\"after cast\")\n\
             screenshot(\"a.png\")\n\
             quit()\n",
            base,
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                Step::WaitSelect,
                Step::Select("Someone".into()),
                Step::Create(
                    eq_client_core::creation::NewCharacter::with_points_in(
                        "Testcleric",
                        (1, 2, 0),
                        (212, 1),
                        4
                    )
                    .unwrap()
                ),
                Step::CreateRefused(
                    eq_client_core::creation::NewCharacter::with_points_in(
                        "Testcleric",
                        (1, 2, 0),
                        (212, 1),
                        4
                    )
                    .unwrap()
                ),
                Step::WaitOnline,
                Step::WaitZone("tox".into()),
                Step::Slash("/camp".into()),
                Step::Slash("/target a cave rat".into()),
                Step::Gm("summon".into()),
                Step::Gm("damage 10000".into()),
                Step::Gm("givemoney 0 0 5 0".into()),
                Step::Press(vec![KeyCode::F1]),
                Step::Press(vec![KeyCode::AltLeft, KeyCode::Digit1]),
                Step::Hold(vec![KeyCode::KeyW], Duration::from_millis(1500)),
                Step::Wait(Duration::from_millis(250)),
                Step::Trace(Duration::from_secs(2)),
                Step::Face,
                Step::Approach(12.0, Duration::from_secs(5)),
                Step::Walk(8.0, Duration::from_secs(30)),
                Step::Click(ClickTarget::Slot(23)),
                Step::RightClick(ClickTarget::Slot(22)),
                Step::Hover(ClickTarget::TitleBox("chat", true)),
                Step::Click(ClickTarget::Scribe),
                Step::Click(ClickTarget::Store),
                Step::Click(ClickTarget::BookRow(0)),
                Step::Click(ClickTarget::MemorizeGem(1)),
                Step::Click(ClickTarget::Trade(TradeClick::Take(0))),
                Step::Click(ClickTarget::Trade(TradeClick::TakeAll)),
                Step::Click(ClickTarget::Trade(TradeClick::Buy(3))),
                Step::Click(ClickTarget::Trade(TradeClick::Sell(23))),
                Step::Click(ClickTarget::Trade(TradeClick::EndShop)),
                Step::Give,
                Step::Click(ClickTarget::Give),
                Step::Report("after cast".into()),
                Step::Screenshot(base.join("a.png")),
                Step::Quit,
            ]
        );
        for bad in [
            "hold(\"W\", 20000)",
            "wait(999999)",
            "press(\"Nope\")",
            "screenshot(\"../a.png\")",
            "screenshot(\"a.jpg\")",
            "teleport(1, 2, 3)",
            "camera(600, -20)",
            "camera(10, -90)",
            "trace(60000)",
            "click(\"slot\", \"x\")",
            "click(\"anything\")",
            "click(\"memorize\", 9)",
            "click(\"memorize\", 0)",
            "create(\"lowercase\", 1, 2, 0, 212, 1, 4)",
            "create(\"Testbad\", 1, 17, 0, 212, 1, 4)",
            "walk(8, 90000)",
            "walk(0, 1000)",
            "gm(\"\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
        // A `#` after a call starts a comment. Inside a text argument it is
        // plain text, and goes with the line.
        assert_eq!(
            parse("gm(\"zone qeynos\") #givemoney 999", base).unwrap(),
            [Step::Gm("zone qeynos".into())]
        );
        assert_eq!(
            parse("gm(\"zone qeynos #givemoney 999\")", base).unwrap(),
            [Step::Gm("zone qeynos #givemoney 999".into())]
        );
    }

    #[test]
    fn a_slash_line_is_taken_as_the_player_would_type_it() {
        let base = Path::new("private");
        for line in [
            "camp desktop",
            "camp server",
            "say hello",
            "ooc",
            "pet",
            "reply thanks",
            "friend Example",
            "quit",
            "discipline 4499",
            "harmtouch 12",
            "ranged 12",
            "consent",
            "guildleader Member",
            "guildmotd Café rendezvous",
        ] {
            assert_eq!(
                parse(&format!("slash(\"{line}\")"), base).unwrap(),
                [Step::Slash(format!("/{line}"))],
                "{line}"
            );
        }
        // Only the chat line's own limits apply: its length, counting the
        // slash, and no control characters.
        let longest = format!("say {}", "a".repeat(crate::chat::MAX_TEXT - 5));
        assert_eq!(
            parse(&format!("slash(\"{longest}\")"), base).unwrap(),
            [Step::Slash(format!("/{longest}"))]
        );
        for bad in [
            format!("slash(\"{longest}a\")"),
            "slash(\"guildmotd bad\u{0007}\")".to_owned(),
            "slash(\"guildmotd bad\u{001b}\")".to_owned(),
            "slash(\"/say doubled\")".to_owned(),
            "slash(\"\")".to_owned(),
        ] {
            assert!(parse(&bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn wait_relocation_is_a_bounded_observation() {
        let base = std::path::Path::new(".");
        assert_eq!(
            parse("wait_relocation(1500)\n", base).unwrap(),
            [Step::WaitRelocation(Duration::from_millis(1500))]
        );
        assert!(parse("wait_relocation()\n", base).is_err());
        assert!(parse("wait_relocation(12, 34, 56)\n", base).is_err());
    }

    #[test]
    fn discipline_slash_takes_any_words_as_the_command_does() {
        let base = std::path::Path::new(".");
        assert_eq!(
            parse("slash(\"discipline defensive\")\n", base).unwrap(),
            [Step::Slash("/discipline defensive".into())]
        );
        assert_eq!(
            parse("slash(\"discipline evasive disc\")\n", base).unwrap(),
            [Step::Slash("/discipline evasive disc".into())]
        );
        assert_eq!(
            parse("slash(\"discipline\")\n", base).unwrap(),
            [Step::Slash("/discipline".into())]
        );
    }

    #[test]
    fn map_controls_name_the_maps_own_buttons_and_rows() {
        use crate::map::MapControl;
        let base = std::path::Path::new(".");
        for (line, control) in [
            (
                "click(\"map_search\", \"names\")",
                MapControl::Search(false),
            ),
            (
                "click(\"map_search\", \"landmark\")",
                MapControl::Search(true),
            ),
            ("click(\"map_watch\", \"clear\")", MapControl::ClearSearch),
            ("click(\"map_watch\", \"track\")", MapControl::Track),
            (
                "click(\"map_watch\", \"track_selected\")",
                MapControl::TrackSelected,
            ),
            ("click(\"map_watch\", \"list\")", MapControl::WatchList),
            (
                "click(\"map_watch\", \"remove\", \"a\", \"rare\", \"guard\")",
                MapControl::RemoveWatch("a rare guard"),
            ),
            (
                "click(\"map_marker\", 5, 42, 3)",
                MapControl::Marker {
                    admission: 5,
                    id: 42,
                    revision: 3,
                },
            ),
            (
                "click(\"map_inspector\", \"clear\")",
                MapControl::ClearInspector,
            ),
            ("click(\"map_target\")", MapControl::Target),
            (
                "click(\"map_landmarks\", \"toggle\")",
                MapControl::Landmarks,
            ),
            (
                "click(\"map_landmarks\", \"save\")",
                MapControl::SaveLandmark,
            ),
            (
                "click(\"map_landmarks\", \"remove\")",
                MapControl::RemoveLandmark,
            ),
            (
                "click(\"map_landmarks\", \"locate\", \"Bank\")",
                MapControl::Locate("Bank"),
            ),
            ("click(\"map_style\", \"less\")", MapControl::Less),
            ("click(\"map_style\", \"more\")", MapControl::More),
            ("click(\"map_style\", \"style\")", MapControl::Style),
            ("click(\"map_style\", \"lock\")", MapControl::Lock),
            ("click(\"map_style\", \"outlines\")", MapControl::Outlines),
        ] {
            assert_eq!(
                parse(&format!("{line}\n"), base).unwrap(),
                [Step::Click(ClickTarget::MapControl(control))],
                "{line}"
            );
        }
        for line in [
            "click(\"map_search\", \"items\")",
            "click(\"map_watch\", \"remove\")",
            "click(\"map_marker\", 5, 42)",
            "click(\"map_style\", \"bright\")",
        ] {
            assert!(parse(&format!("{line}\n"), base).is_err(), "{line}");
        }
    }

    #[test]
    fn book_edit_names_each_spellbook_ordering_button() {
        let base = std::path::Path::new(".");
        for (word, edit) in [
            ("earlier", BookEdit::Earlier),
            ("later", BookEdit::Later),
            ("delete", BookEdit::Delete),
            ("confirm", BookEdit::Confirm),
            ("cancel", BookEdit::Cancel),
        ] {
            assert_eq!(
                parse(&format!("click(\"book_edit\", \"{word}\")\n"), base).unwrap(),
                [Step::Click(ClickTarget::BookEdit(edit))]
            );
        }
        assert!(parse("click(\"book_edit\", \"up\")\n", base).is_err());
    }

    #[test]
    fn world_targets_name_a_spawn_object_or_door_by_its_reported_ids() {
        let base = std::path::Path::new(".");
        assert_eq!(
            parse(
                concat!(
                    "click(\"spawn\", 5, 42, 3)\n",
                    "right_click(\"spawn\", 5, 42, 3)\n",
                    "click_with(\"shift\", \"spawn\", 5, 42, 3)\n",
                    "click(\"object\", 5, 900)\n",
                    "click(\"door\", 5, 12)\n",
                ),
                base
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::Spawn {
                    admission: 5,
                    id: 42,
                    revision: 3
                }),
                Step::RightClick(ClickTarget::Spawn {
                    admission: 5,
                    id: 42,
                    revision: 3
                }),
                Step::ClickWith {
                    keys: vec![KeyCode::ShiftLeft],
                    target: ClickTarget::Spawn {
                        admission: 5,
                        id: 42,
                        revision: 3
                    },
                    right: false,
                },
                Step::Click(ClickTarget::Object {
                    admission: 5,
                    drop_id: 900
                }),
                Step::Click(ClickTarget::Door {
                    admission: 5,
                    id: 12
                }),
            ]
        );
        // A person only clicks them: no hover or held click.
        assert!(parse("hover(\"spawn\", 5, 42, 3)\n", base).is_err());
        assert!(parse("hold_click(800, \"door\", 5, 12)\n", base).is_err());
        assert!(parse("click(\"spawn\", 5, 42)\n", base).is_err());
        assert!(parse("click(\"door\", 5, 300)\n", base).is_err());
        // The scene acts on a right click only for a spawn.
        assert!(parse("right_click(\"door\", 5, 12)\n", base).is_err());
        assert!(parse("right_click_with(\"alt\", \"object\", 5, 900)\n", base).is_err());
    }

    #[test]
    fn a_modified_click_takes_shift_ctrl_and_alt_only() {
        let base = std::path::Path::new(".");
        assert_eq!(
            parse("right_click_with(\"alt\", \"slot\", 22)\n", base).unwrap(),
            [Step::ClickWith {
                keys: vec![KeyCode::AltLeft],
                target: ClickTarget::Slot(22),
                right: true,
            }]
        );
        assert_eq!(
            parse("click_with(\"ctrl+shift\", \"slot\", 23)\n", base).unwrap(),
            [Step::ClickWith {
                keys: vec![KeyCode::ControlLeft, KeyCode::ShiftLeft],
                target: ClickTarget::Slot(23),
                right: false,
            }]
        );
        for refused in [
            "click_with(\"a\", \"slot\", 22)\n",
            "click_with(\"shift+shift\", \"slot\", 22)\n",
            "click_with(\"shift\")\n",
            "click_with(\"nope\", \"slot\", 22)\n",
        ] {
            assert!(parse(refused, base).is_err(), "{refused}");
        }
    }

    #[test]
    fn language_slash_takes_any_words_as_the_typed_command_does() {
        let base = std::path::Path::new(".");
        for (script, step) in [
            ("slash(\"language\")\n", "/language"),
            ("slash(\"language 2\")\n", "/language 2"),
            ("slash(\"language help\")\n", "/language help"),
        ] {
            assert_eq!(parse(script, base).unwrap(), [Step::Slash(step.into())]);
        }
    }

    #[test]
    fn knight_slash_actions_use_the_current_target() {
        let base = std::path::Path::new(".");
        assert_eq!(
            parse("slash(\"harmtouch\")\nslash(\"layonhands\")\n", base).unwrap(),
            [
                Step::Slash("/harmtouch".into()),
                Step::Slash("/layonhands".into())
            ]
        );
    }

    #[test]
    fn a_script_may_shield_its_target() {
        // Words after it are the typed command's to refuse (6.584).
        let base = std::path::Path::new(".");
        assert_eq!(
            parse("slash(\"shield\")\n", base).unwrap(),
            [Step::Slash("/shield".into())]
        );
    }

    #[test]
    fn ranged_slash_uses_the_current_target_and_takes_no_arguments() {
        let base = Path::new("private");
        assert_eq!(
            parse("slash(\"ranged\")\n", base).unwrap(),
            [Step::Slash("/ranged".into())]
        );
    }

    #[test]
    fn moving_steps_name_commands_and_key_steps_name_the_keypad() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "face(128)\n\
                 move(\"forward\", 12000)\n\
                 move(\"turn_left\", 500)\n\
                 camera(64, 0, 150)\n\
                 camera(\"player\", 0, -89)\n\
                 camera(128, -20)\n\
                 camera(\"player\", 256, -15, 300)\n\
                 camera(32, 89)\n\
                 press(\"numlock\")\n\
                 hold(\"numpad8\", 500)\n\
                 press(\"pageup\")\n\
                 press(\"numpad_add\")\n",
                base,
            )
            .unwrap(),
            [
                Step::FaceHeading(128.0),
                Step::Move(MoveCommand::Forward, Duration::from_secs(12)),
                Step::Move(MoveCommand::TurnLeft, Duration::from_millis(500)),
                Step::Camera {
                    heading: 64.0,
                    relative: false,
                    pitch: 0.0,
                    radius: Some(150.0),
                },
                Step::Camera {
                    heading: 0.0,
                    relative: true,
                    pitch: -89.0,
                    radius: None,
                },
                Step::Camera {
                    heading: 128.0,
                    relative: false,
                    pitch: -20.0,
                    radius: None,
                },
                Step::Camera {
                    heading: 256.0,
                    relative: true,
                    pitch: -15.0,
                    radius: Some(300.0),
                },
                Step::Camera {
                    heading: 32.0,
                    relative: false,
                    pitch: 89.0,
                    radius: None,
                },
                Step::Press(vec![KeyCode::NumLock]),
                Step::Hold(vec![KeyCode::Numpad8], Duration::from_millis(500)),
                Step::Press(vec![KeyCode::PageUp]),
                Step::Press(vec![KeyCode::NumpadAdd]),
            ]
        );
        for bad in [
            "face(600)",
            "face(\"north\")",
            "move(\"up\", 1000)",
            "move(\"forward\")",
            "move(\"forward\", 70000)",
            "camera(64, -20, 0)",
            "camera(64, -20, 30000)",
            "camera(64, -20, 100, 5)",
            "camera(64, 95)",
            "camera(64, -95)",
            "camera(\"player\")",
            "camera(\"player\", 64)",
            "press(\"numpad10\")",
            "press(\"numpad\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn world_gestures_have_finite_displacement_and_hold_bounds() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "wheel(\"world\", 64)\nwheel(\"world\", -1.5)\nlook(\"world\", 120, -80, 500)\nlook(\"world\", 0, 0, 10000)",
                base
            )
            .unwrap(),
            [
                Step::WorldWheel(64.0),
                Step::WorldWheel(-1.5),
                Step::WorldLook {
                    delta: bevy::math::Vec2::new(120.0, -80.0),
                    duration: Duration::from_millis(500)
                },
                Step::WorldLook {
                    delta: bevy::math::Vec2::ZERO,
                    duration: Duration::from_secs(10)
                },
            ]
        );
        for bad in [
            "wheel(\"world\", \"NaN\")",
            "wheel(\"world\", \"inf\")",
            "wheel(\"world\", 65)",
            "wheel(\"world\", -65)",
            "wheel(1)",
            "wheel(\"world\", 1, 2)",
            "look(\"world\", 0, 0, 0)",
            "look(\"world\", 0, 0, 10001)",
            "look(\"world\", \"NaN\", 0, 100)",
            "look(\"world\", 0, \"inf\", 100)",
            "look(\"world\", 4097, 0, 100)",
            "look(\"world\", 0, -4097, 100)",
            "look(\"world\", 0, 0)",
            "look(\"world\", 0, 0, 100, \"extra\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn explicit_key_holds_use_the_existing_chord_parser() {
        assert_eq!(
            parse("key_down(\"CTRL+W\")\nkey_up(\"CTRL+W\")", Path::new(".")).unwrap(),
            vec![
                Step::KeyDown(vec![KeyCode::ControlLeft, KeyCode::KeyW]),
                Step::KeyUp(vec![KeyCode::ControlLeft, KeyCode::KeyW])
            ]
        );
        assert!(parse("key_down(\"not-a-key\")", Path::new(".")).is_err());
    }

    #[test]
    fn an_object_approach_names_a_tradeskill_container_kind() {
        let base = Path::new("private");
        assert_eq!(
            parse("approach(\"object\", 21, 12, 8000)\n", base).unwrap(),
            [Step::ApproachObject {
                kind: 21,
                range: 12.0,
                duration: Duration::from_secs(8),
            }]
        );
        // Bags that only hold things are not stations; the range and time
        // keep the targeted approach's bounds.
        for bad in [
            "approach(\"object\")",
            "approach(\"object\", 21, 12)",
            "approach(\"object\", 51, 12, 8000)",
            "approach(\"object\", 8, 12, 8000)",
            "approach(\"object\", 256, 12, 8000)",
            "approach(\"object\", \"wheel\", 12, 8000)",
            "approach(\"object\", 21, 0, 8000)",
            "approach(\"object\", 21, 12, 10001)",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_object_walk_names_a_tradeskill_container_kind() {
        let base = Path::new("private");
        assert_eq!(
            parse("walk(\"object\", 21, 12, 20000)\n", base).unwrap(),
            [Step::WalkObject {
                kind: 21,
                range: 12.0,
                duration: Duration::from_secs(20),
            }]
        );
        // The kinds are `approach object`'s; the range and time keep `walk`'s bounds.
        for bad in [
            "walk(\"object\")",
            "walk(\"object\", 21, 12)",
            "walk(\"object\", 51, 12, 8000)",
            "walk(\"object\", 8, 12, 8000)",
            "walk(\"object\", 256, 12, 8000)",
            "walk(\"object\", \"wheel\", 12, 8000)",
            "walk(\"object\", 21, 0, 8000)",
            "walk(\"object\", 21, 12, 60001)",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_death_wait_takes_a_time_up_to_the_longest_wait() {
        let base = Path::new("private");
        assert_eq!(
            parse("wait_dead(90000)\nwait_dead(120000)\n", base).unwrap(),
            [
                Step::WaitDead(Duration::from_secs(90)),
                Step::WaitDead(MAX_WAIT),
            ]
        );
        for bad in ["wait_dead()", "wait_dead(120001)", "wait_dead(\"soon\")"] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn gm_takes_any_line_a_user_could_type_after_the_hash() {
        // No word list and no argument filter: the server decides what a
        // `#` line does, for a GM and anyone else (6.584). Lines the old
        // list refused go too.
        let base = Path::new("private");
        for command in [
            "gm on",
            "scribespells 10",
            "giveitem 1001 1",
            "pvp ON",
            "show zone_status",
            "show inventory equip gen",
            "set invulnerable on",
            "set language 5 20",
            "setlanguage 28 20",
            "traindisc 52 55",
            "faction reset 406",
            "rules reset",
            "rules setdb Character:FoodLossPerUpdate 32",
            "spawn ScriptFixture 60 1 0 200",
            "depop",
            "depopzone",
            "guild rename 3 Renamed",
            "guild create Example Fixture-Name",
            "goto 1;2",
            "summon Name,Other",
            "SUMMON",
            "shutdown",
            "zone qeynos #givemoney 999",
        ] {
            assert_eq!(
                parse(&format!("gm(\"{command}\")"), base).unwrap(),
                [Step::Gm(command.into())],
                "{command}"
            );
        }
        // Only the chat line's own limits: its length with the `#`, and no
        // control characters; and the `#` is the step's own.
        let longest = "a".repeat(crate::chat::MAX_TEXT - 1);
        assert_eq!(
            parse(&format!("gm(\"{longest}\")"), base).unwrap(),
            [Step::Gm(longest.clone())]
        );
        for bad in [
            format!("gm(\"{longest}a\")"),
            "gm(\"\")".to_owned(),
            "gm(\"#summon\")".to_owned(),
            "gm(\"say bad\u{0007}\")".to_owned(),
        ] {
            assert!(parse(&bad, base).is_err(), "{bad}");
        }
        // `chat say #...` says what typing it would.
        assert_eq!(
            parse("chat(\"say #time\")", base).unwrap(),
            [Step::Chat(eq_client_core::OutboundChat::Say(
                "#time".into()
            ))]
        );
    }

    #[test]
    fn a_script_may_ask_who_is_online() {
        let base = Path::new("private");
        assert_eq!(
            parse("slash(\"who\")\nslash(\"who all wiz 50\")\n", base).unwrap(),
            [
                Step::Slash("/who".into()),
                Step::Slash("/who all wiz 50".into())
            ]
        );
        assert_eq!(
            parse("slash(\"consent Helper\")\nslash(\"corpsedrag\")\n", base).unwrap(),
            [
                Step::Slash("/consent Helper".into()),
                Step::Slash("/corpsedrag".into())
            ]
        );
    }

    #[test]
    fn a_script_may_speak_as_a_player_types() {
        use eq_client_core::OutboundChat;
        let base = Path::new("private");
        assert_eq!(
            parse(
                "chat(\"say Hail there\")\nchat(\"ooc lfg\")\nchat(\"tell Friend inc now\")\n",
                base
            )
            .unwrap(),
            [
                Step::Chat(OutboundChat::Say("Hail there".into())),
                Step::Chat(OutboundChat::Ooc("lfg".into())),
                Step::Chat(OutboundChat::Tell {
                    recipient: "Friend".into(),
                    message: "inc now".into(),
                }),
            ]
        );
        // Never an empty line. A `#` line goes, as typing it would (6.584).
        for bad in [
            "chat()",
            "chat(\"say\")",
            "chat(\"yell hi\")",
            "chat(\"tell Friend\")",
            "chat(\"tell Fr1end hi\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_script_may_drag_a_window_by_its_name() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "drag(\"actions\", 600, 300)\ndrag(\"chat\", 700.5, 450)\n",
                base
            )
            .unwrap(),
            [
                Step::Drag("actions".into(), bevy::math::Vec2::new(600.0, 300.0)),
                Step::Drag("chat".into(), bevy::math::Vec2::new(700.5, 450.0)),
            ]
        );
        assert_eq!(
            parse("drag(\"chat-group-1\", 400, 300)\n", base).unwrap(),
            [Step::Drag(
                "chat-group-1".into(),
                bevy::math::Vec2::new(400.0, 300.0)
            )]
        );
        for bad in [
            "drag()",
            "drag(\"chat\", 1)",
            "drag(\"nowhere\", 1, 2)",
            "drag(\"chat\", -5, 2)",
            "drag(\"chat-group-0\", 400, 300)",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_script_may_make_and_command_a_pet() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "gm(\"makepet SumEarthR2\")\nslash(\"pet back off\")\nclick(\"pet\", \"sit\", \"down\")\n",
                base
            )
            .unwrap(),
            [
                Step::Gm("makepet SumEarthR2".into()),
                Step::Slash("/pet back off".into()),
                Step::Click(ClickTarget::Slash("/pet sit down")),
            ]
        );
        assert_eq!(
            parse("click(\"option\", \"target_ring\")\n", base).unwrap(),
            [Step::Click(ClickTarget::Option(
                eq_client_core::options::Toggle::TargetRing
            ))]
        );
        // A Filters page row goes by its kind's stable name (6.494, K3).
        assert_eq!(
            parse("click(\"filter\", \"ooc\")\n", base).unwrap(),
            [Step::Click(ClickTarget::Filter(
                eq_client_core::filters::FilterCategory::Ooc
            ))]
        );
        for bad in [
            "click(\"filter\", \"swarm_pet_death\")",
            "click(\"filter\", \"loud\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
        // A quality-of-life setting goes by its name in a file too.
        assert_eq!(
            parse("click(\"option\", \"skip_modified_food\")\n", base).unwrap(),
            [Step::Click(ClickTarget::Option(
                eq_client_core::options::Toggle::Qol(eq_client_core::qol::Fix::SkipModifiedFood)
            ))]
        );
        for bad in [
            "click(\"pet\")",
            "click(\"pet\", \"dance\")",
            "click(\"option\")",
            "click(\"option\", \"shiny\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_script_may_choose_and_follow_a_tracked_spawn() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click(\"tracking\", 2)\nclick(\"skill_select\", 3)\nclick(\"track\")\nclick(\"done\", \"tracking\")\n",
                base
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::TrackRow(1)),
                Step::Click(ClickTarget::SkillRow(2)),
                Step::Click(ClickTarget::Track),
                Step::Click(ClickTarget::Done("tracking")),
            ]
        );
        for bad in [
            "click(\"tracking\")",
            "click(\"tracking\", 0)",
            "click(\"track\", 1)",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn cursor_actions_require_separate_ordinary_confirmation_clicks() {
        let base = Path::new("private");
        for (name, target) in [
            ("destroy", ClickTarget::Destroy),
            ("drop", ClickTarget::Drop),
        ] {
            assert_eq!(
                parse(
                    &format!("click(\"{name}\")\nclick(\"answer\", \"no\")\nclick(\"{name}\")\nclick(\"answer\", \"yes\")\n"),
                    base,
                )
                .unwrap(),
                [
                    Step::Click(target),
                    Step::Click(ClickTarget::Answer(false)),
                    Step::Click(target),
                    Step::Click(ClickTarget::Answer(true))
                ]
            );
            for invalid in [
                name.to_owned(),
                format!("{name} 30"),
                format!("click(\"{name}\", 30)"),
                format!("click(\"{name}\", \"all\")"),
                format!("click(\"{name}\", \"yes\")"),
            ] {
                assert!(parse(&invalid, base).is_err());
            }
        }
    }

    #[test]
    fn parses_a_sound_by_its_file_name() {
        let base = Path::new("private");
        assert_eq!(
            parse("sound(\"LevelUp.WAV\")\n", base).unwrap(),
            [Step::Sound("LevelUp.WAV".into())]
        );
        assert_eq!(
            parse("sound(\"theme.MP3\")", base).unwrap(),
            [Step::Sound("theme.MP3".into())]
        );
        for bad in [
            "sound()",
            "sound(\".wav\")",
            "sound(\"theme.ogg\")",
            "sound(\"a.wav\", \"b.wav\")",
            "sound(\"audio/theme.mp3\")",
            r#"sound("audio\\theme.mp3")"#,
            "sound(\"C:theme.mp3\")",
            "sound(\"../levelup.wav\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_hotbuttons_and_holds_that_pick_hotkeys_up() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click(\"hot_button\", 1)\nhold_click(800, \"spell_gem\", 1)\nhold_click(600, \"hot_button\", 10)\n",
                base
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::HotButton(0)),
                Step::HoldClick(ClickTarget::SpellGem(0), Duration::from_millis(800)),
                Step::HoldClick(ClickTarget::HotButton(9), Duration::from_millis(600)),
            ]
        );
        for bad in [
            "click(\"hot_button\", 0)",
            "click(\"hot_button\", 11)",
            "hold_click(800)",
            "hold_click(\"spell_gem\", 1)",
            "hold_click(20000, \"spell_gem\", 1)",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_bag_tint_clicks() {
        let base = Path::new("private");
        assert_eq!(
            parse("click(\"tint\", 23)\nclick(\"tint\", 23, 4)\n", base).unwrap(),
            [
                Step::Click(ClickTarget::Tint(23, None)),
                Step::Click(ClickTarget::Tint(23, Some(4))),
            ]
        );
        for bad in [
            "click(\"tint\", \"x\")",
            "click(\"tint\", 23, \"x\")",
            "click(\"tint\", 23, 4, 5)",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_a_chat_tab_by_its_label() {
        use eq_client_core::chat::ChatTab;
        assert_eq!(
            parse(
                "click(\"chat_tab\", \"say\")\nclick(\"chat_tab\", \"OOC\")\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::ChatTab(ChatTab::Say)),
                Step::Click(ClickTarget::ChatTab(ChatTab::Ooc)),
            ]
        );
        assert!(parse("click(\"chat_tab\", \"nowhere\")\n", Path::new("private")).is_err());
    }

    #[test]
    fn right_click_buff_uses_the_ordinary_effects_button_target() {
        assert_eq!(
            parse(
                "right_click(\"buff\", 4)\nright_click(\"song\", 1)\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::RightClick(ClickTarget::Buff(true, 3)),
                Step::RightClick(ClickTarget::Buff(false, 0))
            ]
        );
        assert!(parse("right_click(\"buff\", 0)\n", Path::new("private")).is_err());
    }

    #[test]
    fn parses_the_skins_effects_window_buttons() {
        assert_eq!(
            parse(
                "hover(\"buff\", 1)\nhover(\"song\", 2)\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Hover(ClickTarget::Buff(true, 0)),
                Step::Hover(ClickTarget::Buff(false, 1)),
            ]
        );
        assert!(parse("hover(\"buff\", 0)\n", Path::new("private")).is_err());
    }

    #[test]
    fn parses_the_skins_merchant_window_clicks() {
        assert_eq!(
            parse(
                "click(\"merchant_row\", 3)\nclick(\"buy_chosen\")\nclick(\"sell_chosen\")\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::Trade(TradeClick::Choose(3))),
                Step::Click(ClickTarget::Trade(TradeClick::BuyChosen)),
                Step::Click(ClickTarget::Trade(TradeClick::SellChosen)),
            ]
        );
    }

    #[test]
    fn parses_a_windows_own_done_button() {
        assert_eq!(
            parse(
                "click(\"done\", \"bank\")\nhover(\"done\", \"world-container\")\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::Done("bank")),
                Step::Hover(ClickTarget::Done("world-container")),
            ]
        );
        assert!(parse("click(\"done\", \"nowhere\")\n", Path::new("private")).is_err());
    }

    #[test]
    fn parses_a_skin_button_by_its_screen_id() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click(\"button\", \"bank\", \"Fake_Button1\")\n\
                 right_click(\"button\", \"bank\", \"Fake_Button1\")\n\
                 hover(\"button\", \"merchant\", \"FakeButton\")\n\
                 hold_click(500, \"button\", \"bank\", \"FakeButton\")\n",
                base
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::Button("bank", "Fake_Button1")),
                Step::RightClick(ClickTarget::Button("bank", "Fake_Button1")),
                Step::Hover(ClickTarget::Button("merchant", "FakeButton")),
                Step::HoldClick(
                    ClickTarget::Button("bank", "FakeButton"),
                    Duration::from_millis(500)
                ),
            ]
        );
        for bad in [
            "click(\"button\", \"bank\")",
            "click(\"button\", \"nowhere\", \"FakeButton\")",
            "click(\"button\", \"bank\", \"Fake-Button\")",
            "click(\"button\", \"bank\", \"Fake\", \"Button\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
        let long = format!("click(\"button\", \"bank\", \"{}\")", "A".repeat(65));
        assert!(parse(&long, base).is_err());
    }

    #[test]
    fn parses_the_quantity_windows_slider_and_number_box() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click(\"slider\", \"quantity\", 50)\nclick(\"slider\", \"max_fps\", 10)\nclick(\"slider\", \"run_boost\", 50)\nclick(\"slider\", \"run_boost\", 100)\nclick(\"pick\", \"amount\")\npress(\"backspace\")\n",
                base
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::Slider(SliderClick::Quantity, 50)),
                Step::Click(ClickTarget::Slider(
                    SliderClick::Level(eq_client_core::options::Level::MaxFps),
                    10
                )),
                Step::Click(ClickTarget::Slider(
                    SliderClick::Level(eq_client_core::options::Level::RunSpeed),
                    50
                )),
                Step::Click(ClickTarget::Slider(
                    SliderClick::Level(eq_client_core::options::Level::RunSpeed),
                    100
                )),
                Step::Click(ClickTarget::Pick(PickButton::Amount)),
                Step::Press(vec![KeyCode::Backspace]),
            ]
        );
        for bad in [
            "click(\"slider\", \"quantity\", 101)",
            "click(\"slider\", \"amount\", 5)",
            "click(\"pick\", \"all\")",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }
}
