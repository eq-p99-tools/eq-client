//! Windows drawn from the installed skin's definitions: the player, target
//! and spell windows, and the windows that hold items. Each keeps its place
//! in the window registry and its behaviour; the skin decides its size,
//! frame and pieces, and what each gauge and label shows follows the
//! official client's numbering. A skin without the window, or a viewer
//! without an installation, keeps the client's own chrome.
mod controls;
mod items;

pub(crate) use controls::{
    Choosing, DropDown, DropDownChoice, KeyFilter, LevelSlider, drop_downs, fill_lists,
    light_choices, scroll_lists, show_choices, show_levels, slide,
};

pub(crate) use items::{Closes, TheirSlot, close, contents, frames, picker, theirs, toggle_bag};

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
        WindowId::ActionsWindow => "EQUI_ActionsWindow.xml",
        WindowId::PetInfo => "EQUI_PetInfoWindow.xml",
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
        _ => return None,
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
}

/// The skin's windows as read, by skin: read once, whatever rebuilds the
/// frames they are drawn in.
#[derive(Resource, Default)]
pub(crate) struct Screens {
    libraries: HashMap<String, Option<Library>>,
    screens: HashMap<(String, WindowId), Option<Screen>>,
}

impl Screens {
    /// A window as this skin defines it, if it does.
    fn get(&mut self, directory: &std::path::Path, skin: &str, id: WindowId) -> Option<&Screen> {
        let (file, name) = source(id)?;
        let library = self
            .libraries
            .entry(skin.to_owned())
            .or_insert_with(|| {
                Library::read(directory, skin)
                    .inspect_err(|error| warn!("UI skin {skin} unreadable: {error}"))
                    .ok()
            })
            .as_ref();
        self.screens
            .entry((skin.to_owned(), id))
            .or_insert_with(|| {
                library?
                    .window(directory, skin, file, name)
                    .inspect_err(|error| warn!("Skin {skin} draws no {name}: {error}"))
                    .ok()
            })
            .as_ref()
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
}

/// A button drawn from the skin, with its piece for each state.
#[derive(Component, Clone)]
pub(crate) struct SkinButton(ButtonLook);

/// A skin button that cannot be used right now, drawn in its disabled look,
/// as Combine is while a combine waits for the server.
#[derive(Component)]
pub(crate) struct Greyed;

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
        &'static super::windows::Frame,
    ),
>;

/// Draws each skinned window from the skin: a frame just built, or one
/// drawn in another skin, is drawn again.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn apply(
    mut commands: Commands,
    skin: Res<super::skin::UiSkin>,
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
    for (frame, id, mut node, mut background, mut border, drawn, state) in &mut frames {
        if drawn.is_some_and(|drawn| drawn.0 == skin.0) {
            continue;
        }
        let Some(screen) = screens.get(directory, &skin.0, *id) else {
            continue;
        };
        commands.entity(frame).insert(Drawn(skin.0.clone()));
        skinned.0.insert(*id);
        reshape(&mut node, screen, state.placed(), *id);
        super::windows::drag_anywhere(&mut commands, frame, *id);
        background.0 = Color::NONE;
        *border = BorderColor::all(Color::NONE);
        commands.entity(frame).despawn_children();
        commands.entity(frame).with_children(|window| {
            let context = Context {
                id: *id,
                paperdoll: paperdoll.as_deref(),
                depth: 0,
            };
            draw(window, screen, &mut art, &context);
        });
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

/// The frame's background, border and title bar, then the pieces inside it.
fn draw(
    window: &mut ChildSpawnerCommands,
    screen: &Screen,
    art: &mut crate::sheets::Art,
    context: &Context,
) {
    let (width, height) = (screen.area.width, screen.area.height);
    // A window the skin sizes to nothing, as Velious hides its casting and
    // short effects windows, shows nothing, not even its frame.
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let mut inside = Area {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    if let Some(template) = &screen.template {
        if let Some(image) = template
            .background
            .as_deref()
            .and_then(|file| art.texture(file))
        {
            window.spawn((
                ImageNode {
                    image,
                    image_mode: NodeImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    },
                    ..default()
                },
                at(0.0, 0.0, width, height),
            ));
        }
        if screen.border {
            inside = border(window, art, &template.border, (width, height));
        }
        if screen.titlebar {
            title(window, art, screen, &template.title, &mut inside);
        }
    }
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
                chat_box(window, art, text, &inside);
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
/// wrapped inside the box's frame.
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
            if let Some(template) = &text.template {
                client = border(frame, art, &template.border, (area.width, area.height));
            }
            let ink = text.color.map_or(theme::INK_BRIGHT, rgb);
            let mut words = frame.spawn((
                theme::text("", Size::Body, ink),
                TextLayout::new(Justify::Left, LineBreak::WordBoundary),
                at(
                    client.x + 4.0,
                    client.y + 4.0,
                    (client.width - 8.0).max(0.0),
                    (client.height - 8.0).max(0.0),
                ),
            ));
            match (owner, text.id.as_deref()) {
                (WindowId::Confirmation, _) => {
                    words.insert(super::resurrection::QuestionText);
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
                _ => (),
            }
        });
}

/// One of the chat window's boxes, filled with the client's chat: its tabs
/// and lines in the output box, and the line the player types in the input
/// box.
fn chat_box(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    text: &eq_client_assets::sidl::TextBox,
    inside: &Area,
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
            let node = at(client.x, client.y, client.width, client.height);
            match text.id.as_deref() {
                Some("CWChatOutput") => super::chat::skinned_output(frame, node),
                Some("CWChatInput") => super::chat::skinned_input(frame, node),
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
            frame
                .spawn(Node {
                    overflow: Overflow::clip(),
                    ..at(client.x, client.y, client.width, client.height)
                })
                .with_children(|clipped| {
                    let origin = Area {
                        x: 0.0,
                        y: 0.0,
                        ..client
                    };
                    pieces(clipped, art, &view.pieces, &origin, context);
                });
        });
}

/// The height of a tab that shows its page's words.
const WORD_TAB_HEIGHT: f32 = 18.0;

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
    let pages = with_client_page(&tabs.pages, context.id);
    let pages = pages.as_ref();
    let tab_box: std::sync::Arc<str> = tabs.name.as_str().into();
    let tab = |index| SkinTab {
        window: context.id,
        tab_box: tab_box.clone(),
        depth: context.depth,
        index,
    };
    // A tab shows its page's picture, or else its words.
    let height = |page: &eq_client_assets::sidl::Page| match (&page.icon[0], &page.title) {
        (Some(piece), _) => to_f32(piece.height),
        (None, Some(_)) => WORD_TAB_HEIGHT,
        (None, None) => 24.0,
    };
    let strip = pages.iter().map(height).fold(0.0, f32::max) + 2.0;
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
                tab_cell(row, art, page, tab(index));
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
            .with_children(|page_area| pieces(page_area, art, &page.pieces, &area, &within));
    }
}

/// One tab in a tab box's row: its page's picture, or its words in a box
/// as wide as they lay out.
fn tab_cell(
    row: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    page: &eq_client_assets::sidl::Page,
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
            cell.insert((
                BackgroundColor(theme::INSET),
                BorderColor::all(theme::EDGE),
                Node {
                    height: px(WORD_TAB_HEIGHT),
                    padding: UiRect::horizontal(px(5)),
                    border: UiRect::all(px(1)),
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
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
    /// Nothing yet: drawn greyed out, as the client's own windows show what
    /// it or the server lacks.
    Nothing,
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
    // A window's Done button, or the give or trade window's Cancel, closes
    // it; even in a window whose other buttons do nothing yet.
    if matches!(
        id,
        "DoneButton" | "GVW_Cancel_Button" | "TRDW_Cancel_Button"
    ) {
        return Some(Does::Closes);
    }
    if owner == WindowId::PetInfo {
        return Some(pet_button(id));
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
        return Some(
            OPTION_CHECKBOXES
                .iter()
                .find(|(checkbox, _)| *checkbox == id)
                .map_or(Does::Nothing, |(_, toggle)| Does::Option(*toggle)),
        );
    }
    Some(match id {
        "CSPW_SpellBook" => Does::Toggles(WindowId::Spellbook),
        "IW_Skills" => Does::Toggles(WindowId::Skills),
        "GVW_Give_Button" | "TRDW_Trade_Button" => Does::Gives,
        "ACP_MeleeAttackButton" => Does::Attack,
        "AMP_SitButton" => Does::Slash("/sit"),
        "AMP_StandButton" => Does::Slash("/stand"),
        "AMP_CampButton" => Does::Slash("/camp"),
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

/// The Options window's checkboxes for the options this client keeps, by
/// screen ID; the Client page's is this client's own.
const OPTION_CHECKBOXES: [(&str, eq_client_core::options::Toggle); 8] = {
    use eq_client_core::options::Toggle;
    [
        ("OGP_PetWindowPopupCheckbox", Toggle::PetWindowPopup),
        ("ODP_ShowTargetRingCheckbox", Toggle::TargetRing),
        ("ODP_ShowHelmCheckbox", Toggle::ShowHelm),
        ("ODP_PCNamesCheckbox", Toggle::PcNames),
        ("ODP_NPCNamesCheckbox", Toggle::NpcNames),
        ("OMP_InvertYAxisCheckbox", Toggle::InvertY),
        ("OMP_MouseWheelZoomCheckbox", Toggle::WheelZoom),
        (CLIENT_FOOD_CHECKBOX, Toggle::SkipModifiedFood),
    ]
};

/// The Client page's checkbox, which this client adds to the skin's
/// Options window.
const CLIENT_FOOD_CHECKBOX: &str = "EQC_SkipModifiedFoodCheckbox";

/// The skin's pages of a tab box, and for the Options window a last page
/// of the options only this client has, drawn with the skin's own
/// checkbox.
fn with_client_page(
    pages: &[eq_client_assets::sidl::Page],
    owner: WindowId,
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
    let (Some(mut checkbox), Some(first)) = (checkbox, pages.first()) else {
        return std::borrow::Cow::Borrowed(pages);
    };
    checkbox.id = Some(CLIENT_FOOD_CHECKBOX.to_owned());
    checkbox.text = Some("Skip Food With Modifiers".to_owned());
    checkbox.tooltip = Some(
        "Leave food and drink with modifiers for you to eat or drink by hand when you get hungry or thirsty."
            .to_owned(),
    );
    checkbox.area.x = 10.0;
    checkbox.area.y = 10.0;
    checkbox.area.width = checkbox.area.width.max(170.0);
    let client = eq_client_assets::sidl::Page {
        name: "EQC_ClientPage".to_owned(),
        title: Some("Client".to_owned()),
        area: first.area,
        template: first.template.clone(),
        pieces: vec![(CLIENT_FOOD_CHECKBOX.to_owned(), Element::Button(checkbox))],
        icon: [None, None],
        title_colors: first.title_colors,
        tooltip: Some("Options only this client has.".to_owned()),
    };
    let mut all = pages.to_vec();
    all.push(client);
    std::borrow::Cow::Owned(all)
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
    let mut drawn = match look.and_then(|piece| art.cut(piece)) {
        Some(image) => window.spawn((image, node)),
        None => window.spawn(node),
    };
    behave(&mut drawn, does, button, owner);
    let ink = match does {
        Does::Nothing => theme::INK_DIM,
        _ => button.text_color.map_or(theme::INK_BRIGHT, rgb),
    };
    drawn.with_children(|inner| {
        // A buff slot's decal is the buff's own icon, which the caption draws.
        if let (Some(decal), Some(place)) = (&button.decal, button.decal_area)
            && !matches!(does, Does::PetBuff(_) | Does::Buff(..))
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

/// What a pressed skin button does, and the state it shows.
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
            drawn.insert((Button, super::windows::SelectorButton(toggles), skin()))
        }
        Does::Closes => drawn.insert((Button, items::Closes(owner))),
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
        Does::PetBuff(slot) => drawn.insert((Shows::PetBuff(slot), Visibility::Hidden)),
        // Hovering a buff names it, as the client's own window does.
        Does::Buff(window, index) => drawn.insert((
            Shows::Buff(window, index),
            Visibility::Hidden,
            Interaction::default(),
            super::tooltip::Tooltip::default(),
        )),
        Does::Option(toggle) => {
            drawn.insert((Button, super::options::OptionCheckbox(toggle), skin()))
        }
        Does::Trains => drawn.insert((
            Button,
            super::training::TrainButton,
            skin(),
            crate::outbox::Needs::Capability(Capability::Training),
        )),
        Does::Answers(accept) => drawn.insert((
            Button,
            super::resurrection::AnswerButton(accept),
            skin(),
            crate::outbox::Needs::Capability(Capability::Resurrection),
        )),
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
        Does::Offered(_) | Does::BagIcon | Does::Nothing => drawn,
    };
    // The skin keeps the pet's Stand under its Sit; one shows at a time.
    if let Does::Slash(command) = does
        && let Some(sits) = pet_posture_button(command)
    {
        drawn.insert((Shows::WhilePetSits(sits), Visibility::Hidden));
    }
    if let Some(tooltip) = &button.tooltip
        && !matches!(does, Does::Nothing)
    {
        drawn.insert(crate::tooltip::Tooltip(tooltip.clone()));
    }
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
        Does::Buff(window, index) => {
            inner.spawn(super::spell_icons::artwork(
                super::spell_icons::Source::Window(window, index),
                area.height.min(area.width) - 4.0,
            ));
        }
        // A box with a picture shows a value, such as the bank's coins;
        // the skin's text there is only a sample, so it stays blank until
        // the client has the value.
        Does::Nothing if button.decal.is_some() => (),
        Does::Toggles(_)
        | Does::Closes
        | Does::Gives
        | Does::Attack
        | Does::Slash(_)
        | Does::Option(_)
        | Does::Trains
        | Does::Answers(_)
        | Does::TurnsPage(_)
        | Does::Combines(_)
        | Does::Maps(_)
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
        Has<Greyed>,
        &'static Interaction,
        &'static mut ImageNode,
    ),
>;

/// Draws each skin button in its state: on while its window is open, the
/// player attacks or its option is on, lit under the pointer, and in its
/// disabled look while greyed.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    (shown, options): (
        Res<super::windows::Shown>,
        Res<super::options::OptionsState>,
    ),
    combat: Res<super::combat::CombatState>,
    mut art: crate::sheets::Art,
    mut buttons: Stateful,
) {
    for (SkinButton(look), selector, attack, checkbox, greyed, interaction, mut image) in
        &mut buttons
    {
        let on = match (selector, checkbox) {
            (Some(selector), _) => shown.is_open(selector.0),
            (None, Some(checkbox)) => options.on(checkbox.0),
            (None, None) => attack && combat.auto_attack,
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
        if let Some(wanted) = piece.and_then(|piece| art.cut(piece))
            && image.rect != wanted.rect
        {
            image.rect = wanted.rect;
        }
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
    let size = |piece: &Option<Piece>| {
        piece.as_ref().map_or((0.0, 0.0), |piece| {
            (to_f32(piece.width), to_f32(piece.height))
        })
    };
    let (left, _) = size(&border.left);
    let (right, _) = size(&border.right);
    let (_, top) = size(&border.top);
    let (_, bottom) = size(&border.bottom);
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
    screen: &Screen,
    [left, middle, right]: &[Option<Piece>; 3],
    inside: &mut Area,
) {
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
    if let Some(words) = &screen.title {
        let node = Node {
            align_items: AlignItems::Center,
            ..at(inside.x + left_width, inside.y, middle_width, tall)
        };
        let ink = screen.title_color.map_or(theme::INK_BRIGHT, rgb);
        let text = theme::text(words.as_str(), font(screen.font), ink);
        aligned(window, node, Align::Left, text);
    }
    if let Some(piece) = right {
        let x = inside.x + inside.width - right_width;
        picture(window, art, piece, at(x, inside.y, right_width, tall));
    }
    inside.y += tall;
    inside.height -= tall;
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
const fn font(number: Option<u8>) -> Size {
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

/// Shows the world in the skinned windows' gauges and labels.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    online: Res<super::online::OnlineState>,
    (hud, inventory, names): (
        Res<super::hud::HudState>,
        Res<super::inventory::InventoryState>,
        Res<super::spellbook::SpellNames>,
    ),
    combat: Res<super::combat::CombatState>,
    mut fills: Query<(&Shows, &mut Node), Without<Text>>,
    mut texts: Query<(&Shows, &mut Text, &mut TextColor)>,
    mut boxes: Query<(&Shows, &mut Visibility)>,
) {
    let world = online.world();
    let pet_sits = world
        .pet()
        .and_then(|pet| world.posture(pet.state.spawn_id))
        == Some(eq_client_core::PostureState::Sitting);
    for (shows, mut visibility) in &mut boxes {
        let shown = match *shows {
            Shows::Attacking => combat.auto_attack,
            Shows::WhilePetSits(sits) => sits == pet_sits,
            Shows::PetBuff(slot) => world
                .pet_buffs()
                .and_then(|buffs| buffs.slots.get(slot).copied().flatten())
                .is_some(),
            Shows::Buff(window, index) => world.buffs().in_window(window, index).is_some(),
            _ => continue,
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
        let fraction = fraction(world, hud.resource_estimate, kind).unwrap_or(0.0);
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
            | Shows::Buff(..) => {
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
        _ => None,
    }
}

/// A gauge's text: the player's name over their hit points, the target's,
/// in its consider colour, over theirs.
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
/// player's hit points, mana and stamina, and of the target's hit points.
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
        let screen = |titlebar, title: Option<&str>| Screen {
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
            titlebar,
            border: false,
            tooltip: None,
            pieces: Vec::new(),
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
                        };
                        commands
                            .spawn(Node::default())
                            .with_children(|window| draw(window, &screen, &mut art, &context));
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
            titlebar: false,
            border: false,
            tooltip: None,
            pieces: Vec::new(),
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
    fn the_options_window_checkboxes_turn_its_options_on_and_off() {
        use eq_client_assets::sidl::{Button, ButtonLook, Element, Page};
        use eq_client_core::options::Toggle;
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
        // The Client page comes last, with the skin's own checkbox.
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
        let pages = with_client_page(std::slice::from_ref(&general), WindowId::Options);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].title.as_deref(), Some("Client"));
        let Element::Button(food) = &pages[1].pieces[0].1 else {
            panic!("a checkbox")
        };
        assert!(matches!(
            does(food.id.as_deref().unwrap(), WindowId::Options),
            Some(Does::Option(Toggle::SkipModifiedFood))
        ));
        // Other windows keep the skin's pages as they are.
        assert_eq!(
            with_client_page(std::slice::from_ref(&general), WindowId::ActionsWindow).len(),
            1
        );
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
}
