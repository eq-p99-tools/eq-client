//! The script language: one bounded step per line, `#` starts a comment.
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::prelude::KeyCode;

/// Most steps one script may queue, counting steps appended while following.
pub(super) const MAX_STEPS: usize = 500;
const MAX_HOLD: Duration = Duration::from_secs(10);
/// Longest wait, also the longest a scripted click waits for the pointer to leave.
pub(super) const MAX_WAIT: Duration = Duration::from_mins(2);
const MAX_TRACE: Duration = Duration::from_secs(10);
const MAX_WALK: Duration = Duration::from_mins(1);
/// `EQEmu` GM commands a script may send, without the leading `#`.
const GM_COMMANDS: [&str; 24] = [
    // GM mode on or off: off, the server lets the player go hungry.
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
    "npcloot",
    "castspell",
    "damage",
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
    /// Creates a character from the selection screen (local test servers).
    Create(eq_client_core::creation::NewCharacter),
    /// Waits until the zone is admitted with a player present.
    WaitOnline,
    /// Waits until the named zone is admitted with a player present.
    WaitZone(String),
    /// Runs a game slash command such as `/camp`; chat text is refused.
    Slash(String),
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
    /// Turns the camera so W walks toward the current target.
    Face,
    /// Holds W toward the current target until it is within a distance, for a
    /// bounded duration.
    Approach(f32, Duration),
    /// Walks a searched path around walls to within a distance of the current
    /// target, for a bounded duration.
    Walk(f32, Duration),
    /// Asks the current target to take the item on the cursor, as clicking
    /// it with the item does.
    Give,
    /// Left-clicks one UI control through Bevy's ordinary interaction state.
    Click(ClickTarget),
    /// Right-clicks one UI control the same way, as a bag is opened or an
    /// item inspected.
    RightClick(ClickTarget),
    /// Rests the pointer on one UI control, found as a click finds it, so
    /// its tooltip shows; it stays hovered until the next click or hover.
    Hover(ClickTarget),
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
        ("login", []) => Step::Login,
        ("wait_servers", []) => Step::WaitServers,
        ("server", name) if !name.is_empty() => Step::Server(name.join(" ")),
        ("wait_select", []) => Step::WaitSelect,
        ("select", [name]) => Step::Select((*name).to_owned()),
        ("create", arguments) => parse_create(arguments)?,
        ("wait_online", []) => Step::WaitOnline,
        ("wait_zone", [zone]) => Step::WaitZone(zone.to_ascii_lowercase()),
        ("slash", words) => parse_slash(words)?,
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
        ("face", []) => Step::Face,
        ("approach", [range, duration]) => {
            Step::Approach(number(range, 1.0, 200.0)?, millis(duration, MAX_HOLD)?)
        }
        ("walk", [range, duration]) => {
            Step::Walk(number(range, 1.0, 200.0)?, millis(duration, MAX_WALK)?)
        }
        ("give", []) => Step::Give,
        ("click", target) => Step::Click(parse_click(target)?),
        ("right_click", target) => Step::RightClick(parse_click(target)?),
        ("hover", target) => Step::Hover(parse_click(target)?),
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

/// `slash <command> [words]`: a slash command a script may type, among
/// those that change nothing beyond the test characters.
fn parse_slash(words: &[&str]) -> Result<Step, String> {
    Ok(match words {
        [command] if matches!(*command, "camp" | "sit" | "stand") => {
            Step::Slash(format!("/{command}"))
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

/// `gm <command> [arguments]`: an allowed `EQEmu` command without its `#`, with up
/// to four plain arguments.
fn parse_gm(words: &[&str]) -> Result<Step, String> {
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
    if arguments.len() > 4 || !arguments.iter().all(plain) {
        return Err("gm takes up to four plain arguments".into());
    }
    if *command == "rules" && !matches!(arguments, ["set", _, _] | ["reload"]) {
        return Err("gm rules takes set <Category:Rule> <value> or reload".into());
    }
    Ok(Step::Gm(words.join(" ")))
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

/// `create <Name> <race> <class> <gender> <deity> <start zone> <stat for free points>`.
fn parse_create(arguments: &[&str]) -> Result<Step, String> {
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
    Ok(Step::Create(character))
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
        "BACKSPACE" => Some(KeyCode::Backspace),
        "DELETE" => Some(KeyCode::Delete),
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

#[cfg(test)]
mod tests {
    use super::*;

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
            "wait_select\nselect Someone\ncreate Testcleric 1 2 0 212 1 4\nwait_online\nwait_zone TOX\nslash camp\nslash target a cave rat\n\
             gm summon\ngm damage 10000\ngm givemoney 0 0 5 0\npress F1 # self\n\
             press alt+1\nhold W 1500\nwait 250\ncamera 128 -20\ncamera player 256 -15\ntrace 2000\nface\napproach 12 5000\nwalk 8 30000\nclick slot 23\nright_click slot 22\nhover close_box chat\n\
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
        for bad in ["drag", "drag chat 1", "drag nowhere 1 2", "drag chat -5 2"] {
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
    fn parses_the_quantity_windows_slider_and_number_box() {
        let base = Path::new("private");
        assert_eq!(
            parse(
                "click slider quantity 50
click slider max_fps 10
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
