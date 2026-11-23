//! This client's Filters page in the Options window: one row per kind of
//! message the server filters cover ([`eq_client_core::chat_filters`]),
//! each a button that steps the kind's mode and a label that says it, cut
//! from the skin's own controls.
//!
//! A skeleton: the official client's own filter controls on the skin are
//! unread, as are the options behind each word (recording checklist
//! 6.494b), so the rows are this client's own. Rows for a kind the
//! generation's packet leaves out are hidden while that generation plays.
use crate::windows::WindowId;
use eq_client_assets::{
    sidl::{Align, Button, Element, Label, Page},
    ui::Area,
};
use eq_client_core::chat_filters::kind_words;
use eq_client_core::filters::FilterCategory;
use std::borrow::Cow;

/// The page's name, which also marks its content as generated.
pub(super) const PAGE: &str = "EQC_FiltersPage";

/// The Filters page's viewport, which `/filter` shows.
#[derive(bevy::prelude::Component, Clone, Copy, Debug)]
pub(crate) struct FiltersPage;

const BUTTON: &str = "EQC_Filter_";
const VALUE: &str = "ValueLabel";
const MARGIN: f32 = 10.0;
const GAP: f32 = 6.0;
/// The note's screen ID; its words follow the session ([`crate::options::FilterNote`]).
pub(super) const NOTE_ID: &str = "EQC_Filter_Note";

/// A row's button's screen ID.
fn button_id(category: FilterCategory) -> String {
    format!("{BUTTON}{}", category.key())
}

/// A row's value label's screen ID.
fn value_id(category: FilterCategory) -> String {
    format!("{BUTTON}{}{VALUE}", category.key())
}

/// The kind a row's button steps, by its screen ID.
pub(super) fn filter_button(id: &str) -> Option<FilterCategory> {
    id.strip_prefix(BUTTON)
        .filter(|rest| !rest.ends_with(VALUE))
        .and_then(FilterCategory::from_key)
}

/// The kind a row's label says the mode of, by its screen ID.
pub(super) fn filter_value(id: &str) -> Option<FilterCategory> {
    id.strip_prefix(BUTTON)?
        .strip_suffix(VALUE)
        .and_then(FilterCategory::from_key)
}

/// A row's button: the skin's template, placed at `area`, saying the kind.
fn row_button(template: &Button, category: FilterCategory, area: Area) -> Button {
    let mut button = template.clone();
    button.id = Some(button_id(category));
    button.anchors = None;
    button.placed = true;
    button.checkbox = false;
    button.text = Some(kind_words(category).to_owned());
    button.tooltip = Some(
        "Click for the next mode the server takes for these. Which official option sets this is unread."
            .to_owned(),
    );
    button.area = area;
    button
}

/// The skin's pages, and for the Options window a Filters page after them,
/// where the skin has a checkbox to cut its buttons from. The page's note
/// wraps within the page's width, `page_width`.
pub(super) fn pages(pages: &[Page], owner: WindowId, page_width: f32) -> Cow<'_, [Page]> {
    if owner != WindowId::Options || pages.iter().any(|page| page.name == PAGE) {
        return Cow::Borrowed(pages);
    }
    let Some(first) = pages.first() else {
        return Cow::Borrowed(pages);
    };
    let pieces = || pages.iter().flat_map(|page| &page.pieces);
    // A plain button where the skin has one, else its checkbox.
    let template = pieces()
        .find_map(|(_, element)| match element {
            Element::Button(button) if !button.checkbox && button.text.is_some() => Some(button),
            _ => None,
        })
        .or_else(|| {
            pieces().find_map(|(_, element)| match element {
                Element::Button(button) if button.checkbox => Some(button),
                _ => None,
            })
        });
    let Some(template) = template else {
        return Cow::Borrowed(pages);
    };
    let label_template = pieces().find_map(|(_, element)| match element {
        Element::Label(label) if label.area.height > 0.0 => Some(label.clone()),
        _ => None,
    });
    let height = template.area.height.max(14.0);
    let width = template.area.width.max(160.0);
    let label = |text: &str, area: Area, align: Align| {
        let mut label = label_template.clone().unwrap_or(Label {
            area,
            anchors: None,
            eq_type: None,
            text: String::new(),
            color: template.text_color,
            align,
            font: template.font,
            wraps: false,
        });
        label.area = area;
        label.anchors = None;
        label.eq_type = None;
        label.align = align;
        label.wraps = false;
        text.clone_into(&mut label.text);
        label
    };
    let mut rows = vec![(
        NOTE_ID.to_owned(),
        Element::Label(Label {
            wraps: true,
            ..label(
                crate::options::filter_note(false),
                Area {
                    x: MARGIN,
                    y: MARGIN,
                    width: (2.0 * width + GAP).min(page_width - 2.0 * MARGIN).max(60.0),
                    height: 3.0 * height,
                },
                Align::Left,
            )
        }),
    )];
    let mut y = MARGIN + 3.0 * height + GAP;
    for category in eq_client_core::chat_filters::rows() {
        let area = Area {
            x: MARGIN,
            y,
            width,
            height,
        };
        rows.push((
            button_id(category),
            Element::Button(row_button(template, category, area)),
        ));
        rows.push((
            value_id(category),
            Element::Label(label(
                "",
                Area {
                    x: MARGIN + width + GAP,
                    y,
                    width: 90.0,
                    height,
                },
                Align::Left,
            )),
        ));
        y += height + 2.0;
    }
    let mut all = pages.to_vec();
    all.push(Page {
        name: PAGE.into(),
        title: Some("Filters".into()),
        pieces: rows,
        tooltip: Some("What the server sends of each kind of message".into()),
        ..first.clone()
    });
    Cow::Owned(all)
}

/// The generated page's full height, for its scrolling viewport.
pub(super) fn content_height(page: &Page) -> Option<f32> {
    (page.name == PAGE).then(|| {
        page.pieces
            .iter()
            .filter_map(|(_, element)| super::tree::placed_area(element))
            .map(|area| area.y + area.height)
            .fold(0.0, f32::max)
            + MARGIN
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_has_a_row_for_every_kind_with_one_and_is_added_once() {
        let first = super::super::qol::tests::source();
        let made = pages(std::slice::from_ref(&first), WindowId::Options, 300.0);
        assert_eq!(made.len(), 2);
        let page = &made[1];
        assert_eq!(page.name, PAGE);
        // The note wraps within the page (6.494d's look).
        let note = page.pieces.iter().find(|(id, _)| id == NOTE_ID);
        match note.map(|(_, element)| element) {
            Some(Element::Label(label)) => {
                assert!(label.area.x + label.area.width <= 300.0 - MARGIN);
                assert!(label.wraps);
            }
            other => panic!("{other:?}"),
        }
        let buttons: Vec<_> = page
            .pieces
            .iter()
            .filter_map(|(id, _)| filter_button(id))
            .collect();
        let rows: Vec<_> = FilterCategory::ALL
            .into_iter()
            .filter(|category| *category != FilterCategory::SwarmPetDeath)
            .collect();
        assert_eq!(buttons, rows);
        let values: Vec<_> = page
            .pieces
            .iter()
            .filter_map(|(id, _)| filter_value(id))
            .collect();
        assert_eq!(values, rows);
        for (id, element) in &page.pieces {
            if let Element::Button(button) = element {
                assert!(!button.checkbox, "{id}");
                assert!(button.placed, "{id}");
            }
        }
        assert!(content_height(page).unwrap() > 24.0 * 14.0);
        assert_eq!(pages(&made, WindowId::Options, 300.0).len(), 2);
        assert_eq!(
            pages(std::slice::from_ref(&first), WindowId::Spellbook, 300.0).len(),
            1
        );
        assert_eq!(filter_button("EQC_Filter_ooc"), Some(FilterCategory::Ooc));
        assert_eq!(filter_button("EQC_Filter_oocValueLabel"), None);
        assert_eq!(
            filter_value("EQC_Filter_oocValueLabel"),
            Some(FilterCategory::Ooc)
        );
    }
}
