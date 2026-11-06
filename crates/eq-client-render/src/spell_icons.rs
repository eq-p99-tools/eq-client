//! Spell artwork from the user's installation, in the skin the windows follow.
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{IconGrid, Piece, SpellGem},
    ui::{Area, InstalledClient},
};

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
    gem: Option<GemArtwork>,
}

/// The authored gem grid and placement, with the square fallback's bounds.
struct GemArtwork {
    grid: Option<IconGrid>,
    offset: Option<(f32, f32)>,
    slot_size: Vec2,
    square: Area,
    skin: Option<String>,
}

impl GemArtwork {
    /// Resolve one image and its display rectangle in the slot's skin. Titanium's
    /// paired grids use the same spell index. Mac-era definitions supply a gem
    /// index separately from the renderer's choice of atlas.
    fn image(
        &self,
        art: &mut super::sheets::Art,
        names: &super::spellbook::SpellNames,
        client: InstalledClient,
        index: usize,
        spell: u32,
    ) -> Option<(ImageNode, Area)> {
        let icon = match client {
            InstalledClient::Titanium => names.icon(spell),
            InstalledClient::EqMac => names.gem_icon(spell),
        };
        let cell = self
            .grid
            .as_ref()
            .zip(icon)
            .and_then(|(grid, icon)| grid.cell(icon));
        report_gem(index, spell, names, icon, cell.as_ref());
        art.draw_in(self.skin.clone());
        let gem = cell.and_then(|cell| {
            let image = art.cut(&cell)?;
            let Vec2 {
                x: width,
                y: height,
            } = image.rect?.size();
            let (x, y) = self.offset.unwrap_or((
                (self.slot_size.x - width) / 2.0,
                (self.slot_size.y - height) / 2.0,
            ));
            Some((
                image,
                Area {
                    x,
                    y,
                    width,
                    height,
                },
            ))
        });
        let picture = gem.or_else(|| Some((art.spell(names.icon(spell)?)?, self.square)));
        art.draw_in(None);
        picture
    }
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
            gem: None,
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

/// One artwork child inside a skinned memorized slot. Gem cells use the
/// grid's dimensions and the skin's offset, or center in the slot.
pub(super) fn gem_artwork(index: usize, gem: &SpellGem, skin: Option<String>) -> impl Bundle {
    let (x, y) = gem.icon_offset.unwrap_or(INSET);
    let size = (gem.area.height - 6.0).max(8.0);
    let (mut artwork, image, node, focus) = artwork_at(Source::Gem(index), (x, y), size);
    artwork.gem = Some(GemArtwork {
        grid: gem.icons.clone(),
        offset: gem.icon_offset,
        slot_size: Vec2::new(gem.area.width, gem.area.height),
        square: Area {
            x,
            y,
            width: size,
            height: size,
        },
        skin,
    });
    (artwork, image, node, focus)
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    online: Res<super::online::OnlineState>,
    names: Res<super::spellbook::SpellNames>,
    settings: Res<super::ViewerSettings>,
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
            Source::Book(index) => book.row_spell(online.world().spell_book(), index),
        };
        if artwork.initialized
            && artwork.shown == spell
            && !art.skin_changed()
            && !names.is_changed()
        {
            continue;
        }
        artwork.initialized = true;
        artwork.shown = spell;
        node.display = Display::None;
        if let (Source::Gem(index), Some(gem), Some(spell)) = (&artwork.source, &artwork.gem, spell)
        {
            if let Some((piece, area)) =
                gem.image(&mut art, &names, settings.0.installed_client, *index, spell)
            {
                *image = piece;
                node.left = px(area.x);
                node.top = px(area.y);
                node.width = px(area.width);
                node.height = px(area.height);
                node.display = Display::Flex;
            }
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
/// colour, and the index and skin cell selected for its artwork.
fn report_gem(
    index: usize,
    spell: u32,
    names: &super::spellbook::SpellNames,
    icon: Option<u32>,
    cell: Option<&Piece>,
) {
    let field = names.gem_tint_field(spell);
    let tint = names.gem_tint(spell);
    debug!(
        "Gem {} holds spell {spell}: tint field {field:?} -> {}, gem index {icon:?} -> {}",
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
    fn titanium_memorized_spells_use_gem_cells_at_the_authored_size() {
        let mut app = crate::testing::app();
        crate::sheets::testing::blank(&mut app);
        let sheet = crate::sheets::testing::sheet(
            &mut app,
            "synthetic-gems.tga",
            image::RgbaImage::from_pixel(80, 64, image::Rgba([255; 4])),
        );
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("synthetic-install".into());
        let mut fields = vec!["0"; 145];
        fields[0] = "73";
        fields[57] = "1";
        fields[144] = "3";
        app.insert_resource(crate::spellbook::SpellNames::parse(&fields.join("^")));
        let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
        let mut player = crate::online::testing::player(1);
        player.memorized_spells[0] = Some(73);
        crate::online::testing::admit(&mut online, 7, player);
        let mut book = eq_client_core::SpellBook::default();
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot: 1,
            spell_id: 73,
            mode: 0,
        });
        crate::online::testing::news(&mut online, [eq_client_core::WorldEvent::SpellBook(book)]);

        let gem = SpellGem {
            id: None,
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 36.0,
                height: 28.0,
            },
            holder: None,
            background: None,
            highlight: None,
            icon_offset: None,
            icons: Some(IconGrid {
                sheets: vec![Piece {
                    texture: "synthetic-gems.tga".into(),
                    x: 4,
                    y: 8,
                    width: 72,
                    height: 56,
                }],
                cell_width: 36,
                cell_height: 28,
                vertical: false,
            }),
        };
        let memorized = app.world_mut().spawn(gem_artwork(0, &gem, None)).id();
        let page = app
            .world_mut()
            .spawn(artwork(Source::BookPlace(1), 40.0))
            .id();
        app.add_systems(Update, update);
        app.update();

        let image = app.world().get::<ImageNode>(memorized).unwrap();
        assert_eq!(image.image, sheet);
        assert_eq!(image.rect, Some(Rect::new(40.0, 36.0, 76.0, 64.0)));
        let node = app.world().get::<Node>(memorized).unwrap();
        assert_eq!(node.display, Display::Flex);
        assert_eq!((node.left, node.top), (px(0), px(0)));
        assert_eq!((node.width, node.height), (px(36), px(28)));
        let book_image = app.world().get::<ImageNode>(page).unwrap();
        assert_ne!(book_image.image, sheet);
        assert_eq!(book_image.rect, Some(Rect::new(120.0, 0.0, 160.0, 40.0)));

        // This index still exists in the book grid, but the gem grid ends.
        fields[144] = "7";
        app.insert_resource(crate::spellbook::SpellNames::parse(&fields.join("^")));
        app.update();
        let image = app.world().get::<ImageNode>(memorized).unwrap();
        assert_ne!(image.image, sheet);
        assert_eq!(image.rect, Some(Rect::new(40.0, 40.0, 80.0, 80.0)));
        let node = app.world().get::<Node>(memorized).unwrap();
        assert_eq!(node.display, Display::Flex);
        assert_eq!((node.left, node.top), (px(3), px(2)));
        assert_eq!((node.width, node.height), (px(22), px(22)));
    }

    #[test]
    fn mac_book_icons_use_the_shared_book_and_gem_column_and_clear_on_page_turn() {
        let mut app = crate::testing::app();
        crate::sheets::testing::blank(&mut app);
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("synthetic-install".into());
        let mut fields = vec!["0"; 147];
        fields[0] = "42";
        fields[131] = "7";
        // A different value in Titanium's column must not select that icon.
        fields[144] = "20";
        app.insert_resource(crate::spellbook::SpellNames::parse_for(
            &fields.join("^"),
            eq_client_assets::ui::InstalledClient::EqMac,
        ));
        let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
        crate::online::testing::admit(&mut online, 7, crate::online::testing::player(1));
        let mut book = eq_client_core::SpellBook::default();
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot: 1,
            spell_id: 42,
            mode: 0,
        });
        crate::online::testing::news(&mut online, [eq_client_core::WorldEvent::SpellBook(book)]);
        let occupied = app
            .world_mut()
            .spawn(artwork(Source::BookPlace(1), 40.0))
            .id();
        let empty = app
            .world_mut()
            .spawn(artwork(Source::BookPlace(0), 40.0))
            .id();
        app.add_systems(Update, update);
        app.update();
        assert_eq!(
            app.world().get::<Node>(occupied).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<ImageNode>(occupied).unwrap().rect,
            Some(Rect::new(40.0, 40.0, 80.0, 80.0))
        );
        assert_eq!(
            app.world().get::<Node>(empty).unwrap().display,
            Display::None
        );
        // Keeping the same spell while selecting another skin must resolve
        // its declared cell again, rather than retain the old rectangle.
        app.world_mut().resource_mut::<crate::skin::UiSkin>().0 = "synthetic".into();
        crate::sheets::testing::blank(&mut app);
        crate::sheets::testing::definitions(
            &mut app,
            "<XML><Ui2DAnimation item=\"A_SpellIcons\">\
            <Grid>true</Grid><Vertical>true</Vertical><CellWidth>40</CellWidth><CellHeight>40</CellHeight>\
            <Frames><Texture>spells01.tga</Texture><Location><X>0</X><Y>0</Y></Location>\
            <Size><CX>80</CX><CY>160</CY></Size></Frames></Ui2DAnimation></XML>",
        );
        app.update();
        assert_eq!(
            app.world().get::<ImageNode>(occupied).unwrap().rect,
            Some(Rect::new(40.0, 120.0, 80.0, 160.0))
        );
        app.world_mut()
            .resource_mut::<crate::spellbook::BookView>()
            .spread = 1;
        app.update();
        assert_eq!(
            app.world().get::<Node>(occupied).unwrap().display,
            Display::None
        );
    }

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
