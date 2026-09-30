//! Dresses characters in their worn gear: each body part draws with the
//! material `eq_client_core::outfit` names for the gear, tinted, loaded from the
//! model's archive once and kept in the base look where the model lacks it.
use bevy::prelude::*;
use eq_client_assets::characters::CharacterAsset;
use eq_client_core::outfit::{self, Appearance};
use std::collections::HashMap;
use std::path::PathBuf;

/// A dressed material: the model's archive, the material's name and its tint.
type Look = (PathBuf, String, Option<[u8; 3]>);

/// Dressed materials; None when the archive has no such material or it could
/// not be loaded.
#[derive(Resource, Default)]
pub(super) struct Wardrobe(HashMap<Look, Option<Handle<StandardMaterial>>>);

type Gpu<'a, 'b> = (&'a mut Assets<Image>, &'b mut Assets<StandardMaterial>);

impl Wardrobe {
    /// The material for a primitive whose base-look material is `base_name`.
    fn material(
        &mut self,
        asset: &CharacterAsset,
        base_name: &str,
        base: &Handle<StandardMaterial>,
        look: &Appearance,
        (images, materials): Gpu,
    ) -> Handle<StandardMaterial> {
        let base_name = base_name.to_ascii_uppercase();
        let tint = outfit::tint(&base_name, look);
        // A look the model does not ship keeps the base material, still tinted.
        let name = Some(outfit::dressed(&base_name, look))
            .filter(|name| asset.has_material(name))
            .unwrap_or_else(|| base_name.clone());
        if name == base_name && tint.is_none() {
            return base.clone();
        }
        let key = (asset.source().to_owned(), name.clone(), tint);
        let entry = self.0.entry(key).or_insert_with(|| {
            let handle = if name == base_name {
                // Only tinted: a copy of the base material.
                materials
                    .get(base)
                    .cloned()
                    .map(|material| materials.add(material))
            } else {
                match asset.material_texture(&name) {
                    Ok(Some((texture, mode))) => {
                        let texture = super::create_texture_images(vec![texture], images);
                        Some(super::create_material(
                            mode,
                            texture.first(),
                            materials,
                            true,
                        ))
                    }
                    Ok(None) => None,
                    Err(error) => {
                        warn!("Armor texture {name} unavailable: {error}");
                        None
                    }
                }
            };
            if let (Some(handle), Some([red, green, blue])) = (&handle, tint)
                && let Some(mut material) = materials.get_mut(handle)
            {
                let alpha = material.base_color.alpha();
                material.base_color = Color::srgb_u8(red, green, blue).with_alpha(alpha);
            }
            handle
        });
        entry.clone().unwrap_or_else(|| base.clone())
    }
}

/// Redraws a character's parts and held items whenever its gear changes:
/// other spawns from their spawn record and wear changes, the player and the
/// paperdoll figure from the player's.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters.
pub(super) fn dress(
    mut commands: Commands,
    online: Res<super::online::OnlineState>,
    settings: Res<super::ViewerSettings>,
    (mut wardrobe, mut library): (ResMut<Wardrobe>, ResMut<super::item_models::ItemLibrary>),
    mut characters: Query<(
        &mut super::character::AnimatedCharacter,
        Option<&super::entities::RemoteEntity>,
    )>,
    mut parts: Query<&mut MeshMaterial3d<StandardMaterial>>,
    (mut images, mut meshes, mut materials): super::item_models::GpuAssets,
) {
    for (mut character, remote) in &mut characters {
        let look = match remote {
            Some(remote) => online.spawns.get(&remote.id).map(|spawn| spawn.appearance),
            // The player's model and its paperdoll copy.
            None => online.player.as_ref().map(|player| player.appearance),
        }
        .unwrap_or_default();
        if character.dressed == Some(look) {
            continue;
        }
        character.dressed = Some(look);
        let directory = settings.0.eq_directory.as_deref();
        hold(&mut commands, &mut character, &look, |name| {
            library.shape(name, directory, (&mut images, &mut meshes, &mut materials))
        });
        let character = &*character;
        for part in &character.parts {
            let handle = match character
                .asset
                .materials
                .get(part.index)
                .and_then(Option::as_ref)
            {
                Some(base_name) => wardrobe.material(
                    &character.asset,
                    base_name,
                    &part.base,
                    &look,
                    (&mut images, &mut materials),
                ),
                None => part.base.clone(),
            };
            if let Ok(mut material) = parts.get_mut(part.entity) {
                material.0 = handle;
            }
        }
    }
}

/// Puts the items this appearance holds in the character's hands, replacing
/// what it held before: the primary in the right hand, the secondary in the
/// left hand or, for a shield, on the shield point.
fn hold(
    commands: &mut Commands,
    character: &mut super::character::AnimatedCharacter,
    look: &Appearance,
    mut shape: impl FnMut(&str) -> Option<std::sync::Arc<super::item_models::Shape>>,
) {
    use eq_client_assets::characters::Attachment;
    use eq_client_core::outfit::TextureSlot;
    for (hand, slot) in [TextureSlot::Primary, TextureSlot::Secondary]
        .into_iter()
        .enumerate()
    {
        let number = look.material(slot);
        if character.held[hand].as_ref().map_or(0, |held| held.number) == number {
            continue;
        }
        if let Some(held) = character.held[hand].take() {
            commands.entity(held.entity).despawn();
        }
        let Some(model) = outfit::held_model(look, slot).and_then(|name| shape(&name)) else {
            continue;
        };
        let point = if slot == TextureSlot::Primary {
            Attachment::RightHand
        } else if outfit::on_shield_point(number, model.flat) {
            Attachment::Shield
        } else {
            Attachment::LeftHand
        };
        let placed = character.attachments[point.index()]
            .map_or(Transform::IDENTITY, Transform::from_matrix);
        let layers = character.layers.clone();
        let entity = commands
            .spawn((
                super::character::HeldItem,
                placed,
                Visibility::Inherited,
                layers.clone(),
            ))
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
}
