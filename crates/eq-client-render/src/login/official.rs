//! The installation's own login screens, drawn from the skin's login set
//! (`EQLSUI.xml`) as the official client draws them: its login screen
//! (`connect`) and its list of the login server's worlds (`serverselect`),
//! at the skin's size, 640 by 480, unscaled and centred on black. The
//! official client places these screens on a larger screen rather than
//! stretching them (its `eqlsUIConfig.ini` keeps their places for each
//! resolution); centring them is inferred.
//!
//! The keys and presses are the front end's own ([`super::form`],
//! [`super::worlds`]): each control carries the action of the client's own
//! windows, which show instead where the installation has no login set. The
//! skin's words are read from the installation; the login server picker
//! and the status lines are this client's, in its own words.
use super::{Availability, Field, FrontEnd, Screen as Showing, form, worlds};
use crate::{
    online::OnlineState,
    skinned::{self, at},
    theme,
    windows::pointer::TakesWheel,
};
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{
        Align, Anchors, Button as SkinnedButton, Element, InterfaceSet, LOGIN_SET, Label, Listbox,
        Screen, TextBox, View, WindowTemplate,
    },
    ui::Area,
};
use eq_client_core::world::ServerList;
use std::{collections::HashMap, path::Path, sync::Arc};

/// The skin's login screen: its file, and the screen's name there.
const LOGIN: (&str, &str) = ("EQLSUI_ConnectWnd.xml", "connect");

/// The skin's list of worlds: its file, and the screen's name there.
const WORLDS: (&str, &str) = ("EQLSUI_ServerSelectWnd.xml", "serverselect");

/// The official client's size for its login screens, where the skin gives
/// a screen none.
const SIZE: (f32, f32) = (640.0, 480.0);

/// Where the login server picker sits on the login screen in a window too
/// short to hold it above the screen ([`picker_place`]): above the account
/// box. Ours.
const PICKER: Area = Area {
    x: 325.0,
    y: 100.0,
    width: 244.0,
    height: 31.0,
};

/// The room the picker's name takes over it: its line, then a gap. Ours.
const PICKER_NAME: f32 = 18.0;

/// The gap between the picker and the screen's top edge, where the picker
/// sits above the screen. Ours.
const PICKER_GAP: f32 = 6.0;

/// Where the login screen says what is happening, under its Cancel button.
/// Ours.
const LOGIN_STATUS: Area = Area {
    x: 325.0,
    y: 440.0,
    width: 250.0,
    height: 36.0,
};

/// Where the list of worlds says what is happening, along the screen's
/// foot. Ours.
const WORLDS_STATUS: Area = Area {
    x: 40.0,
    y: 458.0,
    width: 560.0,
    height: 20.0,
};

/// The highlighted world's row. Ours: the skin gives a list no highlight.
const HIGHLIGHT: Color = Color::srgba(1.0, 0.86, 0.38, 0.28);

/// The skin's login screens.
pub(crate) struct Screens {
    /// The login screen.
    login: Screen,
    /// The list of worlds.
    worlds: Screen,
}

/// The login screens of the skin the windows follow, if it has them. Each
/// skin's are read once a run; a skin whose set cannot be read, or that
/// lacks either screen, is remembered as having none, and the client's own
/// windows show.
#[derive(Resource, Default)]
pub(crate) struct LoginLook {
    read: HashMap<String, Option<Arc<Screens>>>,
    current: Option<Arc<Screens>>,
}

impl LoginLook {
    /// Whether the skin's screens show, rather than the client's own
    /// windows.
    pub(crate) const fn official(&self) -> bool {
        self.current.is_some()
    }
}

/// Reads the login screens of the skin the windows follow, once for each
/// skin, while the run has a login screen.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn read(
    front: Res<FrontEnd>,
    settings: Res<crate::ViewerSettings>,
    skin: Res<crate::skin::UiSkin>,
    mut look: ResMut<LoginLook>,
) {
    let directory = settings.0.eq_directory.as_deref();
    let wanted = directory
        .filter(|_| front.has_login())
        .and_then(|directory| {
            if let Some(known) = look.read.get(&skin.0) {
                return known.clone();
            }
            let loaded = load(directory, &skin.0);
            look.read.insert(skin.0.clone(), loaded.clone());
            loaded
        });
    let same = match (&look.current, &wanted) {
        (Some(current), Some(wanted)) => Arc::ptr_eq(current, wanted),
        (current, wanted) => current.is_none() && wanted.is_none(),
    };
    if !same {
        look.current = wanted;
    }
}

/// The skin's login screens, if its login set defines both.
fn load(directory: &Path, skin: &str) -> Option<Arc<Screens>> {
    let set = InterfaceSet::read(directory, skin, LOGIN_SET)
        .inspect_err(|error| info!("Skin {skin} has no login screens: {error}"))
        .ok()?;
    let screen = |(file, name): (&str, &str)| {
        set.screen(file, name)
            .inspect_err(|error| warn!("Skin {skin} draws no {name}: {error}"))
            .ok()
    };
    Some(Arc::new(Screens {
        login: screen(LOGIN)?,
        worlds: screen(WORLDS)?,
    }))
}

/// The skin's login screen's root.
#[derive(Component)]
pub(crate) struct LoginRoot;

/// The skin's list of worlds' root.
#[derive(Component)]
pub(crate) struct WorldsRoot;

/// What a login screen shows: the front end, and on the list of worlds the
/// login server's list.
#[derive(Clone, Copy)]
struct Context<'a> {
    front: &'a FrontEnd,
    list: Option<&'a ServerList>,
}

/// Draws the skin's login screen while it shows, and again when what it
/// shows changes, or where the login server picker sits.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn login_screen(
    mut commands: Commands,
    (front, online, look): (Res<FrontEnd>, Res<OnlineState>, Res<LoginLook>),
    (windows, scale): (
        Query<&Window, With<bevy::window::PrimaryWindow>>,
        Option<Res<UiScale>>,
    ),
    mut art: crate::sheets::Art,
    roots: Query<Entity, With<LoginRoot>>,
    mut previous: Local<Option<String>>,
) {
    let screens = look
        .current
        .as_ref()
        .filter(|_| front.has_login() && front.screen(&online) == Showing::Login);
    // The window's height as the UI lays out, which places the picker.
    let factor = scale.as_ref().map_or(1.0, |scale| scale.0);
    let height = windows.single().ok().map(|window| window.height() / factor);
    let shown = screens.map(|screens| (screens, picker_place(height, size(&screens.login).1)));
    let signature = shown.map(|(screens, place)| {
        format!(
            "{:p}:{}:{}",
            Arc::as_ptr(screens),
            form::appearance(&front),
            place.y
        )
    });
    if *previous == signature {
        return;
    }
    *previous = signature;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some((screens, place)) = shown else {
        return;
    };
    let context = Context {
        front: &front,
        list: None,
    };
    let frame = stage(&mut commands, LoginRoot, &screens.login, |stage, inside| {
        pieces(stage, &mut art, &screens.login.pieces, &inside, context);
        status(stage, LOGIN_STATUS, form::guidance(&front), Justify::Center);
    });
    commands.entity(frame).with_children(|frame| {
        picker(frame, &mut art, (&screens.login, place), &front);
    });
}

/// Where the login server picker sits, from the screen's top left: on the
/// black above the screen, lined up with its boxes, so that it covers none
/// of the skin's art, where the window, as the UI lays out, has the room
/// there for its name, itself and a gap; else inside, above the account box
/// ([`PICKER`]). Ours.
fn picker_place(window_height: Option<f32>, screen_height: f32) -> Area {
    let room = window_height.map_or(0.0, |height| (height - screen_height) / 2.0);
    if room >= PICKER_NAME + PICKER.height + PICKER_GAP {
        Area {
            y: -(PICKER.height + PICKER_GAP),
            ..PICKER
        }
    } else {
        PICKER
    }
}

/// Draws the skin's list of worlds while it shows, and again when what it
/// shows changes; the same list, drawn again, keeps its place.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn worlds_screen(
    mut commands: Commands,
    (front, online, look): (Res<FrontEnd>, Res<OnlineState>, Res<LoginLook>),
    strings: Option<Res<worlds::LoginStrings>>,
    mut art: crate::sheets::Art,
    roots: Query<Entity, With<WorldsRoot>>,
    rows: Query<&ScrollPosition, With<worlds::Rows>>,
    (mut previous, mut built): (Local<Option<String>>, Local<Option<u64>>),
) {
    let list = online.world().servers();
    let shown = look
        .current
        .as_ref()
        .filter(|_| front.screen(&online) == Showing::Servers)
        .zip(list);
    let signature = shown.map(|(screens, list)| {
        format!(
            "{:p}:{}",
            Arc::as_ptr(screens),
            worlds::appearance(Some(list), &front)
        )
    });
    if *previous == signature {
        return;
    }
    *previous = signature;
    let same = shown.is_some_and(|(_, list)| *built == Some(list.selection_id));
    let offset = rows
        .iter()
        .next()
        .filter(|_| same)
        .map_or(0.0, |position| position.y);
    for root in &roots {
        commands.entity(root).despawn();
    }
    *built = None;
    let Some((screens, list)) = shown else {
        return;
    };
    let context = Context {
        front: &front,
        list: Some(list),
    };
    let strings = strings.as_deref().map(|strings| &strings.0);
    stage(
        &mut commands,
        WorldsRoot,
        &screens.worlds,
        |stage, inside| {
            pieces(stage, &mut art, &screens.worlds.pieces, &inside, context);
            status(
                stage,
                WORLDS_STATUS,
                worlds::guidance(list, &front, strings),
                Justify::Center,
            );
        },
    );
    if offset > 0.0 {
        commands.queue(move |world: &mut World| keep_place(world, offset));
    }
    *built = Some(list.selection_id);
}

/// Puts the list just drawn where the one it replaces was scrolled to.
fn keep_place(world: &mut World, offset: f32) {
    let mut rows = world.query_filtered::<&mut ScrollPosition, With<worlds::Rows>>();
    for mut position in rows.iter_mut(world) {
        position.y = offset;
    }
}

/// The screen's size: the skin's, else the official client's for its login
/// screens.
fn size(screen: &Screen) -> (f32, f32) {
    if screen.area.width > 0.0 && screen.area.height > 0.0 {
        (screen.area.width, screen.area.height)
    } else {
        SIZE
    }
}

/// A screen-filling black cover with the skin's screen in its middle, at
/// the skin's size; nothing of the screen draws outside it. Gives the
/// screen's frame, where this client's own pieces can reach past its edges.
fn stage(
    commands: &mut Commands,
    root: impl Bundle,
    screen: &Screen,
    draw: impl FnOnce(&mut ChildSpawnerCommands, Area),
) -> Entity {
    let (width, height) = size(screen);
    let mut frame = Entity::PLACEHOLDER;
    super::cover(commands, root)
        .insert(BackgroundColor(Color::BLACK))
        .with_children(|cover| {
            frame = cover
                .spawn(Node {
                    width: px(width),
                    height: px(height),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|within| {
                    within
                        .spawn(Node {
                            width: percent(100),
                            height: percent(100),
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|stage| {
                            draw(
                                stage,
                                Area {
                                    x: 0.0,
                                    y: 0.0,
                                    width,
                                    height,
                                },
                            );
                        });
                })
                .id();
        });
    frame
}

/// Where a piece sits inside its container: where the skin places it, or
/// where its anchors keep it as the container stretches.
fn place(area: Area, anchors: Option<Anchors>, inside: &Area) -> Area {
    let area = anchors.map_or(area, |anchors| anchors.within(inside.width, inside.height));
    Area {
        x: inside.x + area.x,
        y: inside.y + area.y,
        ..area
    }
}

/// A screen's pieces, inside this area, in drawing order.
fn pieces(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    pieces: &[(String, Element)],
    inside: &Area,
    context: Context,
) {
    for (name, element) in pieces {
        match element {
            Element::Image { area, piece, .. } => {
                let area = place(*area, None, inside);
                skinned::picture(
                    parent,
                    art,
                    piece,
                    at(area.x, area.y, area.width, area.height),
                );
            }
            Element::Label(label) => self::label(parent, name, label, inside, context),
            Element::Button(button) => self::button(parent, art, button, inside, context),
            Element::TextBox(text) => self::text_box(parent, art, text, inside, context),
            Element::Listbox(list) => self::list(parent, art, list, inside, context),
            Element::View(view) => self::view(parent, art, view, inside, context),
            _ => (),
        }
    }
}

/// A label: the skin's words, or what the client fills in. The world
/// Quick Connect and Play Last Server play on is the one last played on
/// this login server; the account's expansions, which the session does not
/// report, stay blank.
fn label(
    parent: &mut ChildSpawnerCommands,
    name: &str,
    label: &Label,
    inside: &Area,
    context: Context,
) {
    let words = match name {
        "LOGIN_QuickConnectToLabel" | "SERVERSELECT_LastServerLabel" => context
            .front
            .remembered_world()
            .unwrap_or_default()
            .to_owned(),
        "SERVERSELECT_ExpansionsLabel" => String::new(),
        _ => label.text.clone(),
    };
    let justify = match label.align {
        Align::Left => Justify::Left,
        Align::Center => Justify::Center,
        Align::Right => Justify::Right,
    };
    let area = place(label.area, label.anchors, inside);
    let ink = label.color.map_or(theme::INK_BRIGHT, skinned::rgb);
    words_at(
        parent,
        area,
        (words, label.font, ink),
        (justify, label.wraps),
    );
}

/// Words in a box, set as the skin sets them: on one line, or wrapping
/// where it lets them.
fn words_at(
    parent: &mut ChildSpawnerCommands,
    area: Area,
    (words, font, ink): (String, Option<u8>, Color),
    (justify, wraps): (Justify, bool),
) {
    let line_break = if wraps {
        LineBreak::WordBoundary
    } else {
        LineBreak::NoWrap
    };
    parent
        .spawn(Node {
            overflow: Overflow::clip(),
            ..at(area.x, area.y, area.width, area.height)
        })
        .with_child((
            theme::text(words, skinned::font(font), ink),
            TextLayout::new(justify, line_break),
            Node {
                width: percent(100),
                ..default()
            },
        ));
}

/// What a login screen's button does.
#[derive(Clone, Copy)]
enum Does {
    /// What the login screen's control of this action does.
    Login(form::Action),
    /// What the list of worlds' control of this action does.
    Worlds(worlds::Action),
}

/// What a button the skin names does on this screen, and whether it does
/// it now; none for a button this client has nothing for yet, as the login
/// chat, the news, the credits and ordering an expansion.
fn does(id: Option<&str>, context: Context) -> Option<(Does, bool)> {
    let front = context.front;
    let idle = !front.running();
    let availability = front.chosen().map(|server| &server.availability);
    let here = availability == Some(&Availability::Here);
    match (context.list, id?) {
        (None, "ConnectButton") => Some((
            Does::Login(form::Action::Connect),
            idle && !matches!(availability, None | Some(Availability::Unavailable(_))),
        )),
        // Quick Connect plays on a world, so it needs one remembered here.
        (None, "QuickConnectButton") => Some((
            Does::Login(form::Action::QuickConnect),
            idle && here && front.remembered_world().is_some(),
        )),
        (None, "CancelButton") => Some((Does::Login(form::Action::Cancel), true)),
        (Some(list), "PlayButton") => Some((
            Does::Worlds(worlds::Action::Play),
            list.asked.is_none() && front.highlighted.is_some(),
        )),
        (Some(list), "PlayLastServerButton") => Some((
            Does::Worlds(worlds::Action::PlayLast),
            list.asked.is_none()
                && list.servers.iter().any(|server| {
                    server.status.open()
                        && front
                            .remembered_world()
                            .is_some_and(|last| server.name.eq_ignore_ascii_case(last))
                }),
        )),
        // Exit leaves the list for the login screen (inferred: the official
        // client's leads back to a main menu this client does not draw).
        (Some(_), "ExitButton") => Some((Does::Worlds(worlds::Action::Back), true)),
        _ => None,
    }
}

/// A button: the skin's pictures and words, lit as the pointer moves over
/// it, greyed while it does nothing, and greyed for good, with the reason
/// on hover, where this client has nothing for it.
fn button(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    button: &SkinnedButton,
    inside: &Area,
    context: Context,
) {
    let area = place(button.area, button.anchors, inside);
    let does = does(button.id.as_deref(), context);
    let usable = does.is_some_and(|(_, usable)| usable);
    let mut drawn = parent.spawn((
        Button,
        skinned::SkinButton(button.look.clone()),
        at(area.x, area.y, area.width, area.height),
    ));
    let look = if usable {
        button.look.normal.as_ref()
    } else {
        button
            .look
            .disabled
            .as_ref()
            .or(button.look.normal.as_ref())
    };
    if let Some(image) = look.and_then(|piece| art.cut(piece)) {
        drawn.insert(image);
    }
    match does {
        Some((Does::Login(action), _)) => drawn.insert(action),
        Some((Does::Worlds(action), _)) => drawn.insert(action),
        None => drawn.insert(crate::outbox::Needs::Missing),
    };
    if !usable {
        drawn.insert(skinned::Greyed);
    }
    let Some(text) = &button.text else {
        return;
    };
    let ink = if usable {
        button.text_color.map_or(theme::INK_BRIGHT, skinned::rgb)
    } else {
        theme::INK_DIM
    };
    drawn.with_children(|inner| {
        centred(inner, area, (text.clone(), button.font, ink));
    });
}

/// Words in the middle of a box this size.
fn centred(
    parent: &mut ChildSpawnerCommands,
    area: Area,
    (words, font, ink): (String, Option<u8>, Color),
) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            overflow: Overflow::clip(),
            ..at(0.0, 0.0, area.width, area.height)
        })
        .with_child((
            theme::text(words, skinned::font(font), ink),
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
        ));
}

/// The background and border a template draws over a box this size; what
/// lies inside the border.
fn chrome(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    (template, border): (Option<&WindowTemplate>, bool),
    (width, height): (f32, f32),
) -> Area {
    let whole = Area {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let Some(template) = template else {
        return whole;
    };
    if let Some(image) = template
        .background
        .as_deref()
        .and_then(|file| art.texture(file))
    {
        parent.spawn((
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
    if border {
        skinned::border(parent, art, &template.border, (width, height))
    } else {
        whole
    }
}

/// A box of text: the account box, and the password box, which shows a
/// star for each letter. A click on one types there.
fn text_box(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    text: &TextBox,
    inside: &Area,
    context: Context,
) {
    let front = context.front;
    let area = place(text.area, text.anchors, inside);
    let filled = match (context.list, text.id.as_deref()) {
        (None, Some("UsernameEdit")) => {
            Some((form::Action::Account, front.account.clone(), Field::Account))
        }
        (None, Some("PasswordEdit")) => Some((
            form::Action::Password,
            "*".repeat(front.password.chars().count()),
            Field::Password,
        )),
        _ => None,
    };
    let mut drawn = parent.spawn(at(area.x, area.y, area.width, area.height));
    if let Some((action, ..)) = &filled {
        drawn.insert((Button, *action));
    }
    drawn.with_children(|frame| {
        let inner = chrome(
            frame,
            art,
            (text.template.as_ref(), true),
            (area.width, area.height),
        );
        let Some((_, words, field)) = filled else {
            return;
        };
        // A caret shows where typing goes, while the box takes it.
        let typing = front.field == field && !front.running();
        let words = if typing { format!("{words}_") } else { words };
        let ink = if front.running() {
            theme::INK_DIM
        } else {
            text.color.map_or(theme::INK_BRIGHT, skinned::rgb)
        };
        frame
            .spawn(Node {
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                padding: UiRect::horizontal(px(4)),
                ..at(inner.x, inner.y, inner.width, inner.height)
            })
            .with_child((
                theme::text(words, skinned::font(text.font), ink),
                TextLayout::new(Justify::Left, LineBreak::NoWrap),
            ));
    });
}

/// The list of worlds, in the skin's columns: each world's name, then how
/// many play there or that it is down or locked, with the highlighted world
/// lit; it scrolls, with the skin's scrollbar where the skin gives one.
fn list(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    list: &Listbox,
    inside: &Area,
    context: Context,
) {
    let Some(worlds) = context.list else {
        return;
    };
    let area = list
        .anchors
        .map(|anchors| anchors.within(inside.width, inside.height))
        .or(list.area)
        .map_or(*inside, |area| place(area, None, inside));
    let asked = worlds.asked.is_some();
    let size = skinned::font(list.font);
    let row_height = size.px() + 5.0;
    parent
        .spawn((
            Node {
                overflow: Overflow::clip(),
                ..at(area.x, area.y, area.width, area.height)
            },
            crate::windows::KeepsPress,
        ))
        .with_children(|frame| {
            let inner = chrome(
                frame,
                art,
                (list.template.as_ref(), true),
                (area.width, area.height),
            );
            let heading = skinned::headings(frame, art, list, (&inner, theme::INK_BRIGHT));
            let bar = list.scrollbar.as_ref();
            let bar_width = bar.map_or(0.0, skinned::scrollbar::width);
            let widths: Vec<f32> = list.columns.iter().map(|column| column.width).collect();
            let rows = frame
                .spawn((
                    worlds::Rows,
                    TakesWheel,
                    ScrollPosition::default(),
                    Node {
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        ..at(
                            inner.x + 2.0,
                            inner.y + heading + 2.0,
                            (inner.width - 4.0 - bar_width).max(0.0),
                            (inner.height - heading - 4.0).max(0.0),
                        )
                    },
                ))
                .with_children(|rows| {
                    for (index, server) in worlds.servers.iter().enumerate() {
                        let open = server.status.open();
                        let ink = if open && !asked {
                            theme::INK_BRIGHT
                        } else {
                            theme::INK_DIM
                        };
                        let highlighted = context.front.highlighted == Some(index);
                        let mut row = rows.spawn((
                            Button,
                            worlds::Action::World(index),
                            Node {
                                flex_shrink: 0.0,
                                height: px(row_height),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(if highlighted { HIGHLIGHT } else { Color::NONE }),
                        ));
                        if !open {
                            row.insert(crate::tooltip::Tooltip(
                                "This world takes no players now".to_owned(),
                            ));
                        }
                        row.with_children(|cells| {
                            let texts = [server.name.clone(), worlds::detail(server)];
                            for (index, words) in texts.into_iter().enumerate() {
                                let width = widths.get(index).copied().unwrap_or(100.0);
                                cells
                                    .spawn(Node {
                                        width: px(width),
                                        flex_shrink: 0.0,
                                        overflow: Overflow::clip(),
                                        padding: UiRect::horizontal(px(2)),
                                        ..default()
                                    })
                                    .with_child((
                                        theme::text(words, size, ink),
                                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                                    ));
                            }
                        });
                    }
                })
                .id();
            if let Some(look) = bar {
                skinned::scrollbar::spawn(frame, art, look, &inner, (rows, None));
            }
        });
}

/// A window within the screen, as the box of the account's expansions and
/// the list's backdrop: its template's background and, where it has one,
/// its border, then its pieces.
fn view(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    view: &View,
    inside: &Area,
    context: Context,
) {
    let area = place(view.area, view.anchors, inside);
    parent
        .spawn(Node {
            overflow: Overflow::clip(),
            ..at(area.x, area.y, area.width, area.height)
        })
        .with_children(|frame| {
            let inner = chrome(
                frame,
                art,
                (view.template.as_ref(), view.border),
                (area.width, area.height),
            );
            pieces(frame, art, &view.pieces, &inner, context);
        });
}

/// The login server picker, at its place ([`picker_place`]): its name over
/// a button, drawn as the skin draws its Connect button, that shows the
/// login server chosen and chooses the next one; hovering it says what
/// Connect does there. Ours: the official client logs in only where
/// `eqhost.txt` says.
fn picker(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    (screen, area): (&Screen, Area),
    front: &FrontEnd,
) {
    let Some(server) = front.chosen() else {
        return;
    };
    let connect = screen.pieces.iter().find_map(|(_, element)| match element {
        Element::Button(button) if button.id.as_deref() == Some("ConnectButton") => Some(button),
        _ => None,
    });
    let typeface = connect.and_then(|button| button.font);
    words_at(
        parent,
        Area {
            y: area.y - PICKER_NAME,
            height: 16.0,
            ..area
        },
        ("Login server".to_owned(), Some(2), theme::INK_BRIGHT),
        (Justify::Center, false),
    );
    let idle = !front.running();
    let mut drawn = parent.spawn((
        Button,
        form::Action::NextServer,
        at(area.x, area.y, area.width, area.height),
    ));
    if let Some(look) = connect.map(|button| &button.look) {
        drawn.insert(skinned::SkinButton(look.clone()));
        let piece = if idle {
            look.normal.as_ref()
        } else {
            look.disabled.as_ref().or(look.normal.as_ref())
        };
        if let Some(image) = piece.and_then(|piece| art.cut(piece)) {
            drawn.insert(image);
        }
    }
    if !idle {
        drawn.insert(skinned::Greyed);
    }
    let note = match &server.availability {
        Availability::Here => String::new(),
        Availability::Reopens(folder) => form::reopens(folder),
        Availability::Unavailable(reason) => reason.clone(),
    };
    let tip = if front.servers.len() > 1 {
        format!("Click for the next login server. {note}")
    } else {
        note
    };
    drawn.insert(crate::tooltip::Tooltip(tip.trim().to_owned()));
    let ink = match (&server.availability, idle) {
        (Availability::Unavailable(_), _) | (_, false) => theme::INK_DIM,
        _ => connect
            .and_then(|button| button.text_color)
            .map_or(theme::INK_BRIGHT, skinned::rgb),
    };
    drawn.with_children(|inner| centred(inner, area, (server.name.clone(), typeface, ink)));
}

/// A line of this client's own, saying what is happening.
fn status(parent: &mut ChildSpawnerCommands, area: Area, words: String, justify: Justify) {
    words_at(
        parent,
        area,
        (words, Some(2), theme::INK_BRIGHT),
        (justify, true),
    );
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Fake, Heard, server};
    use super::super::{LoginServer, Request, claim, watch};
    use super::*;
    use bevy::input::keyboard::{Key, KeyboardInput};
    use eq_client_core::{
        ClientCommand, WorldEvent,
        servers::{ServerChoice, ServerStatus},
    };
    use std::{
        fmt::Write as _,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    /// Pictures, a template and a backdrop the screens share, each defined
    /// outside the screen files, as the installed set defines its own.
    const PIECES: &str = r#"<XML>
        <Ui2DAnimation item="A_Plain"><Frames><Texture>pieces.tga</Texture>
            <Location><X>0</X><Y>0</Y></Location><Size><CX>64</CX><CY>16</CY></Size></Frames>
        </Ui2DAnimation>
        <Ui2DAnimation item="A_Dim"><Frames><Texture>pieces.tga</Texture>
            <Location><X>0</X><Y>16</Y></Location><Size><CX>64</CX><CY>16</CY></Size></Frames>
        </Ui2DAnimation>
        <WindowDrawTemplate item="WDT_Box"><Background>box.tga</Background></WindowDrawTemplate>
        <StaticAnimation item="Backdrop">
            <Location><X>0</X><Y>0</Y></Location><Size><CX>640</CX><CY>480</CY></Size>
            <Animation>A_Plain</Animation>
        </StaticAnimation>
    </XML>"#;

    /// A button of the test set, in this test's own words.
    fn button(item: &str, id: &str, y: u32) -> String {
        format!(
            "<Button item=\"{item}\"><ScreenID>{id}</ScreenID><Text>Skin {item}</Text>\
             <Location><X>40</X><Y>{y}</Y></Location><Size><CX>120</CX><CY>30</CY></Size>\
             <ButtonDrawTemplate><Normal>A_Plain</Normal><Disabled>A_Dim</Disabled>\
             </ButtonDrawTemplate></Button>"
        )
    }

    /// A label of the test set.
    fn label(item: &str, words: &str, y: u32) -> String {
        format!(
            "<Label item=\"{item}\"><Location><X>325</X><Y>{y}</Y></Location>\
             <Size><CX>200</CX><CY>16</CY></Size><Text>{words}</Text></Label>"
        )
    }

    /// A box of text of the test set.
    fn edit_box(id: &str, y: u32) -> String {
        format!(
            "<Editbox item=\"{id}Box\"><ScreenID>{id}</ScreenID>\
             <DrawTemplate>WDT_Box</DrawTemplate>\
             <Location><X>325</X><Y>{y}</Y></Location>\
             <Size><CX>200</CX><CY>24</CY></Size></Editbox>"
        )
    }

    /// The login screen of the test set.
    fn connect() -> String {
        let boxes = edit_box("UsernameEdit", 160) + &edit_box("PasswordEdit", 200);
        format!(
            "<XML>{}{}{boxes}{}{}{}{}\
             <Screen item=\"connect\"><Size><CX>640</CX><CY>480</CY></Size>\
             <Pieces>Backdrop</Pieces><Pieces>LOGIN_AccountLabel</Pieces>\
             <Pieces>LOGIN_QuickConnectToLabel</Pieces><Pieces>UsernameEditBox</Pieces>\
             <Pieces>PasswordEditBox</Pieces><Pieces>LOGIN_Connect</Pieces>\
             <Pieces>LOGIN_Quick</Pieces><Pieces>LOGIN_Cancel</Pieces>\
             <Pieces>LOGIN_Chat</Pieces></Screen></XML>",
            label("LOGIN_AccountLabel", "Skin account words", 140),
            label("LOGIN_QuickConnectToLabel", "Sample world", 420),
            button("LOGIN_Connect", "ConnectButton", 240),
            button("LOGIN_Quick", "QuickConnectButton", 280),
            button("LOGIN_Cancel", "CancelButton", 320),
            button("LOGIN_Chat", "ChatButton", 360),
        )
    }

    /// The list of worlds of the test set.
    fn server_select() -> String {
        format!(
            "<XML><Listbox item=\"SERVERSELECT_List\"><ScreenID>ServerList</ScreenID>\
             <DrawTemplate>WDT_Box</DrawTemplate>\
             <Location><X>40</X><Y>60</Y></Location><Size><CX>560</CX><CY>300</CY></Size>\
             <Columns><Width>300</Width><Heading>Skin name heading</Heading></Columns>\
             <Columns><Width>200</Width><Heading>Skin state heading</Heading></Columns>\
             </Listbox>{}{}{}{}{}\
             <Screen item=\"serverselect\"><Size><CX>640</CX><CY>480</CY></Size>\
             <Pieces>Backdrop</Pieces><Pieces>SERVERSELECT_List</Pieces>\
             <Pieces>SERVERSELECT_LastServerLabel</Pieces><Pieces>SERVERSELECT_Play</Pieces>\
             <Pieces>SERVERSELECT_PlayLast</Pieces><Pieces>SERVERSELECT_Exit</Pieces>\
             <Pieces>SERVERSELECT_News</Pieces></Screen></XML>",
            label("SERVERSELECT_LastServerLabel", "Sample last", 380),
            button("SERVERSELECT_Play", "PlayButton", 400),
            button("SERVERSELECT_PlayLast", "PlayLastServerButton", 400),
            button("SERVERSELECT_Exit", "ExitButton", 440),
            button("SERVERSELECT_News", "NewsButton", 440),
        )
    }

    /// An installation whose default skin has the test login set, or only
    /// these of its files.
    fn installation(name: &str, files: &[&str]) -> PathBuf {
        let install = std::env::temp_dir().join(format!(
            "eq-client-login-look-{}-{name}",
            std::process::id()
        ));
        let skin = install.join("uifiles").join("default");
        std::fs::create_dir_all(&skin).unwrap();
        let set = [
            ("EQLSUI_Pieces.xml", PIECES.to_owned()),
            (LOGIN.0, connect()),
            (WORLDS.0, server_select()),
        ];
        let mut index = String::from("<XML><Composite>");
        for (file, text) in set.iter().filter(|(file, _)| files.contains(file)) {
            std::fs::write(skin.join(file), text).unwrap();
            write!(index, "<Include>{file}</Include>").unwrap();
        }
        index.push_str("</Composite></XML>");
        std::fs::write(skin.join(LOGIN_SET), index).unwrap();
        install
    }

    /// Every file of the test set.
    const WHOLE: [&str; 3] = ["EQLSUI_Pieces.xml", LOGIN.0, WORLDS.0];

    /// An app on this installation, offering these login servers.
    fn launch(install: &Path, servers: Vec<LoginServer>) -> (App, Arc<Mutex<Heard>>) {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let fake = Fake {
            servers,
            heard: heard.clone(),
        };
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some(install.to_owned());
        app.insert_resource(FrontEnd::new(Box::new(fake)))
            .insert_resource(OnlineState::new(true))
            .insert_resource(crate::online::Updates(std::sync::Mutex::new(None)))
            .init_resource::<Time<Real>>()
            .add_message::<KeyboardInput>()
            .add_message::<AppExit>()
            .add_systems(
                Update,
                (
                    claim,
                    read,
                    (form::form, worlds::worlds),
                    login_screen,
                    worlds_screen,
                    watch,
                )
                    .chain(),
            );
        app.update();
        (app, heard)
    }

    /// The words the screens show.
    fn shown(app: &mut App) -> Vec<String> {
        app.world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect()
    }

    fn count<T: Component>(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<(), With<T>>()
            .iter(app.world())
            .count()
    }

    /// Whether the control with this action is greyed, and whether this
    /// client has nothing for it yet.
    fn control<T: Component + PartialEq>(app: &mut App, action: &T) -> (bool, bool) {
        app.world_mut()
            .query::<(&T, Has<skinned::Greyed>, Has<crate::outbox::Needs>)>()
            .iter(app.world())
            .find(|(each, ..)| *each == action)
            .map(|(_, greyed, missing)| (greyed, missing))
            .expect("the screen shows the control")
    }

    /// Presses the control with this action, as a click would.
    fn press<T: Component + PartialEq>(app: &mut App, action: &T) {
        let pressed = app
            .world_mut()
            .query::<(Entity, &T)>()
            .iter(app.world())
            .find(|(_, each)| *each == action)
            .map(|(entity, _)| entity)
            .expect("the screen shows the control");
        *app.world_mut().get_mut::<Interaction>(pressed).unwrap() = Interaction::Pressed;
        app.update();
    }

    fn type_text(app: &mut App, text: &str) {
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<Window>>()
            .single(app.world())
            .unwrap();
        for character in text.chars() {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::KeyA,
                logical_key: Key::Character(character.to_string().into()),
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        }
        app.update();
    }

    fn world(name: &str, status: ServerStatus, players: Option<u32>) -> ServerChoice {
        ServerChoice {
            name: name.into(),
            status,
            players,
            preferred: false,
        }
    }

    /// The login server lists three worlds, the first of them down.
    fn list(app: &mut App) {
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::ServerSelection {
                selection_id: 5,
                servers: vec![
                    world("Down", ServerStatus::Down, None),
                    world("Busy", ServerStatus::Up, Some(40)),
                    world("Quiet", ServerStatus::Up, Some(2)),
                ],
            }],
        );
        app.update();
    }

    /// What the session was asked since last looked.
    fn queue(heard: &Arc<Mutex<Heard>>) -> Vec<ClientCommand> {
        let heard = heard.lock().unwrap();
        std::iter::from_fn(|| heard.sessions[0].1.try_recv().ok()).collect()
    }

    #[test]
    fn the_skins_login_screen_shows_in_place_of_the_clients_own_and_logs_in() {
        let install = installation("login", &WHOLE);
        let (mut app, heard) = launch(&install, vec![server("Example", "remembered")]);
        std::fs::remove_dir_all(&install).unwrap();
        assert!(app.world().resource::<LoginLook>().official());
        assert_eq!(count::<LoginRoot>(&mut app), 1);
        assert_eq!(count::<form::Root>(&mut app), 0);
        // The skin's words, the remembered account, and the login server
        // picker, which is ours.
        let words = shown(&mut app);
        for expected in ["Skin account words", "Skin LOGIN_Connect", "remembered"] {
            assert!(words.iter().any(|text| text == expected), "{expected}");
        }
        assert!(words.iter().any(|text| text == "Example"));
        // Quick Connect needs a world remembered here; the login chat is
        // this client's to build yet.
        assert_eq!(
            control(&mut app, &form::Action::QuickConnect),
            (true, false)
        );
        assert_eq!(control(&mut app, &form::Action::Connect), (false, false));
        let chat = app
            .world_mut()
            .query_filtered::<Has<skinned::Greyed>, With<crate::outbox::Needs>>()
            .iter(app.world())
            .collect::<Vec<_>>();
        assert_eq!(chat, [true]);
        // The password shows as stars, never as typed.
        type_text(&mut app, "secret");
        let words = shown(&mut app);
        assert!(words.iter().any(|text| text == "******_"));
        assert!(!words.iter().any(|text| text.contains("secret")));
        press(&mut app, &form::Action::Connect);
        assert_eq!(
            heard.lock().unwrap().connects,
            [(0, "remembered".to_owned(), "secret".to_owned())]
        );
        // Under way, Connect is greyed and Cancel stops the login.
        assert_eq!(control(&mut app, &form::Action::Connect), (true, false));
        press(&mut app, &form::Action::Cancel);
        assert!(!app.world().resource::<FrontEnd>().running());
        assert!(super::super::Worker::finished(
            &heard.lock().unwrap().sessions[0].2
        ));
        assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 0);
        // With none under way, Cancel leaves the game.
        press(&mut app, &form::Action::Cancel);
        assert_ne!(app.world().resource::<Messages<AppExit>>().len(), 0);
    }

    #[test]
    fn the_skins_list_of_worlds_plays_the_highlighted_or_the_last_world() {
        let install = installation("worlds", &WHOLE);
        let mut remembered = server("Example", "someone");
        remembered.world = Some("Busy".into());
        let (mut app, heard) = launch(&install, vec![remembered]);
        std::fs::remove_dir_all(&install).unwrap();
        // The login screen names the world Quick Connect plays on.
        assert!(shown(&mut app).iter().any(|text| text == "Busy"));
        assert_eq!(
            control(&mut app, &form::Action::QuickConnect),
            (false, false)
        );
        app.world_mut().resource_mut::<FrontEnd>().request = Some(Request::Connect);
        app.update();
        list(&mut app);
        assert_eq!(count::<WorldsRoot>(&mut app), 1);
        assert_eq!(count::<LoginRoot>(&mut app), 0);
        assert_eq!(count::<worlds::Root>(&mut app), 0);
        let words = shown(&mut app);
        for expected in ["Skin name heading", "40 players", "Down", "2 players"] {
            assert!(words.iter().any(|text| text == expected), "{expected}");
        }
        // The last world's label names the world, never the skin's sample.
        assert_eq!(words.iter().filter(|text| *text == "Busy").count(), 2);
        assert!(!words.iter().any(|text| text == "Sample last"));
        // News is this client's to build yet.
        assert_eq!(count::<crate::outbox::Needs>(&mut app), 1);
        // A click highlights a world, and the list drawn again keeps its
        // place.
        app.world_mut()
            .query_filtered::<&mut ScrollPosition, With<worlds::Rows>>()
            .single_mut(app.world_mut())
            .unwrap()
            .y = 30.0;
        press(&mut app, &worlds::Action::World(2));
        assert_eq!(app.world().resource::<FrontEnd>().highlighted, Some(2));
        let offset = app
            .world_mut()
            .query_filtered::<&ScrollPosition, With<worlds::Rows>>()
            .single(app.world())
            .unwrap()
            .y;
        assert!((offset - 30.0).abs() < 0.01);
        // Play Last Server plays the remembered world, not the highlighted.
        press(&mut app, &worlds::Action::PlayLast);
        assert_eq!(
            queue(&heard),
            [ClientCommand::SelectServer {
                selection_id: 5,
                index: 1
            }]
        );
        // Asked, Play waits for the answer; Exit goes back to logging in.
        assert_eq!(control(&mut app, &worlds::Action::Play), (true, false));
        press(&mut app, &worlds::Action::Back);
        assert!(!app.world().resource::<FrontEnd>().running());
        assert_eq!(count::<WorldsRoot>(&mut app), 0);
        assert_eq!(count::<LoginRoot>(&mut app), 1);
    }

    #[test]
    fn quick_connect_plays_on_the_remembered_world_once_the_list_shows_it() {
        let install = installation("quick", &WHOLE);
        let mut remembered = server("Example", "someone");
        remembered.world = Some("Busy".into());
        let (mut app, heard) = launch(&install, vec![remembered]);
        std::fs::remove_dir_all(&install).unwrap();
        type_text(&mut app, "secret");
        press(&mut app, &form::Action::QuickConnect);
        assert_eq!(heard.lock().unwrap().connects.len(), 1);
        list(&mut app);
        app.update();
        assert_eq!(
            queue(&heard),
            [ClientCommand::SelectServer {
                selection_id: 5,
                index: 1
            }]
        );
        // A plain Connect leaves the choice to the player.
        let mut again = server("Example", "someone");
        again.world = Some("Busy".into());
        let install = installation("plain", &WHOLE);
        let (mut app, heard) = launch(&install, vec![again]);
        std::fs::remove_dir_all(&install).unwrap();
        type_text(&mut app, "secret");
        press(&mut app, &form::Action::Connect);
        list(&mut app);
        app.update();
        assert_eq!(queue(&heard), []);
        assert_eq!(app.world().resource::<FrontEnd>().highlighted, Some(1));
    }

    #[test]
    fn without_both_of_the_skins_screens_the_clients_own_windows_show() {
        for (name, files) in [
            ("none", &[][..]),
            ("half", &["EQLSUI_Pieces.xml", LOGIN.0][..]),
        ] {
            let install = installation(name, files);
            let (mut app, _) = launch(&install, vec![server("Example", "remembered")]);
            std::fs::remove_dir_all(&install).unwrap();
            assert!(!app.world().resource::<LoginLook>().official(), "{name}");
            assert_eq!(count::<LoginRoot>(&mut app), 0, "{name}");
            assert_eq!(count::<form::Root>(&mut app), 1, "{name}");
        }
    }

    #[test]
    fn the_picker_sits_above_the_screen_where_the_window_has_room_for_it() {
        let above = Area { y: -37.0, ..PICKER };
        // A 1080-row window leaves 300 rows above a 480-row screen.
        assert_eq!(picker_place(Some(1080.0), 480.0), above);
        // Its name, itself and the gap take 55 rows.
        assert_eq!(picker_place(Some(590.0), 480.0), above);
        assert_eq!(picker_place(Some(589.0), 480.0), PICKER);
        assert_eq!(picker_place(Some(480.0), 480.0), PICKER);
        // With no window to measure, it stays inside.
        assert_eq!(picker_place(None, 480.0), PICKER);
    }

    #[test]
    fn the_picker_moves_inside_the_screen_in_a_window_too_short_for_it_above() {
        let install = installation("picker", &WHOLE);
        let (mut app, _) = launch(&install, vec![server("Example", "remembered")]);
        std::fs::remove_dir_all(&install).unwrap();
        // Where the picker's top is, from the screen's top left.
        let top = |app: &mut App| {
            let mut pickers = app.world_mut().query::<(&form::Action, &Node)>();
            let tops = pickers
                .iter(app.world())
                .filter(|(action, _)| **action == form::Action::NextServer)
                .map(|(_, node)| node.top)
                .collect::<Vec<_>>();
            assert_eq!(tops.len(), 1, "one picker");
            tops[0]
        };
        let resize = |app: &mut App, height: f32| {
            let mut windows = app.world_mut().query::<&mut Window>();
            let mut window = windows.single_mut(app.world_mut()).unwrap();
            window.resolution.set(1280.0, height);
            app.update();
        };
        // The tests' window, 720 rows tall, has room above the screen.
        assert_eq!(top(&mut app), px(-37.0));
        assert!(shown(&mut app).iter().any(|text| text == "Login server"));
        // A short window draws the screen again with the picker inside, and
        // a tall one draws it above again.
        resize(&mut app, 480.0);
        assert_eq!(top(&mut app), px(100.0));
        assert_eq!(count::<LoginRoot>(&mut app), 1);
        resize(&mut app, 1080.0);
        assert_eq!(top(&mut app), px(-37.0));
        assert_eq!(count::<LoginRoot>(&mut app), 1);
    }
}
