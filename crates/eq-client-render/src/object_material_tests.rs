//! Synthetic objects exercise production zone spawning, batching and material selection.
use super::*;
use bevy::ecs::world::CommandQueue;
use bevy::render::render_resource::Face;
use eq_client_assets::{
    BlendOpacity, MaterialMode, ObjectMaterial, ZoneModel, ZoneObject, ZonePrimitive,
};

fn triangle(mode: MaterialMode) -> ZonePrimitive {
    ZonePrimitive {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        texture_coordinates: Vec::new(),
        indices: vec![0, 1, 2],
        texture: None,
        material_mode: mode,
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn placed_object_sidedness_survives_filtering_batching_and_reflection() {
    let mut zone = ZoneAsset {
        regions: eq_client_assets::regions::ZoneRegions::default(),
        short_name: "synthetic".to_owned(),
        primitives: Vec::new(),
        terrain_materials: Vec::new(),
        point_lights: Vec::new(),
        textures: Vec::new(),
        models: Vec::new(),
        objects: Vec::new(),
        objects_unplaced: 0,
        object_skins_skipped: 0,
        collision: Vec::new(),
    };
    for mode in [
        MaterialMode::Opaque,
        MaterialMode::Masked,
        MaterialMode::Blended(BlendOpacity::Half),
        MaterialMode::Additive,
    ] {
        for double_sided in [false, true] {
            let model = zone.models.len();
            let mut empty = triangle(mode);
            empty.indices.clear();
            zone.models.push(ZoneModel {
                name: format!("MODEL{model}"),
                primitives: vec![empty, triangle(mode)],
                materials: vec![
                    ObjectMaterial {
                        double_sided: !double_sided,
                    },
                    ObjectMaterial { double_sided },
                ],
                collision: Vec::new(),
            });
            for scale in [[1.0; 3], [-1.0, 1.0, 1.0], [-1.0, -1.0, 1.0]] {
                zone.objects.push(ZoneObject {
                    model,
                    translation: [10.0, 0.0, 10.0],
                    rotation: Quat::IDENTITY.to_array(),
                    scale,
                });
            }
        }
    }
    let mut world = World::new();
    let mut queue = CommandQueue::default();
    let mut images = Assets::default();
    let mut meshes = Assets::default();
    let mut materials = Assets::default();
    spawn_static_zone(
        &mut Commands::new(&mut queue, &world),
        zone,
        false,
        &mut images,
        &mut meshes,
        &mut materials,
    );
    queue.apply(&mut world);

    let (mut back, mut front, mut both, mut batched, mut apart) = (0, 0, 0, 0, 0);
    let mut query = world.query::<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &Transform)>();
    for (mesh, material, transform) in query.iter(&world) {
        let material = materials.get(&material.0).unwrap();
        let mesh = meshes.get(&mesh.0).unwrap();
        match material.cull_mode {
            Some(Face::Back) => {
                assert!(!material.double_sided);
                back += 1;
            }
            Some(Face::Front) => {
                assert!(!material.double_sided);
                assert!(transform.to_matrix().determinant() < 0.0);
                assert!(matches!(
                    material.alpha_mode,
                    AlphaMode::Blend | AlphaMode::Add
                ));
                front += 1;
            }
            None => {
                assert!(material.double_sided);
                both += 1;
            }
        }
        if matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
            // Baking reverses only the reflected copy's winding; the shared
            // material remains the model's normal cull direction.
            let indices = mesh.indices().unwrap().iter().collect::<Vec<_>>();
            assert_eq!(indices, [0, 1, 2, 3, 5, 4, 6, 7, 8]);
            assert_ne!(material.cull_mode, Some(Face::Front));
            batched += 1;
        } else {
            assert_eq!(
                mesh.indices().unwrap().iter().collect::<Vec<_>>(),
                [0, 1, 2]
            );
            assert_eq!(
                material.cull_mode == Some(Face::Front),
                !material.double_sided && transform.to_matrix().determinant() < 0.0,
            );
            apart += 1;
        }
    }
    assert_eq!((back, front, both), (6, 2, 8));
    assert_eq!((batched, apart), (4, 12));
}

#[test]
fn callers_without_object_metadata_retain_two_sided_pickable_geometry() {
    let mut meshes = Assets::default();
    let mut materials = Assets::default();
    let draws = create_render_primitives(
        vec![triangle(MaterialMode::Masked)],
        &[],
        &[],
        &mut meshes,
        &mut materials,
        true,
        character::ON_A_SPAWN,
    );
    let (mesh, material) = &draws[0];
    let material = materials.get(material).unwrap();
    assert!(material.double_sided);
    assert_eq!(material.cull_mode, None);
    assert_eq!(material.alpha_mode, AlphaMode::Mask(0.5));
    assert!(
        meshes
            .get(mesh)
            .unwrap()
            .asset_usage
            .contains(RenderAssetUsages::MAIN_WORLD)
    );
}

#[test]
fn reflected_material_variants_are_shared_without_changing_the_source() {
    let mut meshes = Assets::default();
    let mut materials = Assets::default();
    let draws = create_render_primitives(
        vec![triangle(MaterialMode::Blended(BlendOpacity::Quarter))],
        &[ObjectMaterial {
            double_sided: false,
        }],
        &[],
        &mut meshes,
        &mut materials,
        true,
        RenderAssetUsages::RENDER_WORLD,
    );
    let source = &draws[0].1;
    let mut cache = std::collections::HashMap::new();
    let first = gpu_assets::mirrored_material(source, &mut materials, &mut cache);
    let second = gpu_assets::mirrored_material(source, &mut materials, &mut cache);
    assert_eq!(first, second);
    assert_eq!(cache.len(), 1);
    assert_eq!(materials.get(source).unwrap().cull_mode, Some(Face::Back));
    let mirrored = materials.get(&first).unwrap();
    assert_eq!(mirrored.cull_mode, Some(Face::Front));
    assert_eq!(mirrored.alpha_mode, AlphaMode::Blend);
    assert_eq!(mirrored.base_color.to_srgba().alpha, 0.25);
    assert!(!mirrored.double_sided);
}
