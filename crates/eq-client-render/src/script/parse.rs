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
/// `EQEmu` GM commands a script may send, without the leading `#`.
const GM_COMMANDS: [&str; 38] = [
    // The server's own view of the player's stats, weight and coins among
    // them, said back in chat; reads only.
    "showstats",
    // The server's own view of the player's skills, said back in chat;
    // reads only.
    "showskills",
    // GM mode on or off: off, the server lets the player go hungry. On, the
    // book and gem commands below act on a player target instead of the GM.
    "gm",
    // A rule changed in this zone only, such as how fast hunger comes, or
    // the zone's rules reloaded; never stored or reset.
    "rules",
    // The time of day, for every zone.
    "time",
    // A pet of a kind the server knows, such as an earth elemental.
    "makepet",
    "summon",
    "summonitem",
    // Searches the server's items by name, to find one to summon.
    "finditem",
    // Searches the server's tradeskill recipes by name, and lists one's
    // container and components.
    "findrecipe",
    "viewrecipe",
    // A temporary NPC at the GM's feet, and coins or items on the target.
    "spawn",
    // Removes only the selected NPC without starting a respawn timer.
    "depop",
    "npcloot",
    "castspell",
    "damage",
    // A server-issued stun for a local test fixture; duration is milliseconds.
    "stun",
    "givemoney",
    "zone",
    "goto",
    "level",
    "heal",
    "kill",
    "repop",
    "freeze",
    "unfreeze",
    // Shows the target (or the GM) in another material, without changing gear.
    "wc",
    // The targeted NPC says a line, as its quest dialogue would.
    "npcsay",
    // The targeted NPC casts a spell on its target, to bring about a
    // spell's effect on the player as it would come from an NPC.
    "npccast",
    // Which of the server's log categories it writes, so a run can read
    // what the server made of what it sent.
    "logs",
    // Readies another test character: every spell they can use up to a
    // level scribed in their book, every skill at its most for their
    // level, and an item onto their cursor, as summonitem puts one on the
    // GM's.
    "scribespells",
    // Readies the learned discipline list for a local EQEmu fixture.
    "traindisc",
    "maxskills",
    "giveitem",
    // Another test character's standing with a faction: `faction view` and
    // `faction review` read it, `factionassociation <id> <amount>` raises
    // it as a quest reward would, and `faction reset <id>` puts it back.
    "faction",
    "factionassociation",
    // Puts them back as they were for another run: a spell taken out of
    // their gems, and every buff taken off them.
    "unmemspell",
    "nukebuffs",
];

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
        /// Downward pitch in degrees, from level (0) to straight down.
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
    /// An Options window checkbox, by the option's name in a file.
    Option(eq_client_core::options::Toggle),
    /// A slider, pressed this far along it, in percent.
    Slider(SliderClick, u8),
    /// The Keyboard page's filter drop-down, which opens or closes its list.
    KeyFilter,
    /// A choice in the open drop-down's list, from zero.
    Choice(usize),
    /// A row of the Training window's list, its Train button or its Done
    /// button.
    Training(TrainingClick),
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
    /// A place on the skin's spellbook's open pages, from zero
    /// (`SBW_Spell0` to `SBW_Spell15`).
    BookPlace(u8),
    /// The skin's spellbook's arrow: forward (true) or back.
    BookPage(bool),
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
        pitch: number(pitch, -89.0, 0.0)?,
        radius: radius
            .first()
            .map(|radius| number(radius, 1.0, 20_000.0))
            .transpose()?,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive command grammar keeps unknown-command rejection explicit"
)]
pub(super) fn parse_step(line: &str) -> Result<Step, String> {
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
    // A world container that combines: a station, not a bag that only holds things.
    let station = |kind: &str| {
        kind.parse::<u8>()
            .ok()
            .filter(|kind| eq_client_core::tradeskills::combines(*kind))
            .ok_or("expected a tradeskill container type")
    };
    Ok(match (command, rest.as_slice()) {
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
        ("right_click", target) => Step::RightClick(parse_click(target)?),
        ("hover", target) => Step::Hover(parse_click(target)?),
        ("hold_click", [duration, target @ ..]) => {
            Step::HoldClick(parse_click(target)?, millis(duration, MAX_HOLD)?)
        }
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
        _ => return Err("unknown or malformed step".into()),
    })
}

/// `slash <command> [words]`: a slash command a script may type, among
/// those that change nothing beyond the test characters.
fn parse_slash(words: &[&str]) -> Result<Step, String> {
    Ok(match words {
        [command]
            if matches!(
                *command,
                "camp" | "sit" | "stand" | "ranged" | "stopsong" | "harmtouch" | "layonhands"
            ) =>
        {
            Step::Slash(format!("/{command}"))
        }
        ["camp", "desktop"] => Step::Slash("/camp desktop".into()),
        ["discipline", name] if eq_client_core::disciplines::Discipline::named(name).is_some() => {
            Step::Slash(format!("/discipline {name}"))
        }
        ["target", name @ ..] if !name.is_empty() => {
            Step::Slash(format!("/target {}", name.join(" ")))
        }
        // Corpses: consent, summon and drag; a test death leaves one.
        [command @ ("consent" | "deny"), name] => Step::Slash(format!("/{command} {name}")),
        ["log"] => Step::Slash("/log".into()),
        ["shownames", level] => Step::Slash(format!("/shownames {level}")),
        // Corpses summoned or dragged; groups joined, left, disbanded or
        // declined; where and when the player is, which the client answers
        // itself; and how `/who` lists the player.
        [
            command @ ("corpse" | "corpsedrag" | "corpsedrop" | "follow" | "disband" | "loc"
            | "time" | "afk" | "anonymous" | "roleplay" | "raidaccept" | "raiddecline"
            | "raiddisband" | "raidwindow"),
        ] => Step::Slash(format!("/{command}")),
        // The raid's lead, handed on by name or to the target.
        ["makeraidleader", name @ ..] if name.len() <= 1 => Step::Slash(
            ["/makeraidleader"]
                .iter()
                .chain(name)
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
        ),
        // A die, an emote, or another's target.
        ["random", numbers @ ..]
            if numbers.len() <= 2 && numbers.iter().all(|number| number.parse::<u32>().is_ok()) =>
        {
            Step::Slash(
                ["/random"]
                    .iter()
                    .chain(numbers)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        ["emote", words @ ..] if !words.is_empty() => {
            Step::Slash(format!("/emote {}", words.join(" ")))
        }
        ["assist", name @ ..] if name.len() <= 1 => Step::Slash(
            ["/assist"]
                .iter()
                .chain(name)
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
        ),
        // A raid invitation, by name or for the target, and its answers.
        ["raidinvite", name @ ..] if name.len() <= 1 => Step::Slash(
            ["/raidinvite"]
                .iter()
                .chain(name)
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
        ),
        // A group invitation, by name or for the target.
        ["invite", name @ ..] if name.len() <= 1 => Step::Slash(
            ["/invite"]
                .iter()
                .chain(name)
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
        ),
        // A command to the pet, as typed.
        ["pet", words @ ..] if !words.is_empty() => {
            Step::Slash(format!("/pet {}", words.join(" ")))
        }
        // Asking who is online changes nothing.
        ["who", words @ ..] => Step::Slash(
            ["/who"]
                .iter()
                .chain(words)
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => return Err("unknown or malformed slash command".into()),
    })
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
    Ok(ClickTarget::Slash(match words {
        ["invite"] => "/invite",
        ["follow"] => "/follow",
        ["disband" | "decline"] => "/disband",
        _ => return Err("expected invite, follow, disband or decline".into()),
    }))
}

/// `click <target>` or `right_click <target>`: a slot, spellbook, trade or
/// bag tint button.
fn parse_click(words: &[&str]) -> Result<ClickTarget, String> {
    fn value<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, String> {
        text.parse().map_err(|_| format!("expected {what}"))
    }
    Ok(match words {
        ["slot", slot] => ClickTarget::Slot(value(slot, "a slot number")?),
        ["scribe"] => ClickTarget::Scribe,
        ["store"] => ClickTarget::Store,
        ["destroy"] => ClickTarget::Destroy,
        ["drop"] => ClickTarget::Drop,
        ["book", row] => ClickTarget::BookRow(value(row, "a book row number")?),
        ["loot", place] => {
            ClickTarget::Trade(TradeClick::Take(value(place, "a place on the corpse")?))
        }
        ["loot_all"] => ClickTarget::Trade(TradeClick::TakeAll),
        ["loot_done"] => ClickTarget::Trade(TradeClick::EndLoot),
        ["buy", slot] => ClickTarget::Trade(TradeClick::Buy(value(slot, "a merchant slot")?)),
        ["sell", slot] => ClickTarget::Trade(TradeClick::Sell(value(slot, "an inventory slot")?)),
        ["shop_done"] => ClickTarget::Trade(TradeClick::EndShop),
        ["merchant_row", slot] => {
            ClickTarget::Trade(TradeClick::Choose(value(slot, "a merchant slot")?))
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
        ["option", name] => ClickTarget::Option(
            eq_client_core::options::Toggle::all()
                .find(|toggle| toggle.key() == *name)
                .ok_or("expected an option, by its name in a file")?,
        ),
        ["attack"] => ClickTarget::Attack,
        ["training", row] => ClickTarget::Training(TrainingClick::Row(ordinal(row, "a row")?)),
        ["train"] => ClickTarget::Training(TrainingClick::Train),
        ["answer", "yes"] => ClickTarget::Answer(true),
        ["page", "next"] => ClickTarget::Page(true),
        ["page", "back"] => ClickTarget::Page(false),
        ["combine", slot] => ClickTarget::Combine(value(slot, "a pack slot number")?),
        ["map", action] => ClickTarget::Map(map_button(action)?),
        ["answer", "no"] => ClickTarget::Answer(false),
        ["training_done"] => ClickTarget::Training(TrainingClick::Done),
        ["slider", name, percent] => ClickTarget::Slider(
            slider(name)?,
            percent
                .parse::<u8>()
                .ok()
                .filter(|percent| *percent <= 100)
                .ok_or_else(|| String::from("expected a percentage from 0 to 100"))?,
        ),
        ["dropdown", "key_filter"] => ClickTarget::KeyFilter,
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
        ["tint", slot] => ClickTarget::Tint(value(slot, "a bag slot")?, None),
        ["tint", slot, color] => ClickTarget::Tint(
            value(slot, "a bag slot")?,
            Some(value(color, "a palette color")?),
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
        ["book_page", "next"] => ClickTarget::BookPage(true),
        ["book_page", "back"] => ClickTarget::BookPage(false),
        _ => return Err("unknown or malformed step".into()),
    })
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
        .ok_or_else(|| "expected clip_plane, max_fps, mouse_sensitivity or quantity".into())
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

/// `gm <command> [arguments]`: an allowed local test command without its `#`.
fn parse_gm(words: &[&str]) -> Result<Step, String> {
    gm_command(words).map(Step::Gm)
}

/// Validate the text for both parsed and programmatically constructed steps.
pub(super) fn gm_command(words: &[&str]) -> Result<String, String> {
    let [command, arguments @ ..] = words else {
        return Err("gm takes a command".into());
    };
    if !GM_COMMANDS.contains(command) {
        return Err(format!("gm allows only {}", GM_COMMANDS.join(", ")));
    }
    let plain = |argument: &&str| {
        argument
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':'))
    };
    if !arguments.iter().all(plain) {
        return Err("gm takes plain arguments".into());
    }
    match *command {
        "spawn" => match arguments {
            [_, ..] if arguments.len() <= 4 => (),
            _ if fixture_spawn(arguments) => (),
            _ => return Err(concat!(
                "gm spawn takes a name and up to three short arguments, or ",
                "NAME RACE LEVEL TEXTURE HEALTH GENDER CLASS PRIMARY_MODEL SECONDARY_MODEL 0 BODY_TYPE"
            ).into()),
        },
        "depop" if !arguments.is_empty() => return Err("gm depop takes no arguments".into()),
        "traindisc" => {
            // EQEmu compares class levels against these bounds. Keep the
            // range below unavailable class levels such as 127 and 255.
            let level = |word: &str| {
                word.parse::<u8>()
                    .ok()
                    .filter(|value| word.len() <= 3 && (1..=126).contains(value))
            };
            let valid = match arguments {
                [max] => level(max).is_some(),
                [max, min] => level(max).zip(level(min)).is_some_and(|(max, min)| min <= max),
                _ => false,
            };
            if !valid {
                return Err("gm traindisc takes MAX [MIN] with 1 <= MIN <= MAX <= 126".into());
            }
        }
        _ if arguments.len() > 4 => return Err("gm takes up to four plain arguments".into()),
        _ => (),
    }
    if *command == "rules" && !matches!(arguments, ["set", _, _] | ["reload"]) {
        return Err("gm rules takes set <Category:Rule> <value> or reload".into());
    }
    Ok(words.join(" "))
}

/// The complete `EQEmu` temporary NPC form uses explicit decimal fields.
/// Keep the merchant unset so the fixture cannot expose merchant stock.
fn fixture_spawn(arguments: &[&str]) -> bool {
    let [
        name,
        race,
        level,
        texture,
        health,
        gender,
        class,
        primary,
        secondary,
        "0",
        body,
    ] = arguments
    else {
        return false;
    };
    let within = |word: &str, low, high| {
        word.parse::<u32>()
            .is_ok_and(|value| (low..=high).contains(&value))
    };
    (1..=60).contains(&name.len())
        && name.bytes().all(|byte| byte.is_ascii_alphabetic())
        && within(race, 1, u32::from(u16::MAX))
        && within(level, 1, u32::from(u8::MAX))
        && within(texture, 0, u32::from(u8::MAX))
        && within(health, 1, 2_100_000_000)
        && within(gender, 0, 2)
        && within(class, 1, u32::from(u8::MAX))
        && within(primary, 0, u32::MAX)
        && within(secondary, 0, u32::MAX)
        && within(body, 0, u32::from(u8::MAX))
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
/// <Name> <words>`: a line as a player types it. The words are printable
/// ASCII and never a `#` command, which only a `gm` step sends.
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
        (!text.is_empty()
            && !text.starts_with('#')
            && text.chars().all(|c| c.is_ascii_graphic() || c == ' '))
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
    fn deletion_requires_exact_repeated_name_without_position_fallback() {
        assert_eq!(
            parse(
                "delete Disposable confirm Disposable",
                std::path::Path::new(".")
            )
            .unwrap(),
            vec![Step::DeleteCharacter {
                name: "Disposable".into(),
                confirmation: "Disposable".into()
            }]
        );
        for source in [
            "delete Disposable",
            "delete Disposable confirm Other",
            "delete Disposable confirm disposable",
            "delete 0",
            "delete Disposable yes",
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
                "interact 6 42 12\ninspect_link 6 70 0\ncompose_owned_link 6 23",
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
            "interact 42",
            "interact 6 65536 12",
            "interact 6 42 -1",
            "inspect_link 70 0",
            "inspect_link 6 70 -1",
            "inspect_link 6 70 0 raw",
            "compose_owned_link 23",
            "compose_owned_link -1 23",
            "compose_owned_link 6 worn",
            "compose_owned_link 6 23 raw",
        ] {
            assert!(parse(malformed, base).is_err(), "{malformed}");
        }
    }

    #[test]
    fn dialog_and_player_inspection_steps_require_scoped_choices() {
        let base = Path::new(".");
        assert_eq!(
            parse("popup 6 14 no\ninspect_player 6 42 12", base).unwrap(),
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
            "popup yes",
            "popup 6 14 auto",
            "inspect_player 42",
            "inspect_player 6 65536 12",
        ] {
            assert!(parse(text, base).is_err(), "{text}");
        }
    }

    #[test]
    fn a_scrollbar_arrow_is_reached_by_its_windows_key() {
        let steps = parse(
            "click scroll_up chat\nclick scroll_down skills\n",
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
        assert!(parse("click scroll_up nowhere\n", Path::new("private")).is_err());
    }

    #[test]
    fn parses_the_login_steps() {
        let base = Path::new("private");
        assert_eq!(
            parse("login\nwait_servers\nserver Example  World\n", base).unwrap(),
            [
                Step::Login,
                Step::WaitServers,
                Step::Server("Example World".into()),
            ]
        );
        // The account and password come only from the environment.
        for bad in ["login someone", "wait_servers now", "server"] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_bounded_steps_and_rejects_unsafe_input() {
        let base = Path::new("private");
        let steps = parse(
            "wait_select\nselect Someone\ncreate Testcleric 1 2 0 212 1 4\ncreate_refused Testcleric 1 2 0 212 1 4\nwait_online\nwait_zone TOX\nslash camp\nslash target a cave rat\n\
             gm summon\ngm damage 10000\ngm givemoney 0 0 5 0\npress F1 # self\n\
             press alt+1\nhold W 1500\nwait 250\ntrace 2000\nface\napproach 12 5000\nwalk 8 30000\nclick slot 23\nright_click slot 22\nhover close_box chat\n\
             click scribe\nclick store\nclick book 0\nclick memorize 2\nclick loot 0\nclick loot_all\nclick buy 3\nclick sell 23\nclick shop_done\ngive\nclick give\nreport after cast\nscreenshot a.png\nquit\n",
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
            "create lowercase 1 2 0 212 1 4",
            "create Testbad 1 17 0 212 1 4",
            "slash ooc",
            "walk 8 90000",
            "walk 0 1000",
            "gm",
            "gm shutdown",
            "gm SUMMON",
            "gm zone a b c d e",
            "gm goto 1;2",
            "gm summon Name,Other",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
        // `#` starts a comment, so a script can never smuggle one into a step.
        assert_eq!(
            parse("gm zone qeynos #givemoney 999", base).unwrap(),
            [Step::Gm("zone qeynos".into())]
        );
    }

    #[test]
    fn desktop_camp_uses_the_game_command_owner_and_rejects_other_destinations() {
        let base = Path::new("private");
        assert_eq!(
            parse("slash camp desktop\n", base).unwrap(),
            [Step::Slash("/camp desktop".into())]
        );
        for bad in ["slash camp server", "slash camp desktop now"] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn wait_relocation_is_a_bounded_observation() {
        let base = std::path::Path::new(".");
        assert_eq!(
            parse("wait_relocation 1500\n", base).unwrap(),
            [Step::WaitRelocation(Duration::from_millis(1500))]
        );
        assert!(parse("wait_relocation\n", base).is_err());
        assert!(parse("wait_relocation 12 34 56\n", base).is_err());
    }

    #[test]
    fn discipline_slash_names_only_the_supported_set() {
        let base = std::path::Path::new(".");
        assert_eq!(
            parse("slash discipline defensive\n", base).unwrap(),
            [Step::Slash("/discipline defensive".into())]
        );
        assert!(parse("slash discipline 4499\n", base).is_err());
    }

    #[test]
    fn knight_slash_actions_use_the_current_target() {
        let base = std::path::Path::new(".");
        assert_eq!(
            parse("slash harmtouch\nslash layonhands\n", base).unwrap(),
            [
                Step::Slash("/harmtouch".into()),
                Step::Slash("/layonhands".into())
            ]
        );
        assert!(parse("slash harmtouch 12\n", base).is_err());
    }

    #[test]
    fn ranged_slash_uses_the_current_target_and_takes_no_arguments() {
        let base = Path::new("private");
        assert_eq!(
            parse("slash ranged\n", base).unwrap(),
            [Step::Slash("/ranged".into())]
        );
        assert!(parse("slash ranged 12\n", base).is_err());
    }

    #[test]
    fn moving_steps_name_commands_and_key_steps_name_the_keypad() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "face 128\nmove forward 12000\nmove turn_left 500\ncamera 64 0 150\n\
                 camera player 0 -89\ncamera 128 -20\ncamera player 256 -15 300\npress numlock\nhold numpad8 500\npress pageup\n\
                 press numpad_add\n",
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
                Step::Press(vec![KeyCode::NumLock]),
                Step::Hold(vec![KeyCode::Numpad8], Duration::from_millis(500)),
                Step::Press(vec![KeyCode::PageUp]),
                Step::Press(vec![KeyCode::NumpadAdd]),
            ]
        );
        for bad in [
            "face 600",
            "face north",
            "move up 1000",
            "move forward",
            "move forward 70000",
            "camera 64 -20 0",
            "camera 64 -20 30000",
            "camera 64 -20 100 5",
            "camera 64 5",
            "camera player",
            "camera player 64",
            "press numpad10",
            "press numpad",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_object_approach_names_a_tradeskill_container_kind() {
        let base = Path::new("private");
        assert_eq!(
            parse("approach object 21 12 8000\n", base).unwrap(),
            [Step::ApproachObject {
                kind: 21,
                range: 12.0,
                duration: Duration::from_secs(8),
            }]
        );
        // Bags that only hold things are not stations; the range and time
        // keep the targeted approach's bounds.
        for bad in [
            "approach object",
            "approach object 21 12",
            "approach object 51 12 8000",
            "approach object 8 12 8000",
            "approach object 256 12 8000",
            "approach object wheel 12 8000",
            "approach object 21 0 8000",
            "approach object 21 12 10001",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_object_walk_names_a_tradeskill_container_kind() {
        let base = Path::new("private");
        assert_eq!(
            parse("walk object 21 12 20000\n", base).unwrap(),
            [Step::WalkObject {
                kind: 21,
                range: 12.0,
                duration: Duration::from_secs(20),
            }]
        );
        // The kinds are `approach object`'s; the range and time keep `walk`'s bounds.
        for bad in [
            "walk object",
            "walk object 21 12",
            "walk object 51 12 8000",
            "walk object 8 12 8000",
            "walk object 256 12 8000",
            "walk object wheel 12 8000",
            "walk object 21 0 8000",
            "walk object 21 12 60001",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_death_wait_takes_a_time_up_to_the_longest_wait() {
        let base = Path::new("private");
        assert_eq!(
            parse("wait_dead 90000\nwait_dead 120000\n", base).unwrap(),
            [
                Step::WaitDead(Duration::from_secs(90)),
                Step::WaitDead(MAX_WAIT),
            ]
        );
        for bad in ["wait_dead", "wait_dead 120001", "wait_dead soon"] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_gm_may_ready_another_test_character_and_put_them_back() {
        assert_eq!(
            parse(
                "gm gm on\ngm scribespells 10\ngm maxskills\ngm giveitem 1001 1\n\
                 gm unmemspell 200\ngm nukebuffs\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Gm("gm on".into()),
                Step::Gm("scribespells 10".into()),
                Step::Gm("maxskills".into()),
                Step::Gm("giveitem 1001 1".into()),
                Step::Gm("unmemspell 200".into()),
                Step::Gm("nukebuffs".into()),
            ]
        );
    }

    #[test]
    fn gm_traindisc_requires_a_bounded_max_then_optional_min() {
        let base = Path::new("private");
        for command in [
            "traindisc 1",
            "traindisc 55 52",
            "traindisc 52 52",
            "traindisc 126 1",
        ] {
            assert_eq!(
                parse(&format!("gm {command}"), base).unwrap(),
                [Step::Gm(command.into())]
            );
        }
        for arguments in [
            "", "0", "-1", "127", "256", "00055", "55.5", "52 55", "55 0", "55 -1", "55 none",
            "55 52 50",
        ] {
            assert!(
                parse(&format!("gm traindisc {arguments}"), base).is_err(),
                "{arguments}"
            );
        }
    }

    #[test]
    fn a_gm_may_read_raise_and_reset_another_test_characters_faction() {
        assert_eq!(
            parse(
                "gm faction view\ngm factionassociation 406 1500\ngm faction reset 406\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Gm("faction view".into()),
                Step::Gm("factionassociation 406 1500".into()),
                Step::Gm("faction reset 406".into()),
            ]
        );
    }

    #[test]
    fn a_zone_rule_may_change_for_now_but_never_be_stored_or_reset() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "gm rules set Character:FoodLossPerUpdate 4000\ngm rules reload\n",
                base
            )
            .unwrap(),
            [
                Step::Gm("rules set Character:FoodLossPerUpdate 4000".into()),
                Step::Gm("rules reload".into())
            ]
        );
        for bad in [
            "gm rules reset",
            "gm rules setdb Character:FoodLossPerUpdate 32",
            "gm rules set Character:FoodLossPerUpdate",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn gm_fixture_commands_validate_the_complete_spawn_and_bare_depop() {
        let base = Path::new("private");
        for command in [
            "spawn ScriptFixture",
            "spawn ScriptFixture 1 1 0",
            "spawn ScriptFixture 60 1 0 200 2 1 0 0 0 3",
            "depop",
        ] {
            assert_eq!(
                parse(&format!("gm {command}"), base).unwrap(),
                [Step::Gm(command.into())]
            );
        }
        for command in [
            "spawn",
            "spawn ScriptFixture 60 1 0 200",
            "spawn ScriptFixture 60 1 0 200 2 1 0 0 0",
            "spawn ScriptFixture 60 1 0 200 2 1 0 0 0 3 extra",
            "spawn ScriptFixture 60 1 0 200 2 1 0 0 0 undead",
            "spawn ScriptFixture 60 1 0 - 2 1 0 0 0 3",
            "spawn ScriptFixture 60 1 0 0 2 1 0 0 0 3",
            "spawn ScriptFixture 60 1 0 2100000001 2 1 0 0 0 3",
            "spawn ScriptFixture 60 1 0 200 3 1 0 0 0 3",
            "spawn ScriptFixture 60 1 0 200 2 1 -1 0 0 3",
            "spawn ScriptFixture 60 1 0 200 2 1 0 0 1 3",
            "spawn ScriptFixture 60 1 0 200 2 1 0 0 0 256",
            "spawn ScriptFixture2 60 1 0 200 2 1 0 0 0 3",
            "depop ScriptFixture",
            "depop 1",
            "depopall",
            "depopzone",
            "damage 1 2 3 4 5",
        ] {
            assert!(parse(&format!("gm {command}"), base).is_err(), "{command}");
        }
    }

    #[test]
    fn a_script_may_ask_who_is_online() {
        let base = Path::new("private");
        assert_eq!(
            parse("slash who\nslash who all wiz 50\n", base).unwrap(),
            [
                Step::Slash("/who".into()),
                Step::Slash("/who all wiz 50".into())
            ]
        );
        assert_eq!(
            parse("slash consent Helper\nslash corpsedrag\n", base).unwrap(),
            [
                Step::Slash("/consent Helper".into()),
                Step::Slash("/corpsedrag".into())
            ]
        );
        assert!(parse("slash consent\n", base).is_err());
    }

    #[test]
    fn a_script_may_speak_as_a_player_types() {
        use eq_client_core::OutboundChat;
        let base = Path::new("private");
        assert_eq!(
            parse(
                "chat say Hail there
chat ooc lfg
chat tell Friend inc now
",
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
        // Never an empty line or a `#` command, which only a gm step sends.
        for bad in [
            "chat",
            "chat say",
            "chat yell hi",
            "chat tell Friend",
            "chat tell Fr1end hi",
            "chat say #summon",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_script_may_drag_a_window_by_its_name() {
        let base = Path::new("private");
        assert_eq!(
            parse("drag actions 600 300\ndrag chat 700.5 450\n", base).unwrap(),
            [
                Step::Drag("actions".into(), bevy::math::Vec2::new(600.0, 300.0)),
                Step::Drag("chat".into(), bevy::math::Vec2::new(700.5, 450.0)),
            ]
        );
        assert_eq!(
            parse("drag chat-group-1 400 300\n", base).unwrap(),
            [Step::Drag(
                "chat-group-1".into(),
                bevy::math::Vec2::new(400.0, 300.0)
            )]
        );
        for bad in [
            "drag",
            "drag chat 1",
            "drag nowhere 1 2",
            "drag chat -5 2",
            "drag chat-group-0 400 300",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_script_may_make_and_command_a_pet() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "gm makepet SumEarthR2\nslash pet back off\nclick pet sit down\n",
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
            parse("click option target_ring\n", base).unwrap(),
            [Step::Click(ClickTarget::Option(
                eq_client_core::options::Toggle::TargetRing
            ))]
        );
        // A quality-of-life setting goes by its name in a file too.
        assert_eq!(
            parse("click option skip_modified_food\n", base).unwrap(),
            [Step::Click(ClickTarget::Option(
                eq_client_core::options::Toggle::Qol(eq_client_core::qol::Fix::SkipModifiedFood)
            ))]
        );
        for bad in [
            "slash pet",
            "click pet",
            "click pet dance",
            "click option",
            "click option shiny",
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
                    &format!("click {name}\nclick answer no\nclick {name}\nclick answer yes\n"),
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
                format!("click {name} 30"),
                format!("click {name} all"),
                format!("click {name} yes"),
            ] {
                assert!(parse(&invalid, base).is_err());
            }
        }
    }

    #[test]
    fn parses_a_sound_by_its_file_name() {
        let base = Path::new("private");
        assert_eq!(
            parse("sound LevelUp.WAV\n", base).unwrap(),
            [Step::Sound("LevelUp.WAV".into())]
        );
        assert_eq!(
            parse("sound theme.MP3", base).unwrap(),
            [Step::Sound("theme.MP3".into())]
        );
        for bad in [
            "sound",
            "sound .wav",
            "sound theme.ogg",
            "sound a.wav b.wav",
            "sound audio/theme.mp3",
            r"sound audio\theme.mp3",
            "sound C:theme.mp3",
            "sound ../levelup.wav",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_hotbuttons_and_holds_that_pick_hotkeys_up() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click hot_button 1\nhold_click 800 spell_gem 1\nhold_click 600 hot_button 10\n",
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
            "click hot_button 0",
            "click hot_button 11",
            "hold_click 800",
            "hold_click spell_gem 1",
            "hold_click 20000 spell_gem 1",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_bag_tint_clicks() {
        let base = Path::new("private");
        assert_eq!(
            parse("click tint 23\nclick tint 23 4\n", base).unwrap(),
            [
                Step::Click(ClickTarget::Tint(23, None)),
                Step::Click(ClickTarget::Tint(23, Some(4))),
            ]
        );
        for bad in ["click tint x", "click tint 23 x", "click tint 23 4 5"] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_a_chat_tab_by_its_label() {
        use eq_client_core::chat::ChatTab;
        assert_eq!(
            parse(
                "click chat_tab say\nclick chat_tab OOC\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::ChatTab(ChatTab::Say)),
                Step::Click(ClickTarget::ChatTab(ChatTab::Ooc)),
            ]
        );
        assert!(parse("click chat_tab nowhere\n", Path::new("private")).is_err());
    }

    #[test]
    fn right_click_buff_uses_the_ordinary_effects_button_target() {
        assert_eq!(
            parse(
                "right_click buff 4\nright_click song 1\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::RightClick(ClickTarget::Buff(true, 3)),
                Step::RightClick(ClickTarget::Buff(false, 0))
            ]
        );
        assert!(parse("right_click buff 0\n", Path::new("private")).is_err());
    }

    #[test]
    fn parses_the_skins_effects_window_buttons() {
        assert_eq!(
            parse(
                "hover buff 1
hover song 2
",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Hover(ClickTarget::Buff(true, 0)),
                Step::Hover(ClickTarget::Buff(false, 1)),
            ]
        );
        assert!(parse("hover buff 0\n", Path::new("private")).is_err());
    }

    #[test]
    fn parses_the_skins_merchant_window_clicks() {
        assert_eq!(
            parse(
                "click merchant_row 3
click buy_chosen
click sell_chosen
",
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
                "click done bank\nhover done world-container\n",
                Path::new("private")
            )
            .unwrap(),
            [
                Step::Click(ClickTarget::Done("bank")),
                Step::Hover(ClickTarget::Done("world-container")),
            ]
        );
        assert!(parse("click done nowhere\n", Path::new("private")).is_err());
    }

    #[test]
    fn parses_a_skin_button_by_its_screen_id() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click button bank Fake_Button1\nright_click button bank Fake_Button1\n\
                 hover button merchant FakeButton\nhold_click 500 button bank FakeButton\n",
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
            "click button bank",
            "click button nowhere FakeButton",
            "click button bank Fake-Button",
            "click button bank Fake Button",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
        let long = format!("click button bank {}", "A".repeat(65));
        assert!(parse(&long, base).is_err());
    }

    #[test]
    fn parses_the_quantity_windows_slider_and_number_box() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click slider quantity 50
click slider max_fps 10
click slider run_boost 50
click slider run_boost 100
click pick amount
press backspace
",
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
            "click slider quantity 101",
            "click slider amount 5",
            "click pick all",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }
}
