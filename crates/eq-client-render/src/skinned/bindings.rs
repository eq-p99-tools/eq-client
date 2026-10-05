//! Feature-specific skin identifiers mapped to typed actions.
//! No entity construction, GPU assets, input polling or wire sending belongs here.
use crate::windows::WindowId;
use eq_client_core::{
    buffs::EffectWindow,
    inventory::InventorySlot,
    money::{Coin, CoinPlace},
};

/// A group button, in the group window or on the Actions window's Main page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GroupButton {
    Invite,
    Follow,
    Disband,
    Decline,
}

impl GroupButton {
    /// The slash command it runs: Invite invites the target, and Decline
    /// disbands, which with an invitation waiting declines it, as the
    /// installed string for an invitation says.
    pub(super) const fn command(self) -> &'static str {
        match self {
            Self::Invite => "/invite",
            Self::Follow => "/follow",
            Self::Disband | Self::Decline => "/disband",
        }
    }

    /// Whether it answers an invitation, and so shows in the group window
    /// only while one waits.
    pub(super) const fn answers(self) -> bool {
        matches!(self, Self::Follow | Self::Decline)
    }
}

/// A button of the Raid window that runs a raid slash command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RaidButton {
    Invite,
    Accept,
    Decline,
}

impl RaidButton {
    /// The slash command it runs: Invite invites the target, as
    /// `/raidinvite` with no name does.
    pub(super) const fn command(self) -> &'static str {
        match self {
            Self::Invite => "/raidinvite",
            Self::Accept => "/raidaccept",
            Self::Decline => "/raiddecline",
        }
    }

    /// Whether it answers an invitation, and so shows only while one waits.
    pub(super) const fn answers(self) -> bool {
        matches!(self, Self::Accept | Self::Decline)
    }
}

/// What a skin's button does in the client.
#[derive(Clone, Copy)]
pub(super) enum Does {
    /// Opens and closes a window.
    Toggles(WindowId),
    /// Closes its own window.
    Closes,
    /// Hands what the give window holds over.
    Gives,
    /// Holds coins of one kind in a place, which a click picks up or puts
    /// down.
    Coins(CoinPlace, Coin),
    /// Shows the coins of one kind the other player put in the trade.
    Offered(Coin),
    /// Shows a bag's picture.
    BagIcon,
    /// Turns melee auto-attack on or off.
    Attack,
    /// Uses the ability it holds.
    Ability(crate::abilities::AbilityButton),
    /// Runs a game slash command, as the Actions window's sit does.
    Slash(&'static str),
    /// Invites, follows, disbands or declines, as the group window's and the
    /// Actions window's group buttons do.
    Group(GroupButton),
    /// Invites, accepts or declines, as the Raid window's buttons do.
    Raid(RaidButton),
    /// Does a Raid window button's work for the member chosen in it.
    RaidAction(crate::raid::RaidAction),
    /// Something no server type offers yet, which the client has no command
    /// for: drawn under the veil of what the session does not offer.
    Unoffered(eq_client_core::Capability),
    /// Shows the pet's buff in this slot.
    PetBuff(usize),
    /// Shows the player's buff on this button of an effects window.
    Buff(EffectWindow, u32),
    /// Turns an option on or off, and shows which.
    Option(eq_client_core::options::Toggle),
    /// Practices the skill chosen in the Training window.
    Trains,
    /// Answers the confirmation dialog's question: Yes (true) or No.
    Answers(bool),
    /// Turns a book's pages forward (true) or back.
    TurnsPage(bool),
    /// Combines what the tradeskill container in this pack slot holds.
    Combines(InventorySlot),
    /// Zooms, pans or toggles the map.
    Maps(crate::map::MapButton),
    /// Uses the action bound to this slot of the action bar, and shows it.
    HotButton(usize),
    /// Does this to the quantity window's amount, as Accept takes it.
    Picks(crate::inventory::SplitAction),
    /// Does this to the loot open on a corpse or the merchant open, as the
    /// loot window's Done ends the loot and the merchant window's Buy buys.
    Trades(crate::trade::Action),
    /// Shows the item chosen in the merchant window.
    Chosen,
    /// Shows the picture of the item the item display shows.
    ItemIcon,
    /// Shows the spell in this place on the spellbook's open pages, which a
    /// click picks up or scribes into.
    BookPlace(u8),
    /// Turns the spellbook's pages forward (true) or back.
    TurnsSpellbook(bool),
    /// Shows the character in this slot of the character list, which a
    /// click chooses.
    CharacterSlot(u8),
    /// Enters the world with the chosen character.
    EntersWorld,
    /// Leaves the game, from the character list.
    Quits,
    /// Nothing yet: drawn greyed out, as the client's own windows show what
    /// it or the server lacks.
    Nothing,
}

/// What a loot or merchant window's button does: the loot window's Done
/// ends the loot, and the merchant window's Buy and Sell act on what it has
/// chosen. Their others are not in this client: Link all, and the Loot all
/// some skins keep without a place.
pub(super) fn trading_button(id: &str, owner: WindowId) -> Option<Does> {
    use crate::trade::Action;
    Some(match (owner, id) {
        (WindowId::Loot, "DoneButton") => Does::Trades(Action::EndLoot),
        (WindowId::Merchant, "MW_Buy_Button") => Does::Trades(Action::BuyChosen),
        (WindowId::Merchant, "MW_Sell_Button") => Does::Trades(Action::SellChosen),
        (WindowId::Merchant, "MW_Done_Button" | "DoneButton") => Does::Trades(Action::EndShop),
        (WindowId::Merchant, "MW_SelectedItem") => Does::Chosen,
        (WindowId::Loot | WindowId::Merchant, _) => Does::Nothing,
        _ => return None,
    })
}

/// What the client does with a skin's button; None for one it leaves out.
pub(super) fn does(id: &str, owner: WindowId) -> Option<Does> {
    if let Some((place, coin)) = coin_box(id) {
        return Some(Does::Coins(place, coin));
    }
    if let Some(coin) = their_coin_box(id) {
        return Some(Does::Offered(coin));
    }
    if let Some(button) = ability_button(id) {
        return Some(Does::Ability(button));
    }
    if let Some(does) = trading_button(id, owner) {
        return Some(does);
    }
    if owner == WindowId::Item {
        return Some(if id == "IconButton" {
            Does::ItemIcon
        } else {
            Does::Nothing
        });
    }
    // A window's Done button, or the give or trade window's Cancel, closes
    // it; even in a window whose other buttons do nothing yet.
    if matches!(
        id,
        "DoneButton" | "GVW_Cancel_Button" | "TRDW_Cancel_Button"
    ) {
        return Some(Does::Closes);
    }
    if has_own_buttons(owner) {
        return own_button(id, owner);
    }
    // An effects window's buttons, `Buff0` on, are its slots in order.
    if let Some(window) = effect_window(owner) {
        return Some(
            id.strip_prefix("Buff")
                .and_then(|button| button.parse().ok())
                .map_or(Does::Nothing, |button| Does::Buff(window, button)),
        );
    }
    if owner == WindowId::Training && id == "TrainButton" {
        return Some(Does::Trains);
    }
    if owner == WindowId::Map {
        return Some(crate::map::MapButton::for_screen(id).map_or(Does::Nothing, Does::Maps));
    }
    // The action bar's ten buttons; its pages wait for a second page.
    if owner == WindowId::Actions {
        return Some(hot_button(id).map_or(Does::Nothing, Does::HotButton));
    }
    if owner == WindowId::Book {
        match id {
            "LeftButton" => return Some(Does::TurnsPage(false)),
            "RightButton" => return Some(Does::TurnsPage(true)),
            _ => (),
        }
    }
    if owner == WindowId::Quantity {
        return Some(match id {
            "QTYW_Accept_Button" => Does::Picks(crate::inventory::SplitAction::Confirm),
            _ => Does::Nothing,
        });
    }
    // The dialog asks only Yes-or-No questions so far; its OK stays hidden.
    if owner == WindowId::Confirmation {
        return match id {
            "Yes_Button" => Some(Does::Answers(true)),
            "No_Button" => Some(Does::Answers(false)),
            _ => None,
        };
    }
    // The skin keeps Switch to Windowed under Switch to Fullscreen; the
    // client runs in a window.
    if owner == WindowId::Options && id == "ODP_SetWindowedButton" {
        return None;
    }
    if owner == WindowId::Options {
        return Some(option_checkbox(id).map_or(Does::Nothing, Does::Option));
    }
    Some(match id {
        "CSPW_SpellBook" => Does::Toggles(WindowId::Spellbook),
        "IW_Skills" => Does::Toggles(WindowId::Skills),
        "GVW_Give_Button" | "TRDW_Trade_Button" => Does::Gives,
        "ACP_MeleeAttackButton" => Does::Attack,
        "AMP_SitButton" => Does::Slash("/sit"),
        "AMP_StandButton" => Does::Slash("/stand"),
        "AMP_CampButton" => Does::Slash("/camp"),
        "AMP_InviteButton" => Does::Group(GroupButton::Invite),
        "AMP_FollowButton" => Does::Group(GroupButton::Follow),
        "AMP_DisbandButton" => Does::Group(GroupButton::Disband),
        "Container_Icon" if matches!(owner, WindowId::Bag(_) | WindowId::WorldContainer) => {
            Does::BagIcon
        }
        // Shown only while the bag is a tradeskill container; see
        // `tradeskills::show`.
        "Container_Combine" => match owner {
            WindowId::Bag(bag) => Does::Combines(InventorySlot(bag)),
            WindowId::WorldContainer => {
                Does::Combines(eq_client_core::tradeskills::WORLD_CONTAINER)
            }
            _ => return None,
        },
        _ => Does::Nothing,
    })
}

/// What a button of the skin's spellbook does: its places show the open
/// pages' spells and its arrows turn the pages. Its Mem. This Page and
/// Meditate buttons, which the skin parks at a pixel's size, are left out.
pub(super) fn spellbook_button(id: &str) -> Option<Does> {
    Some(match id {
        "SBW_PageUp_Button" => Does::TurnsSpellbook(true),
        "SBW_PageDown_Button" => Does::TurnsSpellbook(false),
        "SBW_MemPage0_Button" | "SBW_MemPage1_Button" | "MeditateButton" => return None,
        _ => id
            .strip_prefix("SBW_Spell")
            .and_then(|place| place.parse::<u8>().ok())
            .filter(|place| usize::from(*place) < crate::spellbook::PLACES)
            .map_or(Does::Nothing, Does::BookPlace),
    })
}

/// Whether a window's buttons are its own (`own_button`): the pet window's,
/// the group and Raid windows', the spellbook's, the character list's and
/// the selector's.
pub(super) const fn has_own_buttons(owner: WindowId) -> bool {
    matches!(
        owner,
        WindowId::PetInfo
            | WindowId::Group
            | WindowId::Raid
            | WindowId::Spellbook
            | WindowId::CharacterSelect
            | WindowId::Selector
    )
}

/// What a button does in a window whose buttons are its own; None for one
/// the client leaves out.
pub(super) fn own_button(id: &str, owner: WindowId) -> Option<Does> {
    match owner {
        WindowId::PetInfo => Some(pet_button(id)),
        WindowId::Group => group_button(id),
        WindowId::Raid => Some(raid_button(id)),
        WindowId::Spellbook => spellbook_button(id),
        WindowId::CharacterSelect => Some(character_button(id)),
        WindowId::Selector => Some(selector_button(id).map_or(Does::Nothing, Does::Toggles)),
        _ => None,
    }
}

/// What a button of the skin's character list does: its eight character
/// buttons show the slots of the server's list in order, Enter World enters
/// with the chosen one and Quit leaves the game. Creating, deleting and
/// rotating characters, the tutorial, exploring and returning home are not
/// in this client yet.
pub(super) fn character_button(id: &str) -> Does {
    match id {
        "Enter_World_Button" => Does::EntersWorld,
        "Quit_Button" => Does::Quits,
        _ => id
            .strip_prefix("Char")
            .and_then(|rest| rest.strip_suffix("_Button"))
            .and_then(|number| number.parse::<u8>().ok())
            .filter(|number| (1..=8).contains(number))
            .map_or(Does::Nothing, |number| Does::CharacterSlot(number - 1)),
    }
}

/// The action bar slot a Hot Button window button holds, from
/// `HB_Button1` to `HB_Button10`.
pub(super) fn hot_button(id: &str) -> Option<usize> {
    id.strip_prefix("HB_Button")?
        .parse::<usize>()
        .ok()
        .filter(|number| (1..=10).contains(number))
        .map(|number| number - 1)
}

/// The option an Options window checkbox turns on and off: one of the
/// official client's options this client keeps, or a quality-of-life
/// setting.
pub(super) fn option_checkbox(id: &str) -> Option<eq_client_core::options::Toggle> {
    qol_checkbox(id)
        .map(eq_client_core::options::Toggle::Qol)
        .or_else(|| {
            OPTION_CHECKBOXES
                .iter()
                .find(|(checkbox, _)| *checkbox == id)
                .map(|(_, toggle)| *toggle)
        })
}

/// The screen ID of a quality-of-life setting's checkbox.
pub(super) fn qol_checkbox_id(fix: eq_client_core::qol::Fix) -> String {
    format!("{QOL_CHECKBOX}{}", fix.key())
}

/// The quality-of-life setting a checkbox turns on and off.
pub(super) fn qol_checkbox(id: &str) -> Option<eq_client_core::qol::Fix> {
    eq_client_core::qol::Fix::from_key(id.strip_prefix(QOL_CHECKBOX)?)
}

/// The window an official selector button opens and closes, or hides and
/// shows again; those for windows this client does not have do nothing.
pub(super) fn selector_button(id: &str) -> Option<WindowId> {
    Some(match id {
        "SELW_ActionsToggleButton" => WindowId::ActionsWindow,
        "SELW_InventoryToggleButton" => WindowId::Inventory,
        "SELW_OptionsToggleButton" => WindowId::Options,
        "SELW_BuffToggleButton" => WindowId::Effects,
        "SELW_MapToggleButton" => WindowId::Map,
        // Windows something else opens, which these hide and show again.
        "SELW_HotboxToggleButton" => WindowId::Actions,
        "SELW_CastSpellToggleButton" => WindowId::Spells,
        "SELW_PetInfoToggleButton" => WindowId::PetInfo,
        "SELW_SDBuffToggleButton" => WindowId::ShortEffects,
        _ => return None,
    })
}

/// The effects window a window is, if it is one.
pub(super) const fn effect_window(id: WindowId) -> Option<EffectWindow> {
    match id {
        WindowId::Effects => Some(EffectWindow::Long),
        WindowId::ShortEffects => Some(EffectWindow::Short),
        _ => None,
    }
}

/// The buff whose name a label of an effects window shows, by the label's
/// number (`EQType`): 500 on for the long window's buttons and 600 on for
/// the short one's, as the skins that name their buffs number them. Other
/// windows' labels keep their own numbering.
pub(super) const fn effect_label(kind: u32) -> Option<(EffectWindow, u32)> {
    match kind {
        500..=599 => Some((EffectWindow::Long, kind - 500)),
        600..=699 => Some((EffectWindow::Short, kind - 600)),
        _ => None,
    }
}

/// The buff a label names, if its window is an effects window.
pub(super) fn buff_label(owner: WindowId, eq_type: Option<u32>) -> Option<(EffectWindow, u32)> {
    effect_window(owner).and(eq_type).and_then(effect_label)
}

/// Whether a pet command's button shows only while the pet sits (Stand) or
/// while it does not (Sit); the skin keeps the two in one place.
pub(super) fn pet_posture_button(command: &str) -> Option<bool> {
    match command {
        "/pet stand up" => Some(true),
        "/pet sit down" => Some(false),
        _ => None,
    }
}

/// The Pet Info window's buttons: each command as `/pet` gives it, and the
/// pet's buff slots.
pub(super) fn pet_button(id: &str) -> Does {
    if let Some(slot) = id
        .strip_prefix("PetBuff")
        .and_then(|slot| slot.parse().ok())
    {
        return Does::PetBuff(slot);
    }
    PET_COMMANDS
        .iter()
        .find(|(button, _)| *button == id)
        .map_or(Does::Nothing, |(_, command)| Does::Slash(command))
}

/// The group window's buttons. The skin parks Looking For Group at a
/// pixel's size.
pub(super) fn group_button(id: &str) -> Option<Does> {
    Some(Does::Group(match id {
        "InviteButton" => GroupButton::Invite,
        "FollowButton" => GroupButton::Follow,
        "DisbandButton" => GroupButton::Disband,
        "DeclineButton" => GroupButton::Decline,
        "LFGButton" => return None,
        _ => return Some(Does::Nothing),
    }))
}

/// The Raid window's buttons: Invite, Accept and Decline run the raid slash
/// commands; Disband, Lock and Unlock, the twelve group buttons, No Group
/// and Make Leader act for the member chosen. Taking a group leader's mark
/// is offered by no server type, and the looting, options, assist, mark,
/// find and dump buttons do nothing yet.
pub(super) fn raid_button(id: &str) -> Does {
    use crate::raid::RaidAction;
    match id {
        "RAID_InviteButton" => Does::Raid(RaidButton::Invite),
        "RAID_AcceptButton" => Does::Raid(RaidButton::Accept),
        "RAID_DeclineButton" => Does::Raid(RaidButton::Decline),
        "RAID_DisbandButton" => Does::RaidAction(RaidAction::Disband),
        "RAID_LockButton" => Does::RaidAction(RaidAction::Lock(true)),
        "RAID_UnlockButton" => Does::RaidAction(RaidAction::Lock(false)),
        "RAID_NoGroupButton" => Does::RaidAction(RaidAction::Move(None)),
        "RAID_MakeLeaderButton" => Does::RaidAction(RaidAction::MakeLeader),
        "RAID_RemoveLeaderButton" => Does::Unoffered(eq_client_core::Capability::RaidGroupLeaders),
        _ => raid_group(id).map_or(Does::Nothing, |group| {
            Does::RaidAction(RaidAction::Move(Some(group)))
        }),
    }
}

/// The raid group a Raid window group button moves into, from 0: its
/// `RAID_Group1Button` to `RAID_Group12Button`.
pub(super) fn raid_group(id: &str) -> Option<u8> {
    id.strip_prefix("RAID_Group")?
        .strip_suffix("Button")?
        .parse::<u8>()
        .ok()
        .filter(|number| (1..=12).contains(number))
        .map(|number| number - 1)
}

/// The Actions window's ability buttons: the Combat page's first to fourth
/// and the Abilities page's first to sixth.
pub(super) fn ability_button(id: &str) -> Option<crate::abilities::AbilityButton> {
    use crate::abilities::{AbilityButton, Page};
    const PLACES: [&str; 6] = ["First", "Second", "Third", "Fourth", "Fifth", "Sixth"];
    let (page, rest) = match id.strip_prefix("ACP_") {
        Some(rest) => (Page::Combat, rest),
        None => (Page::Abilities, id.strip_prefix("AAP_")?),
    };
    let place = rest.strip_suffix("AbilityButton")?;
    let index = PLACES.iter().position(|name| *name == place)?;
    Some(AbilityButton { page, index })
}

/// The place and kind a skin's coin box holds, by its name: the purse's
/// (`IW_Money0` platinum to `IW_Money3` copper), the bank's and the give
/// window's.
pub(super) fn coin_box(id: &str) -> Option<(CoinPlace, Coin)> {
    let (place, index) = [
        ("IW_Money", CoinPlace::Purse),
        ("BW_Money", CoinPlace::Bank),
        ("GVW_MyMoney", CoinPlace::Trade),
        ("TRDW_MyMoney", CoinPlace::Trade),
    ]
    .into_iter()
    .find_map(|(prefix, place)| Some((place, id.strip_prefix(prefix)?)))?;
    let coin = *Coin::ALL.get(index.parse::<usize>().ok()?)?;
    Some((place, coin))
}

/// The coin a trade window's box for the other player's coins shows,
/// numbered platinum to copper as the player's own.
pub(super) fn their_coin_box(id: &str) -> Option<Coin> {
    let index = id.strip_prefix("TRDW_HisMoney")?.parse::<usize>().ok()?;
    Coin::ALL.get(index).copied()
}

/// The skin's Options window checkboxes for the official client's options
/// this client keeps, by screen ID.
const OPTION_CHECKBOXES: [(&str, eq_client_core::options::Toggle); 7] = {
    use eq_client_core::options::Toggle;
    [
        ("OGP_PetWindowPopupCheckbox", Toggle::PetWindowPopup),
        ("ODP_ShowTargetRingCheckbox", Toggle::TargetRing),
        ("ODP_ShowHelmCheckbox", Toggle::ShowHelm),
        ("ODP_PCNamesCheckbox", Toggle::PcNames),
        ("ODP_NPCNamesCheckbox", Toggle::NpcNames),
        ("OMP_InvertYAxisCheckbox", Toggle::InvertY),
        ("OMP_MouseWheelZoomCheckbox", Toggle::WheelZoom),
    ]
};

/// How the screen IDs of the quality-of-life page's checkboxes begin; each
/// ends with its fix's file name.
const QOL_CHECKBOX: &str = "EQC_QoL_";

/// The Pet Info window's command buttons, by screen ID, with the `/pet`
/// line each gives.
pub(crate) const PET_COMMANDS: [(&str, &str); 8] = [
    ("AttackButton", "/pet attack"),
    ("FollowButton", "/pet follow"),
    ("TauntButton", "/pet taunt"),
    ("GuardButton", "/pet guard here"),
    ("SitButton", "/pet sit down"),
    ("StandButton", "/pet stand up"),
    ("BackButton", "/pet back off"),
    ("LostButton", "/pet get lost"),
];
