//! Showing a zone, the same way offline and online. Every entry first leaves
//! the zone shown before, even one whose assets then fail to load, so nothing
//! of the old zone lingers: its drawn scene, collision, terrain, regions and
//! door models.
use super::{
    Collision, HudText, OrbitCamera, Player, PlayerBody, SceneEntity, SceneInfo, TerrainSurface,
    build_collision, character, doors, hud, spawn_player_and_hud, spawn_static_zone,
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
}

impl PreparedEntry {
    /// Uses the latest placement, including corrections received while loading.
    pub(super) fn place(&mut self, position: eq_client_core::WorldPosition) {
        self.entry.placed = placement(position);
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
                match eq_client_assets::characters::load_installed_character(
                    directory, zone_name, model,
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
            },
        })
    }

    /// Builds collision and terrain indices alongside archive decoding.
    pub(super) fn prepare(self) -> PreparedEntry {
        let collision = Collision(build_collision(&self.zone));
        let terrain = TerrainSurface::from_primitives(&self.zone.primitives);
        PreparedEntry {
            entry: self,
            collision,
            terrain,
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
    nearby: Option<ResMut<'w, super::entities::NearbyEntities>>,
    wardrobe: Option<ResMut<'w, super::outfit::Wardrobe>>,
    items: Option<ResMut<'w, super::item_models::ItemLibrary>>,
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
    pub(super) fn leave(&self, commands: &mut Commands, regions: &mut ZoneRegions) {
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
        } = prepared;
        let Entry {
            zone,
            character,
            placed,
            body,
        } = entry;
        commands.insert_resource(collision);
        commands.insert_resource(terrain);
        commands.insert_resource(SceneInfo {
            zone_name: zone.short_name.clone(),
        });
        regions.clone_from(&zone.regions);
        spawn_static_zone(
            commands,
            zone,
            terrain_only,
            &mut self.images,
            &mut self.meshes,
            &mut self.materials,
        );
        let player = spawn_player_and_hud(
            commands,
            placed.translation,
            character.is_none(),
            body,
            &mut self.meshes,
            &mut self.materials,
        );
        if let Some(asset) = character {
            character::spawn(
                commands,
                player,
                asset,
                body.feet_offset,
                &mut self.images,
                &mut self.meshes,
                &mut self.materials,
            );
        }
        commands.entity(player).insert(placed);
        self.focus(placed.translation);
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
