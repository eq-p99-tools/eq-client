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
    /// The spell in this place on the skin's spellbook's open pages.
    BookPlace(u8),
    Action(usize),
}

#[derive(Component)]
pub(super) struct Artwork {
    source: Source,
    shown: Option<u32>,
    initialized: bool,
    /// The skin's gem icons, which a gem shows in place of the square icon
    /// where the skin has them and the spell names its cell.
    gems: Option<eq_client_assets::sidl::IconGrid>,
    /// Whether a gem-icon artwork beside this square icon shows the spell
    /// instead, where the spell names its cell.
    beside_gems: bool,
}

/// Where artwork sits in its button when nothing else places it.
pub(super) const INSET: (f32, f32) = (3.0, 2.0);

/// Child artwork never participates in hit testing; the owning button handles input.
pub(super) fn artwork(source: Source, size: f32) -> impl Bundle {
    artwork_at(source, INSET, size)
}

/// Artwork at this place in the button that owns it.
pub(super) fn artwork_at(
    source: Source,
    (left, top): (f32, f32),
    size: f32,
) -> (Artwork, ImageNode, Node, bevy::ui::FocusPolicy) {
    (
        Artwork {
            source,
            shown: None,
            initialized: false,
            gems: None,
            beside_gems: false,
        },
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: px(left),
            top: px(top),
            width: px(size),
            height: px(size),
            ..default()
        },
        bevy::ui::FocusPolicy::Pass,
    )
}

/// A gem's square icon, at this place and size in the gem; `beside_gems`
/// where a gem-icon artwork shows the spells that name their cell.
pub(super) fn gem_square_artwork(
    index: usize,
    place: (f32, f32),
    size: f32,
    beside_gems: bool,
) -> impl Bundle {
    let (mut artwork, image, node, focus) = artwork_at(Source::Gem(index), place, size);
    artwork.beside_gems = beside_gems;
    (artwork, image, node, focus)
}

/// A gem's spell as the skin's gem icons draw it, one cell's size, at this
/// place in the gem.
pub(super) fn gem_artwork(
    index: usize,
    gems: eq_client_assets::sidl::IconGrid,
    (left, top): (f32, f32),
) -> impl Bundle {
    let (width, height) = (gems.cell_width, gems.cell_height);
    (
        Artwork {
            source: Source::Gem(index),
            shown: None,
            initialized: false,
            gems: Some(gems),
            beside_gems: false,
        },
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: px(left),
            top: px(top),
            width: px(width),
            height: px(height),
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
            Source::BookPlace(place) => book.spell(online.world().spell_book(), place),
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
        if let (Source::Gem(index), Some(spell)) = (&artwork.source, spell) {
            report_gem(*index, spell, &names, artwork.gems.as_ref());
        }
        // A gem shows the spell's gem icon where the skin has them; the
        // square icon where the skin or the spell data lacks it.
        let gem = artwork.gems.as_ref().and_then(|gems| {
            let cell = gems.cell(names.gem_icon(spell?)?)?;
            art.cut(&cell)
        });
        if let Some(piece) = gem {
            *image = piece;
            node.display = Display::Flex;
            continue;
        }
        if artwork.gems.is_some()
            || (artwork.beside_gems && spell.and_then(|spell| names.gem_icon(spell)).is_some())
        {
            continue;
        }
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

/// One debug line for what a gem's spell resolves to: its tint field and
/// colour, and its icon field and the skin cell that field picks.
fn report_gem(
    index: usize,
    spell: u32,
    names: &super::spellbook::SpellNames,
    gems: Option<&eq_client_assets::sidl::IconGrid>,
) {
    let field = names.gem_tint_field(spell);
    let tint = names.gem_tint(spell);
    let icon = names.gem_icon(spell);
    let cell = gems.zip(icon).and_then(|(gems, icon)| gems.cell(icon));
    debug!(
        "Gem {} holds spell {spell}: tint field {field:?} -> {}, icon field {icon:?} -> {}",
        index + 1,
        tint.map_or_else(|| "none".to_owned(), |tint| format!("{tint:#010x}")),
        cell.map_or_else(
            || "no cell".to_owned(),
            |cell| format!("{} at ({}, {})", cell.texture, cell.x, cell.y)
        ),
    );
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
