//! Windows drawn from the installed skin's definitions: the player, target
//! and spell windows, and the windows that hold items. Each keeps its place
//! in the window registry and its behaviour; the skin decides its size,
//! frame and pieces, and what each gauge and label shows follows the
//! official client's numbering. A skin without the window, or a viewer
//! without an installation, keeps the client's own chrome.
mod controls;
mod frame;
mod items;
pub(crate) mod looks;
pub(crate) mod scrollbar;

pub(crate) use controls::{
    AmountBox, Choosing, DropDown, DropDownChoice, KeyFilter, Sets, SkinSlider, drop_downs,
    fill_lists, light_choices, scroll_lists, show_amount, show_choices, show_sliders, slide,
    type_amount,
};

pub(crate) use items::{
    Closes, TheirSlot, close, contents, frames, loot, quantity, theirs, toggle_bag,
};

use super::windows::WindowId;
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{Align, ButtonLook, Element, Gauge, Label, Library, Piece, Screen},
    ui::Area,
};
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

/// The skin a frame is drawn in.
#[derive(Component)]
pub(crate) struct Drawn(String);

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

/// A button drawn from the skin, with its piece for each state.
#[derive(Component, Clone)]
pub(crate) struct SkinButton(ButtonLook);

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
    (paperdoll, mut skinned): (
        Option<Res<super::paperdoll::PaperdollImage>>,
        ResMut<Skinned>,
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
        if drawn.is_some_and(|drawn| drawn.0 == from) {
            continue;
        }
        let Some(screen) = screens.get(directory, &from, *id) else {
            continue;
        };
        commands.entity(frame).insert(Drawn(from.clone()));
        art.draw_in((from != skin.0).then(|| from.clone()));
        skinned.0.insert(*id);
        reshape(&mut node, screen, state.placed(), *id);
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

/// Sizes the frame as the skin does. A window placed neither by the player
/// nor by the official client's UI file opens where the skin puts it, as
/// the official client opens it, unless its description declares a place of
/// its own (`Description::opening`).
fn reshape(node: &mut Node, screen: &Screen, placed: bool, id: WindowId) {
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
        node.position_type = PositionType::Absolute;
        node.left = px(screen.area.x);
        node.top = px(screen.area.y);
        node.right = Val::Auto;
        node.bottom = Val::Auto;
        node.margin = UiRect::ZERO;
    }
}

/// The frame's background, border and title bar, then the pieces inside it;
/// the bottom of its title bar, where it has one, to minimize it to.
fn draw(
    window: &mut ChildSpawnerCommands,
    screen: &Screen,
    art: &mut crate::sheets::Art,
    context: &Context,
) -> Option<f32> {
    let (width, height) = (screen.area.width, screen.area.height);
    // A window the skin sizes to nothing, as Velious hides its casting and
    // short effects windows, shows nothing, not even its frame.
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let mut inside = Area {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    // The window's background: the skin's texture, which the character's
    // saved look tints and fades, or a flat colour of that look's instead. A
    // window the skin makes transparent has none, so what lies behind shows
    // between its pieces, and a look saved for it has nothing to tint or fade:
    // the skin's setting wins (inferred, as is what the setting means).
    if !screen.transparent {
        let mut backdrop = window.spawn((looks::Backdrop(context.id), at(0.0, 0.0, width, height)));
        if let Some(image) = screen
            .template
            .as_ref()
            .and_then(|template| template.background.as_deref())
            .and_then(|file| art.texture(file))
        {
            backdrop.insert(ImageNode {
                image,
                image_mode: NodeImageMode::Tiled {
                    tile_x: true,
                    tile_y: true,
                    stretch_value: 1.0,
                },
                ..default()
            });
        }
    }
    if let Some(template) = &screen.template {
        if screen.border {
            inside = border(window, art, &template.border, (width, height));
        }
        if let Some(bar) = screen.title_bar {
            title(
                window,
                art,
                (screen, template, bar),
                &mut inside,
                context.id,
            );
        }
    }
    let title_bottom = (inside.y > 0.0 && screen.title_bar.is_some()).then_some(inside.y);
    // As in the official client, nothing shows outside the client area: the
    // skin parks pieces there that only some windows use, such as a
    // container's augment labels.
    let client = Area {
        x: 0.0,
        y: 0.0,
        width: inside.width,
        height: inside.height,
    };
    window
        .spawn(Node {
            overflow: Overflow::clip(),
            ..at(inside.x, inside.y, inside.width, inside.height)
        })
        .with_children(|area| pieces(area, art, &screen.pieces, &client, context));
    title_bottom
}

/// Pieces of a window or page, inside this area, in drawing order.
fn pieces(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    pieces: &[(String, Element)],
    inside: &Area,
    context: &Context,
) {
    let inside = *inside;
    for (name, element) in pieces {
        match element {
            Element::Gauge(gauge) => self::gauge(window, art, gauge, &inside),
            Element::Label(label) => self::label(window, name, label, &inside, context.id),
            Element::Image { id, area, piece } => {
                let node = at(
                    inside.x + area.x,
                    inside.y + area.y,
                    area.width,
                    area.height,
                );
                if let Some(image) = art.cut(piece) {
                    let mut picture = window.spawn((image, node));
                    if id.as_deref() == Some("A_AttackIndicatorAnim") {
                        picture.insert((Shows::Attacking, Visibility::Hidden));
                    }
                }
            }
            Element::SpellGem(gem) => spell_gem(window, art, gem, &inside),
            Element::Button(button) => self::button(window, art, button, &inside, context.id),
            Element::InvSlot(slot) => items::slot(window, art, slot, &inside, context.id),
            Element::Slider(slider) => controls::slider(window, art, slider, &inside, context.id),
            Element::Combobox(combobox) => {
                controls::combobox(window, art, combobox, &inside, context.id);
            }
            Element::Listbox(list) => controls::listbox(window, art, list, &inside, context.id),
            Element::TextBox(text) if context.id == WindowId::Chat => {
                chat_box(window, art, text, (&inside, context.tab_frame));
            }
            Element::TextBox(text) => text_box(window, art, text, &inside, context.id),
            Element::Tabs(tabs) if TABBED.contains(&context.id) => {
                // A tab box the skin places sits there; one it stretches
                // fills its container.
                let at = tabs.area.map_or(inside, |at| Area {
                    x: inside.x + at.x,
                    y: inside.y + at.y,
                    width: at.width,
                    height: at.height,
                });
                tabbed(window, art, tabs, &at, context);
            }
            // The first page shows; the client has nothing for the others yet.
            Element::Tabs(tabs) => {
                if let Some(page) = tabs.pages.first() {
                    let at = page.area.unwrap_or(Area {
                        x: 0.0,
                        y: 0.0,
                        width: inside.width,
                        height: inside.height,
                    });
                    let page_inside = Area {
                        x: inside.x + at.x,
                        y: inside.y + at.y,
                        width: at.width,
                        height: at.height,
                    };
                    self::pieces(window, art, &page.pieces, &page_inside, context);
                }
            }
            Element::View(view) if view.name == "IW_CharacterView" => {
                items::figure(window, view, &inside, context.paperdoll);
            }
            // The map draws in its render area, over the skin's parchment.
            Element::View(view)
                if context.id == WindowId::Map && view.name == "MVW_MapRenderArea" =>
            {
                let area = view.anchors.map_or(view.area, |anchors| {
                    anchors.within(inside.width, inside.height)
                });
                window.spawn((
                    super::map::MapCanvas,
                    ImageNode::default(),
                    ZIndex(1),
                    Node {
                        overflow: Overflow::clip(),
                        ..at(
                            inside.x + area.x,
                            inside.y + area.y,
                            area.width,
                            area.height,
                        )
                    },
                ));
            }
            Element::View(view) if !view.pieces.is_empty() => {
                self::view(window, art, view, &inside, context);
            }
            Element::View(_) | Element::Other(_) => (),
        }
    }
}

/// A box of text the client fills: the confirmation dialog's question,
/// wrapped inside the box's frame, or the quantity window's number, which a
/// click on the box lets the player type. Words longer than the box scroll
/// in it, with the skin's scrollbar beside them where it gives one.
fn text_box(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    text: &eq_client_assets::sidl::TextBox,
    inside: &Area,
    owner: WindowId,
) {
    let area = text.anchors.map_or(text.area, |anchors| {
        anchors.within(inside.width, inside.height)
    });
    let number = owner == WindowId::Quantity && text.id.as_deref() == Some(controls::AMOUNT_BOX);
    let mut boxed = window.spawn(at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    ));
    if number {
        boxed.insert((Button, controls::AmountBox));
    }
    boxed.with_children(|frame| {
        let mut client = Area {
            x: 0.0,
            y: 0.0,
            width: area.width,
            height: area.height,
        };
        if let Some(template) = &text.template {
            client = border(frame, art, &template.border, (area.width, area.height));
        }
        let ink = text.color.map_or(theme::INK_BRIGHT, rgb);
        // The number stays on one line, in the middle of the box's height.
        if number {
            let node = Node {
                align_items: AlignItems::Center,
                ..at(
                    client.x + 3.0,
                    client.y,
                    (client.width - 6.0).max(0.0),
                    client.height,
                )
            };
            let words = (controls::Amount, theme::text("", Size::Body, ink));
            aligned(frame, node, Align::Left, words);
            return;
        }
        let bar = text.scrollbar.as_ref();
        let bar_width = bar.map_or(0.0, scrollbar::width);
        // The wheel scrolls the words even where the skin gives no bar.
        let mut scroller = frame.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..at(
                    client.x + 4.0,
                    client.y + 4.0,
                    client.width - 8.0 - bar_width,
                    client.height - 8.0,
                )
            },
            ScrollPosition::default(),
            ScrolledText,
            crate::windows::pointer::TakesWheel,
        ));
        let words_box = scroller.id();
        scroller.with_children(|scroller| {
            let mut words = scroller.spawn((
                theme::text("", Size::Body, ink),
                TextLayout::new(Justify::Left, LineBreak::WordBoundary),
                // As tall as its words, which scroll when the box is shorter.
                Node {
                    width: percent(100),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            match (owner, text.id.as_deref()) {
                (WindowId::Confirmation, _) => {
                    words.insert(super::confirm::QuestionText);
                }
                (WindowId::Note, _) => {
                    words.insert(super::reading::Text::Note);
                }
                (WindowId::Book, Some("Page0")) => {
                    words.insert(super::reading::Text::Page(0));
                }
                (WindowId::Book, Some("Page1")) => {
                    words.insert(super::reading::Text::Page(1));
                }
                (WindowId::Item, Some("ItemDescription")) => {
                    words.insert(super::items::ItemText);
                }
                _ => (),
            }
        });
        if let Some(look) = bar {
            scrollbar::spawn(frame, art, look, &client, (words_box, owner));
        }
    });
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

/// One of the chat window's boxes, filled with the client's chat: its tabs
/// and lines in the output box, and the line the player types in the input
/// box.
fn chat_box(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    text: &eq_client_assets::sidl::TextBox,
    (inside, tab_frame): (&Area, Option<&eq_client_assets::sidl::FrameLook>),
) {
    let area = text.anchors.map_or(text.area, |anchors| {
        anchors.within(inside.width, inside.height)
    });
    window
        .spawn(at(
            inside.x + area.x,
            inside.y + area.y,
            area.width,
            area.height,
        ))
        .with_children(|frame| {
            let client = text.template.as_ref().map_or(
                Area {
                    x: 0.0,
                    y: 0.0,
                    width: area.width,
                    height: area.height,
                },
                |template| border(frame, art, &template.border, (area.width, area.height)),
            );
            match text.id.as_deref() {
                // The output scrolls, with the skin's scrollbar beside its
                // lines where the skin gives it one.
                Some("CWChatOutput") => {
                    let bar = text.scrollbar.as_ref();
                    let width = bar.map_or(0.0, scrollbar::width);
                    let lines = super::chat::skinned_output(
                        frame,
                        at(client.x, client.y, client.width - width, client.height),
                        (art, tab_frame),
                    );
                    if let Some(look) = bar {
                        scrollbar::spawn(frame, art, look, &client, (lines, WindowId::Chat));
                    }
                }
                Some("CWChatInput") => super::chat::skinned_input(
                    frame,
                    at(client.x, client.y, client.width, client.height),
                ),
                _ => (),
            }
        });
}

/// A window within the window, such as the pet window's buffs: its frame,
/// then its pieces, clipped to what lies inside its border. It stretches
/// with the window where the skin anchors it.
fn view(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    view: &eq_client_assets::sidl::View,
    inside: &Area,
    context: &Context,
) {
    let area = view.anchors.map_or(view.area, |anchors| {
        anchors.within(inside.width, inside.height)
    });
    window
        .spawn(at(
            inside.x + area.x,
            inside.y + area.y,
            area.width,
            area.height,
        ))
        .with_children(|frame| {
            let mut client = Area {
                x: 0.0,
                y: 0.0,
                width: area.width,
                height: area.height,
            };
            if let Some(template) = view.template.as_ref().filter(|_| view.border) {
                client = border(frame, art, &template.border, (area.width, area.height));
            }
            // One the skin gives a scrollbar scrolls its pieces beside it, as
            // the loot window's slots do.
            let bar = view.scrollbar.as_ref();
            let width = client.width - bar.map_or(0.0, scrollbar::width);
            let mut clipped = frame.spawn(Node {
                overflow: if bar.is_some() {
                    Overflow::scroll_y()
                } else {
                    Overflow::clip()
                },
                ..at(client.x, client.y, width.max(0.0), client.height)
            });
            if bar.is_some() {
                clipped.insert((
                    ScrollPosition::default(),
                    ScrolledView,
                    crate::windows::pointer::TakesWheel,
                ));
            }
            let scrolled = clipped.id();
            clipped.with_children(|clipped| {
                let origin = Area {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height: client.height,
                };
                pieces(clipped, art, &view.pieces, &origin, context);
                if bar.is_some() {
                    // As tall as its lowest piece, so all of them scroll into
                    // view.
                    let bottom = view
                        .pieces
                        .iter()
                        .filter_map(|(_, element)| placed_area(element))
                        .map(|area| area.y + area.height)
                        .fold(0.0, f32::max);
                    clipped.spawn(Node {
                        width: px(1),
                        height: px(bottom),
                        flex_shrink: 0.0,
                        ..default()
                    });
                }
            });
            if let Some(look) = bar {
                scrollbar::spawn(frame, art, look, &client, (scrolled, context.id));
            }
        });
}

/// A window within a window that scrolls its pieces.
#[derive(Component)]
pub(crate) struct ScrolledView;

/// Where a piece sits in its container, when the skin places it.
const fn placed_area(element: &Element) -> Option<Area> {
    match element {
        Element::InvSlot(slot) => Some(slot.area),
        Element::Button(button) => Some(button.area),
        Element::Label(label) => Some(label.area),
        Element::Gauge(gauge) => Some(gauge.area),
        Element::Image { area, .. } => Some(*area),
        _ => None,
    }
}

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

/// A tab box with every page: a row of tabs, each with its page's picture
/// or its words, across the top, and the chosen page below. A tab of words
/// is as wide as the font lays them out.
fn tabbed(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    tabs: &eq_client_assets::sidl::TabBox,
    inside: &Area,
    context: &Context,
) {
    let pages = with_qol_page(&tabs.pages, context.id, inside.height);
    let pages = pages.as_ref();
    let tab_box: std::sync::Arc<str> = tabs.name.as_str().into();
    let tab = |index| SkinTab {
        window: context.id,
        tab_box: tab_box.clone(),
        depth: context.depth,
        index,
    };
    let strip = tab_strip(pages.iter());
    window
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::FlexStart,
            column_gap: px(2),
            ..at(
                inside.x + 2.0,
                inside.y + 1.0,
                inside.width - 4.0,
                strip - 1.0,
            )
        })
        .with_children(|row| {
            for (index, page) in pages.iter().enumerate() {
                tab_cell(row, art, (page, tabs.tab_frame.as_ref()), tab(index));
            }
        });
    let area = Area {
        x: 0.0,
        y: 0.0,
        width: inside.width,
        height: inside.height - strip,
    };
    let within = Context {
        depth: context.depth + 1,
        ..*context
    };
    for (index, page) in pages.iter().enumerate() {
        window
            .spawn((
                SkinPage(tab(index)),
                Node {
                    display: Display::None,
                    overflow: Overflow::clip(),
                    ..at(inside.x, inside.y + strip, area.width, area.height)
                },
            ))
            .with_children(|page_area| {
                // The skin's page frame, where it names one, with the page's
                // pieces inside it.
                let inside = tabs.page_frame.as_ref().map_or(area, |look| {
                    let insets = frame::around(page_area, art, look);
                    Area {
                        x: insets.left,
                        y: insets.top,
                        width: area.width - insets.left - insets.right,
                        height: area.height - insets.top - insets.bottom,
                    }
                });
                pieces(page_area, art, &page.pieces, &inside, &within);
            });
    }
}

/// How tall a tab box's row of tabs is: its tallest tab, each showing its
/// page's picture or else its words, and a little room.
fn tab_strip<'a>(pages: impl Iterator<Item = &'a eq_client_assets::sidl::Page>) -> f32 {
    let height = |page: &eq_client_assets::sidl::Page| match (&page.icon[0], &page.title) {
        (Some(piece), _) => to_f32(piece.height),
        (None, Some(_)) => WORD_TAB_HEIGHT,
        (None, None) => 24.0,
    };
    pages.map(height).fold(0.0, f32::max) + 2.0
}

/// One tab in a tab box's row: its page's picture, or its words in a box
/// as wide as they lay out, in the skin's tab frame where the tab box names
/// one.
fn tab_cell(
    row: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    (page, tab_frame): (
        &eq_client_assets::sidl::Page,
        Option<&eq_client_assets::sidl::FrameLook>,
    ),
    tab: SkinTab,
) {
    let mut cell = row.spawn((Button, tab.clone()));
    if let Some(tooltip) = &page.tooltip {
        cell.insert(crate::tooltip::Tooltip(tooltip.clone()));
    }
    match (&page.icon[0], &page.title) {
        (Some(piece), _) => {
            let (width, height) = (to_f32(piece.width), to_f32(piece.height));
            cell.insert(Node {
                width: px(width),
                height: px(height),
                flex_shrink: 0.0,
                ..default()
            });
            cell.with_children(|cell| {
                for (face, piece) in page.icon.iter().enumerate() {
                    if let Some(image) = piece.as_ref().and_then(|piece| art.cut(piece)) {
                        cell.spawn((
                            image,
                            TabFace {
                                tab: tab.clone(),
                                active: face == 1,
                            },
                            at(0.0, 0.0, width, height),
                        ));
                    }
                }
            });
        }
        (None, Some(words)) => {
            let [rest, chosen] = page.title_colors;
            let colors = [
                rest.map_or(theme::INK, rgb),
                chosen.map_or(theme::INK_BRIGHT, rgb),
            ];
            let node = Node {
                height: px(WORD_TAB_HEIGHT),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            };
            match tab_frame {
                Some(look) => frame_tab(&mut cell, art, look, node),
                None => {
                    cell.insert((
                        BackgroundColor(theme::INSET),
                        BorderColor::all(theme::EDGE),
                        Node {
                            padding: UiRect::horizontal(px(5)),
                            border: UiRect::all(px(1)),
                            ..node
                        },
                    ));
                }
            }
            cell.with_child((
                TabWords { tab, colors },
                theme::text(words.as_str(), Size::Small, colors[0]),
                TextLayout::new(Justify::Center, LineBreak::NoWrap),
            ));
        }
        (None, None) => {
            cell.insert(Node {
                width: px(24),
                height: px(24),
                flex_shrink: 0.0,
                ..default()
            });
        }
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
            crate::outbox::Needs::Capability(eq_client_core::Capability::Casting),
            BackgroundColor(Color::NONE),
            node,
        ))
        .with_children(|gem_node| {
            if let Some(piece) = &gem.background {
                picture(gem_node, art, piece, at(0.0, 0.0, area.width, area.height));
            }
            let icon = (area.height - 6.0).max(8.0);
            gem_node.spawn(super::spell_icons::artwork(
                super::spell_icons::Source::Gem(usize::from(index)),
                icon,
            ));
            if let Some(piece) = &gem.holder {
                picture(gem_node, art, piece, at(0.0, 0.0, area.width, area.height));
            }
        });
}

/// A group button, in the group window or on the Actions window's Main page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupButton {
    Invite,
    Follow,
    Disband,
    Decline,
}

impl GroupButton {
    /// The slash command it runs: Invite invites the target, and Decline
    /// disbands, which with an invitation waiting declines it, as the
    /// installed string for an invitation says.
    const fn command(self) -> &'static str {
        match self {
            Self::Invite => "/invite",
            Self::Follow => "/follow",
            Self::Disband | Self::Decline => "/disband",
        }
    }

    /// Whether it answers an invitation, and so shows in the group window
    /// only while one waits.
    const fn answers(self) -> bool {
        matches!(self, Self::Follow | Self::Decline)
    }
}

/// A button of the Raid window that runs a raid slash command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RaidButton {
    Invite,
    Accept,
    Decline,
}

impl RaidButton {
    /// The slash command it runs: Invite invites the target, as
    /// `/raidinvite` with no name does.
    const fn command(self) -> &'static str {
        match self {
            Self::Invite => "/raidinvite",
            Self::Accept => "/raidaccept",
            Self::Decline => "/raiddecline",
        }
    }

    /// Whether it answers an invitation, and so shows only while one waits.
    const fn answers(self) -> bool {
        matches!(self, Self::Accept | Self::Decline)
    }
}

/// What a skin's button does in the client.
#[derive(Clone, Copy)]
enum Does {
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
    Ability(super::abilities::AbilityButton),
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
    Maps(super::map::MapButton),
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
fn trading_button(id: &str, owner: WindowId) -> Option<Does> {
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
fn does(id: &str, owner: WindowId) -> Option<Does> {
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
        return Some(super::map::MapButton::for_screen(id).map_or(Does::Nothing, Does::Maps));
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
fn spellbook_button(id: &str) -> Option<Does> {
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
const fn has_own_buttons(owner: WindowId) -> bool {
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
fn own_button(id: &str, owner: WindowId) -> Option<Does> {
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
fn character_button(id: &str) -> Does {
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

/// A coin box's count, right of its coin's picture.
fn coin_count(inner: &mut ChildSpawnerCommands, area: Area, shows: Shows, ink: Color) {
    aligned(
        inner,
        at(0.0, 5.0, area.width - 6.0, area.height - 5.0),
        Align::Right,
        (shows, theme::text("", Size::Body, ink)),
    );
}

/// The action bar slot a Hot Button window button holds, from
/// `HB_Button1` to `HB_Button10`.
fn hot_button(id: &str) -> Option<usize> {
    id.strip_prefix("HB_Button")?
        .parse::<usize>()
        .ok()
        .filter(|number| (1..=10).contains(number))
        .map(|number| number - 1)
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

/// The option an Options window checkbox turns on and off: one of the
/// official client's options this client keeps, or a quality-of-life
/// setting.
fn option_checkbox(id: &str) -> Option<eq_client_core::options::Toggle> {
    qol_checkbox(id)
        .map(eq_client_core::options::Toggle::Qol)
        .or_else(|| {
            OPTION_CHECKBOXES
                .iter()
                .find(|(checkbox, _)| *checkbox == id)
                .map(|(_, toggle)| *toggle)
        })
}

/// How the screen IDs of the quality-of-life page's checkboxes begin; each
/// ends with its fix's file name.
const QOL_CHECKBOX: &str = "EQC_QoL_";

/// The screen ID of a quality-of-life setting's checkbox.
fn qol_checkbox_id(fix: eq_client_core::qol::Fix) -> String {
    format!("{QOL_CHECKBOX}{}", fix.key())
}

/// The quality-of-life setting a checkbox turns on and off.
fn qol_checkbox(id: &str) -> Option<eq_client_core::qol::Fix> {
    eq_client_core::qol::Fix::from_key(id.strip_prefix(QOL_CHECKBOX)?)
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
    let mut all = pages.to_vec();
    all.push(eq_client_assets::sidl::Page {
        pieces: qol_checkboxes(&checkbox, room),
        ..qol
    });
    std::borrow::Cow::Owned(all)
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
    /// Room left around the checkboxes, and between columns.
    const MARGIN: f32 = 10.0;
    /// Room between one checkbox and the next one down.
    const GAP: f32 = 6.0;
    let row = checkbox.area.height + GAP;
    let width = checkbox.area.width.max(190.0);
    // As many rows as fit inside the margins, and at least one.
    let rows = (((room - 2.0 * MARGIN + GAP) / row).floor() as u32).max(1);
    (0u32..)
        .zip(eq_client_core::qol::Fix::settings())
        .map(|(index, fix)| {
            let mut checkbox = checkbox.clone();
            let id = qol_checkbox_id(fix);
            checkbox.id = Some(id.clone());
            checkbox.text = Some(fix.label().to_owned());
            checkbox.tooltip = Some(fix.tooltip().to_owned());
            checkbox.area.x = MARGIN + to_f32(index / rows) * (width + MARGIN);
            checkbox.area.y = MARGIN + to_f32(index % rows) * row;
            checkbox.area.width = width;
            (id, eq_client_assets::sidl::Element::Button(checkbox))
        })
        .collect()
}

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

/// The window an official selector button opens and closes, or hides and
/// shows again; those for windows this client does not have do nothing.
fn selector_button(id: &str) -> Option<WindowId> {
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
const fn effect_window(id: WindowId) -> Option<EffectWindow> {
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
const fn effect_label(kind: u32) -> Option<(EffectWindow, u32)> {
    match kind {
        500..=599 => Some((EffectWindow::Long, kind - 500)),
        600..=699 => Some((EffectWindow::Short, kind - 600)),
        _ => None,
    }
}

/// The buff a label names, if its window is an effects window.
fn buff_label(owner: WindowId, eq_type: Option<u32>) -> Option<(EffectWindow, u32)> {
    effect_window(owner).and(eq_type).and_then(effect_label)
}

/// Whether a pet command's button shows only while the pet sits (Stand) or
/// while it does not (Sit); the skin keeps the two in one place.
fn pet_posture_button(command: &str) -> Option<bool> {
    match command {
        "/pet stand up" => Some(true),
        "/pet sit down" => Some(false),
        _ => None,
    }
}

/// The Pet Info window's buttons: each command as `/pet` gives it, and the
/// pet's buff slots.
fn pet_button(id: &str) -> Does {
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
fn group_button(id: &str) -> Option<Does> {
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
fn raid_button(id: &str) -> Does {
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
fn raid_group(id: &str) -> Option<u8> {
    id.strip_prefix("RAID_Group")?
        .strip_suffix("Button")?
        .parse::<u8>()
        .ok()
        .filter(|number| (1..=12).contains(number))
        .map(|number| number - 1)
}

/// The Actions window's ability buttons: the Combat page's first to fourth
/// and the Abilities page's first to sixth.
fn ability_button(id: &str) -> Option<super::abilities::AbilityButton> {
    use super::abilities::{AbilityButton, Page};
    const PLACES: [&str; 6] = ["First", "Second", "Third", "Fourth", "Fifth", "Sixth"];
    let (page, rest) = match id.strip_prefix("ACP_") {
        Some(rest) => (Page::Combat, rest),
        None => (Page::Abilities, id.strip_prefix("AAP_")?),
    };
    let place = rest.strip_suffix("AbilityButton")?;
    let index = PLACES.iter().position(|name| *name == place)?;
    Some(AbilityButton { page, index })
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
    let area = match does {
        Does::Buff(_, index) if !button.placed => stacked(button.area, index, inside),
        _ => button.anchors.map_or(button.area, |anchors| {
            anchors.within(inside.width, inside.height)
        }),
    };
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
        _ => button.look.normal.as_ref(),
    };
    // A button the skin draws only under the pointer, as the default
    // spellbook's close mark, starts unseen (`buttons`).
    let unseen = look.is_none() && !matches!(does, Does::Nothing);
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

/// Where an effects window's button goes when the skin gives it no place:
/// down the window's left edge a row each, a pixel apart, then on in the
/// next column, so it lines up with the rows of the skins that leave their
/// buttons unplaced and name each buff beside it. How the official client
/// places them is not checked yet.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a window holds few rows"
)]
fn stacked(area: Area, index: u32, inside: &Area) -> Area {
    let pitch = area.height + 1.0;
    let rows = (((inside.height + 1.0) / pitch).floor() as u32).max(1);
    Area {
        x: to_f32(index / rows) * (area.width + 1.0),
        y: to_f32(index % rows) * pitch,
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
        Does::Attack => drawn.insert((
            Button,
            AttackButton,
            skin(),
            crate::outbox::Needs::Capability(Capability::Combat),
        )),
        Does::Ability(place) => drawn.insert((
            Button,
            place,
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
            super::hud::hotbar::Slot(index),
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
        Does::ItemIcon => {
            let (x, y, size) = decal_place(button);
            drawn.insert(super::items::ItemIcon { x, y, size })
        }
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
        Does::HotButton(index) => super::hud::hotbar::contents(inner, index),
        Does::PetBuff(slot) => {
            inner.spawn(super::spell_icons::artwork(
                super::spell_icons::Source::PetBuff(slot),
                area.height.min(area.width) - 4.0,
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
            inner.spawn(super::spell_icons::artwork(
                super::spell_icons::Source::Window(window, index),
                area.height.min(area.width) - 4.0,
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
        | Does::Attack
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

/// Runs a pressed slash button's command, as typing it would; a refusal
/// shows in chat.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn slash(
    online: Res<super::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    mut chat: ResMut<super::chat::ChatState>,
    buttons: Query<(&Interaction, &SlashButton), Changed<Interaction>>,
) {
    for (interaction, SlashButton(command)) in &buttons {
        if *interaction == Interaction::Pressed
            && let Err(reason) = super::chat::submit_game_command(command, &online, &outbox)
        {
            chat.history.push(super::chat::system_line(reason));
        }
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
fn border(
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

/// Whose a group window's gauge is, by its number (`EQType`): 11 to 15
/// the members' in their places, 17 to 21 their pets'.
const fn member_gauge(kind: u32) -> Option<Shows> {
    match kind {
        11..=15 => Some(Shows::Member((kind - 11) as usize)),
        17..=21 => Some(Shows::MemberPet((kind - 17) as usize)),
        _ => None,
    }
}

/// A gauge: its text, then its bar below it, filled as far as its fraction.
fn gauge(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    gauge: &Gauge,
    inside: &Area,
) {
    let area = gauge.area;
    let look = &gauge.look;
    // The bar is its piece's height, or less if the gauge is thinner, as the
    // pet's line under the player's hit points is.
    let bar_height = look
        .background
        .as_ref()
        .or(look.fill.as_ref())
        .map_or(4.0, |piece| to_f32(piece.height))
        .min(area.height - gauge.bar_offset.max(0.0))
        .max(1.0);
    // The bar runs from where the skin starts it to the gauge's right edge.
    let bar_width = (area.width - gauge.bar_left).max(0.0);
    let mut root = window.spawn(at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    ));
    if let Some(shows) = gauge.eq_type.and_then(member_gauge) {
        root.insert((shows, Visibility::Hidden));
    }
    root.with_children(|root| {
        if let Some(kind) = gauge.eq_type {
            let ink = gauge.text_color.map_or(theme::INK_BRIGHT, rgb);
            root.spawn((
                Shows::GaugeText(kind),
                theme::text("", Size::Small, ink),
                at(gauge.text_offset.0, gauge.text_offset.1, area.width, 12.0),
                TextLayout::new(Justify::Left, LineBreak::NoWrap),
            ));
        }
        root.spawn(at(gauge.bar_left, gauge.bar_offset, bar_width, bar_height))
            .with_children(|bar| {
                if let Some(piece) = &look.background {
                    picture(bar, art, piece, at(0.0, 0.0, bar_width, bar_height));
                }
                if let Some(piece) = &look.fill {
                    let mut fill = bar.spawn((
                        gauge.eq_type.map_or(Shows::Fill(0), Shows::Fill),
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            top: px(0),
                            width: percent(0),
                            height: px(bar_height),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                    ));
                    fill.with_children(|fill| {
                        if let Some(mut image) = art.cut(piece) {
                            image.color = gauge.fill_tint.map_or(Color::WHITE, rgb);
                            fill.spawn((image, at(0.0, 0.0, bar_width, bar_height)));
                        }
                    });
                }
                if let Some(piece) = &look.lines {
                    picture(bar, art, piece, at(0.0, 0.0, bar_width, bar_height));
                }
                if let Some(piece) = &look.cap_left {
                    picture(
                        bar,
                        art,
                        piece,
                        at(0.0, 0.0, to_f32(piece.width), bar_height),
                    );
                }
                if let Some(piece) = &look.cap_right {
                    let cap = to_f32(piece.width);
                    picture(bar, art, piece, at(bar_width - cap, 0.0, cap, bar_height));
                }
            });
    });
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
    let node = at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    );
    let buff_name = buff_label(owner, label.eq_type);
    match (label.eq_type, bag) {
        (Some(_), _) if let Some((effects, index)) = buff_name => aligned(
            window,
            node,
            label.align,
            (text, Shows::BuffName(effects, index)),
        ),
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

/// Text in a box the skin sizes, aligned in it as the skin aligns it, on
/// one line.
fn aligned(window: &mut ChildSpawnerCommands, mut node: Node, align: Align, text: impl Bundle) {
    node.justify_content = match align {
        Align::Left => JustifyContent::FlexStart,
        Align::Center => JustifyContent::Center,
        Align::Right => JustifyContent::FlexEnd,
    };
    node.overflow = Overflow::clip();
    window
        .spawn(node)
        .with_child((text, TextLayout::new(Justify::Left, LineBreak::NoWrap)));
}

/// A piece of the skin, stretched over this node.
fn picture(
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
fn at(x: f32, y: f32, width: f32, height: f32) -> Node {
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

fn rgb([red, green, blue]: [u8; 3]) -> Color {
    Color::srgb_u8(red, green, blue)
}

#[allow(clippy::cast_precision_loss, reason = "texture sizes are small")]
const fn to_f32(value: u32) -> f32 {
    value as f32
}

/// Whether a piece that shows only at times shows now; None for one that
/// always shows.
fn shown_now(
    shows: Shows,
    world: &eq_client_core::world::ClientWorld,
    combat: &super::combat::CombatState,
    trade: &super::trade::TradeState,
) -> Option<bool> {
    Some(match shows {
        Shows::Attacking => combat.auto_attack,
        Shows::WhilePetSits(sits) => {
            let pet_sits = world
                .pet()
                .and_then(|pet| world.posture(pet.state.spawn_id))
                == Some(eq_client_core::PostureState::Sitting);
            sits == pet_sits
        }
        Shows::WhileSelling(selling) => {
            matches!(trade.chosen(), Some(crate::trade::Chosen::Carried(_))) == selling
        }
        Shows::PetBuff(slot) => world
            .pet_buffs()
            .and_then(|buffs| buffs.slots.get(slot).copied().flatten())
            .is_some(),
        Shows::Buff(window, index) => world.buffs().in_window(window, index).is_some(),
        Shows::Member(place) => world.group_place(place).is_some(),
        Shows::MemberPet(place) => world.group_pet(place).is_some(),
        Shows::WhileInvited(invited) => world.group_invitation().is_some() == invited,
        Shows::WhileRaidInvited(invited) => world.raid_invitation().is_some() == invited,
        Shows::WhileRaidLocked(locked) => world.raid().is_some_and(|raid| raid.locked) == locked,
        _ => return None,
    })
}

/// Shows the world in the skinned windows' gauges and labels.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    online: Res<super::online::OnlineState>,
    (hud, inventory, names): (
        Res<super::hud::HudState>,
        Res<super::inventory::InventoryState>,
        Res<super::spellbook::SpellNames>,
    ),
    (combat, trade, book, requests): (
        Res<super::combat::CombatState>,
        Res<super::trade::TradeState>,
        Res<super::spellbook::BookView>,
        Res<super::hud::action_bar::ActionRequests>,
    ),
    mut fills: Query<(&Shows, &mut Node), Without<Text>>,
    mut texts: Query<(&Shows, &mut Text, &mut TextColor)>,
    mut boxes: Query<(&Shows, &mut Visibility)>,
) {
    let world = online.world();
    for (shows, mut visibility) in &mut boxes {
        let Some(shown) = shown_now(*shows, world, &combat, &trade) else {
            continue;
        };
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (shows, mut node) in &mut fills {
        let Shows::Fill(kind) = *shows else {
            continue;
        };
        let fraction = filled(world, (hud.resource_estimate, &requests), kind);
        let width = percent(fraction.clamp(0.0, 1.0) * 100.0);
        if node.width != width {
            node.width = width;
        }
    }
    for (shows, mut text, mut color) in &mut texts {
        let (wanted, tint) = match *shows {
            Shows::GaugeText(kind) => gauge_text(world, kind),
            // The spell the player casts.
            Shows::Label(134) => (
                super::hud::action_bar::cast_spell(world)
                    .map_or_else(String::new, |spell| names.label(spell)),
                None,
            ),
            Shows::Label(kind) => (label_text(world, hud.resource_estimate, kind), None),
            Shows::BuffName(window, index) => (
                world
                    .buffs()
                    .in_window(window, index)
                    .map_or_else(String::new, |shown| names.label(shown.spell_id())),
                None,
            ),
            Shows::Coins(place, coin) => (super::coins::shown(world, place, coin), None),
            Shows::Offered(coin) => (
                world
                    .offered_coins()
                    .map_or_else(String::new, |coins| coins.of(coin).to_string()),
                None,
            ),
            Shows::Banker => (inventory.banker().to_owned(), None),
            Shows::PracticePoints => (super::training::practice_points(world), None),
            Shows::Corpse | Shows::Merchant | Shows::ChosenName | Shows::ChosenPrice => {
                (trading_text(*shows, &trade, world), None)
            }
            Shows::BookName(place) => (
                book.spell(world.spell_book(), place)
                    .map_or_else(String::new, |spell| names.label(spell)),
                None,
            ),
            // Two pages to a spread, counted from one.
            Shows::BookPage(right) => {
                ((book.spread * 2 + 1 + usize::from(right)).to_string(), None)
            }
            // The sign shows with the number before it.
            Shows::MemberPercent(place) => (
                if world.group_health(place).is_some() {
                    "%".to_owned()
                } else {
                    String::new()
                },
                None,
            ),
            Shows::RaidCount | Shows::RaidLevel => (raid_text(*shows, world), None),
            Shows::Partner => (
                super::give::partner(world),
                Some(super::give::ink(world, super::give::Side::Theirs)),
            ),
            Shows::Trader => (
                world
                    .player()
                    .map_or_else(String::new, |player| player.name.clone()),
                Some(super::give::ink(world, super::give::Side::Mine)),
            ),
            Shows::Fill(_)
            | Shows::Attacking
            | Shows::WhilePetSits(_)
            | Shows::PetBuff(_)
            | Shows::Buff(..)
            | Shows::WhileSelling(_)
            | Shows::Member(_)
            | Shows::MemberPet(_)
            | Shows::WhileInvited(_)
            | Shows::WhileRaidInvited(_)
            | Shows::WhileRaidLocked(_) => {
                continue;
            }
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
        if let Some(tint) = tint
            && color.0 != tint
        {
            color.0 = tint;
        }
    }
}

/// What the Raid window's count of members and their average level say:
/// nought outside a raid, as the skin's own text says.
fn raid_text(shows: Shows, world: &eq_client_core::world::ClientWorld) -> String {
    let raid = world.raid();
    match shows {
        Shows::RaidCount => raid.map_or(0, |raid| raid.members.len()).to_string(),
        _ => raid
            .and_then(eq_client_core::world::Raid::level_average)
            .unwrap_or(0)
            .to_string(),
    }
}

/// What a loot or merchant window's label says. What the merchant pays for
/// a carried item is not known yet, so its price shows only for a ware.
fn trading_text(
    shows: Shows,
    trade: &super::trade::TradeState,
    world: &eq_client_core::world::ClientWorld,
) -> String {
    let chosen = || crate::trade::chosen(trade, world);
    match shows {
        Shows::Corpse => trade.corpse().unwrap_or_default().to_owned(),
        Shows::Merchant => trade.merchant().unwrap_or_default().to_owned(),
        Shows::ChosenName => {
            chosen().map_or_else(String::new, |(item, _)| item.details.name.clone())
        }
        Shows::ChosenPrice => chosen()
            .and_then(|(_, price)| price)
            .map_or_else(String::new, |price| {
                crate::trade::coin_text(u64::from(price))
            }),
        _ => String::new(),
    }
}

/// How full a gauge is drawn: as `fraction` says, or the spellbook's
/// memorization (9) and scribe (10), as their gauges' names say; empty
/// where nothing says.
fn filled(
    world: &eq_client_core::world::ClientWorld,
    (estimate, requests): (Option<(u32, u32)>, &super::hud::action_bar::ActionRequests),
    kind: u32,
) -> f32 {
    use super::hud::action_bar::{BookChange, book_progress};
    let now = std::time::Instant::now();
    match kind {
        9 => book_progress(world, requests, BookChange::Memorize, now),
        10 => book_progress(world, requests, BookChange::Scribe, now),
        _ => fraction(world, estimate, kind),
    }
    .unwrap_or(0.0)
}

/// How full a gauge is, by the official client's numbering.
#[allow(clippy::cast_possible_truncation, reason = "a fraction for drawing")]
fn fraction(
    world: &eq_client_core::world::ClientWorld,
    estimate: Option<(u32, u32)>,
    kind: u32,
) -> Option<f32> {
    use super::hud::Stat;
    let stat = |stat: Stat| stat.ratio(world, estimate).map(|ratio| ratio as f32);
    match kind {
        1 => stat(Stat::Hp),
        2 => stat(Stat::Mana),
        3 => stat(Stat::Stamina),
        4 => stat(Stat::Experience),
        6 => target_health(world).map(|percent| f32::from(percent) / 100.0),
        // The player's cast; empty until the server begins it.
        7 => Some(
            super::hud::action_bar::cast_progress(world, std::time::Instant::now()).unwrap_or(0.0),
        ),
        16 => world
            .pet()
            .and_then(|pet| world.health(pet.state.spawn_id))
            .map(|percent| f32::from(percent) / 100.0),
        // The group's other members, and their pets, in their places.
        11..=15 => world
            .group_health((kind - 11) as usize)
            .map(|percent| f32::from(percent) / 100.0),
        17..=21 => world
            .group_pet((kind - 17) as usize)
            .and_then(|pet| world.health(pet.state.spawn_id))
            .map(|percent| f32::from(percent) / 100.0),
        _ => None,
    }
}

/// A gauge's text: the player's name over their hit points, the target's,
/// in its consider colour, over theirs, the pet's over its own, and each
/// group member's over theirs.
fn gauge_text(world: &eq_client_core::world::ClientWorld, kind: u32) -> (String, Option<Color>) {
    match kind {
        1 => (
            world
                .player()
                .map_or_else(String::new, |player| player.name.clone()),
            None,
        ),
        6 => {
            let target = world.target().selected;
            let name = target
                .and_then(|id| world.spawn(id))
                .map(|spawn| eq_client_core::entities::display_name(&spawn.state.name))
                .or_else(|| {
                    world
                        .player()
                        .filter(|player| Some(player.spawn_id) == target)
                        .map(|player| player.name.clone())
                })
                .unwrap_or_default();
            let color = theme::con(target.and_then(|id| world.considered(id)));
            (name, Some(color))
        }
        // A group member's name over their health (inferred).
        11..=15 => (
            world
                .group_place((kind - 11) as usize)
                .unwrap_or_default()
                .to_owned(),
            None,
        ),
        // The pet's name, or the skin's own words without one.
        16 => (
            world.pet().map_or_else(
                || "No Pet".to_owned(),
                |pet| eq_client_core::entities::display_name(&pet.state.name),
            ),
            None,
        ),
        _ => (String::new(), None),
    }
}

/// A label's text, by the official client's numbering: percentages of the
/// player's hit points, mana and stamina, of the target's hit points and
/// of each group member's, and the members' names.
fn label_text(
    world: &eq_client_core::world::ClientWorld,
    estimate: Option<(u32, u32)>,
    kind: u32,
) -> String {
    let percent = |fraction: Option<f32>| {
        fraction.map_or_else(String::new, |fraction| {
            format!("{:.0}", (fraction * 100.0).clamp(0.0, 100.0))
        })
    };
    let player = world.player();
    let number = |value: Option<u32>| value.map_or_else(String::new, |value| value.to_string());
    let attribute = |pick: fn(&eq_client_core::BaseAttributes) -> i32| {
        player
            .and_then(|player| player.base_attributes.as_ref())
            .map_or_else(String::new, |attributes| pick(attributes).to_string())
    };
    let vitals = world.vitals();
    match kind {
        1 => player.map_or_else(String::new, |player| player.name.clone()),
        2 => player.map_or_else(String::new, |player| player.level.to_string()),
        3 => player
            .and_then(|player| player.class)
            .and_then(eq_client_core::classes::class_name)
            .unwrap_or_default()
            .to_owned(),
        4 => player
            .and_then(|player| player.deity)
            .and_then(eq_client_core::classes::deity_name)
            .unwrap_or_default()
            .to_owned(),
        // The profile's base attributes; what items and spells add is not
        // reported to the client.
        5 => attribute(|attributes| attributes.strength),
        6 => attribute(|attributes| attributes.stamina),
        7 => attribute(|attributes| attributes.dexterity),
        8 => attribute(|attributes| attributes.agility),
        9 => attribute(|attributes| attributes.wisdom),
        10 => attribute(|attributes| attributes.intelligence),
        11 => attribute(|attributes| attributes.charisma),
        17 => number(world.hit_points().map(|(current, _)| current)),
        18 => number(world.hit_points().map(|(_, maximum)| maximum)),
        19 => percent(fraction(world, estimate, 1)),
        20 => percent(fraction(world, estimate, 2)),
        21 => percent(fraction(world, estimate, 3)),
        29 => target_health(world).map_or_else(String::new, |health| health.to_string()),
        // The group's other members' names and health, in their places.
        30..=34 => world
            .group_place((kind - 30) as usize)
            .unwrap_or_default()
            .to_owned(),
        35..=39 => world
            .group_health((kind - 35) as usize)
            .map_or_else(String::new, |health| health.to_string()),
        124 => number(vitals.mana),
        125 => number(estimate.map(|(mana, _)| mana)),
        126 => number(vitals.endurance),
        127 => number(estimate.map(|(_, endurance)| endurance)),
        _ => String::new(),
    }
}

/// The place and kind a skin's coin box holds, by its name: the purse's
/// (`IW_Money0` platinum to `IW_Money3` copper), the bank's and the give
/// window's.
fn coin_box(id: &str) -> Option<(CoinPlace, Coin)> {
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
fn their_coin_box(id: &str) -> Option<Coin> {
    let index = id.strip_prefix("TRDW_HisMoney")?.parse::<usize>().ok()?;
    Coin::ALL.get(index).copied()
}

/// The target's health in percent, the player's own when they target
/// themselves.
fn target_health(world: &eq_client_core::world::ClientWorld) -> Option<u8> {
    let target = world.target().selected?;
    world.health(target).or_else(|| {
        world
            .player()
            .filter(|player| player.spawn_id == target)
            .and_then(|player| player.hp_percent)
    })
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
            reshape(&mut node, &screen, placed, id);
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
    fn unplaced_effects_buttons_stack_down_the_window_a_row_each() {
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
        let place = |index| {
            let area = stacked(button, index, &window);
            (area.x, area.y)
        };
        assert_eq!(place(0), (0.0, 0.0));
        assert_eq!(place(1), (0.0, 25.0));
        // Fifteen rows fill the window; the sixteenth starts a column.
        assert_eq!(place(14), (0.0, 350.0));
        assert_eq!(place(15), (25.0, 0.0));
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
        // With no target, the target's gauge is empty.
        assert_eq!(gauge_text(world, 6).0, "");
        assert_eq!(fraction(world, None, 6), None);
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
        // Which piece is whose.
        assert_eq!(member_gauge(15), Some(Shows::Member(4)));
        assert_eq!(member_gauge(17), Some(Shows::MemberPet(0)));
        assert_eq!(member_gauge(16), None);
        assert_eq!(
            window_label(WindowId::Group, "GW_HPPercLabel3"),
            Some(Shows::MemberPercent(2))
        );
        assert_eq!(window_label(WindowId::Group, "GW_HPPercLabel6"), None);
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
        assert_eq!(
            group("AMP_FollowButton", WindowId::ActionsWindow),
            Some(GroupButton::Follow)
        );
        // Looking For Group is left out.
        assert!(does("LFGButton", WindowId::Group).is_none());
        // Decline disbands, which declines the invitation waiting.
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
