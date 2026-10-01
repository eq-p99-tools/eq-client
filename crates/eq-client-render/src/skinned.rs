//! Windows drawn from the installed skin's definitions: for now the player
//! and target windows. Each keeps its place in the window registry and its
//! behaviour; the skin decides its size, frame and pieces, and what each
//! gauge and label shows follows the official client's numbering. A skin
//! without the window, or a viewer without an installation, keeps the
//! client's own chrome.
use super::windows::WindowId;
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{Align, ButtonLook, Element, Gauge, Label, Library, Piece, Screen},
    ui::Area,
};
use std::collections::HashMap;

/// The skin's file for a window the client draws from the skin, and the
/// window's name in it.
fn source(id: WindowId) -> Option<(&'static str, &'static str)> {
    let file = match id {
        WindowId::Player => "EQUI_PlayerWindow.xml",
        WindowId::Target => "EQUI_TargetWindow.xml",
        WindowId::Spells => "EQUI_CastSpellWnd.xml",
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
    /// The target window's line, under it: what became of the player's
    /// choice of target. The official client says it in the chat.
    TargetLine,
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

/// A button drawn from the skin, with its piece for each state.
#[derive(Component, Clone)]
pub(crate) struct SkinButton(ButtonLook);

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
        reshape(&mut node, screen, state.placed());
        super::windows::drag_anywhere(&mut commands, frame);
        background.0 = Color::NONE;
        *border = BorderColor::all(Color::NONE);
        commands.entity(frame).despawn_children();
        commands.entity(frame).with_children(|window| {
            draw(window, screen, &mut art);
            if *id == WindowId::Target {
                window.spawn((
                    Shows::TargetLine,
                    theme::text("", Size::Small, theme::INK),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: percent(100),
                        width: px(screen.area.width),
                        ..default()
                    },
                    TextLayout::new(Justify::Center, LineBreak::WordBoundary),
                ));
            }
        });
    }
}

/// Sizes the frame as the skin does. A window the player has not placed
/// opens where the skin puts it, as the official client opens it.
fn reshape(node: &mut Node, screen: &Screen, placed: bool) {
    node.width = px(screen.area.width);
    node.height = px(screen.area.height);
    node.padding = UiRect::ZERO;
    node.border = UiRect::ZERO;
    node.row_gap = Val::ZERO;
    if !placed {
        node.position_type = PositionType::Absolute;
        node.left = px(screen.area.x);
        node.top = px(screen.area.y);
        node.right = Val::Auto;
        node.bottom = Val::Auto;
        node.margin = UiRect::ZERO;
    }
}

/// The frame's background, border and title bar, then the pieces inside it.
fn draw(window: &mut ChildSpawnerCommands, screen: &Screen, art: &mut crate::sheets::Art) {
    let (width, height) = (screen.area.width, screen.area.height);
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
            title(window, art, &template.title, &mut inside);
        }
    }
    for (_, element) in &screen.pieces {
        match element {
            Element::Gauge(gauge) => self::gauge(window, art, gauge, &inside),
            Element::Label(label) => self::label(window, label, &inside),
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
            Element::Button(button) => self::button(window, art, button, &inside),
            Element::Other(_) => (),
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
            crate::outbox::Needs(eq_client_core::Capability::Casting),
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

/// A button the client knows what to do with: for now the spellbook's
/// toggle.
fn button(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    button: &eq_client_assets::sidl::Button,
    inside: &Area,
) {
    // Only buttons the client knows what to do with are drawn.
    let toggles = match button.id.as_deref() {
        Some("CSPW_SpellBook") => WindowId::Spellbook,
        _ => return,
    };
    let Some(image) = button.look.normal.as_ref().and_then(|piece| art.cut(piece)) else {
        return;
    };
    let area = button.area;
    let mut drawn = window.spawn((
        Button,
        super::windows::SelectorButton(toggles),
        SkinButton(button.look.clone()),
        image,
        at(
            inside.x + area.x,
            inside.y + area.y,
            area.width,
            area.height,
        ),
    ));
    if let Some(tooltip) = &button.tooltip {
        drawn.insert(crate::tooltip::Tooltip(tooltip.clone()));
    }
}

/// Draws each skin button in its state: on while its window is open,
/// lit under the pointer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    shown: Res<super::windows::Shown>,
    mut art: crate::sheets::Art,
    mut buttons: Query<(
        &SkinButton,
        &super::windows::SelectorButton,
        &Interaction,
        &mut ImageNode,
    )>,
) {
    for (SkinButton(look), selector, interaction, mut image) in &mut buttons {
        let on = shown.is_open(selector.0);
        let hovered = *interaction != Interaction::None;
        let piece = match (on, hovered) {
            (true, true) => look.pressed_flyby.as_ref().or(look.pressed.as_ref()),
            (true, false) => look.pressed.as_ref(),
            (false, true) => look.flyby.as_ref(),
            (false, false) => None,
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
/// begins below it.
fn title(
    window: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
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
        root.spawn(at(0.0, gauge.bar_offset, area.width, bar_height))
            .with_children(|bar| {
                if let Some(piece) = &look.background {
                    picture(bar, art, piece, at(0.0, 0.0, area.width, bar_height));
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
                            fill.spawn((image, at(0.0, 0.0, area.width, bar_height)));
                        }
                    });
                }
                if let Some(piece) = &look.lines {
                    picture(bar, art, piece, at(0.0, 0.0, area.width, bar_height));
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
                    picture(bar, art, piece, at(area.width - cap, 0.0, cap, bar_height));
                }
            });
    });
}

/// A label: fixed text, or what its number says it shows.
fn label(window: &mut ChildSpawnerCommands, label: &Label, inside: &Area) {
    let area = label.area;
    let mut text = window.spawn((
        theme::text(
            if label.eq_type.is_some() {
                ""
            } else {
                label.text.as_str()
            },
            font(label.font),
            label.color.map_or(theme::INK_BRIGHT, rgb),
        ),
        TextLayout::new(
            match label.align {
                Align::Left => Justify::Left,
                Align::Center => Justify::Center,
                Align::Right => Justify::Right,
            },
            LineBreak::NoWrap,
        ),
        at(
            inside.x + area.x,
            inside.y + area.y,
            area.width,
            area.height,
        ),
    ));
    if let Some(kind) = label.eq_type {
        text.insert(Shows::Label(kind));
    }
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
    hud: Res<super::hud::HudState>,
    combat: Res<super::combat::CombatState>,
    lines: Res<super::notices::Lines>,
    mut fills: Query<(&Shows, &mut Node), Without<Text>>,
    mut texts: Query<(&Shows, &mut Text, &mut TextColor)>,
    mut boxes: Query<(&Shows, &mut Visibility)>,
) {
    let world = online.world();
    for (shows, mut visibility) in &mut boxes {
        if *shows == Shows::Attacking {
            let wanted = if combat.auto_attack {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            visibility.set_if_neq(wanted);
        }
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
            Shows::Label(kind) => (label_text(world, hud.resource_estimate, kind), None),
            Shows::TargetLine => (
                lines.target.text(std::time::Instant::now()).to_owned(),
                None,
            ),
            Shows::Fill(_) | Shows::Attacking => continue,
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
    match kind {
        19 => percent(fraction(world, estimate, 1)),
        20 => percent(fraction(world, estimate, 2)),
        21 => percent(fraction(world, estimate, 3)),
        29 => target_health(world).map_or_else(String::new, |health| health.to_string()),
        _ => String::new(),
    }
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
