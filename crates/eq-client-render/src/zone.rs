//! Showing a zone, the same way offline and online. Every entry first leaves
//! the zone shown before, even one whose assets then fail to load, so nothing
//! of the old zone lingers: its drawn scene, collision, terrain, regions and
//! door models.
use super::{
    Collision, HudText, OrbitCamera, Player, PlayerBody, SceneEntity, SceneInfo, TerrainSurface,
    ViewerSettings, character, collision_triangles, collision_world, doors, hud,
    spawn_player_and_hud, spawn_static_zone, zone_navmesh::ZoneNavMesh,
};
use bevy::{ecs::system::SystemParam, prelude::*};
use eq_client_assets::{ZoneAsset, characters::CharacterAsset, regions::ZoneRegions};
use eq_client_core::{render_position, world::Reset};

/// What a zone's entry spawns: its scene, the player and the HUD.
type Roots = Or<(With<SceneEntity>, With<HudText>, With<hud::HudRoot>)>;

/// The zone to show and the player to stand in it.
pub(super) struct Entry {
    pub zone: ZoneAsset,
    /// The player's model, or None for a placeholder.
    pub character: Option<CharacterAsset>,
    /// Where the player stands, in render coordinates, and which way they face.
    pub placed: Transform,
    pub body: PlayerBody,
}

/// CPU-heavy scene data prepared before returning to the render thread.
pub(super) struct PreparedEntry {
    entry: Entry,
    collision: Collision,
    terrain: TerrainSurface,
    /// The triangles to build the zone's navigation mesh over, when it is
    /// built.
    navmesh: Option<Vec<[[f32; 3]; 3]>>,
}

impl PreparedEntry {
    /// Uses the latest placement, including corrections received while loading.
    pub(super) fn place(&mut self, position: eq_client_core::WorldPosition) {
        self.entry.placed = placement(position);
    }

    /// The renderer yaw the placed character faces.
    pub(super) fn facing(&self) -> f32 {
        self.entry.placed.rotation.to_euler(EulerRot::YXZ).0
    }

    /// Stands the character on the floor at a character select spot: the
    /// first floor at or below the spot's height and a little above it, or
    /// without a spot (or a floor under it) the highest surface at the
    /// centre of the zone's bounds, as the offline viewer starts.
    pub(super) fn stand(&mut self, spot: Option<&eq_client_core::select_spots::Spot>) {
        let (min, max) = self
            .entry
            .zone
            .bounds()
            .map_or((Vec3::splat(-100.0), Vec3::splat(100.0)), |(min, max)| {
                (Vec3::from_array(min), Vec3::from_array(max))
            });
        let centre = (min + max) * 0.5;
        let on_spot = spot.and_then(|spot| {
            let position = spot.position();
            let point = Vec3::from_array(render_position(position));
            let floor = self.terrain.height_below(
                point.x,
                point.z,
                point.y + eq_client_core::select_spots::ABOVE_FLOOR,
            )?;
            Some((Vec3::new(point.x, floor, point.z), position.heading))
        });
        let (feet, heading) = on_spot.unwrap_or_else(|| {
            let floor = self
                .terrain
                .height_at(centre.x, centre.z)
                .unwrap_or(centre.y);
            (Vec3::new(centre.x, floor, centre.z), 0.0)
        });
        self.entry.placed =
            Transform::from_translation(feet + Vec3::Y * self.entry.body.feet_offset)
                .with_rotation(Quat::from_rotation_y(eq_client_core::render_heading(
                    heading,
                )));
    }
}

impl Entry {
    /// The zone the world admitted the player to, with the player where the
    /// server put them, or why it cannot be shown.
    pub(super) fn admission(
        zone_name: &str,
        player: &eq_client_core::PlayerState,
        directory: &std::path::Path,
    ) -> Result<Self, String> {
        let zone = eq_client_assets::load_zone(directory, zone_name)
            .map_err(|error| format!("Zone {zone_name} could not be loaded: {error}"))?;
        let character =
            eq_client_core::races::model(player.race, player.gender).and_then(|model| {
                // The player's own model is a single load, so its donor
                // archives need not outlive it.
                match eq_client_assets::characters::load_installed_character(
                    directory,
                    zone_name,
                    model,
                    &eq_client_assets::characters::DonorArchives::default(),
                ) {
                    Ok(asset) => Some(asset),
                    Err(error) => {
                        warn!("Character model: {error}");
                        None
                    }
                }
            });
        let height = character.as_ref().map_or(6.0, CharacterAsset::height);
        // EQ's rule for the model keeps every later server-placed position
        // (zoning, teleports) at the same height above the feet.
        let feet_offset = eq_client_core::z_offset(player.race, player.size);
        // Lets a logged session be replayed offline with the same feet height.
        debug!(
            "Admission feet offset {feet_offset} for size {} and model height {height}",
            player.size
        );
        Ok(Self {
            zone,
            character,
            placed: placement(player.position),
            body: PlayerBody {
                feet_offset,
                height,
                size: eq_client_core::size_or_default(player.race, player.size),
            },
        })
    }

    /// A listed character's zone at character select, with the character's
    /// model standing in it once [`PreparedEntry::stand`] places it, or why
    /// it cannot be shown.
    pub(super) fn preview(
        zone_name: &str,
        (race, gender): (u32, u8),
        directory: &std::path::Path,
    ) -> Result<Self, String> {
        let zone = eq_client_assets::load_zone(directory, zone_name)
            .map_err(|error| format!("Zone {zone_name} could not be loaded: {error}"))?;
        let character = eq_client_core::races::model(race, u32::from(gender))
            .ok_or_else(|| format!("No model for race {race}"))
            .and_then(|model| {
                eq_client_assets::characters::load_installed_character(
                    directory,
                    zone_name,
                    model,
                    &eq_client_assets::characters::DonorArchives::default(),
                )
                .map_err(|error| format!("Character model: {error}"))
            })?;
        let height = character.height();
        Ok(Self {
            zone,
            character: Some(character),
            placed: Transform::IDENTITY,
            body: PlayerBody {
                // The racial default size, as no list gives one.
                feet_offset: eq_client_core::z_offset(race, 0.0),
                height,
                size: eq_client_core::size_or_default(race, 0.0),
            },
        })
    }

    /// Builds collision and terrain indices alongside archive decoding, and
    /// with `navmesh`, gathers the triangles the zone's mesh is built over.
    pub(super) fn prepare(self, navmesh: bool) -> PreparedEntry {
        let triangles = collision_triangles(&self.zone);
        let navmesh = navmesh.then(|| triangles.clone());
        let collision = Collision(collision_world(&self.zone, triangles));
        let terrain = TerrainSurface::from_primitives(&self.zone.primitives);
        PreparedEntry {
            entry: self,
            collision,
            terrain,
            navmesh,
        }
    }
}

/// Where a server position puts the player's root, facing its heading.
pub(super) fn placement(position: eq_client_core::WorldPosition) -> Transform {
    Transform::from_translation(Vec3::from_array(render_position(position))).with_rotation(
        Quat::from_rotation_y(eq_client_core::render_heading(position.heading)),
    )
}

/// The zone shown, and what entering and leaving one builds and clears.
#[derive(SystemParam)]
pub(super) struct Scene<'w, 's> {
    roots: Query<'w, 's, Entity, Roots>,
    players: Query<'w, 's, &'static mut Transform, With<Player>>,
    cameras: Query<'w, 's, &'static mut OrbitCamera>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    bindposes: ResMut<'w, Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    nearby: Option<ResMut<'w, super::entities::NearbyEntities>>,
    wardrobe: Option<ResMut<'w, super::outfit::Wardrobe>>,
    items: Option<ResMut<'w, super::item_models::ItemLibrary>>,
    navmesh: Option<ResMut<'w, ZoneNavMesh>>,
    settings: Option<Res<'w, ViewerSettings>>,
}

impl Scene<'_, '_> {
    /// Forgets the spawns drawn for an admission the world forgot: a new one,
    /// a camp, or a session that ended.
    pub(super) fn forget(&mut self, reason: Reset, commands: &mut Commands) {
        let gone = matches!(
            reason,
            Reset::Entered | Reset::Camped | Reset::Lost { ended: true, .. }
        );
        if gone {
            if let Some(nearby) = self.nearby.as_mut() {
                nearby.forget(commands);
            }
            if let Some(wardrobe) = self.wardrobe.as_mut() {
                wardrobe.clear();
            }
            if let Some(items) = self.items.as_mut() {
                items.clear_shapes();
            }
        }
    }

    /// Leaves the zone shown: its scene, the player and the HUD, and
    /// everything built from the zone's assets.
    pub(super) fn leave(&mut self, commands: &mut Commands, regions: &mut ZoneRegions) {
        for entity in &self.roots {
            commands.entity(entity).despawn();
        }
        commands.insert_resource(Collision(None));
        commands.insert_resource(TerrainSurface(Vec::new()));
        commands.insert_resource(doors::Models::default());
        commands.insert_resource(SceneInfo {
            zone_name: String::new(),
        });
        *regions = ZoneRegions::default();
        if let Some(navmesh) = self.navmesh.as_mut() {
            navmesh.leave();
        }
    }

    /// Shows a zone with the player in it, the camera on them, and returns
    /// the player, who exists once the commands apply.
    pub(super) fn enter(
        &mut self,
        commands: &mut Commands,
        prepared: PreparedEntry,
        terrain_only: bool,
        regions: &mut ZoneRegions,
    ) -> Entity {
        let PreparedEntry {
            entry,
            collision,
            terrain,
            navmesh,
        } = prepared;
        let Entry {
            zone,
            character,
            placed,
            body,
        } = entry;
        // A clone shares the world's geometry.
        let world = collision.0.clone();
        commands.insert_resource(collision);
        commands.insert_resource(terrain);
        commands.insert_resource(SceneInfo {
            zone_name: zone.short_name.clone(),
        });
        regions.clone_from(&zone.regions);
        if zone.objects_unplaced > 0 || zone.object_skins_skipped > 0 {
            info!(
                "Zone {}: {} object placements name no model its object archive draws; {} object skins left out",
                zone.short_name, zone.objects_unplaced, zone.object_skins_skipped
            );
        }
        if let (Some(triangles), Some(mesh)) = (navmesh, self.navmesh.as_mut()) {
            let directory = self
                .settings
                .as_ref()
                .and_then(|settings| settings.0.settings_directory.clone());
            mesh.start(&zone.short_name, triangles, world, directory);
        }
        spawn_static_zone(
            commands,
            zone,
            terrain_only,
            &mut self.images,
            &mut self.meshes,
            &mut self.materials,
        );
        let character = character.and_then(|asset| {
            character::prepare(
                asset,
                (
                    &mut self.images,
                    &mut self.meshes,
                    &mut self.materials,
                    &mut self.bindposes,
                ),
            )
        });
        let player = spawn_player_and_hud(
            commands,
            placed.translation,
            character.is_none(),
            body,
            &mut self.meshes,
            &mut self.materials,
        );
        if let Some(prepared) = character {
            character::spawn(commands, player, prepared, body.feet_offset);
        }
        commands.entity(player).insert(placed);
        self.focus(placed.translation);
        // Each zone entry starts the camera behind the player's new heading;
        // its pitch and zoom stay as chosen. Inferred, item 6.240.
        for mut camera in &mut self.cameras {
            super::start_behind(&mut camera, &placed);
        }
        player
    }

    /// Puts the player where the server placed them; `entered` is a player
    /// this batch spawned, who only exists once the commands apply.
    pub(super) fn place(
        &mut self,
        commands: &mut Commands,
        placed: Transform,
        entered: Option<Entity>,
    ) {
        if let Some(entity) = entered {
            commands.entity(entity).insert(placed);
        } else if let Ok(mut transform) = self.players.single_mut() {
            *transform = placed;
        }
        self.focus(placed.translation);
    }

    /// Where the player is drawn now.
    pub(super) fn player(&self) -> Option<Transform> {
        self.players.single().ok().copied()
    }

    fn focus(&mut self, point: Vec3) {
        for mut camera in &mut self.cameras {
            camera.focus = point;
        }
    }
}
