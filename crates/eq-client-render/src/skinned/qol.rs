//! The client-added `QoL` page, using the installed skin's controls and text.
use super::{
    bindings::{LOOK_SPRING_CHECKBOX, qol_checkbox_id},
    controls, frame, to_f32,
    tree::tab_strip,
};
use crate::windows::WindowId;
use bevy::prelude::Vec2;
use eq_client_assets::{
    sidl::{Align, Button, Element, Label, Slider},
    ui::Area,
};
use eq_client_core::qol::Fix;

#[cfg(test)]
pub(super) mod tests;

/// This client's Look Spring checkbox, for a skin without one.
pub(super) const LOOK_SPRING_FALLBACK: &str = "EQC_LookSpringCheckbox";
const LOOK_SPRING_TEXT: &str = "Look Spring";
const LOOK_SPRING_TOOLTIP: &str = "Bring the first-person view back level when no look key or mouse look holds it, as the official client's Look Spring does.";

/// The official Look Spring option, where the skin has no checkbox for it:
/// added under Invert Y Axis on the skin's Mouse page, or else a choice for
/// this page.
fn look_spring(pages: &mut [eq_client_assets::sidl::Page]) -> Option<Choice> {
    let skin_has = pages
        .iter()
        .flat_map(|page| &page.pieces)
        .any(|(name, _)| name == LOOK_SPRING_CHECKBOX);
    (!skin_has && !add_look_spring(pages)).then_some(Choice::Official {
        id: LOOK_SPRING_FALLBACK,
        text: LOOK_SPRING_TEXT,
        tooltip: LOOK_SPRING_TOOLTIP,
    })
}

/// Adds a Look Spring checkbox, cut from the skin's Invert Y Axis one, below
/// the lowest control of the page that holds it; false where no page does.
fn add_look_spring(pages: &mut [eq_client_assets::sidl::Page]) -> bool {
    let invert = |name: &String| name == "OMP_InvertYAxisCheckbox";
    let Some(page) = pages
        .iter_mut()
        .find(|page| page.pieces.iter().any(|(name, _)| invert(name)))
    else {
        return false;
    };
    let Some(Element::Button(template)) = page
        .pieces
        .iter()
        .find(|(name, _)| invert(name))
        .map(|(_, element)| element)
    else {
        return false;
    };
    let mut button = template.clone();
    let bottom = page
        .pieces
        .iter()
        .filter_map(|(_, element)| match element {
            Element::Button(button) => Some(button.area),
            Element::Label(label) => Some(label.area),
            Element::Slider(slider) => Some(slider.area),
            _ => None,
        })
        .map(|area| area.y + area.height)
        .fold(0.0, f32::max);
    button.id = Some(LOOK_SPRING_FALLBACK.to_owned());
    button.anchors = None;
    button.placed = true;
    button.text = Some(LOOK_SPRING_TEXT.to_owned());
    button.tooltip = Some(LOOK_SPRING_TOOLTIP.to_owned());
    button.area.y = bottom + GAP;
    page.pieces
        .push((LOOK_SPRING_FALLBACK.to_owned(), Element::Button(button)));
    true
}

/// The skin's pages of a tab box, and for the Options window a last page,
/// its quality-of-life page, with checkboxes and sliders drawn from the skin,
/// within the page frame's client width.
pub(super) fn pages<'a>(
    pages: &'a [eq_client_assets::sidl::Page],
    owner: WindowId,
    size: Vec2,
    page_frame: Option<&eq_client_assets::sidl::FrameLook>,
) -> std::borrow::Cow<'a, [eq_client_assets::sidl::Page]> {
    if owner != WindowId::Options {
        return std::borrow::Cow::Borrowed(pages);
    }
    let skin_pieces = || pages.iter().flat_map(|page| &page.pieces);
    let checkbox = skin_pieces().find_map(|(_, element)| match element {
        Element::Button(button) if button.checkbox => Some(button),
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
    // The authored height chooses a preferred number of rows. Extra tab
    // rows shorten only the measured scrolling viewport, never its content.
    let insets = page_frame.map_or_else(frame::Insets::default, frame::insets);
    let room = Vec2::new(
        (size.x - insets.left - insets.right).max(0.0),
        (size.y - tab_strip(pages.iter().chain([&qol])) - insets.top - insets.bottom).max(0.0),
    );
    let caption = skin_pieces().find_map(|(_, element)| match element {
        Element::Label(label) if label.area.height > 0.0 => Some(label),
        _ => None,
    });
    let slider = skin_pieces().find_map(|(_, element)| match element {
        Element::Slider(slider) => Some(slider),
        _ => None,
    });
    let value = skin_pieces().find_map(|(name, element)| match element {
        Element::Label(label) if name.ends_with("ValueLabel") => Some(label),
        _ => None,
    });
    let mut all = pages.to_vec();
    let look_spring = look_spring(&mut all);
    let mut choices: Vec<_> = look_spring
        .into_iter()
        .chain(Fix::settings().map(Choice::Fix))
        .map(Control::Checkbox)
        .collect();
    if let Some((slider, value)) = slider.zip(value) {
        choices.extend([
            Control::Slider {
                section: Section::Ordinary,
                slider,
                value,
                ids: (
                    "EQC_QoL_UiScaleLabel",
                    controls::UI_SCALE_SLIDER,
                    controls::UI_SCALE_VALUE,
                ),
                text: "Interface scale",
            },
            Control::Slider {
                section: Section::Dangerous,
                slider,
                value,
                ids: (
                    "EQC_QoL_RunSpeedLabel",
                    controls::RUN_SPEED_SLIDER,
                    controls::RUN_SPEED_VALUE,
                ),
                text: "Run a Little Faster",
            },
        ]);
    }
    // Use the height authored for the chosen heading font. Skins without
    // a usable label take both metrics and typography from the checkbox.
    let heading_height = caption.map_or(checkbox.area.height, |label| label.area.height);
    let layout = Layout::new(checkbox, heading_height, room, choices.len());
    let mut pieces = Vec::new();
    let mut y = MARGIN;
    for section in [Section::Ordinary, Section::Dangerous] {
        pieces.push(section.heading(
            checkbox,
            caption,
            Area {
                x: layout.margin,
                y,
                width: layout.available,
                height: heading_height,
            },
        ));
        let section_choices: Vec<_> = choices
            .iter()
            .copied()
            .filter(|choice| choice.section() == section)
            .collect();
        y = layout.append_controls(
            &mut pieces,
            &section_choices,
            checkbox,
            y + heading_height + GAP,
        ) + SECTION_GAP;
    }
    all.push(eq_client_assets::sidl::Page { pieces, ..qol });
    std::borrow::Cow::Owned(all)
}

/// Cuts a caption, slider and value label from the skin and returns their bottom edge.
fn qol_slider(
    slider: &Slider,
    value: &Label,
    (x, y, width): (f32, f32, f32),
    row_width: f32,
    (caption_id, slider_id, value_id): (&str, &str, &str),
    text: &str,
) -> ([(String, Element); 3], f32) {
    let mut slider = slider.clone();
    let mut value = value.clone();
    let mut caption = value.clone();
    caption.eq_type = None;
    caption.anchors = None;
    caption.align = Align::Left;
    text.clone_into(&mut caption.text);
    caption.area = Area {
        x,
        y,
        width,
        height: value.area.height,
    };
    slider.id = Some(slider_id.to_owned());
    value.eq_type = None;
    value.anchors = None;
    if width + slider.area.width + value.area.width + 2.0 * GAP <= row_width {
        // The last control can use the empty space to its right while
        // keeping its full caption, native track and value widths.
        let height = slider.area.height.max(value.area.height);
        caption.area.height = height;
        slider.area.x = x + width + GAP;
        slider.area.y = y + (height - slider.area.height) / 2.0;
        value.area.x = slider.area.x + slider.area.width + GAP;
        value.area.y = y + (height - value.area.height) / 2.0;
    } else {
        slider.area.x = x;
        slider.area.y = y + value.area.height + 2.0;
        value.area.width = value.area.width.min(width).max(0.0);
        // Keep space for the track when the value can go below it instead.
        let beside = width >= slider.area.width.min(width / 2.0) + value.area.width + 4.0;
        slider.area.width = slider
            .area
            .width
            .min(if beside {
                width - value.area.width - 4.0
            } else {
                width
            })
            .max(0.0);
        value.area.x = if beside {
            slider.area.x + slider.area.width + 4.0
        } else {
            x
        };
        value.area.y = if beside {
            slider.area.y
        } else {
            slider.area.y + slider.area.height + 2.0
        };
    }
    let bottom = (slider.area.y + slider.area.height).max(value.area.y + value.area.height);
    (
        [
            (caption_id.to_owned(), Element::Label(caption)),
            (slider_id.to_owned(), Element::Slider(slider)),
            (value_id.to_owned(), Element::Label(value)),
        ],
        bottom,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Ordinary,
    Dangerous,
}

impl Section {
    fn heading(self, checkbox: &Button, style: Option<&Label>, area: Area) -> (String, Element) {
        let (id, text) = match self {
            Self::Ordinary => ("EQC_QoL_SettingsCaption", "Quality of Life"),
            Self::Dangerous => ("EQC_QoL_UnsafeCaption", "Dangerous"),
        };
        (
            id.to_owned(),
            Element::Label(Label {
                area,
                anchors: None,
                eq_type: None,
                text: text.to_owned(),
                color: style.map_or(checkbox.text_color, |label| label.color),
                align: Align::Left,
                font: style.map_or(checkbox.font, |label| label.font),
                wraps: false,
            }),
        )
    }
}

/// What a generated checkbox turns on and off.
#[derive(Clone, Copy)]
enum Choice {
    Fix(Fix),
    /// One of the official client's options, where the skin has no
    /// checkbox for it.
    Official {
        id: &'static str,
        text: &'static str,
        tooltip: &'static str,
    },
}

impl Choice {
    /// The checkbox's screen ID, text and tooltip.
    fn parts(self) -> (String, &'static str, &'static str) {
        match self {
            Self::Fix(fix) => (qol_checkbox_id(fix), fix.label(), fix.tooltip()),
            Self::Official { id, text, tooltip } => (id.to_owned(), text, tooltip),
        }
    }
}

#[derive(Clone, Copy)]
enum Control<'a> {
    Checkbox(Choice),
    Slider {
        section: Section,
        slider: &'a Slider,
        value: &'a Label,
        ids: (&'static str, &'static str, &'static str),
        text: &'static str,
    },
}

impl Control<'_> {
    fn section(self) -> Section {
        match self {
            Self::Checkbox(Choice::Fix(fix)) if fix.is_unsafe() => Section::Dangerous,
            Self::Checkbox(_) => Section::Ordinary,
            Self::Slider { section, .. } => section,
        }
    }
}

const MARGIN: f32 = 10.0;
const GAP: f32 = 6.0;
const SECTION_GAP: f32 = 16.0;

/// Columns must fit the inner width. A short or narrow page grows downward,
/// where its existing viewport can scroll, rather than adding hidden columns.
struct Layout {
    columns: usize,
    width: f32,
    margin: f32,
    available: f32,
}

impl Layout {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a page holds few rows"
    )]
    fn new(checkbox: &Button, heading_height: f32, room: Vec2, count: usize) -> Self {
        let margin = MARGIN.min(room.x / 2.0);
        let available = (room.x - 2.0 * margin).max(0.0);
        let natural_width = checkbox.area.width.max(1.0);
        let capacity = (((available + margin) / (natural_width + margin)).floor() as usize).max(1);
        // Choose columns using a checkbox-sized row estimate. The sections
        // then measure actual controls and can extend into the scroll range.
        let headings = 2.0 * (heading_height + GAP) + SECTION_GAP;
        let pitch = checkbox.area.height.max(0.0) + GAP;
        let rows = (((room.y - 2.0 * MARGIN - headings + GAP) / pitch).floor() as usize).max(1);
        let columns = count.div_ceil(rows).clamp(1, capacity);
        Self {
            columns,
            width: checkbox.area.width.max(190.0).min(
                ((available - margin * to_f32(u32::try_from(columns - 1).unwrap_or(u32::MAX)))
                    / to_f32(u32::try_from(columns).unwrap_or(u32::MAX)))
                .max(0.0),
            ),
            margin,
            available,
        }
    }

    /// Measure each row's tallest control. The slider and value stay with
    /// their caption and can make the row taller than a checkbox.
    fn append_controls(
        &self,
        pieces: &mut Vec<(String, Element)>,
        choices: &[Control<'_>],
        checkbox: &Button,
        mut y: f32,
    ) -> f32 {
        let mut bottom = y;
        for row in choices.chunks(self.columns) {
            bottom = y;
            for (column, choice) in row.iter().copied().enumerate() {
                let x = self.margin
                    + to_f32(u32::try_from(column).unwrap_or(u32::MAX))
                        * (self.width + self.margin);
                let control_bottom = match choice {
                    Control::Checkbox(choice) => {
                        let (id, text, tooltip) = choice.parts();
                        let mut button = checkbox.clone();
                        button.id = Some(id.clone());
                        button.anchors = None;
                        button.placed = true;
                        button.text = Some(text.to_owned());
                        button.tooltip = Some(tooltip.to_owned());
                        button.area = Area {
                            x,
                            y,
                            width: self.width,
                            height: checkbox.area.height,
                        };
                        pieces.push((id, Element::Button(button)));
                        y + checkbox.area.height
                    }
                    Control::Slider {
                        slider,
                        value,
                        ids,
                        text,
                        ..
                    } => {
                        let row_width = if column + 1 == row.len() {
                            self.available + self.margin - x
                        } else {
                            self.width
                        };
                        let (parts, bottom) =
                            qol_slider(slider, value, (x, y, self.width), row_width, ids, text);
                        pieces.extend(parts);
                        bottom
                    }
                };
                bottom = bottom.max(control_bottom);
            }
            y = bottom + GAP;
        }
        bottom
    }
}

/// Generated content needs its full bottom edge and margin in the scroll range.
pub(super) fn content_height(page: &eq_client_assets::sidl::Page) -> Option<f32> {
    (page.name == "EQC_QoLPage").then(|| {
        page.pieces
            .iter()
            .filter_map(|(_, element)| match element {
                eq_client_assets::sidl::Element::Slider(slider) => Some(slider.area),
                _ => super::tree::placed_area(element),
            })
            .map(|area| area.y + area.height)
            .fold(0.0, f32::max)
            + MARGIN
    })
}
