//! The Colors tab uses one compact swatch per setting. Page scrolling is shared with the quality-of-life tab.
use super::color_picker::COMBAT_COLORS;
use crate::windows::WindowId;
use eq_client_assets::{
    sidl::{Align, Button, Element, Label, Page},
    ui::Area,
};
use std::borrow::Cow;

pub(super) const PAGE: &str = "EQC_ColorsPage";

pub(super) fn pages(pages: &[Page], owner: WindowId, width: f32) -> Cow<'_, [Page]> {
    if owner != WindowId::Options || pages.iter().any(|p| p.name == PAGE) {
        return Cow::Borrowed(pages);
    }
    let Some(first) = pages.first() else {
        return Cow::Borrowed(pages);
    };
    let index = pages.iter().position(|p| {
        p.name == "OptionsColorPage"
            || p.title
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("Colors"))
            || p.pieces.iter().any(|(id, _)| id == "OCP_ColorChooseList")
    });
    let template = index.map_or(first, |i| &pages[i]);
    let pieces = || {
        template
            .pieces
            .iter()
            .chain(pages.iter().flat_map(|p| &p.pieces))
    };
    let button = pieces()
        .find_map(|(_, e)| match e {
            Element::Button(b) if !b.checkbox => Some(b),
            _ => None,
        })
        .or_else(|| {
            pieces().find_map(|(_, e)| match e {
                Element::Button(b) => Some(b),
                _ => None,
            })
        });
    let Some(button) = button else {
        return Cow::Borrowed(pages);
    };
    let mut page = Page {
        name: PAGE.into(),
        title: Some("Colors".into()),
        pieces: Vec::new(),
        ..template.clone()
    };
    let width = (width - 28.0).max(18.0);
    let mut row = Rows {
        page: &mut page,
        button,
        y: 6.0,
        width,
    };
    row.label("EQC_FloatingColors", "Floating combat text", false);
    for (id, name, _) in COMBAT_COLORS {
        row.swatch(id, name);
    }
    row.reset();
    row.label("EQC_ChatColors", "Chat colors", false);
    for kind in 1..eq_client_core::options::USER_COLOR_KINDS {
        let fallback = format!("Chat color {kind}");
        let name = crate::local_settings::KIND_NAMES
            .get(kind - 1)
            .copied()
            .or_else(|| EXTRA_NAMES.get(kind.checked_sub(70)?).copied())
            .unwrap_or(&fallback);
        if name != "Unused" {
            row.swatch(&format!("EQC_ChatColor_{kind}"), name);
        }
    }
    let mut result = pages.to_vec();
    if let Some(i) = index {
        result[i] = page;
    } else {
        result.push(page);
    }
    Cow::Owned(result)
}

// The contiguous continuation of the installed color names after the slash help list.
// The remaining configurable slots have no verified names here.
const EXTRA_NAMES: [&str; 13] = [
    "Unused",
    "Item links",
    "Raid chat",
    "Your pet",
    "Damage shields",
    "Leadership",
    "Pet rampages and flurries",
    "Pet critical hits",
    "Focus effects",
    "Experience",
    "System messages",
    "Pet spells",
    "Pet responses",
];

struct Rows<'a> {
    page: &'a mut Page,
    button: &'a Button,
    y: f32,
    width: f32,
}

impl Rows<'_> {
    fn label(&mut self, id: &str, text: &str, inset: bool) {
        let offset = if inset { 24.0 } else { 0.0 };
        let label = Label {
            area: Area {
                x: 6.0 + offset,
                y: self.y,
                width: (self.width - offset).max(0.0),
                height: 20.0,
            },
            text: text.into(),
            align: Align::Left,
            color: self.button.text_color,
            font: self.button.font,
            wraps: false,
            anchors: None,
            eq_type: None,
            client_value: None,
        };
        self.page.pieces.push((id.into(), Element::Label(label)));
        self.y += 23.0;
    }

    fn swatch(&mut self, id: &str, name: &str) {
        self.label(&format!("{id}Caption"), name, true);
        let Some((_, Element::Label(caption))) = self.page.pieces.last() else {
            unreachable!()
        };
        let mut field = caption.clone();
        field.area.x = 6.0;
        field.area.height = 18.0;
        field.area.width = 18.0;
        self.page.pieces.push((id.into(), Element::Label(field)));
    }

    fn reset(&mut self) {
        let mut button = self.button.clone();
        let id = "OCP_UserColorDefault";
        button.id = Some(id.into());
        button.text = Some("Reset Default Colors".into());
        button.tooltip =
            Some("Restore chat and floating combat text colors for this profile.".into());
        button.anchors = None;
        button.placed = true;
        button.checkbox = false;
        button.area = Area {
            x: 6.0,
            y: self.y,
            width: self.width.min(160.0),
            height: 22.0,
        };
        self.page.pieces.push((id.into(), Element::Button(button)));
        self.y += 30.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skinned::color_picker::binding;

    #[test]
    fn empty_authored_colors_page_is_populated_in_place_with_unique_editable_rows() {
        let general = crate::skinned::qol::tests::source();
        let empty = Page {
            name: "OptionsColorPage".into(),
            title: Some("Colors".into()),
            pieces: Vec::new(),
            ..general.clone()
        };
        let skin = [general.clone(), empty];
        let made = pages(&skin, WindowId::Options, 350.0);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0], general);
        let rows = &made[1].pieces;
        let swatches: Vec<_> = rows
            .iter()
            .filter(|(id, _)| binding(id).is_some())
            .collect();
        assert_eq!(swatches.len(), 85); // 86 kinds minus three unused, plus two combat colors.
        for id in [
            "EQC_ChatColor_1",
            "EQC_ChatColor_72",
            "EQC_ChatColor_86",
            COMBAT_COLORS[1].0,
            COMBAT_COLORS[0].0,
        ] {
            let (_, Element::Label(field)) = rows.iter().find(|(name, _)| name == id).unwrap()
            else {
                panic!("swatch")
            };
            assert_eq!((field.area.width, field.area.height), (18.0, 18.0));
        }
        assert_eq!(
            rows.iter()
                .filter(|(id, _)| id == "OCP_UserColorDefault")
                .count(),
            1
        );
        assert_eq!(pages(&made, WindowId::Options, 350.0).len(), 2);
        assert_eq!(pages(&skin[..1], WindowId::Options, 350.0)[1].name, PAGE);
    }
}
