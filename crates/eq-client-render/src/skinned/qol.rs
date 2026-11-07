//! The client-added `QoL` page, using the installed skin's controls and text.
use super::{bindings::qol_checkbox_id, controls, frame, to_f32, tree::tab_strip};
use crate::windows::WindowId;
use bevy::prelude::Vec2;
use eq_client_assets::{
    sidl::{Align, Element, Label, Slider},
    ui::Area,
};

#[cfg(test)]
pub(super) mod tests;

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
        Element::Label(label) => Some(label),
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
    let slider_style = slider.zip(value);
    let first_slider =
        eq_client_core::qol::Fix::settings().count() + usize::from(caption.is_some());
    let layout = Layout::new(
        checkbox,
        caption,
        room,
        first_slider + usize::from(slider_style.is_some()),
    );
    let mut pieces = qol_checkboxes(checkbox, caption, &layout);
    if let Some((slider, value)) = slider_style {
        // Stack the sliders within a single column. Their actual heights
        // contribute to the scroll extent, including a value wrapped below.
        let (x, y, width) = layout.place(first_slider);
        let (scale, bottom) = qol_slider(
            slider,
            value,
            (x, y, width),
            (
                "EQC_QoL_UiScaleLabel",
                controls::UI_SCALE_SLIDER,
                controls::UI_SCALE_VALUE,
            ),
            "Interface scale",
        );
        pieces.extend(scale);
        let (run_speed, _) = qol_slider(
            slider,
            value,
            (x, bottom + GAP, width),
            (
                "EQC_QoL_RunSpeedLabel",
                controls::RUN_SPEED_SLIDER,
                controls::RUN_SPEED_VALUE,
            ),
            "Run a Little Faster",
        );
        pieces.extend(run_speed);
    }
    let mut all = pages.to_vec();
    all.push(eq_client_assets::sidl::Page { pieces, ..qol });
    std::borrow::Cow::Owned(all)
}

/// Cuts a caption, slider and value label from the skin and returns their bottom edge.
fn qol_slider(
    slider: &Slider,
    value: &Label,
    (x, y, width): (f32, f32, f32),
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
    slider.area.x = x;
    slider.area.y = y + value.area.height + 2.0;
    value.area.width = value.area.width.min(width).max(0.0);
    let beside = width > value.area.width + 4.0;
    slider.area.width = slider
        .area
        .width
        .min(if beside {
            width - value.area.width - 4.0
        } else {
            width
        })
        .max(0.0);
    value.eq_type = None;
    value.anchors = None;
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

/// A checkbox cut from the skin's own for each quality-of-life setting, in
/// their order, one under another down a page this tall and then on down
/// the next column.
fn qol_checkboxes(
    checkbox: &eq_client_assets::sidl::Button,
    caption: Option<&Label>,
    layout: &Layout,
) -> Vec<(String, eq_client_assets::sidl::Element)> {
    let settings: Vec<_> = eq_client_core::qol::Fix::settings().collect();
    // The rows of the page: every setting in order, with the caption ahead
    // of the unsafe run that closes it.
    let first_unsafe = settings.iter().position(|fix| fix.is_unsafe());
    let mut rows: Vec<(String, Row)> = Vec::new();
    for (position, fix) in settings.iter().copied().enumerate() {
        if Some(position) == first_unsafe
            && let Some(label) = caption
        {
            rows.push((
                "EQC_QoL_UnsafeCaption".to_owned(),
                Row::Caption(label.clone()),
            ));
        }
        rows.push((qol_checkbox_id(fix), Row::Setting(fix)));
    }
    rows.into_iter()
        .enumerate()
        .map(|(index, (id, row))| match row {
            Row::Setting(fix) => {
                let mut checkbox = checkbox.clone();
                let (x, y, width) = layout.place(index);
                checkbox.id = Some(id.clone());
                checkbox.anchors = None;
                checkbox.placed = true;
                checkbox.text = Some(fix.label().to_owned());
                checkbox.tooltip = Some(fix.tooltip().to_owned());
                checkbox.area.x = x;
                checkbox.area.y = y;
                checkbox.area.width = width;
                (id, Element::Button(checkbox))
            }
            Row::Caption(label) => {
                let mut line = label;
                let (x, y, width) = layout.place(index);
                line.eq_type = None;
                line.anchors = None;
                line.align = Align::Left;
                "Unsafe".clone_into(&mut line.text);
                line.area = Area {
                    x,
                    y,
                    width,
                    height: line.area.height,
                };
                (id, Element::Label(line))
            }
        })
        .collect()
}

/// One row of the quality-of-life page: a setting's checkbox, or the caption
/// that opens the unsafe run.
enum Row {
    Setting(eq_client_core::qol::Fix),
    Caption(eq_client_assets::sidl::Label),
}

const MARGIN: f32 = 10.0;
const GAP: f32 = 6.0;

/// Columns must fit the inner width. A short or narrow page grows downward,
/// where its existing viewport can scroll, rather than adding hidden columns.
struct Layout {
    rows: usize,
    pitch: f32,
    width: f32,
    margin: f32,
}

impl Layout {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a page holds few rows"
    )]
    fn new(
        checkbox: &eq_client_assets::sidl::Button,
        caption: Option<&eq_client_assets::sidl::Label>,
        room: Vec2,
        count: usize,
    ) -> Self {
        let margin = MARGIN.min(room.x / 2.0);
        let available = (room.x - 2.0 * margin).max(0.0);
        let natural_width = checkbox.area.width.max(1.0);
        let columns = (((available + margin) / (natural_width + margin)).floor() as usize).max(1);
        let height = checkbox
            .area
            .height
            .max(caption.map_or(0.0, |line| line.area.height));
        let pitch = height.max(0.0) + GAP;
        let preferred = (((room.y - 2.0 * MARGIN + GAP) / pitch).floor() as usize).max(1);
        let rows = preferred.max(count.div_ceil(columns));
        let columns = count.div_ceil(rows).max(1);
        Self {
            rows,
            pitch,
            width: checkbox.area.width.max(190.0).min(
                ((available - margin * to_f32(u32::try_from(columns - 1).unwrap_or(u32::MAX)))
                    / to_f32(u32::try_from(columns).unwrap_or(u32::MAX)))
                .max(0.0),
            ),
            margin,
        }
    }

    fn place(&self, index: usize) -> (f32, f32, f32) {
        (
            self.margin
                + to_f32(u32::try_from(index / self.rows).unwrap_or(u32::MAX))
                    * (self.width + self.margin),
            MARGIN + to_f32(u32::try_from(index % self.rows).unwrap_or(u32::MAX)) * self.pitch,
            self.width,
        )
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
