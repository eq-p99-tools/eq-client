//! The client-added `QoL` page, using the installed skin's controls and text.
use super::{bindings::qol_checkbox_id, controls, frame, to_f32, tree::tab_strip};
use crate::windows::WindowId;
use bevy::prelude::Vec2;
use eq_client_assets::{sidl::Align, ui::Area};

#[cfg(test)]
pub(super) mod tests;

/// The skin's pages of a tab box, and for the Options window a last page,
/// its quality-of-life page, with a checkbox for each setting, drawn with the
/// skin's own checkbox, within the page frame's client width.
pub(super) fn pages<'a>(
    pages: &'a [eq_client_assets::sidl::Page],
    owner: WindowId,
    size: Vec2,
    page_frame: Option<&eq_client_assets::sidl::FrameLook>,
) -> std::borrow::Cow<'a, [eq_client_assets::sidl::Page]> {
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
    // The authored height chooses a preferred number of rows. Extra tab
    // rows shorten only the measured scrolling viewport, never its content.
    let insets = page_frame.map_or_else(frame::Insets::default, frame::insets);
    let room = Vec2::new(
        (size.x - insets.left - insets.right).max(0.0),
        (size.y - tab_strip(pages.iter().chain([&qol])) - insets.top - insets.bottom).max(0.0),
    );
    let caption = pages
        .iter()
        .flat_map(|page| &page.pieces)
        .find_map(|(_, element)| {
            if let Element::Label(label) = element {
                Some(label)
            } else {
                None
            }
        });
    let has_scale = pages
        .iter()
        .flat_map(|page| &page.pieces)
        .any(|(_, element)| matches!(element, Element::Slider(_)))
        && pages
            .iter()
            .flat_map(|page| &page.pieces)
            .any(|(name, element)| {
                name.ends_with("ValueLabel") && matches!(element, Element::Label(_))
            });
    let rows = eq_client_core::qol::Fix::settings().count()
        + usize::from(caption.is_some())
        + usize::from(has_scale);
    let layout = Layout::new(&checkbox, caption, room, rows);
    let mut pieces = qol_checkboxes(pages, &checkbox, &layout);
    pieces.extend(qol_scale(pages, &layout));
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
    layout: &Layout,
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
    // The scale is the last block in its column. Its full height contributes
    // to the content extent instead of consuming another hidden column.
    // One row more when the page opens its unsafe run with a caption, as
    // qol_checkboxes does whenever the skin lends a line of text for it.
    let first = eq_client_core::qol::Fix::settings().count()
        + usize::from(pieces().any(|(_, element)| matches!(element, Element::Label(_))));
    let (x, y, width) = layout.place(first);
    let mut caption = value.clone();
    caption.eq_type = None;
    caption.anchors = None;
    caption.align = Align::Left;
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
fn qol_checkboxes(
    pages: &[eq_client_assets::sidl::Page],
    checkbox: &eq_client_assets::sidl::Button,
    layout: &Layout,
) -> Vec<(String, eq_client_assets::sidl::Element)> {
    use eq_client_assets::sidl::Element;
    let settings: Vec<_> = eq_client_core::qol::Fix::settings().collect();
    // The caption's line of text, cut from the skin's own, like the scale's.
    let caption =
        pages
            .iter()
            .flat_map(|page| &page.pieces)
            .find_map(|(_, element)| match element {
                Element::Label(label) => Some(label.clone()),
                _ => None,
            });
    // The rows of the page: every setting in order, with the caption ahead
    // of the unsafe run that closes it.
    let first_unsafe = settings.iter().position(|fix| fix.is_unsafe());
    let mut rows: Vec<(String, Row)> = Vec::new();
    for (position, fix) in settings.iter().copied().enumerate() {
        if Some(position) == first_unsafe
            && let Some(label) = &caption
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
