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

fn target_page(pages: &mut [Page], f: Fix) -> Option<&mut Page> {
    let name = destination(f)?;
    let page = pages.iter_mut().find(|p| page_is(p, name))?;
    let advanced = matches!(f, Fix::RichLighting | Fix::TorchShadows | Fix::BetterWater);
    let child = page.pieces.iter().position(|(_, e)| match e {
        Element::Tabs(t) => t.pages.iter().any(|p| page_is(p, "Advanced")),
        _ => false,
    });
    if advanced && let Some(index) = child {
        let Element::Tabs(tabs) = &mut page.pieces[index].1 else {
            unreachable!()
        };
        return tabs.pages.iter_mut().find(|p| page_is(p, "Advanced"));
    }
    Some(page)
}

fn append_button(
    page: &mut Page,
    id: String,
    mut button: eq_client_assets::sidl::Button,
    risk: bool,
    width: f32,
) {
    if page.name != "EQC_QoLPage" {
        begin(page);
    }
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

fn nested_page<'a>(pages: &'a mut [Page], title: &str) -> Option<&'a mut Page> {
    for page in pages {
        if page_is(page, title) || (title == "Spell" && page.name == "SpellParticlesPage") {
            return Some(page);
        }
        for (_, piece) in &mut page.pieces {
            if let Element::Tabs(t) = piece
                && let Some(found) = nested_page(&mut t.pages, title)
            {
                return Some(found);
            }
        }
    }
    None
}

pub(super) fn relocate(pages: &mut [Page], width: f32) {
    let Some(q) = pages.iter().rposition(|p| p.name == "EQC_QoLPage") else {
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
        if contains(pages, &id) {
            continue;
        }
        if let Some(f) = qol_checkbox(&id)
            && let Some(dest) = target_page(pages, f)
            && let Element::Button(b) = e
        {
            append_button(dest, id, b, f.is_unsafe(), width);
            continue;
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
        if let Some(page) = nested_page(pages, "Spell") {
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

fn annotate(pieces: &mut Vec<Piece>) {
    use eq_client_assets::sidl::{Anchor, Anchors};
    let mut marks = Vec::new();
    for (name, e) in pieces.iter_mut() {
        let (id, area, anchors, font, risk) = match e {
            Element::Tabs(t) => {
                for p in &mut t.pages {
                    annotate(&mut p.pieces);
                }
                continue;
            }
            Element::View(v) => {
                annotate(&mut v.pieces);
                continue;
            }
            Element::Button(b) => {
                let id = b.id.as_deref().unwrap_or(name);
                let risk = if let Some(f) = qol_checkbox(id) {
                    f.is_unsafe()
                } else if id == "EQC_SpellEffectsCheckbox" {
                    false
                } else {
                    continue;
                };
                b.tooltip = Some(format!(
                    "{}. {}",
                    if risk {
                        "Unsafe client addition"
                    } else {
                        "Safe client addition"
                    },
                    b.tooltip.as_deref().unwrap_or("")
                ));
                (id, b.area, b.anchors, b.font, risk)
            }
            Element::Slider(s) if s.id.as_deref() == Some(controls::UI_SCALE_SLIDER) => {
                (controls::UI_SCALE_SLIDER, s.area, None, None, false)
            }
            Element::Label(l)
                if super::color_picker::COMBAT_COLORS
                    .iter()
                    .any(|(id, ..)| *id == name) =>
            {
                (name.as_str(), l.area, l.anchors, l.font, false)
            }
            _ => continue,
        };
        let left = |edge| match edge {
            Anchor::Start(x) => Anchor::Start((x - 10.0).max(0.0)),
            Anchor::End(x) => Anchor::End(x + 10.0),
        };
        marks.push((
            format!("EQC_ExtraMark_{id}"),
            Element::Label(Label {
                area: Area {
                    x: (area.x - 10.0).max(0.0),
                    y: area.y,
                    width: 10.0,
                    height: area.height,
                },
                anchors: anchors.map(|a| Anchors {
                    left: left(a.left),
                    right: a.left,
                    top: a.top,
                    bottom: a.bottom,
                }),
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
    pieces.extend(marks);
}

/// Bind the authored pages first, then append any missing client controls.
pub(super) fn options_pages(
    tabs: &eq_client_assets::sidl::TabBox,
    size: bevy::prelude::Vec2,
) -> Vec<Page> {
    use crate::windows::WindowId::Options;
    let mut authored = tabs.pages.clone();
    for page in &mut authored {
        if !page_is(page, "QoL") {
            annotate(&mut page.pieces);
        }
    }
    let original = &authored;
    let filters = super::filters::pages(original, Options, size.x);
    let colors = super::colors::pages(&filters, Options, size.x);
    let fallback = colors.len() > filters.len();
    let source = if fallback {
        filters.as_ref()
    } else {
        colors.as_ref()
    };
    let mut pages = super::qol::pages(source, Options, size, tabs.page_frame.as_ref()).into_owned();
    if pages.len() == source.len() {
        return pages;
    }
    relocate(&mut pages, size.x);
    if fallback
        && let Some(p) = pages.last_mut()
        && let Some(color_page) = colors.last()
    {
        let start = bottom(p) + 6.0;
        for (id, elem) in &color_page.pieces {
            if super::color_picker::COMBAT_COLORS
                .iter()
                .any(|(key, ..)| id.contains(key))
                && let Element::Label(label) = elem
            {
                let mut label = label.clone();
                label.area.y += start;
                p.pieces.push((id.clone(), Element::Label(label)));
            }
        }
    }
    // If a skin provides its own QoL page, keep its position and append only missing controls.
    if let Some(index) = original
        .iter()
        .position(|p| p.name == "EQC_QoLPage" || page_is(p, "QoL"))
        && let Some(generated) = pages.pop()
    {
        let page = &mut pages[index];
        let offset = bottom(page) + 6.0;
        begin(page);
        for (id, mut e) in generated.pieces {
            match &mut e {
                Element::Label(l) => l.area.y += offset,
                Element::Button(b) => b.area.y += offset,
                Element::Slider(s) => s.area.y += offset,
                _ => continue,
            }
            page.pieces.push((id, e));
        }
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Vec2;
    use eq_client_assets::sidl::TabBox;
    #[test]
    fn authored_extension_keeps_its_anchor_and_appears_once() {
        let mut page = super::super::qol::tests::source();
        let fix = Fix::settings().find(|f| f.is_unsafe()).unwrap();
        let id = super::super::bindings::qol_checkbox_id(fix);
        let Element::Button(button) = &mut page.pieces[0].1 else {
            panic!("fixture checkbox")
        };
        button.id = Some(id.clone());
        let original = (button.area, button.anchors);
        let tabs = TabBox {
            name: "Custom".into(),
            area: None,
            pages: vec![page],
            tab_frame: None,
            page_frame: None,
        };
        let pages = options_pages(&tabs, Vec2::new(380.0, 400.0));
        let mut hits = pages
            .iter()
            .flat_map(|p| &p.pieces)
            .filter_map(|(_, e)| match e {
                Element::Button(b) if b.id.as_ref() == Some(&id) => Some(b),
                _ => None,
            });
        let kept = hits.next().unwrap();
        assert_eq!((kept.area, kept.anchors), original);
        assert!(hits.next().is_none());
        assert!(
            pages[0]
                .pieces
                .iter()
                .any(|(n, e)| n == &format!("EQC_ExtraMark_{id}")
                    && matches!(e,Element::Label(l) if l.color==Some([255,92,92])))
        );
        let qol = pages.last().unwrap();
        for (key, ..) in super::super::color_picker::COMBAT_COLORS {
            assert!(contains(std::slice::from_ref(qol), key));
        }
        assert!(!pages.iter().any(|p| p.title.as_deref() == Some("Colors")));
    }

    #[test]
    #[ignore = "requires EQ_PROBE_TAKP with an installed TAKP default skin"]
    fn installed_takp_authored_color_buttons_keep_their_binding_and_position() {
        let dir = std::path::PathBuf::from(std::env::var_os("EQ_PROBE_TAKP").unwrap());
        let skin = "default";
        let library = eq_client_assets::sidl::Library::read(&dir, skin).unwrap();
        let screen = library
            .window(&dir, skin, "EQUI_OptionsWindow.xml", "OptionsWindow")
            .unwrap();
        let tabs = screen
            .pieces
            .iter()
            .find_map(|(_, e)| {
                if let Element::Tabs(t) = e {
                    Some(t)
                } else {
                    None
                }
            })
            .unwrap();
        let pages = options_pages(tabs, Vec2::new(380.0, 400.0));
        let colors = tabs
            .pages
            .iter()
            .position(|p| page_is(p, "Colors"))
            .unwrap();
        let authored = &tabs.pages[colors];
        let built = &pages[colors];
        let mut count = 0;
        for piece in &authored.pieces {
            assert!(built.pieces.contains(piece));
            if let Element::Button(b) = &piece.1
                && let Some(kind) =
                    b.id.as_deref()
                        .and_then(super::super::colors::authored_kind)
            {
                count += 1;
                assert!(!contains(&pages, &format!("EQC_ChatColor_{kind}")));
            }
        }
        assert_eq!(count, 72);
        assert!(contains(&pages, "PCSpellsComboBox"));
        assert!(!contains(&pages, "EQC_Filter_pcspells"));
    }
    #[test]
    fn spell_effects_uses_the_nested_spell_page_and_existing_saved_control() {
        let general = super::super::qol::tests::source();
        let spell = Page {
            name: "SpellParticlesPage".into(),
            title: Some("Spell".into()),
            pieces: vec![],
            ..general.clone()
        };
        let nested = TabBox {
            name: "ParticleTabs".into(),
            area: None,
            pages: vec![spell],
            tab_frame: None,
            page_frame: None,
        };
        let display = Page {
            name: "Display".into(),
            title: Some("Display".into()),
            pieces: vec![("ParticleTabs".into(), Element::Tabs(nested))],
            ..general.clone()
        };
        let tabs = TabBox {
            name: "OptionsTabs".into(),
            area: None,
            pages: vec![display, general],
            tab_frame: None,
            page_frame: None,
        };
        let pages = options_pages(&tabs, Vec2::new(380.0, 400.0));
        let Element::Tabs(t) = &pages[0].pieces[0].1 else {
            panic!("nested tabs")
        };
        assert_eq!(t.pages.len(), 1);
        assert!(contains(&t.pages, "EQC_SpellEffectsCheckbox"));
        assert!(!contains(&pages[1..], "EQC_SpellEffectsCheckbox"));
        let mut app = crate::testing::app();
        app.add_systems(bevy::prelude::Update, crate::options::toggle);
        let control = app
            .world_mut()
            .spawn((
                bevy::prelude::Interaction::None,
                crate::options::OptionCheckbox(eq_client_core::options::Toggle::SpellEffects),
            ))
            .id();
        crate::clicks::testing::click(&mut app, control);
        let options = &app
            .world()
            .resource::<crate::options::OptionsState>()
            .options;
        assert!(!options.spell_effects);
        assert!(
            !eq_client_core::options::Options::read(
                &options.text(),
                eq_client_core::options::Options::default()
            )
            .spell_effects
        );
    }
}
