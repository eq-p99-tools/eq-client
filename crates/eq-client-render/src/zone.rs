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
    /// The checked Titanium head basis, when the admitted model supplies it.
    pub native_head: Option<super::breath::head::Basis>,
    /// How many times its authored size the player's model is drawn.
    pub scale: f32,
    /// What the zone's header says about drawing models, which the mesh's
    /// headroom follows ([`crate::zone_navmesh::settings`]).
    pub zone_scale: eq_client_assets::scale::ZoneScale,
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

    /// Remembered feet must still fit the installed zone after
    /// an installation update. Keep its actual height, including water/air;
    /// never snap it to a different floor of a multi-level zone.
    pub(super) fn preview_place(&mut self, feet: eq_client_core::WorldPosition) -> bool {
        let Some(placed) =
            selection_placement(feet, self.entry.zone.bounds(), self.entry.body.feet_offset)
        else {
            return false;
        };
        self.entry.placed = placed;
        true
    }

    /// The renderer yaw the placed character faces.
    pub(super) fn facing(&self) -> f32 {
        self.entry.placed.rotation.to_euler(EulerRot::YXZ).0
    }

    /// Stands the character on the floor at a character select spot: the
    /// first floor at or below the spot's height and a little above it (at
    /// the spot's own height for a spot afloat in open water), or
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
        let on_spot =
            spot.and_then(|spot| Some((self.terrain.spot_feet(spot)?, spot.position().heading)));
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
    /// The player's own model is drawn by the installed client's rule, with
    /// the zone header's classic model scale flag, as every other spawn is
    /// ([`super::entities::Drawn`]).
    /// `generation` is the look the player's model draws with
    /// ([`eq_client_core::player_models::generation`]); only the drawing
    /// takes it, as the body's height, feet, size and breath come from the
    /// classic model however it is drawn ([`own_model`]).
    pub(super) fn admission(
        zone_name: &str,
        player: &eq_client_core::PlayerState,
        (client, flag): (eq_client_assets::ui::InstalledClient, Option<u32>),
        directory: &std::path::Path,
        generation: eq_client_core::player_models::Generation,
    ) -> Result<Self, String> {
        let zone = eq_client_assets::load_zone(directory, zone_name)
            .map_err(|error| format!("Zone {zone_name} could not be loaded: {error}"))?;
        let model = eq_client_core::races::model(player.race, player.gender);
        let character = model.and_then(|model| {
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
        let native_head = if match client {
            eq_client_assets::ui::InstalledClient::Titanium => true,
            // RoF2's water is not read yet.
            eq_client_assets::ui::InstalledClient::EqMac
            | eq_client_assets::ui::InstalledClient::Rof2 => false,
        } && character.is_some()
        {
            super::breath::head::Basis::load(directory, player, model)
        } else {
            None
        };
        let luclin = match generation {
            eq_client_core::player_models::Generation::Classic => None,
            eq_client_core::player_models::Generation::Luclin => model.and_then(|model| {
                eq_client_assets::characters::load_luclin_character(
                    directory,
                    model,
                    (player.race, player.gender),
                    &eq_client_assets::characters::DonorArchives::default(),
                )
                .inspect_err(|error| {
                    if first_miss(model) {
                        warn!("Luclin model {model} unavailable, drawing the classic one: {error}");
                    }
                })
                .ok()
            }),
        };
        let (character, height) = own_model(character, luclin);
        // Use the model's estimated standing offset to project feet.
        // A fresh server placement can still need a collision step to stand.
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
            native_head,
            scale: super::entities::Drawn::by(
                client,
                flag,
                (player.race, player.gender, player.size),
            )
            .scale,
            zone_scale: eq_client_assets::scale::ZoneScale {
                classic_reduced: flag.is_some_and(|flag| flag != 0),
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
            native_head: None,
            // The preview stands the model at its mesh's own size.
            scale: 1.0,
            zone_scale: eq_client_assets::scale::ZoneScale::default(),
        })
    }

    /// Builds collision and terrain indices alongside archive decoding, and
    /// with `navmesh`, gathers the triangles the zone's mesh is built over.
    pub(super) fn prepare(self, navmesh: bool, titanium_swimming: bool) -> PreparedEntry {
        let triangles = collision_triangles(&self.zone);
        let navmesh = navmesh.then(|| triangles.clone());
        let swim_origin = titanium_swimming.then_some(self.body.feet_offset);
        let collision = Collision(collision_world(&self.zone, triangles, swim_origin));
        let terrain = TerrainSurface::from_primitives(&self.zone.primitives);
        PreparedEntry {
            entry: self,
            collision,
            terrain,
            navmesh,
        }
    }
}

/// Whether this is the first time this run the player's Luclin model `model`
/// could not be read, so it is said once however many zones it is missed in.
/// Other spawns' misses are kept by [`super::entities::NearbyEntities`].
fn first_miss(model: &'static str) -> bool {
    static MISSED: std::sync::Mutex<std::collections::BTreeSet<&str>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    MISSED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(model)
}

/// The model the player is drawn with, and the height their body collides
/// by: the Luclin model where one was read, else the classic one; the height
/// is always the classic model's, so the look changes nothing the player
/// moves, swims or reaches by (6.554).
fn own_model(
    classic: Option<CharacterAsset>,
    luclin: Option<CharacterAsset>,
) -> (Option<CharacterAsset>, f32) {
    let height = classic.as_ref().map_or(6.0, CharacterAsset::height);
    (luclin.or(classic), height)
}

/// Where a server position puts the player's root, facing its heading.
pub(super) fn placement(position: eq_client_core::WorldPosition) -> Transform {
    Transform::from_translation(Vec3::from_array(render_position(position))).with_rotation(
        Quat::from_rotation_y(eq_client_core::render_heading(position.heading)),
    )
}

fn selection_placement(
    position: eq_client_core::WorldPosition,
    bounds: Option<([f32; 3], [f32; 3])>,
    feet_offset: f32,
) -> Option<Transform> {
    let (min, max) = bounds?;
    let origin = Vec3::from_array(render_position(position));
    let margin = Vec3::splat(32.0);
    (origin.cmpge(Vec3::from_array(min) - margin).all()
        && origin.cmple(Vec3::from_array(max) + margin).all()
        && position.heading.is_finite())
    .then(|| {
        let mut placed = placement(position);
        placed.translation.y += feet_offset;
        placed
    })
}

/// The zone shown, and what entering and leaving one builds and clears.
#[derive(SystemParam)]
pub(super) struct Scene<'w, 's> {
    roots: Query<'w, 's, Entity, Roots>,
    players: Query<'w, 's, &'static mut Transform, With<Player>>,
    cameras: Query<'w, 's, (Entity, &'static mut OrbitCamera)>,
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
        for (_, mut camera) in &mut self.cameras {
            camera.reset_view();
            camera.level_look();
        }
        let gone = matches!(
            reason,
            Reset::Entered | Reset::Camped | Reset::Lost { ended: true, .. }
        );
        if gone {
            for (entity, _) in &self.cameras {
                commands
                    .entity(entity)
                    .remove::<super::camera::FreeCamera>();
            }
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
        commands.remove_resource::<super::lighting::SceneLights>();
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
            native_head,
            scale,
            zone_scale,
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
        if zone.object_lights_skipped > 0 {
            info!(
                "Zone {}: {} object placement colour tracks left out: not one colour per vertex of a one-mesh model",
                zone.short_name, zone.object_lights_skipped
            );
        }
        if let (Some(triangles), Some(mesh)) = (navmesh, self.navmesh.as_mut()) {
            let directory = self
                .settings
                .as_ref()
                .and_then(|settings| settings.0.settings_directory.clone());
            let client = self
                .settings
                .as_ref()
                .map_or_else(Default::default, |settings| settings.0.installed_client);
            mesh.start(
                &zone.short_name,
                triangles,
                world,
                directory,
                crate::zone_navmesh::settings(client, zone_scale),
            );
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
            character::spawn(commands, player, prepared, body.feet_offset, scale);
            if let Some(basis) = native_head {
                commands.entity(player).insert(basis);
            }
        }
        commands.entity(player).insert(placed);
        self.focus(placed.translation);
        // Each zone entry starts the camera behind the player's new heading;
        // its pitch and zoom stay as chosen. Inferred, item 6.240.
        for (_, mut camera) in &mut self.cameras {
            camera.reset_view();
            camera.level_look();
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
        for (_, mut camera) in &mut self.cameras {
            camera.focus = point;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_preview_keeps_the_actual_height_and_checks_updated_zone_bounds() {
        let position = eq_client_core::WorldPosition {
            x: 10.0,
            y: -20.0,
            z: 18.0,
            heading: 192.0,
        };
        let bounds = Some(([-128.0; 3], [128.0; 3]));
        let placed = selection_placement(position, bounds, 3.0).unwrap();
        assert_eq!(placed.rotation, placement(position).rotation);
        assert_eq!(
            placed.translation.y, 21.0,
            "keep a point between floors or afloat"
        );
        assert!(selection_placement(position, None, 3.0).is_none());
        // This installation no longer has geometry near the recorded point.
        assert!(selection_placement(position, Some(([500.0; 3], [600.0; 3])), 3.0).is_none());
        for bad in [f32::NAN, f32::INFINITY, 10_000.0] {
            assert!(
                selection_placement(
                    eq_client_core::WorldPosition { x: bad, ..position },
                    bounds,
                    3.0
                )
                .is_none()
            );
        }
    }

    #[test]
    fn a_missing_luclin_model_of_the_players_is_said_once_a_run() {
        assert!(first_miss("TSTLUCLINA"));
        assert!(!first_miss("TSTLUCLINA"));
        assert!(first_miss("TSTLUCLINB"));
    }

    #[test]
    fn the_players_luclin_model_is_drawn_but_the_body_keeps_the_classic_height() {
        let classic = CharacterAsset::testing();
        let mut luclin = CharacterAsset::testing();
        for point in &mut luclin.primitives[0].positions {
            point[1] *= 3.0;
        }
        luclin.set_luclin(true);
        assert_ne!(luclin.height().to_bits(), classic.height().to_bits());
        let height = classic.height();
        let (drawn, collides) = own_model(Some(classic.clone()), Some(luclin));
        assert!(drawn.unwrap().luclin());
        assert_eq!(collides.to_bits(), height.to_bits());
        // Without a Luclin model, the classic one, as before.
        let (drawn, collides) = own_model(Some(classic), None);
        assert!(!drawn.unwrap().luclin());
        assert_eq!(collides.to_bits(), height.to_bits());
        // Without a classic one, the default height whatever is drawn.
        let (drawn, collides) = own_model(None, Some(CharacterAsset::testing()));
        assert!(drawn.is_some());
        assert_eq!(collides.to_bits(), 6.0_f32.to_bits());
    }
}
