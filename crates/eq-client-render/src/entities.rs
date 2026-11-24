//! Nearby spawn presentation. Distant entities retain state but own no render objects.

pub(super) mod outline;

use super::{ViewerSettings, character, online::OnlineState};
use bevy::prelude::*;
use eq_client_assets::characters::{
    CharacterAsset, DonorArchives, load_installed_character, load_luclin_character,
};
use eq_client_core::{
    SpawnKind,
    entities::nearby_with_invisible,
    player_models::{self, Generation, LuclinModels},
    races, render_position,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource, Default)]
pub(super) struct NearbyEntities {
    pub(super) rendered: BTreeMap<u16, Entity>,
    revisions: BTreeMap<u16, u64>,
    /// Which ordinary roots were prepared for a canonical carrier report.
    carrier_roots: BTreeSet<u16>,
    /// Each model read for this zone, by code and look; None where neither
    /// look could be read.
    models: BTreeMap<(&'static str, Generation), Option<character::PreparedCharacter>>,
    loading: Option<(
        (&'static str, Generation),
        super::background::Background<Loaded>,
    )>,
    /// The Luclin models the viewer chose, as they were when this zone's
    /// first spawn was drawn: a change in the client shows from the next
    /// zone, and the options file is read at each entry into the world. The
    /// program reads its keys once, at start-up (R49-44 b): a stated
    /// departure.
    luclin: Option<LuclinModels>,
    /// The Luclin models found missing this run, each said once.
    luclin_missing: BTreeSet<&'static str>,
    /// The archives this zone's models borrow movements from, each parsed once.
    donors: DonorArchives,
    elapsed: f32,
    /// Whether the last pass drew every spawn it chose, so none is still to
    /// pop in.
    settled: bool,
    /// The zone header's classic model scale flag the drawn spawns were
    /// scaled by.
    scaled_by: Option<u32>,
    /// How many spawns the last pass chose to load.
    chosen: usize,
}

impl NearbyEntities {
    /// Forgets the spawns drawn for an admission the world forgot, and the
    /// models loaded for its zone.
    pub(super) fn forget(&mut self, commands: &mut Commands) {
        for (_, entity) in std::mem::take(&mut self.rendered) {
            commands.entity(entity).despawn();
        }
        self.models.clear();
        self.luclin = None;
        self.donors = DonorArchives::default();
        self.loading = None;
        self.revisions.clear();
        self.carrier_roots.clear();
        self.settled = false;
    }

    /// Whether every spawn near the player is drawn, as of the last pass.
    pub(crate) const fn settled(&self) -> bool {
        self.settled
    }

    /// Says whether the last pass drew every spawn it chose.
    #[cfg(test)]
    pub(crate) const fn set_settled(&mut self, settled: bool) {
        self.settled = settled;
    }
}

/// How long a frame may spend drawing spawns while the loading screen hides
/// them coming in. Out in the open, one is drawn every tenth of a second.
/// Either way a model is read from disk in the background, so a frame only
/// puts the ones already read in place.
const HURRIED: std::time::Duration = std::time::Duration::from_millis(40);

#[derive(Component)]
pub(super) struct RemoteEntity {
    pub(super) id: u16,
    /// When the server report this entity moves on from came, once placed.
    seen: Option<std::time::Instant>,
    /// Where its soles were drawn the frame before, since that report, and
    /// how it falls ([`eq_client_core::carried_soles`]).
    soles: Option<eq_client_core::DrawnSoles>,
    /// What is left of the latest report's correction to the drawn model.
    correction: eq_client_core::Correction,
    /// How far under the reported position a drawn model's soles hang, in
    /// world units; None for a marker, which stays where the server says.
    feet: Option<f32>,
    /// The size its model is drawn by ([`Drawn::size`]), which sets how far
    /// its soles reach for the floor.
    size: f32,
    /// Where the client generation has carried it since that report, frame
    /// by frame, as its program steps a spawn
    /// ([`eq_client_core::movement::carry`]).
    carried: Option<eq_client_core::movement::Carried>,
}

impl RemoteEntity {
    /// A spawn not yet placed, whose model's soles hang `feet` under its
    /// reported position (None for a marker), drawn by `size`.
    pub(crate) const fn new(id: u16, feet: Option<f32>, size: f32) -> Self {
        Self {
            id,
            seen: None,
            soles: None,
            correction: eq_client_core::Correction::NONE,
            feet,
            size,
            carried: None,
        }
    }
}

impl RemoteEntity {
    /// How far under the reported position its model's soles hang, in
    /// world units; None for a marker.
    pub(super) const fn feet(&self) -> Option<f32> {
        self.feet
    }
}

/// A model read in the background, and why its Luclin look could not be
/// read where that was the one asked for.
pub(super) struct Loaded {
    asset: Option<CharacterAsset>,
    luclin_missing: Option<String>,
}

/// Reads a spawn's model in the look asked for: the Luclin one where chosen,
/// else (or where it cannot be read) the classic one.
fn load_model<E: std::fmt::Display>(
    generation: Generation,
    luclin: impl FnOnce() -> Result<CharacterAsset, E>,
    classic: impl FnOnce() -> Result<CharacterAsset, E>,
) -> Loaded {
    let luclin_missing = match generation {
        Generation::Classic => None,
        Generation::Luclin => match luclin() {
            Ok(asset) => {
                return Loaded {
                    asset: Some(asset),
                    luclin_missing: None,
                };
            }
            Err(error) => Some(error.to_string()),
        },
    };
    Loaded {
        asset: classic().ok(),
        luclin_missing,
    }
}

/// How the installed client draws a spawn's model: the size it draws it by
/// and the scale of its mesh as authored, by the client's rule
/// ([`eq_client_assets::scale::ModelScale`]), with what the zone's header
/// says ([`eq_client_core::world::ClientWorld::classic_model_scale_flag`]).
/// The player, other spawns and corpses all take it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Drawn {
    /// The size sent, or the race's default, as the client draws by it.
    pub(crate) size: f32,
    /// How many times its authored size the model is drawn.
    pub(crate) scale: f32,
}

impl Drawn {
    /// How `client` draws a spawn of this race, gender and sent size in the
    /// zone `world` is in.
    pub(crate) fn of(
        client: eq_client_assets::ui::InstalledClient,
        world: &eq_client_core::world::ClientWorld,
        (race, gender, size): (u32, u32, f32),
    ) -> Self {
        Self::by(
            client,
            world.classic_model_scale_flag(),
            (race, gender, size),
        )
    }

    /// How `client` draws a spawn of this race, gender and sent size in a
    /// zone whose header sent this classic model scale flag.
    pub(crate) fn by(
        client: eq_client_assets::ui::InstalledClient,
        flag: Option<u32>,
        (race, gender, size): (u32, u32, f32),
    ) -> Self {
        let rule = client.model_scale();
        let zone = eq_client_assets::scale::ZoneScale {
            classic_reduced: flag.is_some_and(|flag| flag != 0),
        };
        Self {
            size: rule.size(race, gender, size),
            scale: rule.drawn_scale(race, gender, size, zone),
        }
    }

    /// How far under its reported position its soles hang, in world units.
    pub(crate) fn feet(self) -> f32 {
        eq_client_core::drawn_feet_offset(self.scale)
    }
}

/// The wire generation an installed client speaks.
pub(crate) const fn dialect(
    client: eq_client_assets::ui::InstalledClient,
) -> eq_client_core::GameDialect {
    match client {
        eq_client_assets::ui::InstalledClient::Titanium => eq_client_core::GameDialect::Titanium,
        eq_client_assets::ui::InstalledClient::EqMac => eq_client_core::GameDialect::EqMac,
        eq_client_assets::ui::InstalledClient::Rof2 => eq_client_core::GameDialect::Rof2,
    }
}

/// How far from the player, `player` in the renderer's frame, a spawn
/// can show with the scene drawn to `clip` from the camera: through a
/// perspective view's corners, where it sees farthest, past the camera's
/// own place behind the player. This is how far spawns are loaded, not how
/// far they show; the far plane decides that ([`super::camera::past_far`]).
/// An orthographic view, or none yet, loads them to the clip.
pub(super) fn loading_reach(
    clip: f32,
    view: Option<(&Transform, &Projection)>,
    player: Vec3,
) -> f32 {
    view.and_then(|(transform, projection)| {
        super::camera::corner_reach(projection)
            .map(|corner| clip * corner + transform.translation.distance(player))
    })
    .unwrap_or(clip)
}

/// The stores a drawn spawn's meshes and materials go in; its model's
/// textures went in when the model was read.
type Stores<'a> = (&'a mut Assets<Mesh>, &'a mut Assets<StandardMaterial>);

/// Maintains a bounded nearby set. Out in the open, at most one new model is
/// instantiated every tenth of a second; behind the loading screen, as many
/// as fit in [`HURRIED`] a frame.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]
pub(super) fn reconcile(
    mut commands: Commands,
    state: Res<OnlineState>,
    (settings, options, loading): (
        Res<ViewerSettings>,
        Res<crate::options::OptionsState>,
        Option<Res<crate::loading::Loading>>,
    ),
    hidden: Option<Res<crate::corpses::HiddenCorpses>>,
    time: Res<Time>,
    mut nearby_state: ResMut<NearbyEntities>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    cameras: Query<(&Transform, &Projection), With<crate::OrbitCamera>>,
) {
    if !state.world().connected() {
        nearby_state.loading = None;
        return;
    }
    // The spawns come once the zone has loaded, or failed to: one whose
    // files are missing still shows its spawns.
    if state.scene_loading() {
        return;
    }
    let Some(player) = state.world().player() else {
        return;
    };
    let Some(directory) = &settings.0.eq_directory else {
        // With no installation nothing is drawn, so nothing is still to come.
        nearby_state.settled = true;
        return;
    };
    let hurried = loading.is_some_and(|loading| loading.covered());
    if let Some((key, job)) = &mut nearby_state.loading
        && let Some(Loaded {
            asset,
            luclin_missing,
        }) = job.poll(state.world().session_id())
    {
        let key = *key;
        if let Some(error) = luclin_missing
            && nearby_state.luclin_missing.insert(key.0)
        {
            // Said once a run, naming the model alone.
            warn!(
                "Luclin model {} unavailable, drawing the classic one: {error}",
                key.0
            );
        }
        let prepared = asset.and_then(|asset| {
            character::prepare(
                asset,
                (&mut images, &mut meshes, &mut materials, &mut bindposes),
            )
        });
        nearby_state.models.insert(key, prepared);
        nearby_state.loading = None;
    }
    nearby_state.elapsed += time.delta_secs();
    if !hurried && nearby_state.elapsed < 0.1 {
        return;
    }
    nearby_state.elapsed = 0.0;
    // A zone header that changes how models are scaled draws every spawn
    // again, as the program scales each one again.
    let flag = state.world().classic_model_scale_flag();
    let rescaled = std::mem::replace(&mut nearby_state.scaled_by, flag) != flag;
    let replaced: Vec<_> = nearby_state
        .rendered
        .keys()
        .copied()
        .filter(|id| {
            rescaled
                || nearby_state.carrier_roots.contains(id)
                    != state.world().carriers().latest(*id).is_some()
                || nearby_state.revisions.get(id).copied()
                    != state.world().spawn(*id).map(|spawn| spawn.revision)
        })
        .collect();
    for id in replaced {
        if let Some(entity) = nearby_state.rendered.remove(&id) {
            commands.entity(entity).despawn();
        }
        nearby_state.revisions.remove(&id);
        nearby_state.carrier_roots.remove(&id);
    }
    let present: BTreeSet<_> = nearby_state.rendered.keys().copied().collect();
    // Spawns are loaded as far as the view could show one, with no distance
    // or count of their own: the camera's far plane alone decides which
    // show (program read N7; the engine's own culling is unread, 6.342).
    // Before the zone gives a clip, as far as the program draws an unclipped
    // zone. A distance given on the command line loads them nearer.
    let reach = {
        let client = settings.0.installed_client;
        let clip = super::camera::clip_distance(client, &options.options, state.world())
            .unwrap_or(eq_client_assets::camera::FarPlane::UNCLIPPED);
        let reach = loading_reach(
            clip,
            cameras.single().ok(),
            Vec3::from_array(render_position(player.position)),
        );
        settings
            .0
            .entity_distance
            .map_or(reach, |distance| distance.min(reach))
    };
    let world = state.world();
    let luclin = *nearby_state
        .luclin
        .get_or_insert(options.options.luclin_models);
    let reveal = options
        .options
        .qol
        .on(eq_client_core::qol::Fix::ShowInvisible);
    let candidates = world
        .spawns()
        .values()
        .map(|spawn| &spawn.state)
        // A body type the program hides is apart from invisibility: no See
        // Invisible shows it, only the QoL fix that shows every spawn.
        .filter(|spawn| reveal || (world.can_see(spawn) && !world.hides_by_body(spawn)))
        // A corpse `/hidecorpses` hid isn't drawn, so isn't picked by the
        // mouse; a name still finds it (6.490).
        .filter(|spawn| {
            !hidden
                .as_deref()
                .is_some_and(|hidden| hidden.hides(spawn.spawn_id))
        });
    let selected = nearby_with_invisible(
        candidates,
        player.spawn_id,
        player.position,
        &present,
        reach,
        usize::MAX,
        true,
    );

    if nearby_state.chosen != selected.len() {
        nearby_state.chosen = selected.len();
        debug!(
            "spawns loaded: {} of {} within {reach:.0} of the player",
            selected.len(),
            state.world().spawns().len(),
        );
    }
    let desired: BTreeSet<_> = selected.iter().copied().collect();
    for id in present.difference(&desired) {
        if let Some(entity) = nearby_state.rendered.remove(id) {
            commands.entity(entity).despawn();
        }
    }
    let missing: Vec<_> = selected
        .into_iter()
        .filter(|id| !nearby_state.rendered.contains_key(id))
        .collect();
    let started = std::time::Instant::now();
    let mut drawn = 0;
    for &id in &missing {
        if drawn > 0 && (!hurried || started.elapsed() >= HURRIED) {
            break;
        }
        if draw(
            &mut commands,
            &state,
            (directory, settings.0.installed_client),
            (id, luclin),
            &mut nearby_state,
            (&mut meshes, &mut materials),
        ) {
            drawn += 1;
        }
    }
    nearby_state.settled = drawn == missing.len();
}

/// Draws one spawn near the player: its race's model, or a marker. The
/// first time the zone shows a race, its model is read from the
/// installation in the background and the spawn waits for it; says whether
/// the spawn was drawn.
#[allow(clippy::too_many_lines)] // One ordinary root and its model or marker are built together.
fn draw(
    commands: &mut Commands,
    state: &OnlineState,
    (directory, client): (&std::path::Path, eq_client_assets::ui::InstalledClient),
    (id, chosen): (u16, LuclinModels),
    nearby_state: &mut NearbyEntities,
    (meshes, materials): Stores,
) -> bool {
    let spawn = &state.world().spawns()[&id].state;
    let carrier = state.world().carriers().latest(id).is_some();
    let own = if carrier {
        None
    } else {
        races::model(spawn.race, spawn.gender)
    };
    // The look changes only which model is drawn: the size, feet and name
    // height below come from the race and size as for a classic model.
    let own_generation = player_models::generation((spawn.race, spawn.gender, spawn.kind), chosen);
    // A spawn whose own model the installation lacks is drawn as the
    // program draws it (0x43c84b, read R49-37a): a Human of its gender,
    // taken as race 1 for its size and feet unless its race is 216 or 472.
    // A carrier keeps its marker, and a race that draws nothing still draws
    // nothing. A race this client has no model name for yet keeps the
    // marker: that gap is ours, not the installation's.
    let unavailable = own
        .is_some_and(|code| matches!(nearby_state.models.get(&(code, own_generation)), Some(None)));
    let (model, race, generation) = if unavailable && !carrier && races::drawn(spawn.race) {
        let (code, race) = races::stand_in(spawn.race, spawn.gender);
        let generation = player_models::generation((race, spawn.gender, spawn.kind), chosen);
        (Some(code), race, generation)
    } else {
        (own, spawn.race, own_generation)
    };
    if let Some(code) = model
        && !nearby_state.models.contains_key(&(code, generation))
    {
        if nearby_state.loading.is_none() {
            let directory = directory.to_path_buf();
            let zone = state.world().zone().to_owned();
            let donors = nearby_state.donors.clone();
            let gender = spawn.gender;
            let mut job = super::background::Background::default();
            job.start(state.world().session_id().unwrap_or_default(), move || {
                load_model(
                    generation,
                    || load_luclin_character(&directory, code, (race, gender), &donors),
                    || {
                        // Read once per model and zone: a failure draws the
                        // Human stand-in for the rest of the zone, so it is
                        // said once.
                        load_installed_character(&directory, &zone, code, &donors).inspect_err(
                            |error| warn!("Model {code} unavailable in {zone}: {error}"),
                        )
                    },
                )
            });
            nearby_state.loading = Some(((code, generation), job));
        }
        return false;
    }
    // Cache failures too, avoiding repeated reads for unsupported models.
    let asset = model.and_then(|code| {
        nearby_state
            .models
            .get(&(code, generation))
            .cloned()
            .flatten()
    });
    let corpse = matches!(spawn.kind, SpawnKind::PlayerCorpse | SpawnKind::NpcCorpse);
    let position = Vec3::from_array(render_position(spawn.position));
    let entity = commands
        .spawn((
            RemoteEntity::new(id, None, spawn.size),
            Name::new(spawn.name.clone()),
            Transform::from_translation(position).with_rotation(Quat::from_rotation_y(
                eq_client_core::render_heading(spawn.position.heading),
            )),
            Visibility::Inherited,
        ))
        .id();
    if !races::drawn(spawn.race) {
        // An unseen marker, such as a spawn point: nothing to draw.
    } else if let Some(asset) = asset {
        // The model is drawn at the installed client's scale of its mesh as
        // authored, not stretched to its size. The server reports the
        // position its feet hang under; the model hangs by it in model
        // units, as it is scaled below its root, and `interpolate` stands its
        // root on the floor by it in world units.
        let drawn = Drawn::of(client, state.world(), (race, spawn.gender, spawn.size));
        let scale = drawn.scale.max(0.01);
        let feet = drawn.feet();
        character::spawn_prepared(commands, entity, &asset, feet / scale);
        commands.entity(entity).insert((
            RemoteEntity::new(id, Some(feet), drawn.size),
            Transform::from_translation(position)
                .with_rotation(Quat::from_rotation_y(eq_client_core::render_heading(
                    spawn.position.heading,
                )))
                .with_scale(Vec3::splat(scale)),
        ));
        if corpse {
            // Lying down, its name shows just over the body.
            commands.entity(entity).insert((
                character::Corpse {
                    fell: state.world().died_at(id),
                    length: asset.lying().map(|length| length * scale),
                    tilt: None,
                },
                super::names::Overhead(character::CORPSE_NAME_HEIGHT / scale),
            ));
        }
    } else {
        // A carrier, a race this client has no model name for, or a spawn
        // whose Human stand-in can't be had either keeps an explicit marker
        // (ours: the program has no such case).
        let height = if corpse {
            0.5
        } else {
            spawn.size.clamp(1.0, 6.0)
        };
        let color = match spawn.kind {
            SpawnKind::Player => Color::srgb(0.4, 0.65, 0.9),
            SpawnKind::Npc => Color::srgb(0.8, 0.65, 0.3),
            _ => Color::srgb(0.4, 0.4, 0.4),
        };
        commands
            .entity(entity)
            .insert(super::names::Overhead(height / 2.0));
        commands.entity(entity).with_children(|children| {
            let mut marker = children.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.6, height, 0.6))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color,
                    unlit: true,
                    ..default()
                })),
            ));
            if carrier {
                marker.insert((
                    crate::carriers::Placeholder {
                        spawn_id: id,
                        root: entity,
                        overhead: height / 2.0,
                    },
                    Visibility::Inherited,
                ));
            }
        });
    }
    nearby_state.rendered.insert(id, entity);
    if carrier {
        nearby_state.carrier_roots.insert(id);
    } else {
        nearby_state.carrier_roots.remove(&id);
    }
    nearby_state
        .revisions
        .insert(id, state.world().spawns()[&id].revision);
    true
}

/// Draws each spawn where the installed client's program carries it on from
/// its latest report ([`eq_client_core::movement::carry`]), sliding the
/// model onto that point over a tenth of a second after each report
/// ([`eq_client_core::Correction`]). A drawn corpse finds how it leans to lie along the floor
/// under it as it is first drawn and when a report moves it
/// ([`eq_client_core::corpse_tilt`]), for [`character::lie`] to lay it by.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn interpolate(
    (state, settings): (Res<OnlineState>, Res<crate::ViewerSettings>),
    time: Res<Time>,
    collision: Option<Res<super::Collision>>,
    mut entities: Query<(
        &mut RemoteEntity,
        &mut Transform,
        Option<&mut character::Corpse>,
    )>,
) {
    let world = collision
        .as_ref()
        .and_then(|collision| collision.0.as_ref());
    let weight = 1.0 - (-time.delta_secs().min(0.1) / 0.1).exp();
    let now = std::time::Instant::now();
    let dialect = dialect(settings.0.installed_client);
    // A spawn on the ground falls as the player does, in the program's units
    // a 50 ms step.
    let physics = crate::motion::physics(settings.0.installed_client, state.world().zone_gravity());
    let step = eq_client_assets::falls::STEP_SECONDS;
    // One that falls under the zone is put back on its top surface, by the
    // generation's rule and the zone header's heights.
    let underworld = settings
        .0
        .installed_client
        .falls()
        .zip(state.world().zone_heights())
        .map(|(falls, heights)| eq_client_core::UnderworldReturn {
            underworld: heights.underworld,
            highest: heights.top - falls.underworld_return.under_top,
            search_from: falls.underworld_return.search_from,
        });
    let fall = eq_client_core::CarriedFall {
        gravity: physics.gravity * step * step,
        kept: physics.air_factor,
        underworld,
    };
    let steps = time.delta_secs() / step;
    for (mut entity, mut transform, corpse) in &mut entities {
        let Some(spawn) = state.world().spawn(entity.id) else {
            continue;
        };
        if let Some(report) = state.world().carriers().latest(entity.id) {
            let pose = report.pose(now);
            transform.translation = Vec3::from_array(render_position(pose));
            transform.rotation =
                Quat::from_rotation_y(eq_client_core::render_heading(pose.heading));
            entity.seen = Some(spawn.reported);
            entity.soles = None;
            entity.carried = None;
            continue;
        }
        // A spawn moves on from its latest report, from when the report
        // came, however late it is drawn.
        let first = entity.seen.is_none();
        let fresh = entity.seen != Some(spawn.reported);
        if fresh {
            entity.seen = Some(spawn.reported);
            entity.soles = None;
            entity.carried = None;
        }
        // Each generation's client steps a spawn on from each report, frame
        // by frame, as its program does.
        // One drawn late is caught up as if it had been drawn all along.
        let carried = entity.carried.get_or_insert_with(|| {
            eq_client_core::entities::carried_since(
                dialect,
                &spawn.state,
                now.saturating_duration_since(spawn.reported),
            )
        });
        if !fresh {
            eq_client_core::movement::carry(dialect, carried, time.delta());
        }
        let mut target = Vec3::from_array(render_position(carried.position));
        let spawn = &spawn.state;
        let forward = spawn.forward;
        // A drawn model on the ground stands on the floor under it, whichever
        // rule put its z, and keeps to the floor as it is carried on, as
        // the generation's client does between reports; one whose client
        // keeps its carried height is drawn there.
        let on_floor = matches!(
            eq_client_core::movement::between_reports(dialect),
            eq_client_core::movement::ZBetween::OnFloor
        );
        if let (true, Some(feet), Some(world)) = (on_floor, entity.feet, world) {
            let soles = target - Vec3::Y * feet;
            let drawn =
                eq_client_core::carried_soles(world, entity.soles, soles, feet, fall, steps);
            entity.soles = Some(drawn);
            let height = drawn.height;
            target.y += height - soles.y;
            if let (true, Some(mut corpse)) = (fresh, corpse)
                && let Some(length) = corpse.length
            {
                corpse.tilt = Some(eq_client_core::corpse_tilt(
                    world,
                    Vec3::new(soles.x, height, soles.z),
                    eq_client_core::render_heading(spawn.position.heading),
                    length,
                    entity.size,
                ));
            }
        }
        if first {
            report_drawn(entity.id, (spawn, forward));
        } else if fresh {
            report_snap(entity.id, (spawn, forward), transform.translation, target);
        }
        if first || transform.translation.distance_squared(target) > 100.0 * 100.0 {
            entity.correction = eq_client_core::Correction::NONE;
            transform.translation = target;
        } else if fresh {
            entity.correction = eq_client_core::Correction::new(transform.translation, target);
            transform.translation = entity.correction.advance(target, 0.0);
        } else {
            transform.translation = entity.correction.advance(target, time.delta_secs());
        }
        transform.rotation = transform.rotation.slerp(
            Quat::from_rotation_y(eq_client_core::render_heading(spawn.position.heading)),
            weight,
        );
    }
}

/// A debug line for each new report of a drawn spawn: where the server put
/// it, how it moves (its velocity, and the forward input the Mac-era client
/// carries it by), and how far it was drawn from where the report puts it
/// (the snap) as the report came. Off unless the log filter asks for this
/// module at debug level; it changes nothing drawn.
fn report_snap(
    id: u16,
    (spawn, forward): (&eq_client_core::SpawnState, f32),
    drawn: Vec3,
    target: Vec3,
) {
    let p = spawn.position;
    let [vx, vy, vz] = spawn.velocity;
    debug!(
        "spawn {id} report ({:.1}, {:.1}, {:.1}) heading {:.1} velocity ({vx:.1}, {vy:.1}, {vz:.1}) \
         forward {forward:.2} snap {:.2} (horizontal {:.2}, vertical {:+.2})",
        p.x,
        p.y,
        p.z,
        p.heading,
        drawn.distance(target),
        Vec2::new(target.x - drawn.x, target.z - drawn.z).length(),
        target.y - drawn.y,
    );
}

/// A debug line as a spawn is first drawn, a corpse after its death
/// included: its kind, where it is reported, how it moves on from there,
/// and the race, gender and size the server sent (0 for none), which set
/// how large it is drawn. Off unless the log filter asks for this module at
/// debug level.
fn report_drawn(id: u16, (spawn, forward): (&eq_client_core::SpawnState, f32)) {
    let p = spawn.position;
    let [vx, vy, vz] = spawn.velocity;
    debug!(
        "spawn {id} drawn as {:?} at report ({:.1}, {:.1}, {:.1}) velocity ({vx:.1}, {vy:.1}, {vz:.1}) \
         forward {forward:.2} race {} gender {} size sent {:.2}",
        spawn.kind, p.x, p.y, p.z, spawn.race, spawn.gender, spawn.size,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;

    #[test]
    fn a_frozen_remote_spawn_still_plays_its_authoritative_death_pose() {
        use bevy::{ecs::system::RunSystemOnce, mesh::skinning::SkinnedMesh};
        use eq_client_core::{Death, PostureState, WorldEvent};
        use std::time::{Duration, Instant};

        let mut world = World::new();
        character::testing::stores(&mut world);
        world.init_resource::<Time>();
        world.init_resource::<crate::spellbook::SpellNames>();
        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(50));
        let mut online = crate::online::OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(9));
        testing::spawns(&mut online, vec![testing::pet(5, 9)]);
        testing::news(
            &mut online,
            [WorldEvent::Posture {
                spawn_id: 5,
                posture: PostureState::Frozen,
            }],
        );
        world.insert_resource(online);
        let prepared = character::testing::prepared(&mut world);
        let root = character::testing::drawn(&mut world, &prepared);
        world
            .entity_mut(root)
            .insert(RemoteEntity::new(5, None, 6.0));
        let joints = world
            .query::<&SkinnedMesh>()
            .single(&world)
            .unwrap()
            .joints
            .clone();
        let pose = |world: &World| {
            joints
                .iter()
                .map(|joint| world.get::<Transform>(*joint).unwrap().to_matrix())
                .collect::<Vec<_>>()
        };
        let before = pose(&world);
        world.run_system_once(character::animate).unwrap();
        assert_eq!(pose(&world), before, "living frozen actors keep their pose");

        let fell = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        let mut online = world.resource_mut::<crate::online::OnlineState>();
        testing::news_at(
            &mut online,
            [WorldEvent::Death(Death {
                spawn_id: 5,
                killer_id: 9,
                corpse_id: 5,
                bind_zone_id: 0,
                corpse_name: None,
            })],
            fell,
        );
        assert_eq!(online.world().posture(5), Some(PostureState::Frozen));
        assert_eq!(
            online.world().spawn(5).unwrap().state.kind,
            SpawnKind::NpcCorpse
        );
        // Reconciliation gives the corpse the retained death time when it
        // draws its model again. Its prior posture must not block that pose.
        world.entity_mut(root).insert(character::Corpse {
            fell: Some(fell),
            ..default()
        });
        world.run_system_once(character::animate).unwrap();
        let fallen = pose(&world);
        let expected =
            eq_client_assets::characters::CharacterAsset::testing().joints("D05", 2.0, false);
        assert_ne!(fallen, before);
        assert_eq!(fallen, expected);
        // Repeated frames hold the corpse's final pose rather than restarting.
        world.run_system_once(character::animate).unwrap();
        assert_eq!(pose(&world), expected);
    }

    #[test]
    fn spawns_load_as_far_as_the_view_could_show_one() {
        let projection = Projection::Perspective(PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_2,
            aspect_ratio: 1.0,
            far: 2400.0,
            ..default()
        });
        let player = Vec3::new(10.0, 0.0, 0.0);
        let camera = Transform::from_xyz(10.0, 30.0, 200.0).looking_at(player, Vec3::Y);
        let reach = loading_reach(2400.0, Some((&camera, &projection)), player);
        // The far plane's corner, the farthest the view shows anything, is
        // loaded, though it lies farther than the clip from the camera.
        let corner = camera.translation
            + 2400.0 * (*camera.forward() + *camera.right() + *camera.up())
            - *camera.forward();
        assert!(!crate::camera::past_far(&projection, &camera, corner));
        assert!(corner.distance(camera.translation) > 2400.0 * 1.7);
        assert!(corner.distance(player) <= reach, "{reach}");
        // Without a perspective view yet, to the clip.
        assert_eq!(
            loading_reach(2400.0, None, player).to_bits(),
            2400.0_f32.to_bits()
        );
    }

    #[test]
    fn a_model_is_read_in_the_look_asked_for_and_falls_back_to_classic() {
        let asset = |luclin| {
            let mut asset = CharacterAsset::testing();
            asset.set_luclin(luclin);
            Ok::<_, &str>(asset)
        };
        // Classic never tries the Luclin archive.
        let loaded = load_model(
            Generation::Classic,
            || panic!("not asked for"),
            || asset(false),
        );
        assert!(!loaded.asset.unwrap().luclin() && loaded.luclin_missing.is_none());
        let loaded = load_model(Generation::Luclin, || asset(true), || panic!("not needed"));
        assert!(loaded.asset.unwrap().luclin() && loaded.luclin_missing.is_none());
        // A missing Luclin archive draws the classic model, and says why.
        let loaded = load_model(
            Generation::Luclin,
            || Err("no globalhum_chr"),
            || asset(false),
        );
        assert!(!loaded.asset.unwrap().luclin());
        assert_eq!(loaded.luclin_missing.as_deref(), Some("no globalhum_chr"));
        let loaded = load_model(Generation::Luclin, || Err("none"), || Err("none"));
        assert!(loaded.asset.is_none() && loaded.luclin_missing.is_some());
    }

    #[test]
    fn a_luclin_player_stands_at_the_size_and_feet_a_classic_one_does() {
        let directory = std::env::temp_dir().join(format!("luclin-draw-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&directory);
        let drawn = |luclin: bool| {
            let mut online = OnlineState::new(true);
            testing::admit(&mut online, 1, testing::player(7));
            let mut human = testing::pet(5, 0);
            (human.kind, human.race, human.gender, human.size) = (SpawnKind::Player, 1, 0, 7.0);
            human.pet_owner = None;
            testing::spawns(&mut online, vec![human]);
            let mut app = App::new();
            character::testing::stores(app.world_mut());
            let classic = character::testing::prepared(app.world_mut());
            // A Luclin model of another height, which must not move it.
            let mut taller = CharacterAsset::testing();
            for point in &mut taller.primitives[0].positions {
                point[1] *= 3.0;
            }
            taller.set_luclin(true);
            let taller = character::testing::prepared_from(app.world_mut(), taller);
            let mut nearby = NearbyEntities::default();
            nearby
                .models
                .insert(("HUM", Generation::Classic), Some(classic));
            nearby
                .models
                .insert(("HUM", Generation::Luclin), Some(taller));
            let mut chosen = LuclinModels::NONE;
            chosen.set(1, 0, Some(luclin));
            // Human also takes Erudite's key (0x43ac64), and an unset key is on.
            chosen.set(3, 0, Some(luclin));
            nearby.luclin = Some(chosen);
            nearby.elapsed = 1.0;
            let config = crate::ViewerConfig {
                eq_directory: Some(directory.clone()),
                ..crate::ViewerConfig::default()
            };
            app.insert_resource(online)
                .insert_resource(crate::ViewerSettings(config))
                .init_resource::<crate::options::OptionsState>()
                .init_resource::<Time>()
                .insert_resource(nearby)
                .add_systems(Update, reconcile);
            app.update();
            let root = app.world().resource::<NearbyEntities>().rendered[&5];
            let character = app
                .world()
                .get::<character::AnimatedCharacter>(root)
                .unwrap();
            let remote = app.world().get::<RemoteEntity>(root).unwrap();
            let scale = app.world().get::<Transform>(root).unwrap().scale;
            (
                character.asset.luclin(),
                (remote.feet.map(f32::to_bits), remote.size.to_bits()),
                scale.to_array().map(f32::to_bits),
            )
        };
        let (classic, luclin) = (drawn(false), drawn(true));
        let _ = std::fs::remove_dir(&directory);
        assert!(!classic.0 && luclin.0);
        assert_eq!((classic.1, classic.2), (luclin.1, luclin.2));
    }

    #[test]
    fn with_no_installation_no_spawn_is_still_to_come() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .init_resource::<crate::options::OptionsState>()
            .init_resource::<Time>()
            .init_resource::<NearbyEntities>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
            .add_systems(Update, reconcile);
        app.update();
        // So the loading screen lifts as soon as the zone has laid out.
        assert!(app.world().resource::<NearbyEntities>().settled());
    }

    #[test]
    fn a_corpse_on_a_slope_lies_along_it() {
        use eq_client_core::WorldPosition;
        // A floor rising 0.3 a unit toward the renderer's +X, the way
        // heading 0 faces.
        let grade = 0.3_f32;
        let corner = |x: f32, z: f32| [x, grade * x, z];
        let floor = eq_client_core::movement::CollisionWorld::new([
            [
                corner(-50.0, -50.0),
                corner(50.0, -50.0),
                corner(50.0, 50.0),
            ],
            [
                corner(-50.0, -50.0),
                corner(50.0, 50.0),
                corner(-50.0, 50.0),
            ],
        ])
        .unwrap();
        let mut corpse = testing::pet(5, 0);
        corpse.pet_owner = None;
        corpse.size = 6.0;
        let feet = eq_client_core::drawn_feet_offset(corpse.size * 0.2);
        corpse.position = WorldPosition {
            z: feet,
            ..WorldPosition::default()
        };
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::spawns(&mut online, vec![corpse]);
        let mut app = App::new();
        character::testing::stores(app.world_mut());
        app.insert_resource(online)
            .insert_resource(super::super::Collision(Some(floor)))
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .init_resource::<crate::options::OptionsState>()
            .init_resource::<Time>()
            .add_systems(Update, (interpolate, character::lie).chain());
        let prepared = character::testing::prepared(app.world_mut());
        let root = character::testing::drawn(app.world_mut(), &prepared);
        app.world_mut().entity_mut(root).insert((
            RemoteEntity::new(5, Some(feet), 6.0),
            character::Corpse {
                fell: None,
                length: prepared.lying(),
                tilt: None,
            },
        ));
        app.update();
        // As first drawn it finds its tilt: the slope ahead, nothing across.
        let tilt = app
            .world()
            .get::<character::Corpse>(root)
            .unwrap()
            .tilt
            .unwrap();
        assert!(
            (-tilt.pitch - grade.atan()).abs() < 1.0_f32.to_radians(),
            "{tilt:?}"
        );
        assert!(tilt.roll.abs() < 1e-4, "{tilt:?}");
        // Its model's facing, turned by its heading and tilt, runs up the
        // slope as the floor does.
        let model = app
            .world()
            .get::<character::AnimatedCharacter>(root)
            .unwrap()
            .model;
        let root_turn = app.world().get::<Transform>(root).unwrap().rotation;
        let hung = app.world().get::<Transform>(model).unwrap().rotation;
        let facing =
            root_turn * hung * Quat::from_rotation_y(-eq_client_core::model_yaw()) * Vec3::Z;
        let rise = facing.y.atan2(Vec2::new(facing.x, facing.z).length());
        assert!(
            (rise - grade.atan()).abs() < 1.0_f32.to_radians(),
            "{facing}"
        );
        // Without a new report it is not found again.
        app.world_mut()
            .get_mut::<character::Corpse>(root)
            .unwrap()
            .tilt = Some(eq_client_core::Tilt::default());
        app.update();
        assert_eq!(
            app.world().get::<character::Corpse>(root).unwrap().tilt,
            Some(eq_client_core::Tilt::default())
        );
    }

    #[test]
    fn a_spawn_drawn_late_is_carried_and_its_corpse_lies_still() {
        use eq_client_core::{Death, WorldEvent, WorldPosition};
        use std::time::{Duration, Instant};
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let mut walker = testing::pet(5, 0);
        walker.pet_owner = None;
        testing::spawns(&mut online, vec![walker]);
        let reported = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        testing::news_at(
            &mut online,
            [WorldEvent::Position {
                spawn_id: 5,
                position: WorldPosition {
                    heading: 128.0,
                    ..WorldPosition::default()
                },
                velocity: [4.0, 0.0, 0.0],
                forward: 0.5,
            }],
            reported,
        );
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .init_resource::<Time>()
            .add_systems(Update, interpolate);
        let drawn = |app: &mut App| {
            let entity = app
                .world_mut()
                .spawn((RemoteEntity::new(5, None, 6.0), Transform::default()))
                .id();
            app.update();
            let at = app.world().get::<Transform>(entity).unwrap().translation;
            (entity, eq_client_core::world_position(at.to_array(), 0.0))
        };
        // Drawn two seconds after its report, it is drawn two seconds along
        // at the program's pace, 11.64 a second for an input of 0.5, not
        // back where the report put it.
        let (walking, at) = drawn(&mut app);
        assert!(at.x > 22.0 && at.x < 24.5, "{at:?}");
        app.world_mut().despawn(walking);
        // It dies; the corpse is drawn anew where the walker had got to,
        // and lies there.
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Death(Death {
                spawn_id: 5,
                killer_id: 7,
                corpse_id: 5,
                bind_zone_id: 0,
                corpse_name: None,
            })],
        );
        let (corpse, fell) = drawn(&mut app);
        assert!(fell.x > 22.0 && fell.x < 24.5, "{fell:?}");
        std::thread::sleep(Duration::from_millis(50));
        app.update();
        let lies = app.world().get::<Transform>(corpse).unwrap().translation;
        let lies = eq_client_core::world_position(lies.to_array(), 0.0);
        assert!((lies.x - fell.x).abs() < 1e-4, "{fell:?} then {lies:?}");
    }
}
