//! A functional particles page using the skin's checkbox and page templates.
use crate::windows::WindowId;
use eq_client_assets::{
    sidl::{Element, Page},
    ui::Area,
};
use std::borrow::Cow;

pub(super) fn pages(pages: &[Page], owner: WindowId) -> Cow<'_, [Page]> {
    if owner != WindowId::Options {
        return Cow::Borrowed(pages);
    }
    let Some(first) = pages.first() else {
        return Cow::Borrowed(pages);
    };
    let checkbox =
        pages
            .iter()
            .flat_map(|page| &page.pieces)
            .find_map(|(_, element)| match element {
                Element::Button(button) if button.checkbox => Some(button.clone()),
                _ => None,
            });
    let Some(mut checkbox) = checkbox else {
        return Cow::Borrowed(pages);
    };
    checkbox.id = Some("EQC_SpellEffectsCheckbox".into());
    checkbox.text = Some("Show Spell Effects".into());
    checkbox.tooltip =
        Some("Draw spell-effect particles. Spell sounds and combat results are unchanged.".into());
    checkbox.area = Area {
        x: 10.0,
        y: 10.0,
        width: checkbox.area.width.max(190.0),
        height: checkbox.area.height,
    };
    let particle = |page: &Page| {
        page.name.to_ascii_lowercase().contains("particle")
            || page
                .title
                .as_deref()
                .is_some_and(|title| title.eq_ignore_ascii_case("Particles"))
    };
    let mut all = pages.to_vec();
    let template = pages.iter().find(|page| particle(page)).unwrap_or(first);
    let page = Page {
        name: "EQC_ParticlesPage".into(),
        title: Some("Particles".into()),
        pieces: vec![("EQC_SpellEffectsCheckbox".into(), Element::Button(checkbox))],
        tooltip: Some("Spell-effect particles".into()),
        ..template.clone()
    };
    if let Some(index) = all.iter().position(particle) {
        all[index] = page;
    } else {
        all.push(page);
    }
    Cow::Owned(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_assets::sidl::{Button, ButtonLook};

    fn source() -> Page {
        Page {
            name: "General".into(),
            title: Some("General".into()),
            area: None,
            template: None,
            icon: [None, None],
            title_colors: [None, None],
            tooltip: None,
            pieces: vec![(
                "checkbox".into(),
                Element::Button(Button {
                    id: None,
                    area: Area {
                        x: 10.0,
                        y: 10.0,
                        width: 150.0,
                        height: 20.0,
                    },
                    placed: true,
                    anchors: None,
                    look: ButtonLook::default(),
                    checkbox: true,
                    text: None,
                    text_color: None,
                    font: None,
                    decal: None,
                    decal_area: None,
                    tooltip: None,
                }),
            )],
        }
    }

    #[test]
    fn particles_page_has_a_bound_persisted_control_and_is_not_duplicated() {
        let first = source();
        let made = pages(std::slice::from_ref(&first), WindowId::Options);
        assert_eq!(made.len(), 2);
        let (_, Element::Button(checkbox)) = &made[1].pieces[0] else {
            panic!("checkbox")
        };
        assert_eq!(
            super::super::bindings::option_checkbox(checkbox.id.as_deref().unwrap()),
            Some(eq_client_core::options::Toggle::SpellEffects)
        );
        assert_eq!(pages(&made, WindowId::Options).len(), 2);
        assert_eq!(
            pages(std::slice::from_ref(&first), WindowId::Spellbook).len(),
            1
        );
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
            !eq_client_core::options::Options::read(&options.text(), Default::default())
                .spell_effects
        );
    }
}
