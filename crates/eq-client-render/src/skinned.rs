//! Windows drawn from the installed skin's definitions: the player, target
//! and spell windows, and the windows that hold items. Each keeps its place
//! in the window registry and its behaviour; the skin decides its size,
//! frame and pieces, and what each gauge and label shows follows the
//! official client's numbering. A skin without the window, or a viewer
//! without an installation, keeps the client's own chrome.
mod tree;
#[cfg(test)]
use tree::{augment_section, text_box};
use tree::{draw, tab_strip};

mod presentation;
pub(crate) use presentation::show;
#[cfg(test)]
use presentation::{fraction, gauge_text, label_text, raid_text, shown_now};

mod bindings;
pub(crate) use bindings::PET_COMMANDS;
use bindings::{Does, buff_label, does, pet_posture_button, qol_checkbox_id};
#[cfg(test)]
use bindings::{GroupButton, RaidButton, effect_label, selector_button};
mod controls;
mod fit;
mod frame;
mod items;
pub(crate) mod looks;
pub(crate) mod scrollbar;

pub(crate) use controls::{
    AmountBox, Choosing, DropDown, DropDownChoice, KeyFilter, Sets, SkinSlider, drop_downs,
    fill_lists, headings, light_choices, scroll_lists, show_amount, show_choices, show_sliders,
    slide, type_amount,
};

pub(crate) use items::{
    Closes, TheirSlot, close, contents, frames, loot, quantity, theirs, toggle_bag,
};

use super::{hud::hotbar, windows::WindowId};
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{Align, Anchor, Anchors, ButtonLook, Element, Gauge, Label, Library, Piece, Screen},
    ui::Area,
};
#[cfg(test)]
use eq_client_core::hotbar::Binding;
use eq_client_core::{
    buffs::EffectWindow,
    inventory::InventorySlot,
    money::{Coin, CoinPlace},
};
use std::collections::HashMap;

/// The skin's file for a window the client draws from the skin, and the
/// window's name in it.
fn source(id: WindowId) -> Option<(&'static str, &'static str)> {
    let file = match id {
        WindowId::Player => "EQUI_PlayerWindow.xml",
        WindowId::Target => "EQUI_TargetWindow.xml",
        WindowId::Spells => "EQUI_CastSpellWnd.xml",
        WindowId::Inventory => "EQUI_Inventory.xml",
        WindowId::Bank => "EQUI_BankWnd.xml",
        WindowId::Bag(_) | WindowId::WorldContainer => "EQUI_Container.xml",
        WindowId::Give => "EQUI_GiveWnd.xml",
        WindowId::Trade => "EQUI_TradeWnd.xml",
        WindowId::Loot => "EQUI_LootWnd.xml",
        WindowId::Merchant => "EQUI_MerchantWnd.xml",
        WindowId::Item => "EQUI_ItemDisplay.xml",
        WindowId::Quantity => "EQUI_QuantityWnd.xml",
        WindowId::Spellbook => "EQUI_SpellBookWnd.xml",
        WindowId::CharacterSelect => "EQUI_CharacterSelect.xml",
        WindowId::ActionsWindow => "EQUI_ActionsWindow.xml",
        WindowId::PetInfo => "EQUI_PetInfoWindow.xml",
        WindowId::Group => "EQUI_GroupWindow.xml",
        WindowId::Raid => "EQUI_RaidWindow.xml",
        WindowId::Selector => "EQUI_SelectorWnd.xml",
        WindowId::CastBar => "EQUI_CastingWindow.xml",
        WindowId::Effects => "EQUI_BuffWindow.xml",
        WindowId::ShortEffects => "EQUI_ShortDurationBuffWindow.xml",
        WindowId::Options => "EQUI_OptionsWindow.xml",
        WindowId::Training => "EQUI_TrainWindow.xml",
        WindowId::Skills => "EQUI_SkillsWindow.xml",
        WindowId::Confirmation => "EQUI_ConfirmationDialog.xml",
        WindowId::Note => "EQUI_NoteWindow.xml",
        WindowId::Book => "EQUI_BookWindow.xml",
        WindowId::Map => "EQUI_MapViewWnd.xml",
        WindowId::Actions => "EQUI_HotButtonWnd.xml",
        WindowId::Chat => "EQUI_ChatWindow.xml",
        WindowId::Status => return None,
    };
    Some((file, id.official()?))
}

/// What a piece of a skinned window shows, in the official client's
/// numbering (`EQType`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shows {
    /// A gauge's fill, as wide as its fraction.
    Fill(u32),
    /// A gauge's text.
    GaugeText(u32),
    /// A label's text.
    Label(u32),
    /// The box that shows the player is attacking.
    Attacking,
    /// The coins of one kind in a place: the purse, the bank or the give
    /// or trade window.
    Coins(CoinPlace, Coin),
    /// The coins of one kind the other player put in the trade.
    Offered(Coin),
    /// The banker the bank is open at.
    Banker,
    /// The character the give or trade window is with, lit once they click
    /// Trade.
    Partner,
    /// The player in the trade window, lit once they click Trade.
    Trader,
    /// The pet window's Sit button (false) or its Stand button (true): the
    /// skin keeps one under the other, and the one that does something
    /// shows.
    WhilePetSits(bool),
    /// A pet buff slot, shown only while it holds a buff: the skin stacks
    /// slots 15 to 29 over slots 0 to 14, so an empty slot would hide the
    /// buff under it.
    PetBuff(usize),
    /// A button of an effects window, shown only while its slot holds a
    /// buff, as the official client leaves an empty slot blank.
    Buff(EffectWindow, u32),
    /// The name of the buff on this button of an effects window.
    BuffName(EffectWindow, u32),
    /// The name of the spell memorized in this gem, counted from 0.
    GemName(u8),
    /// The player's practice points, which the Training window counts.
    PracticePoints,
    /// The name of the corpse the loot window is open on.
    Corpse,
    /// The name of the merchant the merchant window is open at.
    Merchant,
    /// The name of the item chosen in the merchant window.
    ChosenName,
    /// The price the merchant asks for the ware chosen in its window.
    ChosenPrice,
    /// The merchant window's Sell button (true) or its Buy button: the skin
    /// keeps one over the other, and the one for what is chosen shows.
    WhileSelling(bool),
    /// The name of the spell in this place on the spellbook's open pages.
    BookName(u8),
    /// The number of the spellbook's right page (true) or its left.
    BookPage(bool),
    /// The player's pet's gauge, shown only while they have a pet in view,
    /// as a group member's pet's is (inferred): the skins lay it over the
    /// player's own gauges.
    Pet,
    /// A group member's gauge, shown only while their place in the group
    /// window, counted from 0, holds a member.
    Member(usize),
    /// A group member's pet's gauge, shown only while the member in this
    /// place has a pet in view.
    MemberPet(usize),
    /// The sign after a group member's health, said only while their health
    /// shows.
    MemberPercent(usize),
    /// The group window's Follow and Decline buttons (true) or its Invite
    /// and Disband: the skin keeps each pair in the other's places, and the
    /// one for whether an invitation waits shows.
    WhileInvited(bool),
    /// The Raid window's Accept and Decline buttons (true) or its Invite and
    /// Disband, kept in the same places: the one for whether a raid
    /// invitation waits shows.
    WhileRaidInvited(bool),
    /// The Raid window's Unlock button (true) or its Lock, kept in one
    /// place: the one for whether the raid is locked shows.
    WhileRaidLocked(bool),
    /// How many are in the player's raid.
    RaidCount,
    /// The average level in the player's raid.
    RaidLevel,
}

/// What a skinned window is drawn for: the window, and the paperdoll's
/// picture for the inventory's figure.
#[derive(Clone, Copy)]
struct Context<'a> {
    id: WindowId,
    paperdoll: Option<&'a super::paperdoll::PaperdollImage>,
    /// How many tab boxes the pieces lie in: a tab box on a page has its
    /// own tabs and its own page shown.
    depth: u8,
    /// The skin's frame for tabs the client draws that the window's file
    /// defines none of, as the chat's.
    tab_frame: Option<&'a eq_client_assets::sidl::FrameLook>,
}

/// The skin's windows as read, by skin and screen: read once, whatever
/// rebuilds the frames they are drawn in.
#[derive(Resource, Default)]
pub(crate) struct Screens {
    libraries: HashMap<String, Option<Library>>,
    screens: HashMap<(String, &'static str), Option<Screen>>,
}

impl Screens {
    /// A window as this skin defines it, if it does.
    fn get(&mut self, directory: &std::path::Path, skin: &str, id: WindowId) -> Option<&Screen> {
        let (file, name) = source(id)?;
        self.screen(directory, skin, (file, name))
    }

    /// A screen of this skin's file, if the file defines it: a window's, or
    /// one that is no window, as the cursor's attachment.
    fn screen(
        &mut self,
        directory: &std::path::Path,
        skin: &str,
        (file, name): (&str, &'static str),
    ) -> Option<&Screen> {
        let library = Self::library(&mut self.libraries, directory, skin);
        self.screens
            .entry((skin.to_owned(), name))
            .or_insert_with(|| {
                library?
                    .window(directory, skin, file, name)
                    .inspect_err(|error| warn!("Skin {skin} draws no {name}: {error}"))
                    .ok()
            })
            .as_ref()
    }

    /// One of the skin's pictures by its animation's name.
    fn piece(&mut self, directory: &std::path::Path, skin: &str, name: &str) -> Option<Piece> {
        Self::library(&mut self.libraries, directory, skin)?.named_piece(name)
    }

    /// The skin's animations and templates, read once.
    fn library<'a>(
        libraries: &'a mut HashMap<String, Option<Library>>,
        directory: &std::path::Path,
        skin: &str,
    ) -> Option<&'a Library> {
        libraries
            .entry(skin.to_owned())
            .or_insert_with(|| {
                Library::read(directory, skin)
                    .inspect_err(|error| warn!("UI skin {skin} unreadable: {error}"))
                    .ok()
            })
            .as_ref()
    }
}

/// The skin's file and screen for what rides the cursor.
const CURSOR_ATTACHMENT: (&str, &str) = ("EQUI_CursorAttachment.xml", "CursorAttachment");

/// What the skin draws for what rides the cursor (`EQUI_CursorAttachment.xml`);
/// none where it draws none, as without an installation, so the client's own
/// shows.
#[derive(Resource, Default)]
pub(crate) struct CursorLook(pub(crate) Option<CursorPlace>);

/// The skin's cursor attachment: a box that follows the pointer, where the
/// item's picture sits in it, and the skin's pictures of coins.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CursorPlace {
    /// The box's size.
    pub(crate) size: Vec2,
    /// Where the picture sits in the box.
    pub(crate) icon: Area,
    /// Each kind of coin's picture, as the purse shows it: platinum, gold,
    /// silver and copper.
    pub(crate) coins: [Option<Piece>; 4],
}

impl CursorPlace {
    /// The skin's picture of a kind of coin.
    pub(crate) const fn coin(&self, coin: Coin) -> Option<&Piece> {
        self.coins[coin_index(coin)].as_ref()
    }
}

/// A kind of coin's place in the order of their pictures' names.
const fn coin_index(coin: Coin) -> usize {
    match coin {
        Coin::Platinum => 0,
        Coin::Gold => 1,
        Coin::Silver => 2,
        Coin::Copper => 3,
    }
}

/// The skin's names for its pictures of each kind of coin, in that order.
const COIN_PICTURES: [&str; 4] = [
    "A_PlatinumCoin",
    "A_GoldCoin",
    "A_SilverCoin",
    "A_CopperCoin",
];

/// The cursor attachment in a screen: its size and where its first picture
/// (`CA_Anim`) sits, which the official client fills with what rides the
/// cursor (inferred: the skin's sample there is a buff icon).
fn cursor_place(screen: &Screen, coins: [Option<Piece>; 4]) -> Option<CursorPlace> {
    let icon = screen
        .pieces
        .iter()
        .find_map(|(name, element)| match element {
            Element::Image { area, .. } if name == "CA_Anim" => Some(*area),
            _ => None,
        })?;
    Some(CursorPlace {
        size: Vec2::new(screen.area.width, screen.area.height),
        icon,
        coins,
    })
}

/// Reads the skin's cursor attachment again when the skin changes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn cursor_look(
    skin: Res<super::skin::UiSkin>,
    settings: Res<super::ViewerSettings>,
    mut screens: ResMut<Screens>,
    mut look: ResMut<CursorLook>,
) {
    if !skin.is_changed() {
        return;
    }
    let Some(directory) = settings.0.eq_directory.as_deref() else {
        return;
    };
    let coins = COIN_PICTURES.map(|name| screens.piece(directory, &skin.0, name));
    let place = screens
        .screen(directory, &skin.0, CURSOR_ATTACHMENT)
        .and_then(|screen| cursor_place(screen, coins));
    if look.0 != place {
        look.0 = place;
    }
}

/// The skin a frame is drawn in, and how many places the container it
/// shows has, where its window fits its container ([`fit`]).
#[derive(Component)]
pub(crate) struct Drawn(String, Option<u8>);

/// The windows drawn from the skin.
#[derive(Resource, Default)]
pub(crate) struct Skinned(std::collections::BTreeSet<WindowId>);

impl Skinned {
    /// Whether this window is drawn from the skin.
    pub(crate) fn has(&self, id: WindowId) -> bool {
        self.0.contains(&id)
    }

    /// These windows drawn from the skin, for a test.
    #[cfg(test)]
    pub(crate) fn of(ids: &[WindowId]) -> Self {
        Self(ids.iter().copied().collect())
    }
}

/// A skin's button by what its window's file calls it (`ScreenID`), as a
/// script names it.
#[derive(Component)]
pub(crate) struct SkinId {
    pub(crate) window: WindowId,
    pub(crate) id: String,
}

/// A button drawn from the skin, with its piece for each state.
#[derive(Component, Clone)]
pub(crate) struct SkinButton(pub(crate) ButtonLook);

/// A box on a skinned window's title bar: its close box, or its minimize box.
#[derive(Component, Clone, Copy)]
pub(crate) struct TitleBox {
    pub(crate) window: WindowId,
    pub(crate) close: bool,
}

/// A skin button that cannot be used right now, drawn in its disabled look,
/// as Combine is while a combine waits for the server.
#[derive(Component)]
pub(crate) struct Greyed;

/// What a skin's control this client does not have yet carries, over the
/// skin's disabled look: the same reason on hover in every window. It is a
/// button that does nothing, so a press on it stays there rather than
/// dragging its window.
fn missing() -> (crate::outbox::Needs, Button) {
    (crate::outbox::Needs::Missing, Button)
}

/// Window frames, with what the skin changes on them and the skin they are
/// drawn in.
type Frames<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static WindowId,
        &'static mut Node,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        Option<&'static Drawn>,
        &'static mut super::windows::Frame,
    ),
>;

/// The skin a window is drawn from: the player's, or the default skin where
/// the player's sizes the window to nothing and the player asks to see the
/// windows their skin hides.
fn drawn_from(
    screens: &mut Screens,
    directory: &std::path::Path,
    (skin, id): (&str, WindowId),
    hidden_too: bool,
) -> String {
    let default = eq_client_assets::ui::DEFAULT_SKIN;
    let hidden = hidden_too
        && skin != default
        && screens
            .get(directory, skin, id)
            .is_some_and(|screen| screen.area.width <= 0.0 || screen.area.height <= 0.0);
    if hidden { default } else { skin }.to_owned()
}

/// Draws each skinned window from the skin: a frame just built, or one
/// drawn in another skin, is drawn again. A window the skin sizes to
/// nothing is drawn as the default skin draws it while the player asks for
/// such windows ([`eq_client_core::qol::Fix::HiddenWindows`]).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn apply(
    mut commands: Commands,
    (skin, options): (Res<super::skin::UiSkin>, Res<super::options::OptionsState>),
    settings: Res<super::ViewerSettings>,
    mut screens: ResMut<Screens>,
    mut frames: Frames,
    mut art: crate::sheets::Art,
    (paperdoll, mut skinned, online): (
        Option<Res<super::paperdoll::PaperdollImage>>,
        ResMut<Skinned>,
        Option<Res<crate::online::OnlineState>>,
    ),
) {
    let Some(directory) = settings.0.eq_directory.as_deref() else {
        return;
    };
    let hidden_too = options
        .options
        .qol
        .on(eq_client_core::qol::Fix::HiddenWindows);
    for (frame, id, mut node, mut background, mut border, drawn, mut state) in &mut frames {
        let from = drawn_from(&mut screens, directory, (&skin.0, *id), hidden_too);
        let places = online
            .as_deref()
            .and_then(|online| fit::places(online.world(), *id));
        if drawn.is_some_and(|drawn| drawn.0 == from && drawn.1 == places) {
            continue;
        }
        let arranged = if state.placed() {
            None
        } else {
            arranged(
                &mut screens,
                directory,
                (&skin.0, *id),
                &options.options.qol,
            )
        };
        let Some(screen) = screens.get(directory, &from, *id) else {
            continue;
        };
        let fitted = places.and_then(|places| fit::fitted(screen, places));
        let screen = fitted.as_ref().unwrap_or(screen);
        commands.entity(frame).insert(Drawn(from.clone(), places));
        art.draw_in((from != skin.0).then(|| from.clone()));
        skinned.0.insert(*id);
        reshape(&mut node, screen, state.placed(), (*id, arranged));
        super::windows::drag_anywhere(&mut commands, frame, *id);
        background.0 = Color::NONE;
        *border = BorderColor::all(Color::NONE);
        commands.entity(frame).despawn_children();
        let mut title_bottom = None;
        commands.entity(frame).with_children(|window| {
            let context = Context {
                id: *id,
                paperdoll: paperdoll.as_deref(),
                depth: 0,
                tab_frame: screen.tab_frame.as_deref(),
            };
            title_bottom = draw(window, screen, &mut art, &context);
        });
        art.draw_in(None);
        state.drawn_from_skin(&mut node, title_bottom);
    }
}

/// A window's size as drawn: from the skin it is drawn from, zero where that
/// skin draws no such window or sizes it to nothing.
fn drawn_size(
    screens: &mut Screens,
    directory: &std::path::Path,
    (skin, id): (&str, WindowId),
    hidden_too: bool,
) -> Vec2 {
    let from = drawn_from(screens, directory, (skin, id), hidden_too);
    screens
        .get(directory, &from, id)
        .map_or(Vec2::ZERO, |screen| {
            Vec2::new(screen.area.width, screen.area.height).max(Vec2::ZERO)
        })
}

/// Where the client's arrangement opens a window drawn from the skin
/// ([`WindowId::arranged`]), by the sizes the windows are drawn at, while
/// Open Windows Apart is on ([`eq_client_core::qol::Fix::WindowsApart`]);
/// None where the window opens where the skin puts it.
fn arranged(
    screens: &mut Screens,
    directory: &std::path::Path,
    (skin, id): (&str, WindowId),
    qol: &eq_client_core::qol::Settings,
) -> Option<super::windows::Placement> {
    use eq_client_core::qol::Fix;
    if !qol.on(Fix::WindowsApart) {
        return None;
    }
    let hidden_too = qol.on(Fix::HiddenWindows);
    id.arranged(|other| drawn_size(screens, directory, (skin, other), hidden_too))
}

/// Sizes the frame as the skin does. A window placed neither by the player
/// nor by the official client's UI file opens where it opens by default
/// ([`open_by_default`]), unless its description declares a place of its
/// own (`Description::opening`).
fn reshape(
    node: &mut Node,
    screen: &Screen,
    placed: bool,
    (id, arranged): (WindowId, Option<super::windows::Placement>),
) {
    node.width = px(screen.area.width);
    node.height = px(screen.area.height);
    // The skin's size is the window's: no caps or scrolling of the client's own.
    node.min_width = Val::Auto;
    node.min_height = Val::Auto;
    node.max_width = Val::Auto;
    node.max_height = Val::Auto;
    node.overflow = Overflow::visible();
    node.padding = UiRect::ZERO;
    node.border = UiRect::ZERO;
    node.row_gap = Val::ZERO;
    if !placed && id.describe().opening == super::windows::Opening::Skin {
        open_by_default(node, screen, arranged);
    }
}

/// Puts a window drawn from the skin where it opens by default: where the
/// client's arrangement puts it ([`WindowId::arranged`]), or else where the
/// skin puts it, as the official client opens it.
fn open_by_default(node: &mut Node, screen: &Screen, arranged: Option<super::windows::Placement>) {
    if let Some(place) = arranged {
        place.apply(node);
        return;
    }
    node.position_type = PositionType::Absolute;
    node.left = px(screen.area.x);
    node.top = px(screen.area.y);
    node.right = Val::Auto;
    node.bottom = Val::Auto;
    node.margin = UiRect::ZERO;
}

/// Keeps the client's own panels, which no skin draws, where the arrangement
/// puts them while the skin draws the HUD and the player has not placed
/// them ([`WindowId::arranged`], [`eq_client_core::qol::Fix::WindowsApart`]):
/// the Status panel in the bottom right corner, clear of the skin's windows.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn place_own(
    (skinned, options): (Res<Skinned>, Res<super::options::OptionsState>),
    mut frames: Query<(&WindowId, &mut Node, &super::windows::Frame), Without<Drawn>>,
) {
    if !skinned.has(WindowId::Player)
        || !options
            .options
            .qol
            .on(eq_client_core::qol::Fix::WindowsApart)
    {
        return;
    }
    for (id, mut node, frame) in &mut frames {
        if frame.placed() || id.official().is_some() {
            continue;
        }
        let Some(place) = id.arranged(|_| Vec2::ZERO) else {
            continue;
        };
        let mut wanted = node.clone();
        place.apply(&mut wanted);
        if wanted != *node {
            *node = wanted;
        }
    }
}

/// A text box's words, which scroll inside it.
#[derive(Component)]
pub(crate) struct ScrolledText;

/// Shows the start of a text box's words whenever they change, as when the
/// item display shows another item or a book turns its page.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn rewind(
    changed: Query<&ChildOf, Changed<Text>>,
    mut boxes: Query<&mut ScrollPosition, With<ScrolledText>>,
) {
    for parent in &changed {
        if let Ok(mut position) = boxes.get_mut(parent.parent())
            && position.y != 0.0
        {
            position.y = 0.0;
        }
    }
}

/// A window within a window that scrolls its pieces.
#[derive(Component)]
pub(crate) struct ScrolledView;

/// The height of a tab that shows its page's words.
pub(crate) const WORD_TAB_HEIGHT: f32 = 18.0;

/// Windows whose tab boxes show every page, a tab for each.
const TABBED: [WindowId; 2] = [WindowId::ActionsWindow, WindowId::Options];

/// A tab of a skinned window's tab box: the page it shows. Each tab box
/// keeps its own choice, by the name the skin gives it; one on another's
/// page has a depth of one.
#[derive(Component, Clone, PartialEq, Eq)]
pub(crate) struct SkinTab {
    pub(crate) window: WindowId,
    /// The tab box's name.
    pub(crate) tab_box: std::sync::Arc<str>,
    pub(crate) depth: u8,
    pub(crate) index: usize,
}

/// A page of a skinned window's tab box.
#[derive(Component, Clone, PartialEq, Eq)]
pub(crate) struct SkinPage(SkinTab);

/// One of a tab's two pictures: the active one shows on the page shown.
#[derive(Component, Clone)]
pub(crate) struct TabFace {
    tab: SkinTab,
    active: bool,
}

/// A tab's words, in the page's two colours: the first while another page
/// shows, the second while it does.
#[derive(Component, Clone)]
pub(crate) struct TabWords {
    tab: SkinTab,
    colors: [Color; 2],
}

/// The page each tab box of a tabbed window shows, by the tab box's name:
/// its first until another is chosen.
#[derive(Resource, Default)]
pub(crate) struct Tabs(std::collections::BTreeMap<(WindowId, std::sync::Arc<str>), usize>);

impl Tabs {
    fn shows(&self, tab: &SkinTab) -> bool {
        self.0
            .get(&(tab.window, tab.tab_box.clone()))
            .copied()
            .unwrap_or(0)
            == tab.index
    }
}

/// Puts a tab of words in the skin's tab frame: the frame's pieces around
/// it, and room for the words inside them. The chat's tabs are framed so
/// too.
pub(crate) fn frame_tab(
    cell: &mut EntityCommands,
    art: &mut crate::sheets::Art,
    look: &eq_client_assets::sidl::FrameLook,
    node: Node,
) {
    let insets = frame::insets(look);
    cell.insert(Node {
        padding: UiRect {
            left: px(insets.left + 4.0),
            right: px(insets.right + 4.0),
            top: px(insets.top),
            bottom: px(insets.bottom),
        },
        ..node
    });
    cell.with_children(|cell| {
        frame::around(cell, art, look);
    });
}

/// Shows the page each tabbed window has chosen, and its tab lit; a clicked
/// tab chooses its page.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn tabs(
    mut chosen: ResMut<Tabs>,
    clicks: Query<(&Interaction, &SkinTab), Changed<Interaction>>,
    mut pages: Query<(&SkinPage, &mut Node), Without<TabFace>>,
    mut faces: Query<(&TabFace, &mut Node), Without<SkinPage>>,
    mut words: Query<(&TabWords, &mut TextColor)>,
) {
    for (interaction, tab) in &clicks {
        if *interaction == Interaction::Pressed {
            chosen
                .0
                .insert((tab.window, tab.tab_box.clone()), tab.index);
        }
    }
    let display = |shown: bool| if shown { Display::Flex } else { Display::None };
    for (SkinPage(tab), mut node) in &mut pages {
        let wanted = display(chosen.shows(tab));
        if node.display != wanted {
            node.display = wanted;
        }
    }
    for (face, mut node) in &mut faces {
        let wanted = display(chosen.shows(&face.tab) == face.active);
        if node.display != wanted {
            node.display = wanted;
        }
    }
    for (tab, mut color) in &mut words {
        let wanted = tab.colors[usize::from(chosen.shows(&tab.tab))];
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

/// A gem: the spell's icon in the skin's holder. It behaves as the client's
/// own gems do: a click casts, a shifted click forgets, and it greys out
/// where the server takes no casts.
fn spell_gem(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    gem: &eq_client_assets::sidl::SpellGem,
    inside: &Area,
) {
    let Some(index) = gem
        .id
        .as_deref()
        .and_then(|id| id.strip_prefix("CSPW_Spell"))
        .and_then(|number| number.parse::<u8>().ok())
        .filter(|index| *index < 8)
    else {
        return;
    };
    let area = gem.area;
    let mut node = at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    );
    node.justify_content = JustifyContent::Center;
    node.align_items = AlignItems::Center;
    window
        .spawn((
            Button,
            super::hud::SpellGem(index),
            hotbar::Pickable(hotbar::Source::Gem(index)),
            crate::outbox::Needs::Capability(eq_client_core::Capability::Casting),
            BackgroundColor(Color::NONE),
            node,
        ))
        .with_children(|gem_node| {
            if let Some(piece) = &gem.background {
                picture(gem_node, art, piece, at(0.0, 0.0, area.width, area.height));
            }
            // The skin's gem icons where it has them, at its offset or
            // centred; else the square icon, as before.
            if let Some(gems) = gem.icons.clone().filter(|gems| !gems.sheets.is_empty()) {
                #[allow(clippy::cast_precision_loss, reason = "cells are small")]
                let place = gem.icon_offset.unwrap_or((
                    (area.width - gems.cell_width as f32) / 2.0,
                    (area.height - gems.cell_height as f32) / 2.0,
                ));
                gem_node.spawn(super::spell_icons::gem_artwork(
                    usize::from(index),
                    gems,
                    place,
                ));
            } else {
                let icon = (area.height - 6.0).max(8.0);
                gem_node.spawn(super::spell_icons::artwork(
                    super::spell_icons::Source::Gem(usize::from(index)),
                    icon,
                ));
            }
            if let Some(piece) = &gem.holder {
                picture(gem_node, art, piece, at(0.0, 0.0, area.width, area.height));
            }
        });
}

/// A coin box's count, right of its coin's picture.
fn coin_count(inner: &mut ChildSpawnerCommands, area: Area, shows: Shows, ink: Color) {
    aligned(
        inner,
        at(0.0, 5.0, area.width - 6.0, area.height - 5.0),
        Align::Right,
        (shows, theme::text("", Size::Body, ink)),
    );
}

/// The skin's pages of a tab box, and for the Options window a last page,
/// its quality-of-life page, with a checkbox for each setting, drawn with the
/// skin's own checkbox, in the room a page has inside a window this tall.
fn with_qol_page(
    pages: &[eq_client_assets::sidl::Page],
    owner: WindowId,
    inside_height: f32,
) -> std::borrow::Cow<'_, [eq_client_assets::sidl::Page]> {
    use eq_client_assets::sidl::Element;
    if owner != WindowId::Options {
        return std::borrow::Cow::Borrowed(pages);
    }
    let checkbox =
        pages
            .iter()
            .flat_map(|page| &page.pieces)
            .find_map(|(_, element)| match element {
                Element::Button(button) if button.checkbox => Some(button.clone()),
                _ => None,
            });
    let (Some(checkbox), Some(first)) = (checkbox, pages.first()) else {
        return std::borrow::Cow::Borrowed(pages);
    };
    let qol = eq_client_assets::sidl::Page {
        name: "EQC_QoLPage".to_owned(),
        title: Some("QoL".to_owned()),
        area: first.area,
        template: first.template.clone(),
        pieces: Vec::new(),
        icon: [None, None],
        title_colors: first.title_colors,
        tooltip: Some("Quality-of-life fixes only this client has.".to_owned()),
    };
    // The page's room under the row of tabs, its own tab among them.
    let room = inside_height - tab_strip(pages.iter().chain([&qol]));
    let mut pieces = qol_checkboxes(&checkbox, room);
    pieces.extend(qol_scale(pages, &checkbox, room));
    let mut all = pages.to_vec();
    all.push(eq_client_assets::sidl::Page { pieces, ..qol });
    std::borrow::Cow::Owned(all)
}

/// The quality-of-life page's interface scale, in the next place after its checkboxes:
/// a caption, a slider cut from the skin's first slider on the Options
/// window's pages, and the label that shows its value, cut from that
/// slider's own; none where the skin has no slider and value label.
fn qol_scale(
    pages: &[eq_client_assets::sidl::Page],
    checkbox: &eq_client_assets::sidl::Button,
    room: f32,
) -> Vec<(String, eq_client_assets::sidl::Element)> {
    use eq_client_assets::sidl::Element;
    let pieces = || pages.iter().flat_map(|page| &page.pieces);
    let slider = pieces().find_map(|(_, element)| match element {
        Element::Slider(slider) => Some(slider.clone()),
        _ => None,
    });
    let label = pieces().find_map(|(name, element)| match element {
        Element::Label(label) if name.ends_with("ValueLabel") => Some(label.clone()),
        _ => None,
    });
    let (Some(mut slider), Some(mut value)) = (slider, label) else {
        return Vec::new();
    };
    // The caption over the slider, in the next place with room for both
    // down the page, else at the top of the next column.
    let tall = value.area.height + 2.0 + slider.area.height;
    let first = eq_client_core::qol::Fix::settings().count();
    let (x, y, width) = (first..first + 64)
        .map(|index| qol_place(checkbox, room, index))
        .find(|&(_, y, _)| y + tall <= room - 10.0 || y <= 10.0)
        .unwrap_or_default();
    let mut caption = value.clone();
    caption.eq_type = None;
    "Interface scale".clone_into(&mut caption.text);
    caption.area = Area {
        x,
        y,
        width,
        height: value.area.height,
    };
    slider.id = Some(controls::UI_SCALE_SLIDER.to_owned());
    slider.area.x = x;
    slider.area.y = y + value.area.height + 2.0;
    slider.area.width = slider
        .area
        .width
        .min(width - value.area.width.max(40.0) - 4.0);
    value.eq_type = None;
    value.area.x = slider.area.x + slider.area.width + 4.0;
    value.area.y = slider.area.y;
    vec![
        ("EQC_QoL_UiScaleLabel".to_owned(), Element::Label(caption)),
        (
            controls::UI_SCALE_SLIDER.to_owned(),
            Element::Slider(slider),
        ),
        (controls::UI_SCALE_VALUE.to_owned(), Element::Label(value)),
    ]
}

/// A checkbox cut from the skin's own for each quality-of-life setting, in
/// their order, one under another down a page this tall and then on down
/// the next column.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a page holds few rows"
)]
fn qol_checkboxes(
    checkbox: &eq_client_assets::sidl::Button,
    room: f32,
) -> Vec<(String, eq_client_assets::sidl::Element)> {
    eq_client_core::qol::Fix::settings()
        .enumerate()
        .map(|(index, fix)| {
            let mut checkbox = checkbox.clone();
            let id = qol_checkbox_id(fix);
            let (x, y, width) = qol_place(&checkbox, room, index);
            checkbox.id = Some(id.clone());
            checkbox.text = Some(fix.label().to_owned());
            checkbox.tooltip = Some(fix.tooltip().to_owned());
            checkbox.area.x = x;
            checkbox.area.y = y;
            checkbox.area.width = width;
            (id, eq_client_assets::sidl::Element::Button(checkbox))
        })
        .collect()
}

/// Where the quality-of-life page's place this far along sits, and how wide it is: one
/// under another down a page this tall, then on down the next column, each
/// as tall as the skin's checkbox and two such rows for the scale.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a page holds few rows"
)]
fn qol_place(
    checkbox: &eq_client_assets::sidl::Button,
    room: f32,
    index: usize,
) -> (f32, f32, f32) {
    /// Room left around the places, and between columns.
    const MARGIN: f32 = 10.0;
    /// Room between one place and the next one down.
    const GAP: f32 = 6.0;
    let row = checkbox.area.height + GAP;
    let width = checkbox.area.width.max(190.0);
    // As many rows as fit inside the margins, and at least one.
    let rows = (((room - 2.0 * MARGIN + GAP) / row).floor() as usize).max(1);
    (
        MARGIN + to_f32(u32::try_from(index / rows).unwrap_or(u32::MAX)) * (width + MARGIN),
        MARGIN + to_f32(u32::try_from(index % rows).unwrap_or(u32::MAX)) * row,
        width,
    )
}

/// The gem, counted from 0, whose spell a label names, by the label's number
/// (`EQType`): 60 for the first gem to 67 for the eighth, in whichever
/// window it is. The skins that name their gems' spells give the label
/// beside each gem these numbers in the gems' order; that the official
/// client fills them so is inferred.
fn gem_label(kind: u32) -> Option<u8> {
    match kind {
        60..=67 => u8::try_from(kind - 60).ok(),
        _ => None,
    }
}

/// The name of the spell memorized in this gem, or nothing while the gem is
/// empty.
fn gem_name(
    world: &eq_client_core::world::ClientWorld,
    names: &super::spellbook::SpellNames,
    gem: u8,
) -> String {
    world
        .gem(usize::from(gem))
        .map_or_else(String::new, |spell| names.label(spell))
}

/// A skin's button: what the client does with it, or greyed out where it
/// does nothing yet.
fn button(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    button: &eq_client_assets::sidl::Button,
    inside: &Area,
    owner: WindowId,
) {
    let Some(does) = does(button.id.as_deref().unwrap_or_default(), owner) else {
        return;
    };
    let area = button_area(button, does, inside);
    let node = at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    );
    let look = match does {
        Does::Nothing => button
            .look
            .disabled
            .as_ref()
            .or(button.look.normal.as_ref()),
        // The item display's icon button draws the item's icon as its
        // Normal piece, with nothing of the template under it (from the
        // program, Titanium sitting 9e).
        Does::ItemIcon => None,
        _ => button.look.normal.as_ref(),
    };
    // A button the skin draws only under the pointer, as the default
    // spellbook's close mark, starts unseen (`buttons`).
    let unseen = look.is_none() && !matches!(does, Does::Nothing | Does::ItemIcon);
    let look = look.or_else(|| button.look.flyby.as_ref().filter(|_| unseen));
    let mut drawn = match look.and_then(|piece| art.cut(piece)) {
        Some(mut image) => {
            if unseen {
                image.color.set_alpha(0.0);
            }
            window.spawn((image, node))
        }
        None => window.spawn(node),
    };
    behave(&mut drawn, does, button, owner);
    if let Some(id) = &button.id {
        drawn.insert(SkinId {
            window: owner,
            id: id.clone(),
        });
    }
    if let Some(shows) = paired(does, owner) {
        drawn.insert((shows, Visibility::Hidden));
    }
    let ink = match does {
        Does::Nothing => theme::INK_DIM,
        _ => button.text_color.map_or(theme::INK_BRIGHT, rgb),
    };
    drawn.with_children(|inner| {
        // A buff slot's decal is the buff's own icon, which the caption draws.
        if let (Some(decal), Some(place)) = (&button.decal, button.decal_area)
            && !matches!(
                does,
                Does::PetBuff(_)
                    | Does::Buff(..)
                    | Does::Chosen
                    | Does::ItemIcon
                    | Does::BookPlace(_)
            )
        {
            picture(
                inner,
                art,
                decal,
                at(place.x, place.y, place.width, place.height),
            );
        }
        caption(inner, does, (button, area), owner, ink);
    });
}

/// Where a skin's button sits in its window: where the skin places it or
/// its anchors put it. A buff slot the skin does neither for, an effects
/// window's or the pet window's, is stacked ([`stacked`]); any other such
/// button keeps the skin's default place, the window's top left.
fn button_area(button: &eq_client_assets::sidl::Button, does: Does, inside: &Area) -> Area {
    let slot = match does {
        Does::Buff(_, index) => Some(index),
        Does::PetBuff(slot) => u32::try_from(slot).ok(),
        _ => None,
    };
    match slot {
        Some(slot) if !button.placed && button.anchors.is_none() => {
            stacked(button.area, slot, inside)
        }
        _ => button.anchors.map_or(button.area, |anchors| {
            anchors.within(inside.width, inside.height)
        }),
    }
}

/// Where a buff slot's button goes when the skin gives it no place and no
/// anchors: down the window's right edge in one column, a pixel in from the
/// right and the top and a pixel apart, in the slots' order. Slots past the
/// window's bottom wait below it, as the skins that leave these buttons
/// unplaced number their slots on down. That is where those skins draw the
/// numbered slots, and the project owner saw the official client fill them;
/// that it places such buttons so is inferred (a recording item).
fn stacked(area: Area, slot: u32, inside: &Area) -> Area {
    Area {
        x: inside.width - area.width - 1.0,
        y: 1.0 + to_f32(slot) * (area.height + 1.0),
        ..area
    }
}

/// When a button the skin keeps in another's place shows: the pet's Stand
/// under its Sit, the group window's Follow and Decline over Invite and
/// Disband, and the Raid window's Accept and Decline over its Invite and
/// Disband, and its Unlock over its Lock. One of each pair shows at a time.
fn paired(does: Does, owner: WindowId) -> Option<Shows> {
    use crate::raid::RaidAction;
    match does {
        Does::Slash(command) => pet_posture_button(command).map(Shows::WhilePetSits),
        Does::Group(group) if owner == WindowId::Group => {
            Some(Shows::WhileInvited(group.answers()))
        }
        Does::Raid(raid) => Some(Shows::WhileRaidInvited(raid.answers())),
        Does::RaidAction(RaidAction::Disband) => Some(Shows::WhileRaidInvited(false)),
        Does::RaidAction(RaidAction::Lock(lock)) => Some(Shows::WhileRaidLocked(!lock)),
        _ => None,
    }
}

/// What a pressed skin button does, and the state it shows.
#[allow(clippy::too_many_lines)] // One arm for each kind of button.
fn behave(
    drawn: &mut EntityCommands,
    does: Does,
    button: &eq_client_assets::sidl::Button,
    owner: WindowId,
) {
    use eq_client_core::Capability;
    let skin = || SkinButton(button.look.clone());
    match does {
        Does::Toggles(toggles) => {
            drawn.insert((Button, super::windows::SelectorButton(toggles), skin()));
            // Greyed where the session does not offer the window.
            match toggles.needs() {
                Some(needs) => drawn.insert(crate::outbox::Needs::Capability(needs)),
                None => drawn,
            }
        }
        Does::Closes => drawn.insert((Button, items::Closes(owner), skin())),
        Does::Gives => drawn.insert((Button, super::give::GiveButton)),
        Does::Coins(place, coin) => drawn.insert((Button, super::coins::CoinBox { place, coin })),
        Does::Bound(binding) => {
            drawn.insert((
                Button,
                skin(),
                hotbar::Pickable(hotbar::Source::Fixed(binding)),
                hotbar::need(Some(binding)),
            ));
            // The button and its hotbutton run one command, from one table;
            // Melee Attack, which has none, toggles in `combat::input`.
            match hotbar::command(binding) {
                Some(command) => drawn.insert(SlashButton(command)),
                None => drawn.insert(AttackButton),
            }
        }
        Does::Ability(place) => drawn.insert((
            Button,
            place,
            hotbar::Pickable(hotbar::Source::Ability(place)),
            skin(),
            crate::outbox::Needs::Capability(Capability::Abilities),
        )),
        Does::Slash(command) => drawn.insert((Button, SlashButton(command), skin())),
        Does::Group(group) => drawn.insert((
            Button,
            SlashButton(group.command()),
            skin(),
            crate::outbox::Needs::Capability(Capability::Grouping),
        )),
        Does::Raid(raid) => drawn.insert((
            Button,
            SlashButton(raid.command()),
            skin(),
            crate::outbox::Needs::Capability(Capability::Raiding),
        )),
        Does::RaidAction(action) => drawn.insert((
            Button,
            action,
            skin(),
            crate::outbox::Needs::Capability(Capability::Raiding),
        )),
        Does::Unoffered(capability) => {
            drawn.insert((Button, skin(), crate::outbox::Needs::Capability(capability)))
        }
        Does::PetBuff(slot) => drawn.insert((Shows::PetBuff(slot), Visibility::Hidden)),
        // Hovering a buff names it, as the client's own window does.
        Does::Buff(window, index) => drawn.insert((
            Shows::Buff(window, index),
            Visibility::Hidden,
            Interaction::default(),
            super::tooltip::Tooltip::default(),
        )),
        Does::Option(toggle) => {
            drawn.insert((Button, super::options::OptionCheckbox(toggle), skin()));
            // Greyed where the session does not offer what it is for.
            match crate::outbox::Needs::of(toggle) {
                Some(needs) => drawn.insert(needs),
                None => drawn,
            }
        }
        Does::Trains => drawn.insert((
            Button,
            super::training::TrainButton,
            skin(),
            crate::outbox::Needs::Capability(Capability::Training),
        )),
        // A question shows only where the session offers what it is about.
        Does::Answers(accept) => {
            drawn.insert((Button, super::confirm::AnswerButton(accept), skin()))
        }
        Does::TurnsPage(forward) => {
            drawn.insert((Button, super::reading::PageButton(forward), skin()))
        }
        Does::Combines(container) => drawn.insert((
            Button,
            super::tradeskills::CombineButton(container),
            skin(),
            crate::outbox::Needs::Capability(Capability::Tradeskills),
        )),
        Does::Maps(action) => drawn.insert((Button, action, skin())),
        Does::HotButton(index) => drawn.insert((
            Button,
            hotbar::Slot(index),
            hotbar::Pickable(hotbar::Source::Slot(index)),
            skin(),
            crate::outbox::Needs::Nothing,
        )),
        Does::Picks(action) => drawn.insert((Button, action, skin())),
        Does::Trades(action) => {
            trades(drawn, action, skin());
            &mut *drawn
        }
        Does::Chosen => {
            chosen_picture(drawn, button);
            &mut *drawn
        }
        // The icon fills the whole button, as its Normal piece would.
        Does::ItemIcon => drawn.insert(super::items::ItemIcon {
            x: 0.0,
            y: 0.0,
            size: button.area.width.min(button.area.height),
        }),
        Does::BookPlace(_)
        | Does::TurnsSpellbook(_)
        | Does::CharacterSlot(_)
        | Does::EntersWorld
        | Does::Quits => {
            listed(drawn, does, skin());
            &mut *drawn
        }
        Does::Offered(_) | Does::BagIcon => drawn,
        Does::Nothing => drawn.insert(missing()),
    };
    if let Some(tooltip) = &button.tooltip
        && !matches!(does, Does::Nothing)
    {
        drawn.insert(crate::tooltip::Tooltip(tooltip.clone()));
    }
}

/// A spellbook's or the character list's button: a place on the book's
/// open pages, which hovering names (`spellbook::book_present`), an arrow
/// that turns the pages, a character's button, which hovering says the level
/// of (`character_select::names`), Enter World or Quit.
fn listed(drawn: &mut EntityCommands, does: Does, skin: SkinButton) {
    use crate::character_select::Action;
    let tooltip = crate::tooltip::Tooltip::default;
    match does {
        Does::BookPlace(place) => drawn.insert((
            Button,
            crate::spellbook::BookPlace(place),
            skin,
            crate::outbox::Needs::Capability(eq_client_core::Capability::Spellbook),
            tooltip(),
        )),
        Does::TurnsSpellbook(forward) => {
            drawn.insert((Button, crate::spellbook::TurnsPages(forward), skin))
        }
        Does::CharacterSlot(slot) => drawn.insert((Button, Action::Choose(slot), skin, tooltip())),
        Does::EntersWorld => drawn.insert((Button, Action::Enter, skin)),
        Does::Quits => drawn.insert((Button, Action::Quit, skin)),
        _ => drawn,
    };
}

/// A loot or merchant window's button that does this; the skin keeps the
/// merchant window's Sell over its Buy, and the one for what is chosen shows.
fn trades(drawn: &mut EntityCommands, action: crate::trade::Action, skin: SkinButton) {
    use crate::trade::Action;
    drawn.insert((
        Button,
        action,
        skin,
        crate::outbox::Needs::Capability(crate::trade::needs(action)),
    ));
    match action {
        Action::BuyChosen => drawn.insert((Shows::WhileSelling(false), Visibility::Hidden)),
        Action::SellChosen => drawn.insert((Shows::WhileSelling(true), Visibility::Hidden)),
        _ => drawn,
    };
}

/// The merchant window's box for the chosen item, its picture where the
/// skin's sample sits; hovering it names the item.
fn chosen_picture(drawn: &mut EntityCommands, button: &eq_client_assets::sidl::Button) {
    let (x, y, size) = decal_place(button);
    drawn.insert((
        crate::trade::ChosenPicture { x, y, size },
        Interaction::default(),
    ));
}

/// Where a buff slot's icon goes in its button: where the skin puts the
/// button's picture (its decal), or else just inside the button at its
/// height; its left and top, and its size.
fn buff_icon_place(button: &eq_client_assets::sidl::Button, area: Area) -> ((f32, f32), f32) {
    if button.decal_area.is_some() {
        let (x, y, size) = decal_place(button);
        ((x, y), size)
    } else {
        (super::spell_icons::INSET, area.height.min(area.width) - 4.0)
    }
}

/// Where a button's picture of an item goes: where the skin puts its
/// sample picture, or else just inside the button; its left, top and size.
fn decal_place(button: &eq_client_assets::sidl::Button) -> (f32, f32, f32) {
    let place = button.decal_area.unwrap_or(Area {
        x: 1.0,
        y: 1.0,
        width: button.area.width - 2.0,
        height: button.area.height - 2.0,
    });
    (place.x, place.y, place.width.min(place.height))
}

/// What a skin button shows on itself: its words, a count of coins, an
/// ability's name or a bag's picture.
fn caption(
    inner: &mut ChildSpawnerCommands,
    does: Does,
    (button, area): (&eq_client_assets::sidl::Button, Area),
    owner: WindowId,
    ink: Color,
) {
    // A square button's words wrap, as the Actions window's do; a wide one
    // keeps them on one line, centred.
    let words = |text: &str| {
        (
            theme::text(text, Size::Small, ink),
            TextLayout::new(Justify::Center, LineBreak::WordBoundary),
            at(1.0, 2.0, area.width - 2.0, area.height - 4.0),
        )
    };
    match does {
        Does::Coins(place, coin) => coin_count(inner, area, Shows::Coins(place, coin), ink),
        Does::Offered(coin) => coin_count(inner, area, Shows::Offered(coin), ink),
        Does::BagIcon => {
            let part = match owner {
                WindowId::Bag(bag) => Some(items::BagPart::Icon(InventorySlot(bag))),
                WindowId::WorldContainer => Some(items::BagPart::WorldIcon),
                _ => None,
            };
            if let Some(part) = part {
                inner.spawn((
                    part,
                    ImageNode::default(),
                    at(0.0, 0.0, area.width, area.height),
                ));
            }
        }
        Does::Ability(place) => {
            inner.spawn((super::abilities::AbilityLabel(place), words("")));
        }
        Does::HotButton(index) => hotbar::contents(inner, index, Some(decal_place(button))),
        Does::PetBuff(slot) => {
            let (place, size) = buff_icon_place(button, area);
            inner.spawn(super::spell_icons::artwork_at(
                super::spell_icons::Source::PetBuff(slot),
                place,
                size,
            ));
        }
        // The spell's picture, where the skin puts its sample, and the
        // mark that shows a right click chose it (`spellbook::present`).
        Does::BookPlace(place) => {
            let (x, y, size) = decal_place(button);
            inner.spawn(super::spell_icons::artwork_at(
                super::spell_icons::Source::BookPlace(place),
                (x, y),
                size,
            ));
            inner.spawn((
                crate::spellbook::ChosenMark(place),
                Node {
                    border: UiRect::all(px(2)),
                    display: Display::None,
                    ..at(0.0, 0.0, area.width, area.height)
                },
                BorderColor::all(theme::EDGE_HOVER),
                bevy::ui::FocusPolicy::Pass,
            ));
        }
        Does::Buff(window, index) => {
            let (place, size) = buff_icon_place(button, area);
            inner.spawn(super::spell_icons::artwork_at(
                super::spell_icons::Source::Window(window, index),
                place,
                size,
            ));
        }
        // A box with a picture shows a value, such as the bank's coins;
        // the skin's text there is only a sample, so it stays blank until
        // the client has the value.
        // The item's picture is drawn over it (`trade::picture`,
        // `items::icon`).
        Does::Chosen | Does::ItemIcon => (),
        Does::CharacterSlot(slot) => slot_name(inner, slot, (button, area), ink),
        Does::Nothing if button.decal.is_some() => (),
        Does::Toggles(_)
        | Does::Closes
        | Does::Gives
        | Does::Bound(_)
        | Does::Slash(_)
        | Does::Group(_)
        | Does::Raid(_)
        | Does::RaidAction(_)
        | Does::Unoffered(_)
        | Does::Option(_)
        | Does::Trains
        | Does::Answers(_)
        | Does::TurnsPage(_)
        | Does::Combines(_)
        | Does::Maps(_)
        | Does::Picks(_)
        | Does::Trades(_)
        | Does::TurnsSpellbook(_)
        | Does::EntersWorld
        | Does::Quits
        | Does::Nothing => {
            if let Some(text) = &button.text {
                if area.height >= 30.0 && area.width < area.height * 1.5 {
                    inner.spawn(words(text));
                } else {
                    aligned(
                        inner,
                        at(0.0, (area.height - 12.0) / 2.0, area.width, 12.0),
                        Align::Center,
                        theme::text(text.as_str(), Size::Small, ink),
                    );
                }
            }
        }
    }
}

/// A character button's words: the character's name, or the skin's words
/// for an empty slot (`character_select::names`).
fn slot_name(
    inner: &mut ChildSpawnerCommands,
    slot: u8,
    (button, area): (&eq_client_assets::sidl::Button, Area),
    ink: Color,
) {
    let empty = button.text.clone().unwrap_or_default();
    aligned(
        inner,
        at(0.0, (area.height - 12.0) / 2.0, area.width, 12.0),
        Align::Center,
        (
            theme::text(empty.as_str(), Size::Small, ink),
            crate::character_select::SlotName { slot, empty, ink },
        ),
    );
}

/// The skin's melee attack button, lit while the player attacks.
#[derive(Component)]
pub(crate) struct AttackButton;

/// A skin button that runs a game slash command.
#[derive(Component, Clone, Copy)]
pub(crate) struct SlashButton(pub(crate) &'static str);

/// Runs a clicked slash button's command, as typing it would; a refusal
/// shows in chat.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn slash(
    online: Res<super::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    mut chat: ResMut<super::chat::ChatState>,
    buttons: hotbar::Clicks<SlashButton>,
) {
    for SlashButton(command) in buttons.iter() {
        super::chat::run_game_command(command, &online, &outbox, &mut chat);
    }
}

/// The skin buttons that show a state, with what decides it.
type Stateful<'w, 's> = Query<
    'w,
    's,
    (
        &'static SkinButton,
        Option<&'static super::windows::SelectorButton>,
        Has<AttackButton>,
        Option<&'static super::options::OptionCheckbox>,
        (
            Has<Greyed>,
            Has<scrollbar::ScrollArrow>,
            Option<&'static crate::character_select::Action>,
        ),
        &'static Interaction,
        &'static mut ImageNode,
    ),
>;

/// Draws each skin button in its state: on while its window is open, the
/// player attacks, its option is on or its character is chosen, lit under
/// the pointer, and in its disabled look while greyed.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    (shown, options, online): (
        Res<super::windows::Shown>,
        Res<super::options::OptionsState>,
        Res<super::online::OnlineState>,
    ),
    combat: Res<super::combat::CombatState>,
    mut art: crate::sheets::Art,
    mut buttons: Stateful,
) {
    let chosen = online
        .selection
        .as_ref()
        .and_then(crate::character_select::Selection::chosen);
    for (
        SkinButton(look),
        selector,
        attack,
        checkbox,
        (greyed, arrow, character),
        interaction,
        mut image,
    ) in &mut buttons
    {
        let on = match (selector, checkbox, character) {
            (Some(selector), ..) => shown.displayed(selector.0),
            (None, Some(checkbox), _) => options.on(checkbox.0),
            (None, None, Some(crate::character_select::Action::Choose(slot))) => {
                chosen == Some(*slot)
            }
            // A scrollbar's arrow is down while it is held.
            (None, None, _) if arrow => *interaction == Interaction::Pressed,
            (None, None, _) => attack && combat.auto_attack,
        };
        let hovered = *interaction != Interaction::None;
        let piece = match (greyed, on, hovered) {
            (true, _, _) => look.disabled.as_ref(),
            (false, true, true) => look.pressed_flyby.as_ref().or(look.pressed.as_ref()),
            (false, true, false) => look.pressed.as_ref(),
            (false, false, true) => look.flyby.as_ref(),
            (false, false, false) => None,
        }
        .or(look.normal.as_ref());
        match piece.and_then(|piece| art.cut(piece)) {
            Some(wanted) => {
                if image.rect != wanted.rect {
                    image.rect = wanted.rect;
                }
                if image.color.alpha() == 0.0 {
                    image.color.set_alpha(1.0);
                }
            }
            // A button the skin draws only under the pointer.
            None if image.color.alpha() != 0.0 => image.color.set_alpha(0.0),
            None => (),
        }
    }
}

/// How far a window border reaches in from each edge: its sides' widths
/// and its top's and bottom's heights.
fn border_insets(border: &eq_client_assets::sidl::Border) -> frame::Insets {
    let size = |piece: &Option<Piece>| {
        piece.as_ref().map_or((0.0, 0.0), |piece| {
            (to_f32(piece.width), to_f32(piece.height))
        })
    };
    frame::Insets {
        left: size(&border.left).0,
        top: size(&border.top).1,
        right: size(&border.right).0,
        bottom: size(&border.bottom).1,
    }
}

/// The border's corners and its edges stretched between them; what lies
/// inside it is the window's.
pub(crate) fn border(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    border: &eq_client_assets::sidl::Border,
    (width, height): (f32, f32),
) -> Area {
    let frame::Insets {
        left,
        top,
        right,
        bottom,
    } = border_insets(border);
    let (across, down) = (width - left - right, height - top - bottom);
    for (piece, (x, y, stretch_x, stretch_y)) in [
        (&border.top_left, (0.0, 0.0, None, None)),
        (&border.top, (left, 0.0, Some(across), None)),
        (&border.top_right, (width - right, 0.0, None, None)),
        (&border.left, (0.0, top, None, Some(down))),
        (&border.right, (width - right, top, None, Some(down))),
        (&border.bottom_left, (0.0, height - bottom, None, None)),
        (&border.bottom, (left, height - bottom, Some(across), None)),
        (
            &border.bottom_right,
            (width - right, height - bottom, None, None),
        ),
    ] {
        if let Some(piece) = piece {
            let size = (
                stretch_x.unwrap_or(to_f32(piece.width)),
                stretch_y.unwrap_or(to_f32(piece.height)),
            );
            picture(window, art, piece, at(x, y, size.0, size.1));
        }
    }
    Area {
        x: left,
        y: top,
        width: across,
        height: down,
    }
}

/// The title bar along the top of what lies inside the border, which then
/// begins below it, with the window's title on it where the skin gives one.
fn title(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    (screen, template, bar): (
        &Screen,
        &eq_client_assets::sidl::WindowTemplate,
        eq_client_assets::sidl::TitleBar,
    ),
    inside: &mut Area,
    owner: WindowId,
) {
    let [left, middle, right] = &template.title;
    let Some(middle) = middle else {
        return;
    };
    let tall = to_f32(middle.height);
    let wide = |piece: &Option<Piece>| piece.as_ref().map_or(0.0, |piece| to_f32(piece.width));
    let (left_width, right_width) = (wide(left), wide(right));
    if let Some(piece) = left {
        picture(window, art, piece, at(inside.x, inside.y, left_width, tall));
    }
    let middle_width = inside.width - left_width - right_width;
    picture(
        window,
        art,
        middle,
        at(inside.x + left_width, inside.y, middle_width, tall),
    );
    // Where the official client writes it on the bar is not checked yet:
    // after the bar's left end, in the middle of its height.
    let node = Node {
        align_items: AlignItems::Center,
        ..at(inside.x + left_width, inside.y, middle_width, tall)
    };
    let ink = screen.title_color.map_or(theme::INK_BRIGHT, rgb);
    if let Some(words) = &screen.title {
        let text = theme::text(words.as_str(), font(screen.font), ink);
        aligned(window, node, Align::Left, text);
    } else if owner == WindowId::Item {
        // The item display gives no words of its own: the item's name goes
        // there (`items::update`).
        let text = (
            theme::text("", font(screen.font), ink),
            super::items::ItemTitle,
        );
        aligned(window, node, Align::Left, text);
    }
    if let Some(piece) = right {
        let x = inside.x + inside.width - right_width;
        picture(window, art, piece, at(x, inside.y, right_width, tall));
    }
    title_boxes(window, art, (template, bar), (inside, tall), owner);
    inside.y += tall;
    inside.height -= tall;
}

/// The title bar's close and minimize boxes, where the skin's window has
/// them: at the bar's right end, the close box rightmost, each in the
/// middle of the bar's height. Where the official client puts them is not
/// checked yet. A close box closes the window as its Done button would;
/// on a window this client keeps open, it stays greyed.
fn title_boxes(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    (template, bar): (
        &eq_client_assets::sidl::WindowTemplate,
        eq_client_assets::sidl::TitleBar,
    ),
    (inside, tall): (&Area, f32),
    owner: WindowId,
) {
    let frame = window.target_entity();
    let closes =
        owner.describe().toggle != crate::windows::Toggle::Never || items::framed_while_open(owner);
    let mut right = inside.x + inside.width;
    for (wanted, look, close) in [
        (bar.close_box, &template.close_box, true),
        (bar.minimize_box, &template.minimize_box, false),
    ] {
        let Some((look, normal)) = look
            .as_ref()
            .filter(|_| wanted)
            .and_then(|look| Some((look, look.normal.as_ref()?)))
        else {
            continue;
        };
        let (width, height) = (to_f32(normal.width), to_f32(normal.height));
        right -= width;
        let node = at(
            right,
            inside.y + (tall - height).max(0.0) / 2.0,
            width,
            height,
        );
        let Some(image) = art.cut(normal) else {
            continue;
        };
        let mut drawn = window.spawn((
            image,
            node,
            Button,
            SkinButton(look.clone()),
            TitleBox {
                window: owner,
                close,
            },
        ));
        match (close, closes) {
            // The loot and merchant windows close as their Done buttons do,
            // ending the loot or the shopping.
            (true, _) if owner == WindowId::Loot => drawn.insert(crate::trade::Action::EndLoot),
            (true, _) if owner == WindowId::Merchant => drawn.insert(crate::trade::Action::EndShop),
            // The item display closes as the client's own panel's Close does.
            (true, _) if owner == WindowId::Item => drawn.insert(super::items::CloseItem),
            (true, true) => drawn.insert(items::Closes(owner)),
            // The client keeps this window open: its close box is one more
            // control the client does not have yet.
            (true, false) => drawn.insert((Greyed, missing())),
            (false, _) => drawn.insert(super::windows::Minimize(frame)),
        };
    }
}

/// Whose a gauge is, for one that shows only while its owner is there, by
/// its number (`EQType`): 11 to 15 the group members' in their places, 16
/// the player's pet's, 17 to 21 the members' pets'.
const fn owned_gauge(kind: u32) -> Option<Shows> {
    match kind {
        11..=15 => Some(Shows::Member((kind - 11) as usize)),
        16 => Some(Shows::Pet),
        17..=21 => Some(Shows::MemberPet((kind - 17) as usize)),
        _ => None,
    }
}

/// Where a gauge's bar and its ends lie in the gauge, each piece at its own
/// size from where the skin starts the bar: the left end, then the bar, as
/// long as its background (or its fill, without one), then the right end.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BarParts {
    left: Option<Area>,
    bar: Area,
    right: Option<Area>,
}

/// Lays out a gauge's bar. The pieces keep their own sizes rather than
/// stretching to the gauge (inferred): the skins fit their gauges to them.
/// The default skin's gauges are exactly as wide as their ends and bar, and
/// its pet line, as wide as the hit point gauge but with no ends, lies under
/// that bar's middle only at its pieces' size. The Velious skin's hit point
/// background sits in a gauge wider than its window, and only at its own
/// size does it end where the bar's fill does.
fn bar_parts(gauge: &Gauge) -> BarParts {
    let look = &gauge.look;
    let (x, y) = (gauge.bar_left, gauge.bar_offset);
    let sized = |piece: &Piece, x: f32| Area {
        x,
        y,
        width: to_f32(piece.width),
        height: to_f32(piece.height),
    };
    let left = look.cap_left.as_ref().map(|cap| sized(cap, x));
    let start = x + left.map_or(0.0, |cap| cap.width);
    let pieces = [&look.background, &look.fill, &look.lines];
    let mut middle = pieces.iter().copied().flatten();
    let bar = Area {
        x: start,
        y,
        width: middle.next().map_or(0.0, |piece| to_f32(piece.width)),
        height: pieces
            .iter()
            .copied()
            .flatten()
            .map(|piece| to_f32(piece.height))
            .fold(0.0, f32::max),
    };
    let right = look
        .cap_right
        .as_ref()
        .map(|cap| sized(cap, start + bar.width));
    BarParts { left, bar, right }
}

/// A step of a bar the skin builds from gauges stacked over one another,
/// each its own colour: one whose fill starts left of its box and is wider
/// than the box, as each part of the Velious skin's hit point bars past its
/// first fifth is. Its fill shows whole, from the bar's start to the level,
/// only while the level ends past the box's left edge, so the highest step
/// the level reaches colours the whole filled part (inferred from how the
/// project owner saw the official client; a recording item). Decided from
/// the gauge alone.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Step {
    /// How far along the bar the box starts, as a fraction of the bar.
    from: f32,
}

impl Step {
    /// The step a gauge is, if it is one.
    fn of(gauge: &Gauge, parts: &BarParts) -> Option<Self> {
        let fill = gauge.look.fill.as_ref()?;
        (parts.bar.x < 0.0 && to_f32(fill.width) > gauge.area.width).then_some(Self {
            from: -parts.bar.x / parts.bar.width,
        })
    }

    /// Whether the fill shows at this fraction: once the level ends past
    /// the box's left edge. Both are fractions of the bar, so at an exact
    /// step they are equal and the lower colour shows.
    fn shows(self, fraction: f32) -> bool {
        fraction > self.from
    }
}

/// A gauge: its text, then its bar, filled as far as its fraction. The
/// gauge cuts off whatever lies outside it, as the skins expect (inferred):
/// both skins move the pet gauge's text out of its two pixel high box. A
/// [`Step`]'s fill is the one thing that crosses its box's sides, drawn
/// between what lies under it and over it, both still cut.
fn gauge(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    gauge: &Gauge,
    inside: &Area,
) {
    let area = gauge.area;
    let parts = bar_parts(gauge);
    let step = Step::of(gauge, &parts);
    let mut root = window.spawn(Node {
        overflow: if step.is_some() {
            Overflow::clip_y()
        } else {
            Overflow::clip()
        },
        ..at(
            inside.x + area.x,
            inside.y + area.y,
            area.width,
            area.height,
        )
    });
    if let Some(shows) = gauge.eq_type.and_then(owned_gauge) {
        root.insert((shows, Visibility::Hidden));
    }
    root.with_children(|root| {
        if step.is_none() {
            gauge_layer(root, art, gauge, &parts, Layer::Whole);
            return;
        }
        let cut = || Node {
            overflow: Overflow::clip(),
            ..at(0.0, 0.0, area.width, area.height)
        };
        root.spawn(cut())
            .with_children(|under| gauge_layer(under, art, gauge, &parts, Layer::Under));
        gauge_layer(root, art, gauge, &parts, Layer::Fill(step));
        root.spawn(cut())
            .with_children(|over| gauge_layer(over, art, gauge, &parts, Layer::Over));
    });
}

/// The pieces of a gauge one drawing pass puts down.
#[derive(Clone, Copy)]
enum Layer {
    /// Everything, in order: the text, the background, the fill, the lines
    /// and the ends.
    Whole,
    /// The text and the background.
    Under,
    /// The fill alone, a step's.
    Fill(Option<Step>),
    /// The lines and the ends.
    Over,
}

/// Puts down a gauge's pieces of this layer, each where the skin starts it.
fn gauge_layer(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    gauge: &Gauge,
    parts: &BarParts,
    layer: Layer,
) {
    let look = &gauge.look;
    let natural = |piece: &Piece| at(0.0, 0.0, to_f32(piece.width), to_f32(piece.height));
    let placed = |place: Area| at(place.x, place.y, place.width, place.height);
    let (under, fill, over) = match layer {
        Layer::Whole => (true, Some(None), true),
        Layer::Under => (true, None, false),
        Layer::Fill(step) => (false, Some(step), false),
        Layer::Over => (false, None, true),
    };
    if under && let Some(kind) = gauge.eq_type {
        let ink = gauge.text_color.map_or(theme::INK_BRIGHT, rgb);
        parent.spawn((
            Shows::GaugeText(kind),
            theme::text("", Size::Small, ink),
            at(
                gauge.text_offset.0,
                gauge.text_offset.1,
                gauge.area.width,
                12.0,
            ),
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
        ));
    }
    parent.spawn(placed(parts.bar)).with_children(|bar| {
        if under && let Some(piece) = &look.background {
            picture(bar, art, piece, natural(piece));
        }
        if let (Some(step), Some(piece)) = (fill, &look.fill) {
            let mut node = bar.spawn((
                gauge.eq_type.map_or(Shows::Fill(0), Shows::Fill),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: percent(0),
                    height: px(piece.height),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            if let Some(step) = step {
                node.insert(step);
            }
            node.with_children(|fill| {
                if let Some(mut image) = art.cut(piece) {
                    image.color = gauge.fill_tint.map_or(Color::WHITE, rgb);
                    fill.spawn((image, natural(piece)));
                }
            });
        }
        if over && let Some(piece) = &look.lines {
            picture(bar, art, piece, natural(piece));
        }
    });
    if over {
        for (piece, place) in [(&look.cap_left, parts.left), (&look.cap_right, parts.right)] {
            if let (Some(piece), Some(place)) = (piece, place) {
                picture(parent, art, piece, placed(place));
            }
        }
    }
}

/// What a loot, merchant or spellbook window's label shows: the corpse's
/// or the merchant's name, the chosen item's name or price, or a spell's
/// name or a page's number on the spellbook's open pages.
fn window_label(owner: WindowId, name: &str) -> Option<Shows> {
    if owner == WindowId::Spellbook {
        return match name {
            "SBW_LeftPageNum" => Some(Shows::BookPage(false)),
            "SBW_RightPageNum" => Some(Shows::BookPage(true)),
            _ => name
                .strip_prefix("SBW_SpellName")
                .and_then(|place| place.parse::<u8>().ok())
                .filter(|place| usize::from(*place) < crate::spellbook::PLACES)
                .map(Shows::BookName),
        };
    }
    if owner == WindowId::Raid {
        return match name {
            "RAID_PlayerCountLabel" => Some(Shows::RaidCount),
            "RAID_LevelAverageLabel" => Some(Shows::RaidLevel),
            _ => None,
        };
    }
    if owner == WindowId::Group {
        return name
            .strip_prefix("GW_HPPercLabel")
            .and_then(|place| place.parse::<usize>().ok())
            .filter(|place| (1..=5).contains(place))
            .map(|place| Shows::MemberPercent(place - 1));
    }
    Some(match (owner, name) {
        (WindowId::Loot, "LW_CorpseName") => Shows::Corpse,
        (WindowId::Merchant, "MW_MerchantName") => Shows::Merchant,
        (WindowId::Merchant, "MW_SelectedItemLabel") => Shows::ChosenName,
        (WindowId::Merchant, "MW_SelectedPriceLabel") => Shows::ChosenPrice,
        _ => return None,
    })
}

/// A label: fixed text, or what its number says it shows; a bag's window
/// names its bag.
fn label(
    window: &mut ChildSpawnerCommands,
    name: &str,
    label: &Label,
    inside: &Area,
    owner: WindowId,
) {
    let area = label.anchors.map_or(label.area, |anchors| {
        anchors.within(inside.width, inside.height)
    });
    let bag = match owner {
        WindowId::Bag(bag) if name == "Container_Label" => {
            Some(items::BagPart::Name(InventorySlot(bag)))
        }
        WindowId::WorldContainer if name == "Container_Label" => Some(items::BagPart::WorldName),
        _ => None,
    };
    let banker = owner == WindowId::Bank && name == "BW_BankerName";
    let partner = matches!(
        (owner, name),
        (WindowId::Give, "GVW_NPCName") | (WindowId::Trade, "TRDW_HisName")
    );
    // The player's own name in the trade window, lit once they click Trade.
    let trader = owner == WindowId::Trade && name == "TRDW_MyName";
    let named = window_label(owner, name);
    // The Training window's practice points and the coins the player carries.
    let counted = match name {
        "TRNW_PracticeCount" if owner == WindowId::Training => Some(Shows::PracticePoints),
        _ if owner == WindowId::Training => name
            .strip_prefix("TRNW_CoinCount")
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| Coin::ALL.get(index))
            .map(|coin| Shows::Coins(CoinPlace::Purse, *coin)),
        _ => None,
    };
    // The book window's page numbers.
    let page_number = match (owner, name) {
        (WindowId::Book, "BOOK_Page0Number") => Some(0),
        (WindowId::Book, "BOOK_Page1Number") => Some(1),
        _ => None,
    };
    let filled = label.eq_type.is_some()
        || bag.is_some()
        || banker
        || partner
        || trader
        || named.is_some()
        || counted.is_some()
        || page_number.is_some();
    let words = if filled { "" } else { label.text.as_str() };
    let text = theme::text(
        words,
        font(label.font),
        label.color.map_or(theme::INK_BRIGHT, rgb),
    );
    let mut node = at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    );
    // Wrapping words in a box stretching down the window start at its top.
    if line_at_top(label.anchors, label.wraps) {
        node.height = Val::Auto;
    }
    let buff_name = buff_label(owner, label.eq_type);
    match (label.eq_type, bag) {
        (Some(_), _) if let Some((effects, index)) = buff_name => aligned(
            window,
            node,
            label.align,
            (text, Shows::BuffName(effects, index)),
        ),
        (Some(kind), _) if let Some(gem) = gem_label(kind) => {
            aligned(window, node, label.align, (text, Shows::GemName(gem)));
        }
        (Some(kind), _) => aligned(window, node, label.align, (text, Shows::Label(kind))),
        (None, Some(bag)) => aligned(window, node, label.align, (text, bag)),
        (None, None) if banker => aligned(window, node, label.align, (text, Shows::Banker)),
        (None, None) if let Some(side) = page_number => {
            aligned(
                window,
                node,
                label.align,
                (text, super::reading::Text::Number(side)),
            );
        }
        (None, None) if let Some(shows) = counted => {
            aligned(window, node, label.align, (text, shows));
        }
        (None, None) if partner => aligned(window, node, label.align, (text, Shows::Partner)),
        (None, None) if trader => aligned(window, node, label.align, (text, Shows::Trader)),
        (None, None) if let Some(shows) = named => {
            aligned(window, node, label.align, (text, shows));
        }
        (None, None) => match controls::value_label(name, owner) {
            Some(controls::ValueLabel::Shows(level)) => aligned(
                window,
                node,
                label.align,
                (text, controls::LevelValue(level)),
            ),
            Some(controls::ValueLabel::Blank) => aligned(
                window,
                node,
                label.align,
                theme::text("", font(label.font), theme::INK_DIM),
            ),
            None => aligned(window, node, label.align, text),
        },
    }
}

/// Whether a label's text sits at the top of its box: one whose words wrap
/// in a box stretching down its window, from its top to near its foot, as
/// the merchant window's name, which the skins follow with the list just
/// under that top (inferred, recording item 6.220). Any other box,
/// including a one-line row filling its container, centres its line.
fn line_at_top(anchors: Option<Anchors>, wraps: bool) -> bool {
    wraps
        && anchors.is_some_and(|anchors| {
            matches!(
                (anchors.top, anchors.bottom),
                (Anchor::Start(_), Anchor::End(_))
            )
        })
}

/// Text in a box the skin sizes, aligned in it as the skin aligns it, on
/// one line in the middle of the box's height.
pub(crate) fn aligned(
    window: &mut ChildSpawnerCommands,
    mut node: Node,
    align: Align,
    text: impl Bundle,
) {
    node.justify_content = match align {
        Align::Left => JustifyContent::FlexStart,
        Align::Center => JustifyContent::Center,
        Align::Right => JustifyContent::FlexEnd,
    };
    // The one line sits in the middle of the box's height, so a box taller
    // than its text, such as a gem's name row, doesn't leave it at the top
    // (inferred, recording item 6.220).
    node.align_items = AlignItems::Center;
    node.overflow = Overflow::clip();
    window
        .spawn(node)
        .with_child((text, TextLayout::new(Justify::Left, LineBreak::NoWrap)));
}

/// A piece of the skin, stretched over this node.
pub(crate) fn picture(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    piece: &Piece,
    node: Node,
) {
    if let Some(image) = art.cut(piece) {
        parent.spawn((image, node));
    }
}

/// A node at this place and size in its parent.
pub(crate) fn at(x: f32, y: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(width.max(0.0)),
        height: px(height.max(0.0)),
        ..default()
    }
}

/// The client's size for one of the official client's fonts.
pub(crate) const fn font(number: Option<u8>) -> Size {
    match number {
        None | Some(0..=1) => Size::Small,
        Some(2) => Size::Body,
        Some(3) => Size::Label,
        Some(_) => Size::Heading,
    }
}

pub(crate) fn rgb([red, green, blue]: [u8; 3]) -> Color {
    Color::srgb_u8(red, green, blue)
}

#[allow(clippy::cast_precision_loss, reason = "texture sizes are small")]
const fn to_f32(value: u32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_bar_shows_the_title_the_skin_gives_its_window() {
        let middle = Piece {
            texture: "title.tga".into(),
            x: 0,
            y: 0,
            width: 8,
            height: 14,
        };
        let screen = |titlebar: bool, title: Option<&str>| Screen {
            name: "Window".into(),
            title: title.map(str::to_owned),
            title_color: None,
            font: Some(3),
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 60.0,
            },
            template: Some(eq_client_assets::sidl::WindowTemplate {
                title: [None, Some(middle.clone()), None],
                ..default()
            }),
            title_bar: titlebar.then_some(eq_client_assets::sidl::TitleBar::default()),
            border: false,
            tooltip: None,
            pieces: Vec::new(),
            tab_frame: None,
            transparent: false,
        };
        // The words drawn in a window from this screen.
        let words = |screen: Screen| {
            let mut app = App::new();
            app.init_resource::<crate::sheets::Sheets>()
                .init_resource::<Assets<Image>>()
                .init_resource::<super::super::skin::UiSkin>()
                .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
                .add_systems(
                    Update,
                    move |mut commands: Commands, mut art: crate::sheets::Art| {
                        let context = Context {
                            id: WindowId::ActionsWindow,
                            paperdoll: None,
                            depth: 0,
                            tab_frame: None,
                        };
                        commands.spawn(Node::default()).with_children(|window| {
                            draw(window, &screen, &mut art, &context);
                        });
                    },
                );
            app.update();
            let mut texts = app.world_mut().query::<&Text>();
            texts
                .iter(app.world())
                .map(|text| text.0.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(words(screen(true, Some("Sample"))), ["Sample"]);
        // Without a title bar, or words for it, there is no title.
        assert_eq!(words(screen(false, Some("Sample"))), Vec::<String>::new());
        assert_eq!(words(screen(true, None)), Vec::<String>::new());
        // A window the skin sizes to nothing shows nothing.
        let hidden = Screen {
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            ..screen(true, Some("Sample"))
        };
        assert_eq!(words(hidden), Vec::<String>::new());
    }

    #[test]
    fn an_unplaced_window_opens_where_the_skin_puts_it_unless_it_says_otherwise() {
        let screen = Screen {
            name: "Window".into(),
            title: None,
            title_color: None,
            font: None,
            area: Area {
                x: 120.0,
                y: 80.0,
                width: 200.0,
                height: 150.0,
            },
            template: None,
            title_bar: None,
            border: false,
            tooltip: None,
            pieces: Vec::new(),
            tab_frame: None,
            transparent: false,
        };
        let skin = (px(120.0), px(80.0));
        let at = |id, placed| {
            let mut node = super::super::windows::placed(id, Node::default());
            reshape(&mut node, &screen, placed, (id, None));
            (node.left, node.top)
        };
        // Pop-ups open at the skin's place as the HUD does.
        for id in [
            WindowId::Give,
            WindowId::Merchant,
            WindowId::Inventory,
            WindowId::Player,
        ] {
            assert_eq!(at(id, false), skin, "{id:?}");
        }
        // A window the player or the UI file placed stays there.
        assert_ne!(at(WindowId::Give, true), skin);
        // A bag keeps its own place until the official client's is known.
        assert_ne!(at(WindowId::Bag(22), false), skin);
        // The client's arrangement, for a panel it places, comes before the
        // skin's place, and a placed window stays where it was put.
        let arranged = |placed| {
            let mut node = Node::default();
            let place = super::super::windows::Placement::TopRight(0.0, 0.0);
            reshape(&mut node, &screen, placed, (WindowId::Effects, Some(place)));
            (node.left, node.right, node.top)
        };
        assert_eq!(arranged(false), (Val::Auto, px(0.0), px(0.0)));
        assert_eq!(arranged(true), (Val::Auto, Val::Auto, Val::Auto));
    }

    #[test]
    fn the_status_panel_moves_clear_of_the_skins_hud() {
        let mut app = App::new();
        app.init_resource::<Skinned>()
            .init_resource::<crate::options::OptionsState>()
            .add_systems(Update, place_own);
        let opens = super::super::windows::placed(WindowId::Status, Node::default());
        let status = app
            .world_mut()
            .spawn((
                WindowId::Status,
                opens.clone(),
                crate::windows::Frame::default(),
            ))
            .id();
        app.update();
        // The client's own HUD keeps it where the registry opens it.
        assert_eq!(app.world().get::<Node>(status), Some(&opens));
        app.insert_resource(Skinned::of(&[WindowId::Player]));
        app.update();
        let node = app.world().get::<Node>(status).unwrap();
        assert_eq!(
            (node.left, node.top, node.right, node.bottom),
            (Val::Auto, Val::Auto, px(0.0), px(0.0))
        );
    }

    #[test]
    fn the_hot_button_windows_buttons_hold_the_action_bars_slots() {
        assert!(matches!(
            does("HB_Button1", WindowId::Actions),
            Some(Does::HotButton(0))
        ));
        assert!(matches!(
            does("HB_Button10", WindowId::Actions),
            Some(Does::HotButton(9))
        ));
        // One page so far: the page buttons are greyed out.
        for id in [
            "HB_PageLeftButton",
            "HB_PageRightButton",
            "HB_Button11",
            "HB_Button0",
        ] {
            assert!(
                matches!(does(id, WindowId::Actions), Some(Does::Nothing)),
                "{id}"
            );
        }
    }

    #[test]
    fn the_actions_windows_buttons_run_what_their_hotbuttons_run_and_a_hold_picks_them_up() {
        use eq_client_assets::sidl::{Button, ButtonLook, Element};
        let kinds = [
            ("ACP_MeleeAttackButton", Binding::Attack),
            ("AMP_SitButton", Binding::Sit),
            ("AMP_StandButton", Binding::Stand),
            ("AMP_CampButton", Binding::Camp),
            ("AMP_InviteButton", Binding::Invite),
            ("AMP_FollowButton", Binding::Follow),
            ("AMP_DisbandButton", Binding::Disband),
        ];
        let button = |id: &str| {
            let area = Area {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            };
            Element::Button(Button {
                id: Some(id.into()),
                area,
                placed: true,
                anchors: None,
                look: ButtonLook::default(),
                checkbox: false,
                text: None,
                text_color: None,
                font: None,
                decal: None,
                decal_area: None,
                tooltip: None,
            })
        };
        let screen = Screen {
            name: "ActionsWindow".into(),
            title: None,
            title_color: None,
            font: None,
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 140.0,
                height: 30.0,
            },
            template: None,
            title_bar: None,
            border: false,
            tooltip: None,
            pieces: kinds
                .iter()
                .map(|(id, _)| ((*id).to_owned(), button(id)))
                .collect(),
            tab_frame: None,
            transparent: false,
        };
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(
                Update,
                move |mut commands: Commands, mut art: crate::sheets::Art| {
                    let context = Context {
                        id: WindowId::ActionsWindow,
                        paperdoll: None,
                        depth: 0,
                        tab_frame: None,
                    };
                    commands.spawn(Node::default()).with_children(|window| {
                        draw(window, &screen, &mut art, &context);
                    });
                },
            );
        app.update();
        let mut controls = app.world_mut().query::<(
            &hotbar::Pickable,
            &crate::outbox::Needs,
            Option<&SlashButton>,
            Has<AttackButton>,
        )>();
        let mut drawn = Vec::new();
        for (pickable, needs, slash, attack) in controls.iter(app.world()) {
            let hotbar::Source::Fixed(binding) = pickable.0 else {
                panic!("{pickable:?}");
            };
            // Each runs what its hotbutton runs, and is veiled as it is.
            assert_eq!(*needs, hotbar::need(Some(binding)), "{binding:?}");
            assert_eq!(slash.map(|slash| slash.0), hotbar::command(binding));
            assert_eq!(attack, binding == Binding::Attack, "{binding:?}");
            drawn.push(format!("{binding:?}"));
        }
        let mut wanted: Vec<_> = kinds.iter().map(|(_, b)| format!("{b:?}")).collect();
        drawn.sort();
        wanted.sort();
        assert_eq!(drawn, wanted);
    }

    #[test]
    fn a_hot_buttons_pictures_fill_where_the_skin_puts_its_decal() {
        use eq_client_assets::sidl::{Button, ButtonLook, Element};
        // As the stock skins' HB_Button1: 40 by 40, its decal the same.
        let whole = Area {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        };
        let screen = Screen {
            name: "HotButtonWnd".into(),
            title: None,
            title_color: None,
            font: None,
            area: whole,
            template: None,
            title_bar: None,
            border: false,
            tooltip: None,
            pieces: vec![(
                "slot".into(),
                Element::Button(Button {
                    id: Some("HB_Button1".into()),
                    area: whole,
                    placed: true,
                    anchors: None,
                    look: ButtonLook::default(),
                    checkbox: false,
                    text: None,
                    text_color: None,
                    font: None,
                    decal: None,
                    decal_area: Some(whole),
                    tooltip: None,
                }),
            )],
            tab_frame: None,
            transparent: false,
        };
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(
                Update,
                move |mut commands: Commands, mut art: crate::sheets::Art| {
                    let context = Context {
                        id: WindowId::Actions,
                        paperdoll: None,
                        depth: 0,
                        tab_frame: None,
                    };
                    commands.spawn(Node::default()).with_children(|window| {
                        draw(window, &screen, &mut art, &context);
                    });
                },
            );
        app.update();
        // The spell's icon and the item's picture, each the decal's size at
        // its place.
        let mut pictures = app
            .world_mut()
            .query_filtered::<&Node, (With<ImageNode>, With<bevy::ui::FocusPolicy>)>();
        let placed: Vec<_> = pictures
            .iter(app.world())
            .filter(|node| node.position_type == PositionType::Absolute)
            .map(|node| (node.left, node.top, node.width, node.height))
            .collect();
        assert_eq!(placed, [(px(0), px(0), px(40), px(40)); 2]);
    }

    #[test]
    fn a_button_the_client_does_not_have_yet_says_so_on_hover() {
        use crate::outbox::Needs;
        use eq_client_assets::sidl::{Button, ButtonLook, Element};
        let button = |id: &str, x| {
            Element::Button(Button {
                id: Some(id.into()),
                area: Area {
                    x,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
                placed: true,
                anchors: None,
                look: ButtonLook::default(),
                checkbox: false,
                text: None,
                text_color: None,
                font: None,
                decal: None,
                decal_area: None,
                tooltip: Some("Sample".into()),
            })
        };
        let screen = Screen {
            name: "HotButtonWnd".into(),
            title: None,
            title_color: None,
            font: None,
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 30.0,
            },
            template: None,
            title_bar: None,
            border: false,
            tooltip: None,
            pieces: vec![
                ("page".into(), button("HB_PageLeftButton", 0.0)),
                ("slot".into(), button("HB_Button1", 20.0)),
            ],
            tab_frame: None,
            transparent: false,
        };
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(
                Update,
                move |mut commands: Commands, mut art: crate::sheets::Art| {
                    let context = Context {
                        id: WindowId::Actions,
                        paperdoll: None,
                        depth: 0,
                        tab_frame: None,
                    };
                    commands.spawn(Node::default()).with_children(|window| {
                        draw(window, &screen, &mut art, &context);
                    });
                },
            );
        app.update();
        let mut controls = app.world_mut().query::<(
            &Needs,
            Has<Interaction>,
            Has<crate::tooltip::Tooltip>,
            Has<bevy::prelude::Button>,
        )>();
        let drawn: Vec<_> = controls
            .iter(app.world())
            .map(|(needs, hovers, tooltip, button)| (*needs, hovers, tooltip, button))
            .collect();
        // The page button the client lacks gives its reason on hover in
        // place of the skin's tooltip, and is a button, which keeps a press
        // from dragging its window; the slot beside it works and keeps its
        // own tooltip.
        assert_eq!(drawn.len(), 2);
        assert!(drawn.contains(&(Needs::Missing, true, false, true)));
        assert!(drawn.contains(&(Needs::Nothing, true, true, true)));
        // Each keeps what the skin's file calls it, as a script names it.
        let mut skins = app.world_mut().query::<&SkinId>();
        let mut names: Vec<_> = skins
            .iter(app.world())
            .map(|skin| (skin.window, skin.id.clone()))
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                (WindowId::Actions, "HB_Button1".to_owned()),
                (WindowId::Actions, "HB_PageLeftButton".to_owned()),
            ]
        );
    }

    #[test]
    fn a_containers_augment_section_is_not_drawn() {
        use eq_client_assets::sidl::{Button, ButtonLook, Element};
        let button = |id: &str, x| {
            Element::Button(Button {
                id: Some(id.into()),
                area: Area {
                    x,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
                placed: true,
                anchors: None,
                look: ButtonLook::default(),
                checkbox: false,
                text: None,
                text_color: None,
                font: None,
                decal: None,
                decal_area: None,
                tooltip: None,
            })
        };
        let screen = Screen {
            name: "Container".into(),
            title: None,
            title_color: None,
            font: None,
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 30.0,
            },
            template: None,
            title_bar: None,
            border: false,
            tooltip: None,
            pieces: vec![
                ("combine".into(), button("Container_Combine", 0.0)),
                ("insert".into(), button("InsertButton", 20.0)),
                ("delete".into(), button("DeleteButton", 40.0)),
            ],
            tab_frame: None,
            transparent: false,
        };
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(
                Update,
                move |mut commands: Commands, mut art: crate::sheets::Art| {
                    for id in [
                        WindowId::Bag(23),
                        WindowId::WorldContainer,
                        WindowId::Actions,
                    ] {
                        let context = Context {
                            id,
                            paperdoll: None,
                            depth: 0,
                            tab_frame: None,
                        };
                        commands.spawn(Node::default()).with_children(|window| {
                            draw(window, &screen, &mut art, &context);
                        });
                    }
                },
            );
        app.update();
        let mut skins = app.world_mut().query::<&SkinId>();
        let mut names: Vec<_> = skins
            .iter(app.world())
            .map(|skin| (skin.window, skin.id.clone()))
            .collect();
        names.sort();
        // A bag and a world container keep Combine and draw no augment
        // button; another window with buttons of those names draws them.
        assert_eq!(
            names,
            [
                (WindowId::Actions, "Container_Combine".to_owned()),
                (WindowId::Actions, "DeleteButton".to_owned()),
                (WindowId::Actions, "InsertButton".to_owned()),
                (WindowId::Bag(23), "Container_Combine".to_owned()),
                (WindowId::WorldContainer, "Container_Combine".to_owned()),
            ]
        );
    }

    #[test]
    fn a_containers_augment_labels_go_with_its_buttons() {
        let label = Element::Label(eq_client_assets::sidl::Label {
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            anchors: None,
            eq_type: None,
            text: String::new(),
            color: None,
            align: eq_client_assets::sidl::Align::Left,
            font: None,
            wraps: false,
        });
        assert!(augment_section(
            WindowId::Bag(23),
            "AugmentInfoLabel",
            &label
        ));
        assert!(augment_section(
            WindowId::WorldContainer,
            "Augment_Label",
            &label
        ));
        assert!(!augment_section(
            WindowId::Bag(23),
            "Container_Label",
            &label
        ));
        assert!(!augment_section(
            WindowId::Merchant,
            "Augment_Label",
            &label
        ));
    }

    /// An Options window with a checkbox the client keeps nothing for, one it
    /// keeps, and the key list with its scrollbar.
    fn options_fixture() -> Screen {
        use eq_client_assets::sidl::{Button, ButtonLook, Column, Element, Listbox, ScrollbarLook};
        let checkbox = |id: &str, x| {
            Element::Button(Button {
                id: Some(id.into()),
                area: Area {
                    x,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
                placed: true,
                anchors: None,
                look: ButtonLook::default(),
                checkbox: true,
                text: None,
                text_color: None,
                font: None,
                decal: None,
                decal_area: None,
                tooltip: None,
            })
        };
        let keys = Element::Listbox(Listbox {
            id: Some("OKP_KeyboardAssignmentList".into()),
            area: Some(Area {
                x: 0.0,
                y: 30.0,
                width: 200.0,
                height: 100.0,
            }),
            anchors: None,
            template: None,
            columns: vec![Column {
                heading: "Command".into(),
                width: 120.0,
                header: None,
            }],
            scrollbar: Some(ScrollbarLook::default()),
            font: None,
            header: None,
        });
        Screen {
            name: "OptionsWindow".into(),
            title: None,
            title_color: None,
            font: None,
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 140.0,
            },
            template: None,
            title_bar: None,
            border: false,
            tooltip: None,
            pieces: vec![
                ("missing".into(), checkbox("ODP_LevelOfDetailCheckbox", 0.0)),
                ("ring".into(), checkbox("ODP_ShowTargetRingCheckbox", 30.0)),
                ("keys".into(), keys),
            ],
            tab_frame: None,
            transparent: false,
        }
    }

    /// A quantity window with its slider and number box.
    fn quantity_fixture() -> Screen {
        use eq_client_assets::sidl::{Element, Slider, SliderLook, TextBox};
        let piece = |x| Area {
            x,
            y: 0.0,
            width: 60.0,
            height: 20.0,
        };
        Screen {
            name: "QuantityWnd".into(),
            pieces: vec![
                (
                    "QTYW_Slider".into(),
                    Element::Slider(Slider {
                        id: Some("QTYW_Slider".into()),
                        area: piece(0.0),
                        look: SliderLook::default(),
                    }),
                ),
                (
                    "QTYW_SliderInput".into(),
                    Element::TextBox(TextBox {
                        id: Some("QTYW_SliderInput".into()),
                        area: piece(70.0),
                        anchors: None,
                        template: None,
                        color: None,
                        font: None,
                        scrollbar: None,
                    }),
                ),
            ],
            ..options_fixture()
        }
    }

    #[test]
    fn a_window_the_skin_makes_transparent_draws_no_background() {
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(
                Startup,
                |mut commands: Commands, mut art: crate::sheets::Art| {
                    for transparent in [false, true] {
                        let screen = Screen {
                            transparent,
                            ..options_fixture()
                        };
                        let context = Context {
                            id: WindowId::Options,
                            paperdoll: None,
                            depth: 0,
                            tab_frame: None,
                        };
                        commands.spawn(Node::default()).with_children(|window| {
                            draw(window, &screen, &mut art, &context);
                        });
                    }
                },
            );
        app.update();
        // Only the window the skin leaves opaque has a background for the
        // character's saved look to tint and fade.
        let mut backdrops = app.world_mut().query::<&looks::Backdrop>();
        assert_eq!(backdrops.iter(app.world()).count(), 1);
    }

    #[test]
    fn every_part_of_a_skinned_window_a_press_lands_on_keeps_it_from_the_frame() {
        use bevy::ui::FocusPolicy;
        let (screen, quantity) = (options_fixture(), quantity_fixture());
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(
                Update,
                move |mut commands: Commands, mut art: crate::sheets::Art| {
                    let context = Context {
                        id: WindowId::Options,
                        paperdoll: None,
                        depth: 0,
                        tab_frame: None,
                    };
                    commands.spawn(Node::default()).with_children(|window| {
                        draw(window, &screen, &mut art, &context);
                        // A node that only reacts to the pointer, as the
                        // chat's lines do.
                        window.spawn((Node::default(), Interaction::default()));
                    });
                    let context = Context {
                        id: WindowId::Quantity,
                        ..context
                    };
                    commands.spawn(Node::default()).with_children(|window| {
                        draw(window, &quantity, &mut art, &context);
                    });
                },
            )
            .add_systems(PostUpdate, crate::windows::block_clicks);
        app.update();
        // A skinned window's frame drags under a press that reaches it: every
        // part that reacts to the pointer, and the list with its rows and
        // headings, keeps the press. Here: the two checkboxes, the list, its
        // scrollbar's arrows, gutter and thumb, the node that reacts to the
        // pointer, and the quantity window's slider and number box.
        let mut parts = app.world_mut().query_filtered::<&FocusPolicy, Or<(
            With<Interaction>,
            With<crate::windows::KeepsPress>,
        )>>();
        let policies: Vec<_> = parts.iter(app.world()).copied().collect();
        assert_eq!(policies, [FocusPolicy::Block; 10]);
        let mut wired = app
            .world_mut()
            .query_filtered::<(), Or<(With<SkinSlider>, With<controls::AmountBox>)>>();
        assert_eq!(wired.iter(app.world()).count(), 2);
    }

    #[test]
    fn the_loot_windows_done_ends_the_loot_and_its_places_are_the_corpses_slots() {
        use crate::trade::Action;
        assert!(matches!(
            does("DoneButton", WindowId::Loot),
            Some(Does::Trades(Action::EndLoot))
        ));
        // Link all, and the Loot all Velious keeps without a place, are not
        // in this client.
        assert!(matches!(
            does("BroadcastButton", WindowId::Loot),
            Some(Does::Nothing)
        ));
        assert!(matches!(
            does("LootAllButton", WindowId::Loot),
            Some(Does::Nothing)
        ));
        // Elsewhere a Done button closes its window.
        assert!(matches!(
            does("DoneButton", WindowId::Training),
            Some(Does::Closes)
        ));
    }

    #[test]
    fn the_merchant_windows_buttons_and_labels_act_on_what_it_has_chosen() {
        use crate::trade::Action;
        for (id, action) in [
            ("MW_Buy_Button", Action::BuyChosen),
            ("MW_Sell_Button", Action::SellChosen),
            ("MW_Done_Button", Action::EndShop),
        ] {
            assert!(
                matches!(does(id, WindowId::Merchant), Some(Does::Trades(found)) if found == action),
                "{id}"
            );
        }
        assert!(matches!(
            does("MW_SelectedItem", WindowId::Merchant),
            Some(Does::Chosen)
        ));
        assert_eq!(
            window_label(WindowId::Merchant, "MW_SelectedPriceLabel"),
            Some(Shows::ChosenPrice)
        );
        assert_eq!(
            window_label(WindowId::Loot, "LW_CorpseName"),
            Some(Shows::Corpse)
        );
        assert_eq!(window_label(WindowId::Merchant, "MW_Other"), None);
    }

    #[test]
    fn the_character_lists_buttons_are_its_slots_and_its_way_in_and_out() {
        let list = WindowId::CharacterSelect;
        assert!(matches!(
            does("Char1_Button", list),
            Some(Does::CharacterSlot(0))
        ));
        assert!(matches!(
            does("Char8_Button", list),
            Some(Does::CharacterSlot(7))
        ));
        assert!(matches!(does("Char9_Button", list), Some(Does::Nothing)));
        assert!(matches!(
            does("Enter_World_Button", list),
            Some(Does::EntersWorld)
        ));
        assert!(matches!(does("Quit_Button", list), Some(Does::Quits)));
        // What this client lacks yet is greyed.
        for lacking in [
            "Delete_Button",
            "Rotate_Button",
            "Explore_Button",
            "Go_Home_Button",
        ] {
            assert!(
                matches!(does(lacking, list), Some(Does::Nothing)),
                "{lacking}"
            );
        }
    }

    #[test]
    fn the_spellbooks_places_arrows_and_labels_are_its_open_pages() {
        let book = WindowId::Spellbook;
        assert!(matches!(does("SBW_Spell0", book), Some(Does::BookPlace(0))));
        assert!(matches!(
            does("SBW_Spell15", book),
            Some(Does::BookPlace(15))
        ));
        assert!(matches!(does("SBW_Spell16", book), Some(Does::Nothing)));
        assert!(matches!(
            does("SBW_PageUp_Button", book),
            Some(Does::TurnsSpellbook(true))
        ));
        assert!(matches!(
            does("SBW_PageDown_Button", book),
            Some(Does::TurnsSpellbook(false))
        ));
        assert!(matches!(does("DoneButton", book), Some(Does::Closes)));
        // The skin parks these at a pixel's size.
        assert!(does("SBW_MemPage0_Button", book).is_none());
        assert!(does("MeditateButton", book).is_none());
        assert_eq!(
            window_label(book, "SBW_SpellName7"),
            Some(Shows::BookName(7))
        );
        assert_eq!(
            window_label(book, "SBW_LeftPageNum"),
            Some(Shows::BookPage(false))
        );
        assert_eq!(
            window_label(book, "SBW_RightPageNum"),
            Some(Shows::BookPage(true))
        );
        assert_eq!(window_label(book, "SBW_SpellName16"), None);
    }

    #[test]
    fn the_item_displays_icon_button_draws_no_template_under_the_icon() {
        use eq_client_assets::sidl::{Button, ButtonLook, Element, Piece};
        let whole = Area {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        };
        let screen = Screen {
            name: "ItemDisplayWindow".into(),
            title: None,
            title_color: None,
            font: None,
            area: whole,
            template: None,
            title_bar: None,
            border: false,
            tooltip: None,
            pieces: vec![(
                "icon".into(),
                Element::Button(Button {
                    id: Some("IconButton".into()),
                    area: whole,
                    placed: true,
                    anchors: None,
                    look: ButtonLook {
                        normal: Some(Piece {
                            texture: "sample.tga".into(),
                            x: 0,
                            y: 0,
                            width: 40,
                            height: 40,
                        }),
                        ..ButtonLook::default()
                    },
                    checkbox: false,
                    text: None,
                    text_color: None,
                    font: None,
                    decal: None,
                    decal_area: Some(Area {
                        x: 4.0,
                        y: 4.0,
                        width: 32.0,
                        height: 32.0,
                    }),
                    tooltip: None,
                }),
            )],
            tab_frame: None,
            transparent: false,
        };
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(
                Update,
                move |mut commands: Commands, mut art: crate::sheets::Art| {
                    let context = Context {
                        id: WindowId::Item,
                        paperdoll: None,
                        depth: 0,
                        tab_frame: None,
                    };
                    commands.spawn(Node::default()).with_children(|window| {
                        draw(window, &screen, &mut art, &context);
                    });
                },
            );
        app.update();
        let mut boxes = app
            .world_mut()
            .query::<(&crate::items::ItemIcon, Has<ImageNode>)>();
        let drawn: Vec<_> = boxes
            .iter(app.world())
            .map(|(place, image)| (place.x, place.y, place.size, image))
            .collect();
        assert_eq!(drawn, [(0.0, 0.0, 40.0, false)]);
    }

    #[test]
    fn the_item_displays_icon_box_shows_the_items_picture() {
        assert!(matches!(
            does("IconButton", WindowId::Item),
            Some(Does::ItemIcon)
        ));
        assert!(matches!(does("Other", WindowId::Item), Some(Does::Nothing)));
    }

    #[test]
    fn a_text_box_scrolls_its_words_and_shows_new_words_from_the_top() {
        use eq_client_assets::sidl::{ScrollbarLook, TextBox};
        let description = TextBox {
            id: Some("ItemDescription".into()),
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 60.0,
            },
            anchors: None,
            template: None,
            color: None,
            font: None,
            scrollbar: Some(ScrollbarLook::default()),
        };
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(
                Startup,
                move |mut commands: Commands, mut art: crate::sheets::Art| {
                    let inside = Area {
                        x: 0.0,
                        y: 0.0,
                        width: 200.0,
                        height: 60.0,
                    };
                    commands.spawn(Node::default()).with_children(|window| {
                        text_box(window, &mut art, &description, &inside, WindowId::Item);
                    });
                },
            )
            .add_systems(Update, rewind);
        app.update();
        // The item's words sit in a box the wheel and the skin's scrollbar
        // scroll.
        let (words, parent) = app
            .world_mut()
            .query_filtered::<(Entity, &ChildOf), With<crate::items::ItemText>>()
            .single(app.world())
            .map(|(words, parent)| (words, parent.parent()))
            .unwrap();
        let world = app.world();
        assert!(world.get::<ScrolledText>(parent).is_some());
        assert!(
            world
                .get::<crate::windows::pointer::TakesWheel>(parent)
                .is_some()
        );
        let mut bars = app.world_mut().query::<&scrollbar::Scrollbar>();
        assert_eq!(bars.iter(app.world()).count(), 1);
        // Scrolled down, it shows the top of the next item's words.
        app.world_mut().get_mut::<ScrollPosition>(parent).unwrap().y = 40.0;
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(parent).unwrap().y, 40.0);
        app.world_mut().get_mut::<Text>(words).unwrap().0 = "Another item".into();
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(parent).unwrap().y, 0.0);
    }

    #[test]
    fn the_options_window_checkboxes_turn_its_options_on_and_off() {
        use eq_client_assets::sidl::{Button, ButtonLook, Element, Page};
        use eq_client_core::{options::Toggle, qol::Fix};
        assert!(matches!(
            does("ODP_ShowTargetRingCheckbox", WindowId::Options),
            Some(Does::Option(Toggle::TargetRing))
        ));
        assert!(matches!(
            does("ODP_ShowHelmCheckbox", WindowId::Options),
            Some(Does::Option(Toggle::ShowHelm))
        ));
        // What this client does not keep yet is greyed out, and of the two
        // window-mode buttons the skin stacks, only fullscreen shows.
        assert!(matches!(
            does("ODP_LevelOfDetailCheckbox", WindowId::Options),
            Some(Does::Nothing)
        ));
        assert!(does("ODP_SetWindowedButton", WindowId::Options).is_none());
        // The QoL page comes last, a checkbox for each quality-of-life
        // setting in the skin's own, one under another.
        let checkbox = Button {
            id: Some("OGP_PetWindowPopupCheckbox".into()),
            area: Area {
                x: 10.0,
                y: 175.0,
                width: 150.0,
                height: 20.0,
            },
            placed: true,
            anchors: None,
            look: ButtonLook::default(),
            checkbox: true,
            text: Some("Pet Window Popup".into()),
            text_color: None,
            font: None,
            decal: None,
            decal_area: None,
            tooltip: None,
        };
        let general = Page {
            name: "OptionsGeneralPage".into(),
            title: Some("General".into()),
            area: None,
            template: None,
            pieces: vec![("checkbox".into(), Element::Button(checkbox))],
            icon: [None, None],
            title_colors: [None, None],
            tooltip: None,
        };
        let pages = with_qol_page(std::slice::from_ref(&general), WindowId::Options, 400.0);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].title.as_deref(), Some("QoL"));
        let settings: Vec<_> = Fix::settings().collect();
        assert_eq!(pages[1].pieces.len(), settings.len());
        let mut above: Option<Area> = None;
        for ((_, piece), fix) in pages[1].pieces.iter().zip(settings.iter().copied()) {
            let Element::Button(checkbox) = piece else {
                panic!("a checkbox")
            };
            assert!(checkbox.checkbox);
            assert_eq!(checkbox.text.as_deref(), Some(fix.label()));
            assert_eq!(checkbox.tooltip.as_deref(), Some(fix.tooltip()));
            assert!(matches!(
                does(checkbox.id.as_deref().unwrap(), WindowId::Options),
                Some(Does::Option(toggle)) if toggle == Toggle::Qol(fix)
            ));
            if let Some(above) = above {
                assert!((checkbox.area.x - above.x).abs() < f32::EPSILON);
                assert!(checkbox.area.y >= above.y + above.height);
            }
            above = Some(checkbox.area);
        }
        // A page too short for another row goes on in the next column.
        let short = with_qol_page(std::slice::from_ref(&general), WindowId::Options, 70.0);
        let areas: Vec<Area> = short[1]
            .pieces
            .iter()
            .map(|(_, piece)| match piece {
                Element::Button(checkbox) => checkbox.area,
                _ => panic!("a checkbox"),
            })
            .collect();
        assert!(areas[1].x >= areas[0].x + areas[0].width);
        assert!((areas[1].y - areas[0].y).abs() < f32::EPSILON);
        // Other windows keep the skin's pages as they are.
        assert_eq!(
            with_qol_page(
                std::slice::from_ref(&general),
                WindowId::ActionsWindow,
                400.0
            )
            .len(),
            1
        );
    }

    #[test]
    fn the_qol_page_ends_with_the_interface_scale_cut_from_the_skins_slider() {
        use eq_client_assets::sidl::{Button, ButtonLook, Element, Page};
        let area = |x, y, width, height| Area {
            x,
            y,
            width,
            height,
        };
        let checkbox = Button {
            id: Some("OGP_PetWindowPopupCheckbox".into()),
            area: area(10.0, 175.0, 150.0, 20.0),
            placed: true,
            anchors: None,
            look: ButtonLook::default(),
            checkbox: true,
            text: Some("Pet Window Popup".into()),
            text_color: None,
            font: None,
            decal: None,
            decal_area: None,
            tooltip: None,
        };
        let general = Page {
            name: "OptionsGeneralPage".into(),
            title: Some("General".into()),
            area: None,
            template: None,
            pieces: vec![("checkbox".into(), Element::Button(checkbox))],
            icon: [None, None],
            title_colors: [None, None],
            tooltip: None,
        };
        let settings: Vec<_> = eq_client_core::qol::Fix::settings().collect();
        // With the skin's slider and its value label on a page, the QoL page
        // ends with the interface scale: a caption over a slider cut from
        // the skin's, its value beside it, all inside the page.
        let display = Page {
            name: "OptionsDisplayPage".into(),
            title: Some("Display".into()),
            pieces: vec![
                (
                    "ODP_ClipPlaneSlider".into(),
                    Element::Slider(eq_client_assets::sidl::Slider {
                        id: Some("ODP_ClipPlaneSlider".into()),
                        area: area(20.0, 40.0, 150.0, 16.0),
                        look: eq_client_assets::sidl::SliderLook::default(),
                    }),
                ),
                (
                    "ODP_ClipPlaneValueLabel".into(),
                    Element::Label(eq_client_assets::sidl::Label {
                        area: area(180.0, 40.0, 40.0, 14.0),
                        anchors: None,
                        eq_type: None,
                        text: "100 %".into(),
                        color: None,
                        align: eq_client_assets::sidl::Align::Left,
                        font: None,
                        wraps: false,
                    }),
                ),
            ],
            ..general.clone()
        };
        let skin = [general.clone(), display];
        let pages = with_qol_page(&skin, WindowId::Options, 400.0);
        let qol = &pages[2].pieces;
        assert_eq!(qol.len(), settings.len() + 3);
        let last_checkbox = match &qol[settings.len() - 1].1 {
            Element::Button(checkbox) => checkbox.area,
            _ => panic!("a checkbox"),
        };
        let [
            (_, Element::Label(caption)),
            (slider_id, Element::Slider(slider)),
            (value_id, Element::Label(value)),
        ] = &qol[settings.len()..]
        else {
            panic!("a caption, a slider and its value")
        };
        assert_eq!(caption.text, "Interface scale");
        assert_eq!(slider.id.as_deref(), Some(slider_id.as_str()));
        assert_eq!(
            controls::slider_sets(slider.id.as_deref(), WindowId::Options),
            Some(controls::Sets::Level(
                eq_client_core::options::Level::UiScale
            ))
        );
        assert!(matches!(
            controls::value_label(value_id, WindowId::Options),
            Some(controls::ValueLabel::Shows(
                eq_client_core::options::Level::UiScale
            ))
        ));
        let below = |area: Area| {
            area.y >= last_checkbox.y + last_checkbox.height || area.x > last_checkbox.x
        };
        assert!(below(caption.area) && below(slider.area));
        assert!(slider.area.y >= caption.area.y + caption.area.height);
        assert!(value.area.x >= slider.area.x + slider.area.width);
        assert!(slider.area.y + slider.area.height <= 400.0);
    }

    #[test]
    fn a_window_the_skin_hides_is_drawn_from_the_default_skin_when_asked() {
        let install =
            std::env::temp_dir().join(format!("eq-hidden-windows-{}", std::process::id()));
        let raid = |width: u32| {
            format!(
                "<XML><Screen item=\"RaidWindow\"><Location><X>100</X><Y>78</Y></Location>                 <Size><CX>{width}</CX><CY>{width}</CY></Size></Screen></XML>"
            )
        };
        for (skin, width) in [(eq_client_assets::ui::DEFAULT_SKIN, 333), ("hiding", 0)] {
            let folder = install.join("uifiles").join(skin);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("EQUI_RaidWindow.xml"), raid(width)).unwrap();
        }
        let folder = install
            .join("uifiles")
            .join(eq_client_assets::ui::DEFAULT_SKIN);
        std::fs::write(folder.join("EQUI_Animations.xml"), "<XML></XML>").unwrap();
        std::fs::write(folder.join("EQUI_Templates.xml"), "<XML></XML>").unwrap();
        let mut screens = Screens::default();
        let from = |screens: &mut Screens, skin, hidden_too| {
            drawn_from(screens, &install, (skin, WindowId::Raid), hidden_too)
        };
        let hidden_off = from(&mut screens, "hiding", false);
        let hidden_on = from(&mut screens, "hiding", true);
        let default_on = from(&mut screens, eq_client_assets::ui::DEFAULT_SKIN, true);
        std::fs::remove_dir_all(&install).unwrap();
        // The skin's own nothing, unless the player asks for such windows.
        assert_eq!(hidden_off, "hiding");
        assert_eq!(hidden_on, eq_client_assets::ui::DEFAULT_SKIN);
        assert_eq!(default_on, eq_client_assets::ui::DEFAULT_SKIN);
    }

    #[test]
    fn unplaced_buff_slots_stack_down_the_windows_right_edge() {
        let button = Area {
            x: 0.0,
            y: 0.0,
            width: 24.0,
            height: 24.0,
        };
        let window = Area {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 375.0,
        };
        let place = |slot| {
            let area = stacked(button, slot, &window);
            (area.x, area.y)
        };
        // A pixel in from the right and the top, a pixel apart.
        assert_eq!(place(0), (175.0, 1.0));
        assert_eq!(place(1), (175.0, 26.0));
        assert_eq!(place(14), (175.0, 351.0));
        // One column: the sixteenth waits below the window's bottom.
        assert_eq!(place(15), (175.0, 376.0));
        assert_eq!(place(24), (175.0, 601.0));
    }

    /// A buff button as the effects windows' skins lay them out: no place
    /// of its own, no anchors and no picture.
    fn buff_button(width: f32) -> eq_client_assets::sidl::Button {
        eq_client_assets::sidl::Button {
            id: Some("Buff1".into()),
            area: Area {
                x: 0.0,
                y: 0.0,
                width,
                height: 24.0,
            },
            placed: false,
            anchors: None,
            look: ButtonLook::default(),
            checkbox: false,
            text: None,
            text_color: None,
            font: None,
            decal: None,
            decal_area: None,
            tooltip: None,
        }
    }

    #[test]
    fn an_effects_button_the_skin_anchors_goes_where_its_anchors_put_it() {
        use eq_client_assets::sidl::{Anchor, Anchors};
        let window = Area {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 375.0,
        };
        let buff = Does::Buff(EffectWindow::Long, 1);
        let place = |button: &eq_client_assets::sidl::Button| {
            let area = button_area(button, buff, &window);
            (area.x, area.y, area.width, area.height)
        };
        let mut button = buff_button(24.0);
        // Neither placed nor anchored: stacked down the right edge, as the
        // pet window's buff slots are; other buttons keep the window's top
        // left.
        assert_eq!(place(&button), (175.0, 26.0, 24.0, 24.0));
        let pet = button_area(&button, Does::PetBuff(2), &window);
        assert_eq!((pet.x, pet.y), (175.0, 51.0));
        let other = button_area(&button, Does::Closes, &window);
        assert_eq!((other.x, other.y), (0.0, 0.0));
        // Anchored to the window's right edge, where its anchors put it.
        button.anchors = Some(Anchors {
            left: Anchor::End(24.0),
            top: Anchor::Start(25.0),
            right: Anchor::End(0.0),
            bottom: Anchor::Start(49.0),
        });
        assert_eq!(place(&button), (176.0, 25.0, 24.0, 24.0));
        // A placed button stays where the skin placed it.
        button.anchors = None;
        button.placed = true;
        button.area.x = 30.0;
        assert_eq!(place(&button), (30.0, 0.0, 24.0, 24.0));
    }

    #[test]
    fn a_buff_slots_icon_sits_where_the_skin_puts_the_buttons_picture() {
        let mut button = buff_button(200.0);
        // No picture: just inside the button, at its height, as before.
        assert_eq!(
            buff_icon_place(&button, button.area),
            (crate::spell_icons::INSET, 20.0)
        );
        // A picture on the right: the icon fills it.
        button.decal_area = Some(Area {
            x: 176.0,
            y: 2.0,
            width: 20.0,
            height: 20.0,
        });
        assert_eq!(buff_icon_place(&button, button.area), ((176.0, 2.0), 20.0));
    }

    #[test]
    fn the_skins_selector_toggles_the_windows_the_player_opens_and_closes() {
        use crate::windows::Toggle;
        assert!(matches!(
            does("SELW_InventoryToggleButton", WindowId::Selector),
            Some(Does::Toggles(WindowId::Inventory))
        ));
        assert!(matches!(
            does("SELW_MapToggleButton", WindowId::Selector),
            Some(Does::Toggles(WindowId::Map))
        ));
        // A window this client does not have.
        assert!(matches!(
            does("SELW_FriendsToggleButton", WindowId::Selector),
            Some(Does::Nothing)
        ));
        // Each button it maps opens a window the player toggles, or hides
        // and shows one something else opens.
        for (button, toggle) in [
            ("SELW_ActionsToggleButton", Toggle::Opens),
            ("SELW_InventoryToggleButton", Toggle::Opens),
            ("SELW_OptionsToggleButton", Toggle::Opens),
            ("SELW_BuffToggleButton", Toggle::Opens),
            ("SELW_MapToggleButton", Toggle::Opens),
            ("SELW_HotboxToggleButton", Toggle::Hides),
            ("SELW_CastSpellToggleButton", Toggle::Hides),
            ("SELW_PetInfoToggleButton", Toggle::Hides),
            ("SELW_SDBuffToggleButton", Toggle::Hides),
        ] {
            assert_eq!(
                selector_button(button).unwrap().describe().toggle,
                toggle,
                "{button}"
            );
        }
    }

    #[test]
    fn the_effects_windows_buttons_and_labels_are_their_slots_in_order() {
        assert!(matches!(
            does("Buff3", WindowId::Effects),
            Some(Does::Buff(EffectWindow::Long, 3))
        ));
        assert!(matches!(
            does("Buff11", WindowId::ShortEffects),
            Some(Does::Buff(EffectWindow::Short, 11))
        ));
        assert!(matches!(
            does("Buff3", WindowId::Player),
            Some(Does::Nothing)
        ));
        assert_eq!(effect_label(524), Some((EffectWindow::Long, 24)));
        assert_eq!(effect_label(600), Some((EffectWindow::Short, 0)));
        assert_eq!(effect_label(17), None);
        // Only the effects windows' labels name buffs.
        assert_eq!(
            buff_label(WindowId::Effects, Some(503)),
            Some((EffectWindow::Long, 3))
        );
        assert_eq!(buff_label(WindowId::Inventory, Some(503)), None);
    }

    #[test]
    fn the_pet_window_buttons_command_the_pet_and_show_its_buffs() {
        assert!(matches!(
            does("AttackButton", WindowId::PetInfo),
            Some(Does::Slash("/pet attack"))
        ));
        assert!(matches!(
            does("LostButton", WindowId::PetInfo),
            Some(Does::Slash("/pet get lost"))
        ));
        assert!(matches!(
            does("PetBuff29", WindowId::PetInfo),
            Some(Does::PetBuff(29))
        ));
        // Elsewhere the same names do nothing of the pet's.
        assert!(matches!(
            does("AttackButton", WindowId::Player),
            Some(Does::Nothing)
        ));
        // Stand lies under Sit: each shows only while it does something.
        assert_eq!(pet_posture_button("/pet sit down"), Some(false));
        assert_eq!(pet_posture_button("/pet stand up"), Some(true));
        assert_eq!(pet_posture_button("/pet attack"), None);
    }
    use crate::online::{OnlineState, testing};
    use eq_client_core::WorldEvent;

    #[test]
    fn gauges_and_labels_show_what_their_numbers_say() {
        let mut state = OnlineState::new(true);
        let mut player = testing::player(7);
        player.name = "Example".into();
        player.hp_percent = Some(40);
        testing::admit(&mut state, 1, player);
        testing::resources(&mut state, 50, 25);
        testing::news(
            &mut state,
            [WorldEvent::HealthPercent {
                spawn_id: 7,
                percent: 40,
            }],
        );
        let world = state.world();
        assert_eq!(gauge_text(world, 1).0, "Example");
        assert_eq!(fraction(world, None, 1), Some(0.4));
        assert_eq!(label_text(world, None, 19), "40");
        // Mana and stamina need their maxima, here estimated.
        assert_eq!(label_text(world, Some((100, 100)), 20), "50");
        assert_eq!(label_text(world, Some((100, 100)), 21), "25");
        assert_eq!(label_text(world, None, 20), "");
        // Numbers the client does not know yet show nothing.
        assert_eq!(label_text(world, None, 99), "");
        assert_eq!(fraction(world, None, 16), None);
        // With no target, the target's gauge is empty, and so is the
        // label for its name.
        assert_eq!(gauge_text(world, 6).0, "");
        assert_eq!(label_text(world, None, 28), "");
        assert_eq!(fraction(world, None, 6), None);
        // A creature targeted: its name as players see it, in the label and
        // the gauge alike.
        let mut creature = testing::pet(9, 7);
        creature.name = "a_gnoll001".into();
        creature.pet_owner = None;
        testing::spawn_entry(&mut state, 9, creature);
        state.select_target(Some(9));
        assert_eq!(label_text(state.world(), None, 28), "a gnoll");
        assert_eq!(gauge_text(state.world(), 6).0, "a gnoll");
        // The player targeting themselves.
        state.select_target(Some(7));
        assert_eq!(label_text(state.world(), None, 28), "Example");
        assert_eq!(gauge_text(state.world(), 6).0, "Example");
    }

    /// Draws labels with these numbers into one window, as a skin lays them
    /// out, and gives each label's text with what it shows.
    fn draw_labels(app: &mut App, owner: WindowId, kinds: &[u32]) -> Vec<(Shows, Entity)> {
        use bevy::ecs::system::RunSystemOnce;
        let inside = Area {
            x: 0.0,
            y: 0.0,
            width: 130.0,
            height: 215.0,
        };
        let kinds = kinds.to_vec();
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(|window| {
                    for kind in &kinds {
                        let words = Label {
                            area: Area {
                                x: 25.0,
                                y: 15.0,
                                width: 95.0,
                                height: 23.0,
                            },
                            anchors: None,
                            eq_type: Some(*kind),
                            text: String::new(),
                            color: None,
                            align: Align::Center,
                            font: None,
                            wraps: false,
                        };
                        label(window, "Name", &words, &inside, owner);
                    }
                });
            })
            .unwrap();
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &Shows), With<Text>>()
            .iter(world)
            .map(|(entity, shows)| (*shows, entity))
            .collect()
    }

    #[test]
    fn a_line_of_text_sits_in_the_middle_of_a_taller_box() {
        use bevy::ecs::system::RunSystemOnce;
        for (align, justify) in [
            (Align::Left, JustifyContent::FlexStart),
            (Align::Center, JustifyContent::Center),
            (Align::Right, JustifyContent::FlexEnd),
        ] {
            let mut app = App::new();
            app.world_mut()
                .run_system_once(move |mut commands: Commands| {
                    commands.spawn(Node::default()).with_children(|window| {
                        // A gem's name row in the Velious skin: 23 tall.
                        aligned(
                            window,
                            at(25.0, 15.0, 95.0, 23.0),
                            align,
                            Text::new("Courage"),
                        );
                    });
                })
                .unwrap();
            let mut texts = app.world_mut().query::<(Entity, &Text, &ChildOf)>();
            let (line, row) = texts
                .iter(app.world())
                .find(|(_, text, _)| text.0 == "Courage")
                .map(|(line, _, parent)| (line, parent.parent()))
                .unwrap();
            let node = app.world().get::<Node>(row).unwrap();
            assert_eq!(node.align_items, AlignItems::Center);
            // Across, the line keeps the skin's alignment, and the box keeps
            // the skin's size.
            assert_eq!(node.justify_content, justify);
            assert_eq!(node.width, Val::Px(95.0));
            assert_eq!(node.height, Val::Px(23.0));
            let layout = app.world().get::<TextLayout>(line).unwrap();
            assert_eq!(layout.linebreak, LineBreak::NoWrap);
        }
    }

    #[test]
    fn wrapping_words_in_a_box_stretching_down_the_window_start_at_its_top() {
        use bevy::ecs::system::RunSystemOnce;
        let stretched = |top, bottom| Anchors {
            left: Anchor::Start(4.0),
            top,
            right: Anchor::End(4.0),
            bottom,
        };
        // The merchant window's name: from 4 below the top to 24 above the
        // foot, its words wrapping.
        let name = stretched(Anchor::Start(4.0), Anchor::End(24.0));
        assert!(line_at_top(Some(name), true));
        // One-line rows filling their container, and fixed boxes, centre.
        assert!(!line_at_top(Some(name), false));
        assert!(!line_at_top(
            Some(stretched(Anchor::Start(0.0), Anchor::End(0.0))),
            false
        ));
        assert!(!line_at_top(
            Some(stretched(Anchor::Start(4.0), Anchor::Start(20.0))),
            true
        ));
        assert!(!line_at_top(None, true));
        // Drawn in a merchant window 480 high, the name keeps to the top
        // instead of the middle, where the list drawn after it covers it.
        let mut app = App::new();
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(|window| {
                    let words = Label {
                        area: Area {
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                        },
                        anchors: Some(name),
                        eq_type: None,
                        text: "Sample".into(),
                        color: None,
                        align: Align::Center,
                        font: None,
                        wraps: true,
                    };
                    let inside = Area {
                        x: 0.0,
                        y: 0.0,
                        width: 382.0,
                        height: 456.0,
                    };
                    label(
                        window,
                        "MW_MerchantName",
                        &words,
                        &inside,
                        WindowId::Merchant,
                    );
                });
            })
            .unwrap();
        let mut boxes = app.world_mut().query::<(&Shows, &ChildOf)>();
        let row = boxes
            .iter(app.world())
            .find(|(shows, _)| **shows == Shows::Merchant)
            .map(|(_, parent)| parent.parent())
            .unwrap();
        let node = app.world().get::<Node>(row).unwrap();
        assert_eq!((node.top, node.height), (Val::Px(4.0), Val::Auto));
        assert_eq!(node.width, Val::Px(374.0));
    }

    #[test]
    fn the_gem_bars_labels_name_the_spell_in_each_gem() {
        // By number, in the gems' order.
        assert_eq!(gem_label(59), None);
        assert_eq!(gem_label(60), Some(0));
        assert_eq!(gem_label(67), Some(7));
        assert_eq!(gem_label(68), None);
        let mut app = crate::testing::app();
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.memorized_spells = [None; 8];
        player.memorized_spells[0] = Some(42);
        player.memorized_spells[7] = Some(43);
        testing::admit(&mut online, 1, player);
        app.insert_resource(online)
            .insert_resource(super::super::spellbook::SpellNames::parse(
                "42^First synthetic spell\n43^Second synthetic spell",
            ))
            .add_systems(Update, show);
        let labels = draw_labels(&mut app, WindowId::Spells, &[60, 61, 67, 28]);
        let text = |app: &App, shows| {
            let (_, label) = labels
                .iter()
                .find(|(each, _)| *each == shows)
                .expect("a label showing this");
            app.world().get::<Text>(*label).unwrap().0.clone()
        };
        app.update();
        assert_eq!(text(&app, Shows::GemName(0)), "First synthetic spell");
        // An empty gem's label is blank.
        assert_eq!(text(&app, Shows::GemName(1)), "");
        assert_eq!(text(&app, Shows::GemName(7)), "Second synthetic spell");
        // Other numbers keep their own meaning.
        assert_eq!(text(&app, Shows::Label(28)), "");
        // Forgotten, the first gem's name goes.
        testing::spell(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            eq_client_core::SpellUpdate::Slot {
                slot: 0,
                spell_id: 42,
                mode: 2,
            },
        );
        app.update();
        assert_eq!(text(&app, Shows::GemName(0)), "");
        assert_eq!(text(&app, Shows::GemName(7)), "Second synthetic spell");
    }

    #[test]
    fn the_group_window_shows_each_other_member_in_their_place() {
        use eq_client_core::group::GroupUpdate;
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        // A member in view at 60%, with a pet; another member elsewhere.
        let mut friend = testing::pet(9, 7);
        friend.name = "Friend".into();
        friend.kind = eq_client_core::SpawnKind::Player;
        friend.pet_owner = None;
        testing::spawn_entry(&mut state, 9, friend);
        testing::spawn_entry(&mut state, 10, testing::pet(10, 9));
        testing::news(
            &mut state,
            [
                WorldEvent::HealthPercent {
                    spawn_id: 9,
                    percent: 60,
                },
                WorldEvent::Group(GroupUpdate::Members {
                    leader: "Friend".into(),
                    members: vec!["Friend".into(), "Other".into()],
                }),
            ],
        );
        let world = state.world();
        assert_eq!(gauge_text(world, 11).0, "Friend");
        assert_eq!(gauge_text(world, 12).0, "Other");
        assert_eq!(gauge_text(world, 13).0, "");
        assert_eq!(fraction(world, None, 11), Some(0.6));
        assert_eq!(fraction(world, None, 12), None);
        assert_eq!(label_text(world, None, 30), "Friend");
        assert_eq!(label_text(world, None, 35), "60");
        assert_eq!(label_text(world, None, 36), "");
        // The first member's pet shows under them.
        assert_eq!(fraction(world, None, 17), Some(1.0));
        assert_eq!(fraction(world, None, 18), None);
        // Empty places, and the pet of a member out of view, are hidden;
        // Invite and Disband show while no invitation waits.
        let shown = |shows| {
            shown_now(
                shows,
                world,
                &crate::combat::CombatState::default(),
                &crate::trade::TradeState::default(),
            )
        };
        assert_eq!(shown(Shows::Member(1)), Some(true));
        assert_eq!(shown(Shows::Member(2)), Some(false));
        assert_eq!(shown(Shows::MemberPet(0)), Some(true));
        assert_eq!(shown(Shows::MemberPet(1)), Some(false));
        assert_eq!(shown(Shows::WhileInvited(false)), Some(true));
        assert_eq!(shown(Shows::WhileInvited(true)), Some(false));
        // Which piece is whose; the player's own always shows.
        assert_eq!(owned_gauge(15), Some(Shows::Member(4)));
        assert_eq!(owned_gauge(16), Some(Shows::Pet));
        assert_eq!(owned_gauge(17), Some(Shows::MemberPet(0)));
        assert_eq!(owned_gauge(1), None);
        assert_eq!(
            window_label(WindowId::Group, "GW_HPPercLabel3"),
            Some(Shows::MemberPercent(2))
        );
        assert_eq!(window_label(WindowId::Group, "GW_HPPercLabel6"), None);
    }

    #[test]
    fn the_players_pet_gauge_shows_only_while_they_have_a_pet() {
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        let shown = |state: &OnlineState| {
            shown_now(
                Shows::Pet,
                state.world(),
                &crate::combat::CombatState::default(),
                &crate::trade::TradeState::default(),
            )
        };
        // Without a pet, the gauge hides and says nothing.
        assert_eq!(shown(&state), Some(false));
        assert_eq!(gauge_text(state.world(), 16).0, "");
        // Another player's pet is not theirs.
        testing::spawn_entry(&mut state, 8, testing::pet(8, 9));
        assert_eq!(shown(&state), Some(false));
        testing::spawn_entry(&mut state, 10, testing::pet(10, 7));
        assert_eq!(shown(&state), Some(true));
        assert_eq!(gauge_text(state.world(), 16).0, "Gabober");
        assert_eq!(fraction(state.world(), None, 16), Some(1.0));
    }

    /// A piece of a gauge, of this size.
    fn gauge_piece(width: u32, height: u32) -> Piece {
        Piece {
            texture: "gauge.tga".into(),
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    /// A gauge 90 by 30 at the window's corner, its bar started here.
    fn test_gauge(
        look: eq_client_assets::sidl::GaugeLook,
        bar_left: f32,
        bar_offset: f32,
    ) -> Gauge {
        Gauge {
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 90.0,
                height: 30.0,
            },
            eq_type: Some(1),
            look,
            fill_tint: None,
            text_color: None,
            text_offset: (0.0, 0.0),
            bar_offset,
            bar_left,
        }
    }

    #[test]
    fn a_gauges_pieces_keep_their_own_sizes_from_where_the_skin_starts_the_bar() {
        use eq_client_assets::sidl::GaugeLook;
        let area = |x, y, width, height| Area {
            x,
            y,
            width,
            height,
        };
        // Its ends either side of the bar, and the bar as long as its
        // background, though the gauge is longer.
        let ended = GaugeLook {
            background: Some(gauge_piece(60, 8)),
            fill: Some(gauge_piece(60, 8)),
            lines: Some(gauge_piece(60, 8)),
            cap_left: Some(gauge_piece(3, 8)),
            cap_right: Some(gauge_piece(3, 8)),
        };
        assert_eq!(
            bar_parts(&test_gauge(ended, 2.0, 20.0)),
            BarParts {
                left: Some(area(2.0, 20.0, 3.0, 8.0)),
                bar: area(5.0, 20.0, 60.0, 8.0),
                right: Some(area(65.0, 20.0, 3.0, 8.0)),
            }
        );
        // A fill alone, started before the gauge: the gauge shows the part
        // of it past its own left edge.
        let fill = GaugeLook {
            fill: Some(gauge_piece(60, 6)),
            ..GaugeLook::default()
        };
        assert_eq!(
            bar_parts(&test_gauge(fill, -20.0, 0.0)),
            BarParts {
                left: None,
                bar: area(-20.0, 0.0, 60.0, 6.0),
                right: None,
            }
        );
        // Nothing to draw.
        assert_eq!(
            bar_parts(&test_gauge(GaugeLook::default(), 0.0, 16.0)).bar,
            area(0.0, 16.0, 0.0, 0.0)
        );
    }

    #[test]
    fn a_gauge_cuts_off_what_lies_outside_it() {
        use bevy::ecs::system::RunSystemOnce;
        use eq_client_assets::sidl::GaugeLook;
        // A thin pet line whose bar starts above it and whose text the skin
        // puts far above it.
        let line = Gauge {
            area: Area {
                x: 4.0,
                y: 30.0,
                width: 70.0,
                height: 2.0,
            },
            eq_type: Some(16),
            text_offset: (0.0, -40.0),
            ..test_gauge(
                GaugeLook {
                    background: Some(gauge_piece(60, 8)),
                    fill: Some(gauge_piece(60, 8)),
                    ..GaugeLook::default()
                },
                0.0,
                -3.0,
            )
        };
        let inside = Area {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<crate::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()));
        app.world_mut()
            .run_system_once(move |mut commands: Commands, mut art: crate::sheets::Art| {
                commands
                    .spawn(Node::default())
                    .with_children(|window| gauge(window, &mut art, &line, &inside));
            })
            .unwrap();
        let world = app.world_mut();
        let pieces: Vec<(Entity, Shows, Node)> = world
            .query::<(Entity, &Shows, &Node)>()
            .iter(world)
            .map(|(entity, shows, node)| (entity, *shows, node.clone()))
            .collect();
        let find = |wanted: Shows| {
            pieces
                .iter()
                .find(|(_, shows, _)| *shows == wanted)
                .cloned()
                .unwrap()
        };
        // The gauge cuts off what lies outside its box, its text included;
        // it hides while the player has no pet.
        let (root, _, root_node) = find(Shows::Pet);
        assert_eq!(root_node.overflow, Overflow::clip());
        assert_eq!(world.get::<Visibility>(root), Some(&Visibility::Hidden));
        let (text, ..) = find(Shows::GaugeText(16));
        assert_eq!(world.get::<ChildOf>(text).unwrap().parent(), root);
        // The bar keeps its pieces' size from where the skin starts it, so
        // only its rows inside the line show.
        let (fill, _, fill_node) = find(Shows::Fill(16));
        assert_eq!(fill_node.height, px(8));
        let bar = world.get::<ChildOf>(fill).unwrap().parent();
        let bar_node = world.get::<Node>(bar).unwrap();
        assert_eq!(world.get::<ChildOf>(bar).unwrap().parent(), root);
        assert_eq!(
            (bar_node.left, bar_node.top, bar_node.width, bar_node.height),
            (px(0), px(-3), px(60), px(8))
        );
    }

    /// Draws these gauges into one window, in their order.
    fn draw_gauges(app: &mut App, gauges: Vec<Gauge>) {
        use bevy::ecs::system::RunSystemOnce;
        let inside = Area {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 50.0,
        };
        app.world_mut()
            .run_system_once(move |mut commands: Commands, mut art: crate::sheets::Art| {
                commands.spawn(Node::default()).with_children(|window| {
                    for each in &gauges {
                        gauge(window, &mut art, each, &inside);
                    }
                });
            })
            .unwrap();
    }

    /// Each gauge's fill, with the bar it lies in and the gauge's root.
    fn gauge_fills(world: &mut World) -> Vec<(Entity, Entity, Entity)> {
        let fills: Vec<Entity> = world
            .query::<(Entity, &Shows)>()
            .iter(world)
            .filter(|(_, shows)| matches!(shows, Shows::Fill(_)))
            .map(|(entity, _)| entity)
            .collect();
        fills
            .into_iter()
            .map(|fill| {
                let bar = world.get::<ChildOf>(fill).unwrap().parent();
                let root = world.get::<ChildOf>(bar).unwrap().parent();
                (fill, bar, root)
            })
            .collect()
    }

    /// A length in pixels.
    fn pixels(value: Val) -> f32 {
        let Val::Px(pixels) = value else {
            panic!("{value:?} is not in pixels");
        };
        pixels
    }

    #[test]
    fn a_gauge_started_left_of_its_box_stays_cut_unless_its_fill_is_wider() {
        use eq_client_assets::sidl::GaugeLook;
        // Started a pixel left of its box, its fill as wide as the box.
        let lone = test_gauge(
            GaugeLook {
                background: Some(gauge_piece(90, 8)),
                fill: Some(gauge_piece(90, 8)),
                ..GaugeLook::default()
            },
            -1.0,
            0.0,
        );
        assert_eq!(Step::of(&lone, &bar_parts(&lone)), None);
        let mut app = crate::testing::app();
        draw_gauges(&mut app, vec![lone]);
        let world = app.world_mut();
        let fills = gauge_fills(world);
        let [(fill, _, root)] = fills[..] else {
            panic!("one fill: {fills:?}");
        };
        assert_eq!(world.get::<Node>(root).unwrap().overflow, Overflow::clip());
        assert_eq!(world.get::<Step>(fill), None);
    }

    #[test]
    fn a_steps_fill_alone_crosses_its_box_from_the_bars_start() {
        use eq_client_assets::sidl::GaugeLook;
        // A box 90 wide whose bar starts 30 to its left, after a 3 wide end,
        // with a fill 120 long, wider than the box, and text, a background,
        // lines and ends.
        let step = Gauge {
            text_offset: (-30.0, 0.0),
            ..test_gauge(
                GaugeLook {
                    background: Some(gauge_piece(120, 8)),
                    fill: Some(gauge_piece(120, 8)),
                    lines: Some(gauge_piece(120, 8)),
                    cap_left: Some(gauge_piece(3, 8)),
                    cap_right: Some(gauge_piece(3, 8)),
                },
                -33.0,
                0.0,
            )
        };
        assert_eq!(
            Step::of(&step, &bar_parts(&step)),
            Some(Step { from: 0.25 })
        );
        let mut app = crate::testing::app();
        draw_gauges(&mut app, vec![step]);
        let world = app.world_mut();
        let fills = gauge_fills(world);
        let [(fill, bar, root)] = fills[..] else {
            panic!("one fill: {fills:?}");
        };
        // The fill lies in the gauge, cut only above and below, in a bar
        // started where the skin starts it.
        assert_eq!(world.get::<Step>(fill), Some(&Step { from: 0.25 }));
        assert_eq!(
            world.get::<Node>(root).unwrap().overflow,
            Overflow::clip_y()
        );
        let bar_node = world.get::<Node>(bar).unwrap();
        assert_eq!((bar_node.left, bar_node.width), (px(-30), px(120)));
        // Everything else lies in a node cut at the box: the text and the
        // background under the fill, the lines and the ends over it.
        let layers = world.get::<Children>(root).unwrap().to_vec();
        let [under, middle, over] = layers[..] else {
            panic!("three layers: {layers:?}");
        };
        assert_eq!(middle, bar);
        for cut in [under, over] {
            let node = world.get::<Node>(cut).unwrap();
            assert_eq!(node.overflow, Overflow::clip());
            assert_eq!(
                (node.left, node.top, node.width, node.height),
                (px(0), px(0), px(90), px(30))
            );
        }
        let text = world
            .query::<(Entity, &Shows)>()
            .iter(world)
            .find(|(_, shows)| **shows == Shows::GaugeText(1))
            .map(|(entity, _)| entity)
            .unwrap();
        assert_eq!(world.get::<ChildOf>(text).unwrap().parent(), under);
    }

    #[test]
    fn a_bar_coloured_in_steps_takes_the_colour_of_the_highest_step_reached() {
        use eq_client_assets::sidl::GaugeLook;
        // A bar 50 long at the window's x 10, coloured in five steps as the
        // Velious skin builds its hit point bars (made-up numbers): a gauge
        // over the whole bar, then one per further fifth, its box from there
        // to the bar's end and its fill, 50 long, started at the bar's start.
        let steps: Vec<Gauge> = (0..5u8)
            .map(|step| {
                let start = 10.0 * f32::from(step);
                Gauge {
                    area: Area {
                        x: 10.0 + start,
                        y: 4.0,
                        width: 50.0 - start,
                        height: 6.0,
                    },
                    ..test_gauge(
                        GaugeLook {
                            fill: Some(gauge_piece(50, 6)),
                            ..GaugeLook::default()
                        },
                        -start,
                        0.0,
                    )
                }
            })
            .collect();
        // The steps whose fills draw at this hit point percent. Each one
        // draws from the bar's start to the level, and later gauges draw
        // over earlier ones, so the highest step drawn colours it all.
        let drawn = |level: u8| {
            let mut app = crate::testing::app();
            let mut online = OnlineState::new(true);
            let mut player = testing::player(7);
            player.hp_percent = Some(level);
            testing::admit(&mut online, 1, player);
            app.insert_resource(online).add_systems(Update, show);
            draw_gauges(&mut app, steps.clone());
            app.update();
            let world = app.world_mut();
            let mut drawn = Vec::new();
            for (fill, bar, root) in gauge_fills(world) {
                let fill = world.get::<Node>(fill).unwrap();
                let place = pixels(world.get::<Node>(root).unwrap().left);
                let start = place + pixels(world.get::<Node>(bar).unwrap().left);
                let Val::Percent(width) = fill.width else {
                    panic!("{:?}", fill.width);
                };
                assert!((start - 10.0).abs() < 0.001, "{start}");
                assert!((width - f32::from(level)).abs() < 0.001, "{width}");
                if fill.display != Display::None {
                    let step = [10.0, 20.0, 30.0, 40.0, 50.0]
                        .iter()
                        .position(|at: &f32| (place - at).abs() < 0.001)
                        .unwrap();
                    drawn.push(step);
                }
            }
            drawn.sort_unstable();
            drawn
        };
        // 70%: three steps draw, the third on top.
        assert_eq!(drawn(70), [0, 1, 2, 3]);
        // At an exact step, the lower colour shows.
        assert_eq!(drawn(60), [0, 1, 2]);
        assert_eq!(drawn(100), [0, 1, 2, 3, 4]);
        assert_eq!(drawn(21), [0, 1]);
        assert_eq!(drawn(10), [0]);
        assert_eq!(drawn(0), [0]);
    }

    #[test]
    fn group_buttons_run_the_group_commands_and_answer_in_their_places() {
        let group = |id, owner| match does(id, owner) {
            Some(Does::Group(button)) => Some(button),
            _ => None,
        };
        assert_eq!(
            group("InviteButton", WindowId::Group),
            Some(GroupButton::Invite)
        );
        assert_eq!(
            group("DeclineButton", WindowId::Group),
            Some(GroupButton::Decline)
        );
        // Looking For Group is left out.
        assert!(does("LFGButton", WindowId::Group).is_none());
        // Each runs what the Actions window's button runs; Decline disbands,
        // which declines the invitation waiting.
        assert_eq!(GroupButton::Invite.command(), "/invite");
        assert_eq!(GroupButton::Follow.command(), "/follow");
        assert_eq!(GroupButton::Disband.command(), "/disband");
        assert_eq!(GroupButton::Decline.command(), "/disband");
        // In the group window, Follow and Decline take Invite's and
        // Disband's places while an invitation waits.
        assert_eq!(
            paired(Does::Group(GroupButton::Follow), WindowId::Group),
            Some(Shows::WhileInvited(true))
        );
        assert_eq!(
            paired(Does::Group(GroupButton::Disband), WindowId::Group),
            Some(Shows::WhileInvited(false))
        );
        assert_eq!(
            paired(Does::Group(GroupButton::Follow), WindowId::ActionsWindow),
            None
        );
    }

    #[test]
    fn raid_buttons_run_the_raid_commands_and_the_labels_count_the_raid() {
        use crate::raid::RaidAction;
        use eq_client_core::raid::{RaidMember, RaidUpdate};
        let raid = |id| match does(id, WindowId::Raid) {
            Some(Does::Raid(button)) => Some(button),
            _ => None,
        };
        assert_eq!(raid("RAID_InviteButton"), Some(RaidButton::Invite));
        assert_eq!(raid("RAID_AcceptButton"), Some(RaidButton::Accept));
        assert_eq!(raid("RAID_DeclineButton"), Some(RaidButton::Decline));
        assert_eq!(RaidButton::Invite.command(), "/raidinvite");
        let action = |id| match does(id, WindowId::Raid) {
            Some(Does::RaidAction(action)) => Some(action),
            _ => None,
        };
        assert_eq!(action("RAID_DisbandButton"), Some(RaidAction::Disband));
        assert_eq!(action("RAID_UnlockButton"), Some(RaidAction::Lock(false)));
        assert_eq!(action("RAID_Group1Button"), Some(RaidAction::Move(Some(0))));
        assert_eq!(
            action("RAID_Group12Button"),
            Some(RaidAction::Move(Some(11)))
        );
        assert_eq!(action("RAID_Group13Button"), None);
        assert_eq!(action("RAID_NoGroupButton"), Some(RaidAction::Move(None)));
        assert_eq!(
            action("RAID_MakeLeaderButton"),
            Some(RaidAction::MakeLeader)
        );
        // No server type takes a group leader's mark; the looting buttons do
        // nothing yet.
        assert!(matches!(
            does("RAID_RemoveLeaderButton", WindowId::Raid),
            Some(Does::Unoffered(
                eq_client_core::Capability::RaidGroupLeaders
            ))
        ));
        assert!(matches!(
            does("RAID_AddLooterButton", WindowId::Raid),
            Some(Does::Nothing)
        ));
        // Accept and Decline take Invite's and Disband's places while an
        // invitation waits, and Unlock takes Lock's while the raid is locked.
        assert_eq!(
            paired(Does::Raid(RaidButton::Accept), WindowId::Raid),
            Some(Shows::WhileRaidInvited(true))
        );
        assert_eq!(
            paired(Does::RaidAction(RaidAction::Disband), WindowId::Raid),
            Some(Shows::WhileRaidInvited(false))
        );
        assert_eq!(
            paired(Does::RaidAction(RaidAction::Lock(false)), WindowId::Raid),
            Some(Shows::WhileRaidLocked(true))
        );
        assert_eq!(
            window_label(WindowId::Raid, "RAID_PlayerCountLabel"),
            Some(Shows::RaidCount)
        );
        assert_eq!(
            window_label(WindowId::Raid, "RAID_LevelAverageLabel"),
            Some(Shows::RaidLevel)
        );
        assert_eq!(window_label(WindowId::Raid, "RAID_PlayerListLabel"), None);
        // Nought outside a raid; then the members and their average level.
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        assert_eq!(raid_text(Shows::RaidCount, state.world()), "0");
        assert_eq!(raid_text(Shows::RaidLevel, state.world()), "0");
        let member = |name: &str, level| {
            WorldEvent::Raid(RaidUpdate::Added(RaidMember {
                name: name.into(),
                group: None,
                class: 1,
                level,
                group_leader: false,
            }))
        };
        testing::news(
            &mut state,
            [
                WorldEvent::Raid(RaidUpdate::Invited {
                    inviter: "Leader".into(),
                }),
                WorldEvent::Raid(RaidUpdate::Created {
                    leader: "Leader".into(),
                }),
                member("Leader", 30),
                member("Example", 5),
            ],
        );
        assert_eq!(raid_text(Shows::RaidCount, state.world()), "2");
        assert_eq!(raid_text(Shows::RaidLevel, state.world()), "17");
        // In the raid, no invitation waits: Invite and Disband show.
        let shown = |shows| {
            shown_now(
                shows,
                state.world(),
                &crate::combat::CombatState::default(),
                &crate::trade::TradeState::default(),
            )
        };
        assert_eq!(shown(Shows::WhileRaidInvited(false)), Some(true));
        assert_eq!(shown(Shows::WhileRaidInvited(true)), Some(false));
        assert_eq!(shown(Shows::WhileRaidLocked(false)), Some(true));
    }

    #[test]
    fn the_cursor_attachment_is_its_box_and_its_first_pictures_place() {
        let piece = Piece {
            texture: "sample.tga".into(),
            x: 0,
            y: 0,
            width: 40,
            height: 40,
        };
        let image = |name: &str, x: f32| {
            (
                name.to_owned(),
                Element::Image {
                    id: Some(name.to_owned()),
                    area: Area {
                        x,
                        y: 5.0,
                        width: 40.0,
                        height: 40.0,
                    },
                    piece: piece.clone(),
                },
            )
        };
        let mut screen = Screen {
            name: "CursorAttachment".into(),
            title: None,
            title_color: None,
            font: None,
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            },
            template: None,
            title_bar: None,
            border: false,
            tooltip: None,
            pieces: vec![image("CA_Anim2", 9.0), image("CA_Anim", 5.0)],
            tab_frame: None,
            transparent: false,
        };
        let gold = Some(piece.clone());
        let place = cursor_place(&screen, [None, gold.clone(), None, None]).unwrap();
        assert_eq!(place.size, Vec2::splat(50.0));
        assert_eq!(
            place.icon,
            Area {
                x: 5.0,
                y: 5.0,
                width: 40.0,
                height: 40.0,
            }
        );
        assert_eq!(place.coin(Coin::Gold), gold.as_ref());
        assert_eq!(place.coin(Coin::Copper), None);
        // A screen without the picture's place draws no attachment.
        screen.pieces.truncate(1);
        assert_eq!(cursor_place(&screen, Default::default()), None);
    }
}
