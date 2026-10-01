//! Default Titanium spell artwork loaded only from the user's installation.
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(super) enum Source {
    Effect(u16),
    Buff(u32),
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

#[derive(Resource, Default)]
pub(super) struct Icons(BTreeMap<u32, Option<Handle<Image>>>);

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

/// `EQUI_Animations` `A_SpellIcons` uses a horizontal 6-by-6 grid of 40px cells.
fn address(icon: u32) -> (u32, Rect) {
    let cell = icon % 36;
    let x = f32::from(u16::try_from(cell % 6 * 40).expect("cell is bounded"));
    let y = f32::from(u16::try_from(cell / 6 * 40).expect("cell is bounded"));
    (icon / 36 + 1, Rect::new(x, y, x + 40.0, y + 40.0))
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    hud: Res<super::hud::HudState>,
    names: Res<super::spellbook::SpellNames>,
    book: Res<super::spellbook::BookView>,
    bindings: Res<super::hud::hotbar::Bindings>,
    settings: Res<super::ViewerSettings>,
    mut cache: ResMut<Icons>,
    mut images: ResMut<Assets<Image>>,
    mut icons: Query<(&mut Artwork, &mut ImageNode, &mut Node)>,
) {
    for (mut artwork, mut image, mut node) in &mut icons {
        let spell = match artwork.source {
            Source::Effect(id) => hud
                .buff_state
                .effects()
                .get(&id)
                .map(|effect| u32::from(effect.spell_id)),
            Source::Buff(slot) => hud
                .buff_state
                .slots()
                .as_ref()
                .and_then(|buffs| buffs.get(&slot))
                .map(|buff| buff.spell_id),
            Source::Gem(index) => hud.spells.get(index).copied().flatten(),
            Source::Action(index) => bindings
                .gem(index)
                .and_then(|gem| hud.spells.get(gem).copied().flatten()),
            Source::Book(index) => hud.spell_book.as_ref().and_then(|spells| {
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
        let Some(directory) = settings.0.eq_directory.as_deref() else {
            continue;
        };
        let (sheet, rect) = address(icon);
        let handle = cache.0.entry(sheet).or_insert_with(|| {
            let pixels = image::open(
                directory
                    .join("uifiles/default")
                    .join(format!("spells{sheet:02}.tga")),
            )
            .ok()?
            .into_rgba8();
            if pixels.width() != 256 || pixels.height() != 256 {
                return None;
            }
            let mut texture = Image::new(
                bevy::render::render_resource::Extent3d {
                    width: 256,
                    height: 256,
                    depth_or_array_layers: 1,
                },
                bevy::render::render_resource::TextureDimension::D2,
                pixels.into_raw(),
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
            );
            texture.sampler = bevy::image::ImageSampler::nearest();
            Some(images.add(texture))
        });
        if let Some(handle) = handle {
            image.image = handle.clone();
            image.rect = Some(rect);
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
        let mut hud = super::super::hud::HudState::default();
        hud.spells[0] = Some(73);
        let mut images = Assets::<Image>::default();
        let handle = images.add(Image::default());
        let mut icons = Icons::default();
        icons.0.insert(1, Some(handle));
        app.insert_resource(hud)
            .insert_resource(super::super::spellbook::SpellNames::parse(
                &fields.join("^"),
            ))
            .init_resource::<super::super::spellbook::BookView>()
            .init_resource::<super::super::hud::hotbar::Bindings>()
            .insert_resource(super::super::ViewerSettings(super::super::ViewerConfig {
                eq_directory: Some(std::path::PathBuf::from("unused-cached-sheet")),
                ..default()
            }))
            .insert_resource(images)
            .insert_resource(icons)
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
        app.world_mut()
            .resource_mut::<super::super::hud::HudState>()
            .spells[0] = None;
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
    fn artwork_moves_across_columns_then_rows_then_sheets() {
        assert_eq!(address(0), (1, Rect::new(0.0, 0.0, 40.0, 40.0)));
        assert_eq!(address(1), (1, Rect::new(40.0, 0.0, 80.0, 40.0)));
        assert_eq!(address(6), (1, Rect::new(0.0, 40.0, 40.0, 80.0)));
        assert_eq!(address(36), (2, Rect::new(0.0, 0.0, 40.0, 40.0)));
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
