//! Spell artwork from the user's installation, in the skin the windows follow.
use bevy::prelude::*;

pub(super) enum Source {
    Effect(u16),
    Buff(u32),
    /// The pet's buff in this slot.
    PetBuff(usize),
    /// The player's buff on this button of an effects window.
    Window(eq_client_core::buffs::EffectWindow, u32),
    Gem(usize),
    Book(usize),
    Action(usize),
}

#[derive(Component)]
pub(super) struct Artwork {
    source: Source,
    shown: Option<u32>,
    initialized: bool,
}

/// Child artwork never participates in hit testing; the owning button handles input.
pub(super) fn artwork(source: Source, size: f32) -> impl Bundle {
    (
        Artwork {
            source,
            shown: None,
            initialized: false,
        },
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: px(3),
            top: px(2),
            width: px(size),
            height: px(size),
            ..default()
        },
        bevy::ui::FocusPolicy::Pass,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    online: Res<super::online::OnlineState>,
    names: Res<super::spellbook::SpellNames>,
    book: Res<super::spellbook::BookView>,
    bindings: Res<super::hud::hotbar::Bindings>,
    mut art: super::sheets::Art,
    mut icons: Query<(&mut Artwork, &mut ImageNode, &mut Node)>,
) {
    for (mut artwork, mut image, mut node) in &mut icons {
        let spell = match artwork.source {
            Source::Effect(id) => online
                .world()
                .buffs()
                .effects()
                .get(&id)
                .map(|effect| u32::from(effect.spell_id)),
            Source::Buff(slot) => online
                .world()
                .buffs()
                .slots()
                .as_ref()
                .and_then(|buffs| buffs.get(&slot))
                .map(|buff| buff.spell_id),
            Source::PetBuff(slot) => online
                .world()
                .pet_buffs()
                .and_then(|buffs| buffs.slots.get(slot).copied().flatten())
                .map(|buff| buff.spell_id),
            Source::Window(window, button) => online
                .world()
                .buffs()
                .in_window(window, button)
                .map(eq_client_core::buffs::Shown::spell_id),
            Source::Gem(index) => online.world().gem(index),
            Source::Action(index) => bindings.gem(index).and_then(|gem| online.world().gem(gem)),
            Source::Book(index) => online.world().spell_book().and_then(|spells| {
                spells
                    .slots()
                    .iter()
                    .flatten()
                    .nth(book.page * super::spellbook::ROWS_PER_PAGE + index)
                    .copied()
            }),
        };
        if artwork.initialized && artwork.shown == spell {
            continue;
        }
        artwork.initialized = true;
        artwork.shown = spell;
        node.display = Display::None;
        let Some(icon) = spell.and_then(|spell| names.icon(spell)) else {
            continue;
        };
        if let Some(piece) = art.spell(icon) {
            image.image = piece.image;
            image.rect = piece.rect;
            node.display = Display::Flex;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clearing_a_gem_removes_previous_artwork() {
        let mut fields = vec!["0"; 145];
        fields[0] = "73";
        fields[1] = "Synthetic spell";
        let mut app = App::new();
        let mut online = super::super::online::OnlineState::new(true);
        let mut player = crate::online::testing::player(1);
        player.memorized_spells[0] = Some(73);
        crate::online::testing::admit(&mut online, 1, player);
        app.init_resource::<Assets<Image>>();
        crate::sheets::testing::blank(&mut app);
        app.insert_resource(online)
            .insert_resource(super::super::spellbook::SpellNames::parse(
                &fields.join("^"),
            ))
            .init_resource::<super::super::spellbook::BookView>()
            .init_resource::<super::super::hud::hotbar::Bindings>()
            .insert_resource(super::super::ViewerSettings(super::super::ViewerConfig {
                eq_directory: Some(std::path::PathBuf::from("unused-cached-sheet")),
                ..default()
            }))
            .add_systems(Update, update);
        let entity = app.world_mut().spawn(artwork(Source::Gem(0), 30.0)).id();
        let action = app.world_mut().spawn(artwork(Source::Action(0), 30.0)).id();
        app.update();
        assert_eq!(
            app.world().get::<Node>(action).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(entity).unwrap().display,
            Display::Flex
        );
        crate::online::testing::spell(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::SpellUpdate::Slot {
                slot: 0,
                spell_id: 0,
                mode: 2,
            },
        );
        app.update();
        assert_eq!(
            app.world().get::<Node>(action).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(entity).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn a_pets_buff_shows_in_its_slot() {
        let mut fields = vec!["0"; 145];
        fields[0] = "73";
        fields[1] = "Synthetic spell";
        let mut app = App::new();
        let mut online = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        crate::online::testing::spawn_entry(&mut online, 8, crate::online::testing::pet(8, 1));
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::PetBuffs(
                eq_client_core::pets::PetBuffs {
                    pet: 8,
                    slots: vec![
                        Some(eq_client_core::pets::PetBuff {
                            spell_id: 73,
                            ticks: 5,
                        }),
                        None,
                    ],
                },
            )],
        );
        app.init_resource::<Assets<Image>>();
        crate::sheets::testing::blank(&mut app);
        app.insert_resource(online)
            .insert_resource(super::super::spellbook::SpellNames::parse(
                &fields.join("^"),
            ))
            .init_resource::<super::super::spellbook::BookView>()
            .init_resource::<super::super::hud::hotbar::Bindings>()
            .insert_resource(super::super::ViewerSettings(super::super::ViewerConfig {
                eq_directory: Some(std::path::PathBuf::from("unused-cached-sheet")),
                ..default()
            }))
            .add_systems(Update, update);
        let first = app
            .world_mut()
            .spawn(artwork(Source::PetBuff(0), 20.0))
            .id();
        let second = app
            .world_mut()
            .spawn(artwork(Source::PetBuff(1), 20.0))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Node>(first).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(second).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn spell_data_names_the_icon_or_none() {
        let mut fields = vec!["0"; 145];
        fields[0] = "73";
        fields[1] = "Synthetic spell";
        fields[144] = "36";
        assert_eq!(
            super::super::spellbook::SpellNames::parse(&fields.join("^")).icon(73),
            Some(36)
        );
        fields[144] = "-1";
        assert_eq!(
            super::super::spellbook::SpellNames::parse(&fields.join("^")).icon(73),
            None
        );
    }
}
