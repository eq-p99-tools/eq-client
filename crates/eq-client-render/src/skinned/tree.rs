//! Builds the installed skin's window, page, tab and scrolling-container trees.
//! Widget behavior and live values remain with the skin owner and presentation module.
use super::{
    Context, ScrolledText, ScrolledView, Shows, SkinPage, SkinTab, TABBED, TabFace, TabWords,
    WORD_TAB_HEIGHT, aligned, at, border, button, controls, frame, frame_tab, gauge, items, label,
    looks, rgb, scrollbar, spell_gem, title, to_f32, with_qol_page,
};
use crate::{
    theme::{self, Size},
    windows::WindowId,
};
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{Align, Element, Screen},
    ui::Area,
};

/// The frame's background, border and title bar, then the pieces inside it;
/// the bottom of its title bar, where it has one, to minimize it to.
pub(super) fn draw(
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
pub(super) fn pieces(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    pieces: &[(String, Element)],
    inside: &Area,
    context: &Context,
) {
    let inside = *inside;
    for (name, element) in pieces {
        if augment_section(context.id, name, element) {
            continue;
        }
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
                    crate::map::MapCanvas,
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
pub(super) fn text_box(
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
                    words.insert(crate::confirm::QuestionText);
                }
                (WindowId::Note, _) => {
                    words.insert(crate::reading::Text::Note);
                }
                (WindowId::Book, Some("Page0")) => {
                    words.insert(crate::reading::Text::Page(0));
                }
                (WindowId::Book, Some("Page1")) => {
                    words.insert(crate::reading::Text::Page(1));
                }
                (WindowId::Item, Some("ItemDescription")) => {
                    words.insert(crate::items::ItemText);
                }
                _ => (),
            }
        });
        if let Some(look) = bar {
            scrollbar::spawn(frame, art, look, &client, (words_box, Some(owner)));
        }
    });
}

/// One of the chat window's boxes, filled with the client's chat: its tabs
/// and lines in the output box, and the line the player types in the input
/// box.
pub(super) fn chat_box(
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
                    let lines = crate::chat::skinned_output(
                        frame,
                        at(client.x, client.y, client.width - width, client.height),
                        (art, tab_frame),
                    );
                    if let Some(look) = bar {
                        scrollbar::spawn(frame, art, look, &client, (lines, Some(WindowId::Chat)));
                    }
                }
                Some("CWChatInput") => crate::chat::skinned_input(
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
pub(super) fn view(
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
                scrollbar::spawn(frame, art, look, &client, (scrolled, Some(context.id)));
            }
        });
}

/// Whether a piece is part of a container window's augment section: its
/// list, its two labels (named by piece, as labels keep no `ScreenID`) and
/// its Insert, Remove and Delete buttons. The client does not augment, so
/// the section is not drawn at all rather than drawn greyed, as the project
/// owner asked; a skin parks it beside the slots, where a window fitted to
/// its bag ([`super::fit`]) would otherwise show its edge.
pub(super) fn augment_section(owner: WindowId, name: &str, element: &Element) -> bool {
    if !matches!(owner, WindowId::Bag(_) | WindowId::WorldContainer) {
        return false;
    }
    match element {
        Element::Button(button) => matches!(
            button.id.as_deref(),
            Some("InsertButton" | "RemoveButton" | "DeleteButton")
        ),
        Element::Listbox(list) => list.id.as_deref() == Some("AugmentList"),
        Element::Label(_) => matches!(name, "Augment_Label" | "AugmentInfoLabel"),
        _ => false,
    }
}

/// Where a piece sits in its container, when the skin places it.
pub(super) const fn placed_area(element: &Element) -> Option<Area> {
    match element {
        Element::InvSlot(slot) => Some(slot.area),
        Element::Button(button) => Some(button.area),
        Element::Label(label) => Some(label.area),
        Element::Gauge(gauge) => Some(gauge.area),
        Element::Image { area, .. } => Some(*area),
        _ => None,
    }
}

/// A tab box with every page: a row of tabs, each with its page's picture
/// or its words, across the top, and the chosen page below. A tab of words
/// is as wide as the font lays them out.
pub(super) fn tabbed(
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
pub(super) fn tab_strip<'a>(pages: impl Iterator<Item = &'a eq_client_assets::sidl::Page>) -> f32 {
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
pub(super) fn tab_cell(
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
