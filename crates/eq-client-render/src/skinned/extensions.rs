//! Append client-only controls after authored content, respecting authored IDs anywhere in the skin.
use super::{bindings::qol_checkbox, controls};
use eq_client_assets::{
    sidl::{Align, Element, Label, Page},
    ui::Area,
};
use eq_client_core::qol::Fix;

type Piece = (String, Element);

fn named((name, e): &Piece, id: &str) -> bool {
    name == id
        || match e {
            Element::Button(b) => b.id.as_deref() == Some(id),
            Element::Slider(s) => s.id.as_deref() == Some(id),
            Element::Combobox(c) => c.id.as_deref() == Some(id),
            Element::Listbox(l) => l.id.as_deref() == Some(id),
            Element::Tabs(t) => contains(&t.pages, id),
            Element::View(v) => v.pieces.iter().any(|piece| named(piece, id)),
            _ => false,
        }
}

pub(super) fn contains(pages: &[Page], id: &str) -> bool {
    pages
        .iter()
        .flat_map(|p| &p.pieces)
        .any(|piece| named(piece, id))
}

pub(super) const START: &str = "EQC_ExtensionStart";
pub(super) fn begin(page: &mut Page) {
    if !page.pieces.is_empty() && !page.pieces.iter().any(|(id, _)| id == START) {
        page.pieces
            .push((START.into(), Element::Other(START.into())));
    }
}
pub(super) fn bottom(page: &Page) -> f32 {
    pieces_bottom(&page.pieces)
}

pub(super) fn pieces_bottom(pieces: &[Piece]) -> f32 {
    pieces
        .iter()
        .filter_map(|(_, e)| match e {
            Element::Tabs(t) => t.area,
            Element::Listbox(l) => l.area,
            Element::View(v) => Some(v.area),
            Element::Combobox(c) => Some(c.area),
            Element::Slider(s) => Some(s.area),
            Element::TextBox(t) => Some(t.area),
            Element::SpellGem(g) => Some(g.area),
            _ => super::tree::placed_area(e),
        })
        .map(|a| a.y + a.height)
        .fold(0.0, f32::max)
}

pub(super) fn mark(page: &mut Page, id: &str, risk: bool, x: f32, y: f32, font: Option<u8>) {
    let key = format!("EQC_ExtraMark_{id}");
    if page.pieces.iter().any(|(n, _)| *n == key) {
        return;
    }
    page.pieces.push((
        key,
        Element::Label(Label {
            area: Area {
                x,
                y,
                width: 10.0,
                height: 18.0,
            },
            anchors: None,
            text: "*".into(),
            color: Some(if risk { [255, 92, 92] } else { [92, 174, 255] }),
            font,
            align: Align::Left,
            wraps: false,
            eq_type: None,
            client_value: None,
        }),
    ));
}

fn page_is(p: &Page, title: &str) -> bool {
    p.title
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case(title))
        || p.name.eq_ignore_ascii_case(&format!("Options{title}Page"))
}

fn destination(f: Fix) -> Option<&'static str> {
    match f {
        Fix::AskBeforeDeletingSpells | Fix::ShowTimesInChat | Fix::FlashOnTells => Some("General"),
        Fix::CorpsesAlongGround
        | Fix::LongChaseCamera
        | Fix::RichLighting
        | Fix::TorchShadows
        | Fix::BetterWater
        | Fix::ShowFps => Some("Display"),
        _ => None,
    }
}

fn append_button(
    page: &mut Page,
    id: String,
    mut button: eq_client_assets::sidl::Button,
    risk: bool,
    width: f32,
) {
    begin(page);
    let y = bottom(page) + 6.0;
    let status = if risk {
        "Unsafe client addition"
    } else {
        "Safe client addition"
    };
    let text = button.tooltip.take().unwrap_or_default();
    button.tooltip = Some(format!("{status}. {text}"));
    button.area = Area {
        x: 18.0,
        y,
        width: (width - 34.0).max(10.0),
        height: button.area.height.max(18.0),
    };
    button.placed = true;
    button.anchors = None;
    mark(page, &id, risk, 6.0, y, button.font);
    page.pieces.push((id, Element::Button(button)));
}

fn spell_page(pages: &mut [Page]) -> Option<&mut Page> {
    for page in pages {
        if page.name == "SpellParticlesPage" || page_is(page, "Spell") {
            return Some(page);
        }
        for (_, piece) in &mut page.pieces {
            if let Element::Tabs(t) = piece
                && let Some(found) = spell_page(&mut t.pages)
            {
                return Some(found);
            }
        }
    }
    None
}

pub(super) fn relocate(pages: &mut [Page], width: f32) {
    let Some(q) = pages.iter().position(|p| p.name == "EQC_QoLPage") else {
        return;
    };
    let mut controls = std::mem::take(&mut pages[q].pieces);
    let template = controls.iter().find_map(|(_, e)| {
        if let Element::Button(b) = e {
            Some(b.clone())
        } else {
            None
        }
    });
    relocate_scale(pages, &mut controls, width);
    for (id, e) in controls {
        if id == super::qol::OUTLINE_TRACKED || id == "EQC_QoL_MapCaption" {
            continue;
        }
        if let Some(f) = qol_checkbox(&id) {
            if contains(pages, &id) {
                continue;
            }
            if let Some(dest) =
                destination(f).and_then(|name| pages.iter_mut().find(|p| page_is(p, name)))
                && let Element::Button(b) = e
            {
                append_button(dest, id, b, f.is_unsafe(), width);
                continue;
            }
        }
        pages[q].pieces.push((id, e));
    }
    let id = "EQC_SpellEffectsCheckbox";
    if !contains(pages, id)
        && let Some(mut b) = template
    {
        b.id = Some(id.to_owned());
        b.text = Some("Show Spell Effects".into());
        b.tooltip = Some("Draw spell effects.".into());
        if let Some(page) = spell_page(pages) {
            append_button(
                page,
                id.into(),
                b,
                false,
                page.area.map_or(width, |a| a.width),
            );
        } else {
            append_button(&mut pages[q], id.into(), b, false, width);
        }
    }
}

fn relocate_scale(pages: &mut [Page], controls: &mut Vec<Piece>, width: f32) {
    let Some(d) = pages.iter().position(|p| page_is(p, "Display")) else {
        return;
    };
    let ids = [
        "EQC_QoL_UiScaleLabel",
        controls::UI_SCALE_SLIDER,
        controls::UI_SCALE_VALUE,
    ];
    if contains(pages, controls::UI_SCALE_SLIDER) {
        controls.retain(|(id, _)| !ids.contains(&id.as_str()));
        return;
    }
    begin(&mut pages[d]);
    let y = bottom(&pages[d]) + 6.0;
    let mut moved = Vec::new();
    for id in ids {
        if let Some(i) = controls.iter().position(|(n, _)| n == id) {
            moved.push(controls.remove(i));
        }
    }
    for (id, mut e) in moved {
        match &mut e {
            Element::Label(l) => {
                l.area.x = 18.0;
                l.area.y = y;
                l.area.width = (width - 28.0).max(1.0);
                if id == controls::UI_SCALE_VALUE {
                    l.area.x = (width - 52.0).max(0.0);
                    l.area.width = 44.0;
                    l.area.y += 20.0;
                } else {
                    mark(&mut pages[d], &id, false, 6.0, y, l.font);
                }
            }
            Element::Slider(s) => {
                s.area.x = 18.0;
                s.area.y = y + 20.0;
                s.area.width = (width - 82.0).max(1.0);
            }
            _ => {}
        }
        pages[d].pieces.push((id, e));
    }
}
