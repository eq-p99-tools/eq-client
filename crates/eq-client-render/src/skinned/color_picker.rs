//! A color field backed by three existing percentage options. The UI always
//! previews the representable color that the options file will restore.
use super::{at, controls::SkinSlider};
use crate::{
    keys::{Typing, search},
    options::OptionsState,
    theme,
    windows::{Shown, WindowId},
};
use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
    window::PrimaryWindow,
};
use eq_client_core::options::{Level, Options};

pub(super) const COMBAT_COLORS: [(&str, &str, [Level; 3]); 2] = [
    (
        "EQC_CombatOutgoingPicker",
        "Other damage",
        [
            Level::CombatTextOutgoingRed,
            Level::CombatTextOutgoingGreen,
            Level::CombatTextOutgoingBlue,
        ],
    ),
    (
        "EQC_CombatIncomingPicker",
        "Damage to you",
        [
            Level::CombatTextIncomingRed,
            Level::CombatTextIncomingGreen,
            Level::CombatTextIncomingBlue,
        ],
    ),
];

mod popup;
pub(super) use popup::{ResetColors, draw_swatch};
pub(crate) use popup::{manage, show_swatches};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Binding {
    Combat([Level; 3]),
    Chat(u8),
}

pub(super) fn binding(name: &str) -> Option<Binding> {
    if let Some(kind) = name.strip_prefix("EQC_ChatColor_") {
        let kind: u8 = kind.parse().ok()?;
        return (kind > 0 && usize::from(kind) < eq_client_core::options::USER_COLOR_KINDS)
            .then_some(Binding::Chat(kind));
    }
    COMBAT_COLORS
        .iter()
        .find(|(id, ..)| *id == name)
        .map(|(_, _, levels)| Binding::Combat(*levels))
}

impl Binding {
    fn read(self, options: &Options, look: Option<&crate::chat::ChatLook>) -> Srgba {
        match self {
            Self::Combat(levels) => color(levels.map(|level| options.level(level))),
            Self::Chat(kind) => {
                let [r, g, b] = options.user_colors[usize::from(kind)]
                    .or_else(|| look.and_then(|l| l.message_color(255 + u32::from(kind))))
                    .unwrap_or([255; 3]);
                Srgba::rgb_u8(r, g, b)
            }
        }
    }

    fn write(self, options: &mut Options, rgb: Srgba) {
        match self {
            Self::Combat(levels) => {
                for (level, fraction) in levels.into_iter().zip([rgb.red, rgb.green, rgb.blue]) {
                    options.set_level(level, level.at(fraction));
                }
            }
            Self::Chat(kind) => {
                let [r, g, b, _] = rgb.to_u8_array();
                options.user_colors[usize::from(kind)] = Some([r, g, b]);
            }
        }
    }
}

const GAP: f32 = 4.0;
const FOOT: f32 = 20.0;

fn square(width: f32) -> f32 {
    (width - GAP - 15.0).clamp(0.0, 80.0)
}

pub(super) fn height(width: f32) -> f32 {
    square(width) + GAP + FOOT
}

#[derive(Component)]
pub(crate) struct Picker {
    binding: Binding,
    hsv: Hsva,
    saved: Option<Srgba>,
}

impl Picker {
    fn load(&mut self, options: &Options, look: Option<&crate::chat::ChatLook>) {
        let rgb = self.binding.read(options, look);
        if self.saved != Some(rgb) {
            self.hsv = Hsva::from(rgb);
            self.saved = Some(rgb);
        }
    }

    fn store(&mut self, state: &mut ResMut<OptionsState>, rgb: Srgba) {
        self.binding.write(&mut state.options, rgb);
        self.saved = Some(self.binding.read(&state.options, None));
    }
}

fn color(levels: [u16; 3]) -> Srgba {
    let [r, g, b] = levels.map(|n| f32::from(n.min(100)) / 100.0);
    Srgba::rgb(r, g, b)
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Square(Entity),
    Hue(Entity),
    Hex(Entity),
}

#[derive(Component)]
pub(crate) enum Paint {
    Square(Entity),
    Swatch(Entity),
    Cursor(Entity),
    HueMark(Entity),
    Hex(Entity),
}

fn hue_color(hue: f32) -> Color {
    Color::from(Hsva::hsv(hue, 1.0, 1.0))
}

fn gradient(line: LinearGradient) -> BackgroundGradient {
    line.in_srgb().into()
}

fn target(part: Part, node: Node) -> impl Bundle {
    (part, Button, node, RelativeCursorPosition::default())
}

/// Draw the same field for any binding. Semantic slider commands still aim
/// at visible controls in the options page and inherit its scroll and occlusion.
pub(super) fn draw(
    parent: &mut ChildSpawnerCommands,
    binding: Binding,
    node: Node,
    width: f32,
    font: theme::Size,
) {
    let mut root = parent.spawn((
        node,
        Picker {
            binding,
            hsv: Hsva::default(),
            saved: None,
        },
    ));
    let id = root.id();
    let side = square(width);
    let field_width = (width - FOOT - GAP).clamp(0.0, 86.0);
    root.with_children(|ui| {
        ui.spawn((
            target(
                Part::Square(id),
                Node {
                    overflow: Overflow::clip(),
                    ..at(0.0, 0.0, side, side)
                },
            ),
            Paint::Square(id),
            BackgroundColor(Color::WHITE),
            gradient(LinearGradient::to_right(vec![
                Color::WHITE.into(),
                Color::srgba(1.0, 1.0, 1.0, 0.0).into(),
            ])),
        ))
        .with_children(|field| {
            field.spawn((
                FocusPolicy::Pass,
                at(0.0, 0.0, side, side),
                gradient(LinearGradient::to_bottom(vec![
                    Color::srgba(0.0, 0.0, 0.0, 0.0).into(),
                    Color::BLACK.into(),
                ])),
            ));
            field.spawn((Paint::Cursor(id), marker(6.0, 6.0, -3.0)));
        });
        let hues = [0, 60, 120, 180, 240, 300, 360].map(|h: u16| hue_color(f32::from(h)).into());
        ui.spawn((
            target(
                Part::Hue(id),
                Node {
                    overflow: Overflow::clip(),
                    ..at(side + GAP, 0.0, (width - side - GAP).clamp(0.0, 15.0), side)
                },
            ),
            gradient(LinearGradient::to_bottom(hues.into())),
        ))
        .with_child((Paint::HueMark(id), marker(15.0, 3.0, 0.0)));
        hex_field(ui, id, side + GAP, field_width, font);
        swatch(ui, id, width, field_width, side + GAP);
    });
}

fn hex_field(
    ui: &mut ChildSpawnerCommands,
    id: Entity,
    y: f32,
    field_width: f32,
    font: theme::Size,
) {
    ui.spawn((
        target(
            Part::Hex(id),
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                overflow: Overflow::clip(),
                ..at(0.0, y, field_width, FOOT)
            },
        ),
        BorderColor::all(theme::EDGE_LIGHT),
        BackgroundColor(theme::WELL),
        crate::tooltip::Tooltip("Enter #RRGGBB, then press Enter to save. Escape cancels.".into()),
    ))
    .with_child((
        Paint::Hex(id),
        FocusPolicy::Pass,
        theme::text("", font, theme::INK_BRIGHT),
    ));
}

fn swatch(ui: &mut ChildSpawnerCommands, id: Entity, width: f32, field_width: f32, y: f32) {
    let swatch_width = (width - field_width - GAP).clamp(0.0, FOOT);
    ui.spawn((
        Paint::Swatch(id),
        Node {
            border: UiRect::all(px(1)),
            ..at((field_width + GAP).min(width), y, swatch_width, FOOT)
        },
        BorderColor::all(theme::EDGE_LIGHT),
        BackgroundColor(Color::BLACK),
    ));
}

/// A legacy script channel's pointer target inside the preview swatch.
#[derive(Component)]
pub(crate) struct ScriptChannel;

fn marker(width: f32, height: f32, left: f32) -> impl Bundle {
    (
        FocusPolicy::Pass,
        Node {
            border: UiRect::all(px(1)),
            margin: UiRect {
                left: px(left),
                top: px(-height / 2.0),
                ..default()
            },
            ..at(0.0, 0.0, width, height)
        },
        BorderColor::all(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
    )
}

#[derive(Resource, Default)]
pub(crate) struct HexEdit {
    field: Option<Entity>,
    draft: String,
    paste: Option<ClipboardRead>,
    profile: Option<crate::profile_files::CharacterProfile>,
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(crate) fn focus(
    (mouse, shown, profile): (
        Res<ButtonInput<MouseButton>>,
        Res<Shown>,
        Res<crate::profile_files::Profile>,
    ),
    windows: Query<&Window, With<PrimaryWindow>>,
    fields: Query<(&Part, &Interaction, &InheritedVisibility, &ComputedNode)>,
    mut edit: ResMut<HexEdit>,
    mut typing: ResMut<Typing>,
) {
    let old = edit.field;
    let clicked = mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]);
    let field = if shown.displayed(WindowId::Options)
        && windows.single().is_ok_and(|window| window.focused)
    {
        fields
            .iter()
            .find_map(|(part, interaction, visible, node)| match part {
                Part::Hex(id)
                    if visible.get()
                        && node.size().min_element() > 0.0
                        && (if clicked {
                            mouse.just_pressed(MouseButton::Left)
                                && *interaction == Interaction::Pressed
                        } else {
                            Some(*id) == old && edit.profile.as_ref() == profile.key_ref()
                        }) =>
                {
                    Some(*id)
                }
                _ => None,
            })
    } else {
        None
    };
    if old != field {
        *edit = HexEdit {
            field,
            profile: profile.key(),
            ..default()
        };
    }
    typing.color_hex = field.is_some();
    if typing.color_hex {
        typing.composing = false;
        typing.counting = false;
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn edit_hex(
    mut events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<Key>>,
    mut clipboard: Option<ResMut<Clipboard>>,
    mut edit: ResMut<HexEdit>,
    mut typing: ResMut<Typing>,
    mut pickers: Query<&mut Picker>,
    mut options: ResMut<OptionsState>,
) {
    let HexEdit {
        field,
        draft,
        paste,
        ..
    } = &mut *edit;
    let Some(mut picker) = field.and_then(|id| pickers.get_mut(id).ok()) else {
        events.clear();
        return;
    };
    let events: Vec<_> = events.read().cloned().collect();
    match search::edit_up_to(draft, 32, &events, &keys, clipboard.as_deref_mut(), paste) {
        Some(search::Finish::Enter) => {
            let raw = draft.trim();
            let raw = raw.strip_prefix('#').unwrap_or(raw);
            if raw.len() == 6
                && raw.bytes().all(|byte| byte.is_ascii_hexdigit())
                && let Ok(rgb) = Srgba::hex(raw)
            {
                picker.hsv = rgb.into();
                picker.store(&mut options, rgb);
                *edit = HexEdit::default();
            }
        }
        Some(search::Finish::Escape) => {
            typing.escape_consumed = true;
            *edit = HexEdit::default();
        }
        None => {}
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn pick(
    look: Option<Res<crate::chat::ChatLook>>,
    inputs: Query<(&Part, &Interaction, &RelativeCursorPosition)>,
    mut pickers: Query<&mut Picker>,
    mut options: ResMut<OptionsState>,
) {
    for (part, interaction, cursor) in &inputs {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(at) = cursor
            .normalized
            .map(|n| (n + Vec2::splat(0.5)).clamp(Vec2::ZERO, Vec2::ONE))
        else {
            continue;
        };
        let (Part::Square(id) | Part::Hue(id)) = *part else {
            continue;
        };
        let Ok(mut picker) = pickers.get_mut(id) else {
            continue;
        };
        picker.load(&options.options, look.as_deref());
        match part {
            Part::Square(_) => {
                picker.hsv.saturation = at.x;
                picker.hsv.value = 1.0 - at.y;
            }
            Part::Hue(_) => picker.hsv.hue = at.y * 360.0,
            Part::Hex(_) => unreachable!(),
        }
        let rgb = picker.hsv.into();
        picker.store(&mut options, rgb);
    }
}

type PaintedParts<'w, 's> = Query<
    'w,
    's,
    (
        &'static Paint,
        Option<&'static mut BackgroundColor>,
        Option<&'static mut Node>,
        Option<&'static mut Text>,
    ),
>;

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn present(
    look: Option<Res<crate::chat::ChatLook>>,
    options: Res<OptionsState>,
    edit: Res<HexEdit>,
    mut pickers: Query<&mut Picker>,
    mut parts: PaintedParts,
) {
    for mut picker in &mut pickers {
        picker.load(&options.options, look.as_deref());
    }
    for (paint, background, node, text) in &mut parts {
        let id = match *paint {
            Paint::Square(id)
            | Paint::Swatch(id)
            | Paint::Cursor(id)
            | Paint::HueMark(id)
            | Paint::Hex(id) => id,
        };
        let Ok(picker) = pickers.get(id) else {
            continue;
        };
        let rgb = picker.binding.read(&options.options, look.as_deref());
        match (paint, background, node, text) {
            (Paint::Square(_), Some(mut bg), ..) => {
                bg.set_if_neq(BackgroundColor(hue_color(picker.hsv.hue)));
            }
            (Paint::Swatch(_), Some(mut bg), ..) => {
                bg.set_if_neq(BackgroundColor(rgb.into()));
            }
            (Paint::Cursor(_), _, Some(mut node), _) => {
                let left = percent(100.0 * picker.hsv.saturation);
                let top = percent(100.0 * (1.0 - picker.hsv.value));
                if node.left != left || node.top != top {
                    node.left = left;
                    node.top = top;
                }
            }
            (Paint::HueMark(_), _, Some(mut node), _) => {
                let top = percent(picker.hsv.hue / 3.6);
                if node.top != top {
                    node.top = top;
                }
            }
            (Paint::Hex(_), _, _, Some(mut text)) => {
                let words = if edit.field == Some(id) {
                    format!("{}|", edit.draft)
                } else {
                    rgb.to_hex()
                };
                if text.0 != words {
                    text.0 = words;
                }
            }
            _ => {}
        }
    }
}
