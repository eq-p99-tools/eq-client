//! Dresses characters in their worn gear: each body part draws with the
//! material `eq_client_core::outfit` names for the gear, tinted, loaded from the
//! model's archive once and kept in the base look where the model lacks it,
//! and only the body and head the gear calls for are drawn.
use crate::asset_loads::{Asset, Loads};
use bevy::prelude::*;
use eq_client_assets::characters::{CharacterAsset, Piece};
use eq_client_core::outfit::{self, Appearance, Shape};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// A dressed material: the model's archive, the material's name and its tint.
type Look = (PathBuf, String, Option<[u8; 3]>);

/// Texture identity excludes tint so all colors share one decoded image.
type Texture = (PathBuf, String);

/// Admission-owned dressed materials and their asynchronous texture requests.
#[derive(Resource, Default)]
pub(super) struct Wardrobe {
    materials: HashMap<Look, Handle<StandardMaterial>>,
    textures: HashMap<Texture, Option<Handle<StandardMaterial>>>,
    loads: Loads<
        Texture,
        (
            eq_client_assets::ZoneTexture,
            eq_client_assets::MaterialMode,
        ),
    >,
}

impl Wardrobe {
    /// Releases cached ownership and obsolete reads; active entities retain handles.
    pub(super) fn clear(&mut self) {
        self.materials.clear();
        self.textures.clear();
        self.loads.clear();
    }

    /// Requests a worn texture without reading an archive on the frame thread.
    fn material(
        &mut self,
        asset: &Arc<CharacterAsset>,
        (base_name, helm): (&str, bool),
        base: &Handle<StandardMaterial>,
        look: &Appearance,
        materials: &mut Assets<StandardMaterial>,
    ) -> Asset<Handle<StandardMaterial>> {
        let base_name = base_name.to_ascii_uppercase();
        let tint = outfit::tint(&base_name, helm, look);
        let name = Some(outfit::dressed(&base_name, look))
            .filter(|name| asset.has_material(name))
            .unwrap_or_else(|| base_name.clone());
        if name == base_name && tint.is_none() {
            return Asset::Ready(base.clone());
        }
        let key = (asset.source().to_owned(), name.clone(), tint);
        if let Some(handle) = self.materials.get(&key) {
            return Asset::Ready(handle.clone());
        }
        let texture_key = (asset.source().to_owned(), name.clone());
        let original = if name == base_name {
            base.clone()
        } else if let Some(texture) = self.textures.get(&texture_key) {
            texture.clone().unwrap_or_else(|| base.clone())
        } else {
            let asset = Arc::clone(asset);
            self.loads.request(texture_key, move || {
                asset
                    .material_texture(&name)
                    .inspect_err(|error| warn!("Armor texture {name} unavailable: {error}"))
                    .ok()
                    .flatten()
            });
            return Asset::Pending;
        };
        let handle = tinted(&original, tint, materials);
        self.materials.insert(key, handle.clone());
        Asset::Ready(handle)
    }
}

/// A tint changes the material only; its image handles remain shared.
fn tinted(
    base: &Handle<StandardMaterial>,
    tint: Option<[u8; 3]>,
    materials: &mut Assets<StandardMaterial>,
) -> Handle<StandardMaterial> {
    let Some([red, green, blue]) = tint else {
        return base.clone();
    };
    let Some(mut material) = materials.get(base).cloned() else {
        return base.clone();
    };
    material.base_color = Color::srgb_u8(red, green, blue).with_alpha(material.base_color.alpha());
    materials.add(material)
}

/// Installs at most one decoded armor texture per frame; each tint reuses it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(super) fn poll_materials(
    mut wardrobe: ResMut<Wardrobe>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if let Some((key, texture)) = wardrobe.loads.poll() {
        let material = texture.map(|(texture, mode)| {
            let images = super::create_texture_images(vec![texture], &mut images);
            super::create_material(mode, images.first(), &mut materials, true)
        });
        wardrobe.textures.insert(key, material);
    }
}

/// Whether a piece of a model is drawn in this shape.
pub(super) fn shows(piece: Piece, shape: Shape) -> bool {
    match piece {
        Piece::Body(body) => body == shape.body,
        Piece::Head(head) => head == shape.head,
        Piece::Fixed => true,
    }
}

/// The body and head a model draws with for an appearance; `helm` says
/// whether its helm shows.
fn shape_of(asset: &CharacterAsset, race: u32, look: &Appearance, helm: bool) -> Shape {
    outfit::shape(
        race,
        look,
        helm,
        |body| asset.pieces.contains(&Piece::Body(body)),
        |head| asset.pieces.contains(&Piece::Head(head)),
    )
}

/// Redraws a character's parts and held items whenever its gear changes:
/// other spawns from their spawn record and wear changes, a character
/// shown at character select from its list's look, the player and the
/// paperdoll figure from the player's.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters.
pub(super) fn dress(
    mut commands: Commands,
    online: Res<super::online::OnlineState>,
    (settings, options): (
        Res<super::ViewerSettings>,
        Res<super::options::OptionsState>,
    ),
    (mut wardrobe, mut library): (ResMut<Wardrobe>, ResMut<super::item_models::ItemLibrary>),
    mut characters: Query<(
        &mut super::character::AnimatedCharacter,
        Option<&super::entities::RemoteEntity>,
        Option<&super::select_preview::Look>,
    )>,
    mut parts: Query<(&mut MeshMaterial3d<StandardMaterial>, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut character, remote, listed) in &mut characters {
        let ((race, look), helm) = worn(listed, remote, online.world(), options.options.show_helm);
        if character.dressed == Some((look, helm)) {
            continue;
        }
        let directory = settings.0.eq_directory.as_deref();
        let mut ready = hold(&mut commands, &mut character, &look, |name| {
            library.shape(name, directory)
        });
        let shape = shape_of(&character.asset, race, &look, helm);
        let character = &mut *character;
        for part in &character.parts {
            let piece = character
                .asset
                .pieces
                .get(part.index)
                .copied()
                .unwrap_or(Piece::Fixed);
            let shown = shows(piece, shape);
            if let Some(slot) = character.shown.get_mut(part.index) {
                *slot = shown;
            }
            let helm = matches!(piece, Piece::Head(head) if head > 0);
            let handle = match character
                .asset
                .materials
                .get(part.index)
                .and_then(Option::as_ref)
            {
                Some(base_name) => wardrobe.material(
                    &character.asset,
                    (base_name, helm),
                    &part.base,
                    &look,
                    &mut materials,
                ),
                None => Asset::Ready(part.base.clone()),
            };
            let handle = match handle {
                Asset::Ready(handle) => Some(handle),
                Asset::Pending => {
                    ready = false;
                    None
                }
                Asset::Missing => Some(part.base.clone()),
            };
            if let Ok((mut material, mut visibility)) = parts.get_mut(part.entity) {
                if let Some(handle) = handle {
                    material.0 = handle;
                }
                *visibility = if shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
        character.dressed = ready.then_some((look, helm));
    }
}

/// What a character wears, by race and appearance, and whether its helm
/// shows: a character shown at select wears its list's look, other spawns
/// their spawn record's, and the player (and the paperdoll copy) the
/// player's. Other characters always show their helms, and so does a
/// listed one, whose list says nothing of it; the player's own follows the
/// Show My Helm option.
fn worn(
    listed: Option<&super::select_preview::Look>,
    remote: Option<&super::entities::RemoteEntity>,
    world: &eq_client_core::world::ClientWorld,
    show_helm: bool,
) -> ((u32, Appearance), bool) {
    let look = match (listed, remote) {
        (Some(listed), _) => Some((listed.race, listed.appearance)),
        (None, Some(remote)) => world
            .spawn(remote.id)
            .map(|spawn| &spawn.state)
            .map(|spawn| (spawn.race, spawn.appearance)),
        (None, None) => world
            .player()
            .map(|player| (player.race, player.appearance)),
    }
    .unwrap_or_default();
    (look, remote.is_some() || listed.is_some() || show_helm)
}

/// Puts the items this appearance holds in the character's hands, replacing
/// what it held before: the primary in the right hand, the secondary in the
/// left hand or, for a shield, on the shield point.
fn hold(
    commands: &mut Commands,
    character: &mut super::character::AnimatedCharacter,
    look: &Appearance,
    mut shape: impl FnMut(&str) -> Asset<Arc<super::item_models::Shape>>,
) -> bool {
    use eq_client_assets::characters::Attachment;
    use eq_client_core::outfit::TextureSlot;
    let mut ready = true;
    for (hand, slot) in [TextureSlot::Primary, TextureSlot::Secondary]
        .into_iter()
        .enumerate()
    {
        let number = look.material(slot);
        if character.held[hand].as_ref().map_or(0, |held| held.number) == number {
            continue;
        }
        let model = outfit::held_model(look, slot).map_or(Asset::Missing, |name| shape(&name));
        if matches!(model, Asset::Pending) {
            ready = false;
            continue;
        }
        if let Some(held) = character.held[hand].take() {
            commands.entity(held.entity).despawn();
        }
        let Asset::Ready(model) = model else {
            continue;
        };
        let point = if slot == TextureSlot::Primary {
            Attachment::RightHand
        } else if outfit::on_shield_point(number, model.flat) {
            Attachment::Shield
        } else {
            Attachment::LeftHand
        };
        // Held items show only at a point the skeleton has; animation keeps
        // them there.
        let (placed, shown) = character.attachments[point.index()]
            .map_or((Transform::IDENTITY, Visibility::Hidden), |point| {
                (Transform::from_matrix(point), Visibility::Inherited)
            });
        let layers = character.layers.clone();
        let entity = commands
            .spawn((super::character::HeldItem, placed, shown, layers.clone()))
            .with_children(|parent| {
                for (mesh, material) in &model.primitives {
                    parent.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        layers.clone(),
                    ));
                }
            })
            .id();
        commands.entity(character.model).add_child(entity);
        character.held[hand] = Some(super::character::Held {
            number,
            entity,
            point,
        });
    }
    ready
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_variants_share_an_image_and_keep_the_base_material_unchanged() {
        let mut images = Assets::<Image>::default();
        let image = images.add(super::super::create_image(1, 1, vec![255; 4]));
        let mut materials = Assets::<StandardMaterial>::default();
        let base = materials.add(StandardMaterial {
            base_color_texture: Some(image.clone()),
            base_color: Color::WHITE.with_alpha(0.5),
            ..default()
        });
        let red = tinted(&base, Some([255, 0, 0]), &mut materials);
        let blue = tinted(&base, Some([0, 0, 255]), &mut materials);
        assert_ne!(red, blue);
        for handle in [&base, &red, &blue] {
            let material = materials.get(handle).unwrap();
            assert_eq!(material.base_color_texture.as_ref(), Some(&image));
            assert_eq!(material.base_color.alpha(), 0.5);
        }
        assert_eq!(
            materials.get(&base).unwrap().base_color,
            Color::WHITE.with_alpha(0.5)
        );
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn pending_held_item_keeps_the_old_visual_until_its_replacement_arrives() {
        use crate::character::{AnimatedCharacter, Held, testing};
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        testing::stores(&mut world);
        let prepared = testing::prepared(&mut world);
        let root = testing::drawn(&mut world, &prepared);
        let previous = world.spawn_empty().id();
        world.get_mut::<AnimatedCharacter>(root).unwrap().held[0] = Some(Held {
            entity: previous,
            number: 9,
            point: eq_client_assets::characters::Attachment::RightHand,
        });
        let mut look = Appearance::default();
        look.materials[outfit::TextureSlot::Primary.index()] = 10;
        let ready = world
            .run_system_once(
                move |mut commands: Commands, mut query: Query<&mut AnimatedCharacter>| {
                    hold(
                        &mut commands,
                        &mut query.get_mut(root).unwrap(),
                        &look,
                        |_| Asset::Pending,
                    )
                },
            )
            .unwrap();
        assert!(!ready);
        assert!(world.get_entity(previous).is_ok());
        assert_eq!(
            world.get::<AnimatedCharacter>(root).unwrap().held[0]
                .as_ref()
                .unwrap()
                .number,
            9
        );
        let ready = world
            .run_system_once(
                move |mut commands: Commands, mut query: Query<&mut AnimatedCharacter>| {
                    hold(
                        &mut commands,
                        &mut query.get_mut(root).unwrap(),
                        &look,
                        |name| {
                            assert_eq!(name, "IT10");
                            Asset::Ready(Arc::new(crate::item_models::Shape {
                                primitives: Vec::new(),
                                bounds: None,
                                flat: false,
                            }))
                        },
                    )
                },
            )
            .unwrap();
        assert!(ready);
        assert!(world.get_entity(previous).is_err());
        assert_eq!(
            world.get::<AnimatedCharacter>(root).unwrap().held[0]
                .as_ref()
                .unwrap()
                .number,
            10
        );
    }

    #[test]
    fn a_character_shown_at_select_wears_its_lists_look_with_its_helm() {
        let world = eq_client_core::world::ClientWorld::default();
        let mut appearance = Appearance::default();
        appearance.materials[1] = 3;
        let listed = super::super::select_preview::Look {
            race: 8,
            appearance,
        };
        assert_eq!(
            worn(Some(&listed), None, &world, false),
            ((8, appearance), true)
        );
        // The player's own model, with no player yet, wears nothing and
        // follows Show My Helm.
        assert_eq!(
            worn(None, None, &world, false),
            ((0, Appearance::default()), false)
        );
    }
}
