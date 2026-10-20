//! Spell particles, drawn from the installed client's own effect files: a
//! spell's caster stage runs at the caster while its cast is under way, and
//! its target stage on the target as the spell lands (both inferred, items
//! 6.224 and 6.226). Each running emitter is one mesh of camera-facing
//! quads, rebuilt every frame from particles kept on the CPU. Nothing here
//! sends anything.
use bevy::{asset::RenderAssetUsages, camera::visibility::NoFrustumCulling, prelude::*};
use eq_client_assets::spells::effects::{Emitter, Placement, SpellEffects};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use super::background::Background;

/// How long a target stage keeps starting particles after its landing
/// (inferred, item 6.224).
const TARGET_SECONDS: f32 = 1.5;
/// The most particles one emitter keeps alive; past it new ones are skipped.
const EMITTER_CAP: usize = 256;
/// The most particles kept alive in all; past it new ones are skipped.
const TOTAL_CAP: usize = 4_096;
/// Where the middle of a body sits, as a share of its height (inferred).
const CHEST: f32 = 0.55;
/// How much of a particle's start velocity is a random direction, so a
/// spray spreads rather than moving as one (inferred).
const SCATTER: f32 = 0.5;
/// The share of its life over which a particle fades out (inferred).
const FADE: f32 = 0.3;

/// The installation's effect files and the images read for them.
#[derive(Resource, Default)]
pub(super) struct SpellParticles {
    /// The installation, once the files are asked for.
    directory: Option<PathBuf>,
    /// The files' read, under way.
    read: Background<Option<SpellEffects>>,
    /// The files, once read; None while reading or when this client has
    /// none checked or they could not be read.
    effects: Option<Arc<SpellEffects>>,
    /// Each image by its file name, kept for the installation's life: there
    /// are a few hundred at most, and a failed read is kept as failed.
    images: HashMap<String, Picture>,
    /// The particles alive in all at the last frame.
    alive: usize,
}

/// An emitter image.
enum Picture {
    /// Being read off the frame thread.
    Reading(Background<Option<image::RgbaImage>>),
    /// Drawn with this material.
    Ready(Handle<StandardMaterial>),
    /// Unreadable; logged once.
    Failed,
}

/// What a drawn character is running.
#[derive(Component, Default)]
pub(super) struct Running {
    /// The spell whose caster stage runs, if a cast is under way.
    casting: Option<u16>,
    /// The landing whose target stage was last started, so an old one is
    /// not replayed.
    landing: Option<u64>,
    /// Its emitters, each drawn by its own entity.
    sprays: Vec<Spray>,
}

/// One running emitter at one placement.
struct Spray {
    emitter: Emitter,
    placement: Placement,
    /// Whether it belongs to the caster stage, which stops with the cast.
    caster: bool,
    /// Until when it starts particles; None while its cast runs.
    until: Option<f32>,
    particles: Particles,
    /// The entity drawing it, once its image is ready and it has a particle.
    drawn: Option<Drawn>,
}

/// A spray's entity and mesh.
struct Drawn {
    entity: Entity,
    mesh: Handle<Mesh>,
    /// Whether it shows. A spray with no particles is hidden rather than
    /// emptied: Bevy 0.19's mesh allocator skips a mesh with no vertex data
    /// (`bevy_render` `mesh/allocator.rs:374`) but still copies it
    /// (`allocator.rs:423`), logging a use-after-free error each frame.
    shown: bool,
}

/// Loads the installation's effect files off the frame thread, once.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn load(settings: Res<super::ViewerSettings>, mut particles: ResMut<SpellParticles>) {
    let config = &settings.0;
    if particles.directory.is_none() {
        let Some(directory) = config.eq_directory.clone() else {
            return;
        };
        let client = config.installed_client;
        let path = directory.clone();
        particles
            .read
            .start(0, move || match client.spell_effects(&path) {
                Some(Ok(effects)) => Some(effects),
                Some(Err(error)) => {
                    warn!("spell particles are off: {error}");
                    None
                }
                None => None,
            });
        particles.directory = Some(directory);
    }
    if let Some(effects) = particles.read.poll(Some(0)) {
        particles.effects = effects.map(Arc::new);
    }
}

/// The parts of a drawn character the particles need.
type Bodies<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static super::character::AnimatedCharacter,
        Option<&'static super::entities::RemoteEntity>,
        Has<super::Player>,
        Option<&'static mut Running>,
    ),
>;

/// The meshes, materials and images the particles draw with.
type Drawing<'w> = (
    ResMut<'w, Assets<Mesh>>,
    ResMut<'w, Assets<StandardMaterial>>,
    ResMut<'w, Assets<Image>>,
);

/// Starts, stops, steps and draws the particles of every drawn character.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    time: Res<Time>,
    (online, spells): (
        Res<super::online::OnlineState>,
        Res<super::spellbook::SpellNames>,
    ),
    mut particles: ResMut<SpellParticles>,
    mut sounds: ResMut<super::sounds::Sounds>,
    mut bodies: Bodies,
    places: Query<&GlobalTransform>,
    camera: Query<&GlobalTransform, With<super::OrbitCamera>>,
    (mut meshes, mut materials, mut images): Drawing,
) {
    let Some(effects) = particles.effects.clone() else {
        return;
    };
    let world = online.world();
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    let facing = camera.iter().next().map_or((Vec3::X, Vec3::Y), |camera| {
        (camera.right().into(), camera.up().into())
    });
    let mut alive = 0;
    let mut room = TOTAL_CAP.saturating_sub(particles.alive);
    for (entity, character, remote, own, running) in &mut bodies {
        let id = remote.map(|entity| entity.id).or_else(|| {
            own.then(|| world.player().map(|player| player.spawn_id))
                .flatten()
        });
        let Some(id) = id else { continue };
        let Some(mut running) = running else {
            // A landing from before the character was drawn is not replayed.
            commands.entity(entity).insert(Running {
                landing: world.landing_on(id).map(|landing| landing.count),
                ..Running::default()
            });
            continue;
        };
        let level = if own {
            world.player().map_or(0, |player| u16::from(player.level))
        } else {
            world
                .spawn(id)
                .map_or(0, |spawn| u16::from(spawn.state.level))
        };
        let heard = running.follow(world, (id, level), (&effects, &spells), now);
        let Ok(model) = places.get(character.model) else {
            continue;
        };
        let body = Body::of(character, model);
        // The player's own are heard on the listener, others' where they
        // stand.
        let place = (!own).then(|| body.at(Placement::Chest));
        for number in heard {
            sounds.play_number(number, place);
        }
        for spray in &mut running.sprays {
            let starting = spray.until.is_none_or(|until| now < until);
            let at = body.at(spray.placement);
            spray
                .particles
                .step(&spray.emitter, at, dt, starting, &mut room);
            alive += spray.particles.len();
            draw(
                &mut commands,
                spray,
                (&mut particles, &mut materials, &mut images),
                &mut meshes,
                (facing, &character.layers),
            );
        }
        running.sprays.retain(|spray| {
            let done = spray.until.is_some_and(|until| now >= until) && spray.particles.len() == 0;
            if done && let Some(drawn) = &spray.drawn {
                commands.entity(drawn.entity).despawn();
            }
            !done
        });
    }
    particles.alive = alive;
}

impl Running {
    /// Starts and stops the stages: the caster stage follows the cast under
    /// way, and each new landing starts the target stage. Returns the sounds
    /// those starts play, by number: the caster stage's as a cast starts and
    /// the target stage's as a spell lands, each once (inferred, item 6.272).
    fn follow(
        &mut self,
        world: &eq_client_core::world::ClientWorld,
        (id, level): (u16, u16),
        (effects, spells): (&SpellEffects, &super::spellbook::SpellNames),
        now: f32,
    ) -> Vec<u32> {
        let mut heard = Vec::new();
        let effect_of = |spell: u16| {
            spells
                .effect(u32::from(spell))
                .and_then(|number| effects.effect(number))
        };
        let casting = world.casting_of(id);
        if casting != self.casting {
            for spray in self.sprays.iter_mut().filter(|spray| spray.caster) {
                spray.until.get_or_insert(now);
            }
            if let Some(effect) = casting.and_then(effect_of) {
                let started = start(effects, effect.caster_slots(level), true, None);
                self.sprays.extend(started);
                heard.extend(effect.caster_sound);
            }
            self.casting = casting;
        }
        let landing = world.landing_on(id);
        let count = landing.map(|landing| landing.count);
        if count != self.landing {
            if let Some(landing) = landing
                && let Some(effect) = effect_of(landing.spell_id)
            {
                let slots = effect.target_slots(landing.caster_level);
                let started = start(effects, slots, false, Some(now + TARGET_SECONDS));
                self.sprays.extend(started);
                heard.extend(effect.target_sound);
            }
            self.landing = count;
        }
        heard
    }
}

/// Where the placements are on a drawn body this frame.
struct Body {
    feet: Vec3,
    head: Vec3,
    /// The left and right hands, at the middle of the body on a skeleton
    /// without them.
    hands: [Vec3; 2],
}

impl Body {
    fn of(character: &super::character::AnimatedCharacter, model: &GlobalTransform) -> Self {
        use eq_client_assets::characters::Attachment;
        let (_, top) = character.asset.span().unwrap_or_default();
        let feet = model.transform_point(Vec3::new(0.0, character.soles, 0.0));
        let head = model.transform_point(Vec3::new(0.0, top, 0.0));
        let hand = |attachment: Attachment| {
            character.attachments[attachment.index()].map_or(feet.lerp(head, CHEST), |point| {
                model.transform_point(point.w_axis.truncate())
            })
        };
        Self {
            feet,
            head,
            hands: [hand(Attachment::LeftHand), hand(Attachment::RightHand)],
        }
    }

    fn at(&self, placement: Placement) -> Vec3 {
        match placement {
            Placement::LeftHand => self.hands[0],
            Placement::RightHand => self.hands[1],
            Placement::Head => self.head,
            Placement::Chest => self.feet.lerp(self.head, CHEST),
            Placement::Feet => self.feet,
        }
    }
}

/// The emitters a stage's slots start.
fn start<'a>(
    effects: &'a SpellEffects,
    slots: impl Iterator<Item = &'a eq_client_assets::spells::effects::Slot> + 'a,
    caster: bool,
    until: Option<f32>,
) -> impl Iterator<Item = Spray> + 'a {
    slots.filter_map(move |slot| {
        Some(Spray {
            emitter: effects.emitter(slot.emitter)?.clone(),
            placement: slot.placement,
            caster,
            until,
            particles: Particles::new(slot.emitter),
            drawn: None,
        })
    })
}

/// Rebuilds a spray's mesh, once its image is ready.
fn draw(
    commands: &mut Commands,
    spray: &mut Spray,
    (particles, materials, images): (
        &mut SpellParticles,
        &mut Assets<StandardMaterial>,
        &mut Assets<Image>,
    ),
    meshes: &mut Assets<Mesh>,
    (facing, layers): ((Vec3, Vec3), &bevy::camera::visibility::RenderLayers),
) {
    let quads = spray.particles.quads(&spray.emitter, facing);
    let Some(drawn) = &mut spray.drawn else {
        if let Some(material) = particles.material(&spray.emitter.image, materials, images)
            && let Some(mesh) = spray_mesh(quads)
        {
            let mesh = meshes.add(mesh);
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::IDENTITY,
                    // The mesh moves every frame, so its first bounds say nothing.
                    NoFrustumCulling,
                    layers.clone(),
                ))
                .id();
            spray.drawn = Some(Drawn {
                entity,
                mesh,
                shown: true,
            });
        }
        return;
    };
    let frame = Frame::of(drawn.shown, !quads.indices.is_empty());
    if frame.rewrite
        && let Some(mut mesh) = meshes.get_mut(&drawn.mesh)
    {
        rewrite(&mut mesh, quads);
    }
    if let Some(visibility) = frame.visibility {
        drawn.shown = visibility != Visibility::Hidden;
        commands.entity(drawn.entity).insert(visibility);
    }
}

/// What a drawn spray does this frame.
#[derive(Debug, PartialEq)]
struct Frame {
    /// Whether its mesh takes this frame's particles.
    rewrite: bool,
    /// Its new visibility, when that changes.
    visibility: Option<Visibility>,
}

impl Frame {
    /// With particles it draws them, showing again if hidden; with none it
    /// keeps its last mesh and hides, so no empty mesh is ever extracted.
    fn of(shown: bool, particles: bool) -> Self {
        Self {
            rewrite: particles,
            visibility: match (shown, particles) {
                (false, true) => Some(Visibility::Inherited),
                (true, false) => Some(Visibility::Hidden),
                _ => None,
            },
        }
    }
}

/// A spray's mesh, None while it has no particles. It is rewritten every
/// frame, so it keeps its data in the main world too: a render-world-only
/// mesh gives its data up at extraction and the next rewrite would panic.
fn spray_mesh(quads: Quads) -> Option<Mesh> {
    if quads.indices.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    rewrite(&mut mesh, quads);
    Some(mesh)
}

fn rewrite(mesh: &mut Mesh, quads: Quads) {
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, quads.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, quads.uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, quads.colors);
    mesh.insert_indices(bevy::mesh::Indices::U32(quads.indices));
}

impl SpellParticles {
    /// An image's material, starting its read the first time it is asked
    /// for; None until it is ready or when it cannot be read.
    fn material(
        &mut self,
        name: &str,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<StandardMaterial>> {
        let directory = self.directory.clone()?;
        let picture = self.images.entry(name.to_owned()).or_insert_with(|| {
            let mut read = Background::default();
            let file = name.to_owned();
            read.start(0, move || {
                eq_client_assets::spells::effects::effect_image(&directory, &file)
                    .map_err(|error| warn!("spell particle image unread: {error}"))
                    .ok()
            });
            Picture::Reading(read)
        });
        if let Picture::Reading(read) = picture {
            match read.poll(Some(0)) {
                Some(Some(pixels)) => {
                    let texture = images.add(texture(pixels));
                    *picture = Picture::Ready(materials.add(StandardMaterial {
                        base_color_texture: Some(texture),
                        alpha_mode: AlphaMode::Add,
                        unlit: true,
                        double_sided: true,
                        cull_mode: None,
                        ..default()
                    }));
                }
                Some(None) => *picture = Picture::Failed,
                None => {}
            }
        }
        match picture {
            Picture::Ready(material) => Some(material.clone()),
            Picture::Reading(_) | Picture::Failed => None,
        }
    }
}

fn texture(pixels: image::RgbaImage) -> Image {
    let (width, height) = pixels.dimensions();
    Image::new(
        bevy::render::render_resource::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        pixels.into_raw(),
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// One particle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Particle {
    position: Vec3,
    velocity: Vec3,
    age: f32,
}

/// An emitter's live particles and what it owes in new ones.
#[derive(Debug)]
struct Particles {
    live: Vec<Particle>,
    /// The share of a particle owed from earlier frames.
    owed: f32,
    /// A small random sequence, seeded per emitter.
    seed: u32,
}

/// One frame's mesh data.
struct Quads {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Particles {
    fn new(seed: u32) -> Self {
        Self {
            live: Vec::new(),
            owed: 0.0,
            seed: seed.wrapping_mul(2_654_435_761) | 1,
        }
    }

    fn len(&self) -> usize {
        self.live.len()
    }

    /// The next random number in [-1, 1].
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        #[allow(clippy::cast_precision_loss)] // A random share needs no precision.
        let share = (self.seed >> 8) as f32 / (1u32 << 24) as f32;
        share.mul_add(2.0, -1.0)
    }

    fn random_direction(&mut self) -> Vec3 {
        Vec3::new(self.random(), self.random(), self.random()).normalize_or_zero()
    }

    /// Ages and moves the live particles, drops the spent ones and, while
    /// `starting`, starts new ones at `at` as long as `room` lasts. Returns
    /// how many started.
    fn step(
        &mut self,
        emitter: &Emitter,
        at: Vec3,
        dt: f32,
        starting: bool,
        room: &mut usize,
    ) -> usize {
        let acceleration = Vec3::from_array(eq_client_core::render_vector(emitter.acceleration));
        for particle in &mut self.live {
            particle.age += dt;
            particle.velocity += acceleration * dt;
            particle.position += particle.velocity * dt;
        }
        self.live.retain(|particle| particle.age < emitter.life);
        if !starting {
            self.owed = 0.0;
            return 0;
        }
        self.owed += emitter.rate * dt;
        let mut started = 0;
        let velocity = Vec3::from_array(eq_client_core::render_vector(emitter.velocity));
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            if self.live.len() >= EMITTER_CAP || *room == 0 {
                continue;
            }
            *room -= 1;
            let offset = self.random_direction() * emitter.spread * self.random().abs();
            let scatter = self.random_direction() * velocity.length() * SCATTER;
            self.live.push(Particle {
                position: at + offset,
                velocity: velocity + scatter,
                age: 0.0,
            });
            started += 1;
        }
        started
    }

    /// The live particles as camera-facing quads.
    fn quads(&self, emitter: &Emitter, (right, up): (Vec3, Vec3)) -> Quads {
        let count = self.live.len();
        let mut quads = Quads {
            positions: Vec::with_capacity(count * 4),
            uvs: Vec::with_capacity(count * 4),
            colors: Vec::with_capacity(count * 4),
            indices: Vec::with_capacity(count * 6),
        };
        let half = emitter.size * 0.5;
        let [start, end] = emitter
            .colors
            .map(|color| Vec3::from_array(color.map(|channel| f32::from(channel) / 255.0)));
        let side = match emitter.cells {
            16 => 4,
            4 => 2,
            _ => 1,
        };
        #[allow(clippy::cast_precision_loss)] // At most 4 cells a side.
        let cell = 1.0 / side as f32;
        for particle in &self.live {
            let life = (particle.age / emitter.life.max(f32::EPSILON)).clamp(0.0, 1.0);
            let fade = ((1.0 - life) / FADE).min(1.0);
            let color = start.lerp(end, life) * fade;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )] // A cell number below 16.
            let frame = ((life * (side * side) as f32) as u32).min(side * side - 1);
            #[allow(clippy::cast_precision_loss)] // A cell number below 16.
            let (u, v) = ((frame % side) as f32 * cell, (frame / side) as f32 * cell);
            #[allow(clippy::cast_possible_truncation)] // Capped well below u32.
            let first = quads.positions.len() as u32;
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = particle.position + (right * x + up * y) * half;
                quads.positions.push(corner.to_array());
                quads.uvs.push([
                    u + if x > 0.0 { cell } else { 0.0 },
                    v + if y > 0.0 { 0.0 } else { cell },
                ]);
                quads.colors.push([color.x, color.y, color.z, fade]);
            }
            quads
                .indices
                .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
        }
        quads
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitter(rate: f32, life: f32) -> Emitter {
        Emitter {
            name: String::from("Test"),
            image: String::from("test.tga"),
            rate,
            life,
            spread: 1.0,
            velocity: [0.0, 0.0, 2.0],
            acceleration: [0.0, 0.0, -1.0],
            size: 1.0,
            colors: [[255, 255, 255], [0, 0, 255]],
            cells: 16,
        }
    }

    /// Effect 1 with a caster sound of 104 and a target sound of 137, its
    /// slots empty; spell 42 names it.
    fn sounding() -> (SpellEffects, super::super::spellbook::SpellNames) {
        let mut effects = vec![0; 268 * 2];
        effects[268 + 64..268 + 68].copy_from_slice(&104_u32.to_le_bytes());
        effects[268 + 200..268 + 204].copy_from_slice(&137_u32.to_le_bytes());
        let effects = SpellEffects::parse(&effects, b"EDD\x00110\x00").expect("parsed");
        let mut fields = vec!["0"; 160];
        fields[0] = "42";
        fields[1] = "Synthetic spell";
        fields[145] = "1";
        (
            effects,
            super::super::spellbook::SpellNames::parse(&fields.join("^")),
        )
    }

    #[test]
    fn a_new_cast_and_a_new_landing_each_sound_once() {
        use crate::online::{OnlineState, testing};
        use eq_client_core::{SpellUpdate, WorldEvent};
        let (effects, spells) = sounding();
        let mut online = OnlineState::new(true);
        // Admitted where the server flags a landing with 4, as Titanium's do.
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 1,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: Some(4),
            }],
        );
        let mut running = Running::default();
        let mut follow =
            |online: &OnlineState| running.follow(online.world(), (7, 1), (&effects, &spells), 0.0);
        assert_eq!(follow(&online), Vec::<u32>::new());
        testing::spell(
            &mut online,
            SpellUpdate::Began {
                caster_id: 7,
                spell_id: 42,
                duration_ms: 3000,
            },
        );
        assert_eq!(follow(&online), [104]);
        // The same cast still under way is not heard again.
        assert_eq!(follow(&online), Vec::<u32>::new());
        let landing = eq_client_core::SpellEffect {
            target_id: 7,
            caster_id: 7,
            caster_level: 1,
            instrument_modifier: 10,
            spell_id: 42,
            spell_level: 1,
            effect_flag: 4,
            push: None,
        };
        testing::news(&mut online, [WorldEvent::SpellEffect(landing.clone())]);
        assert_eq!(follow(&online), [137]);
        assert_eq!(follow(&online), Vec::<u32>::new());
        // Each landing is new, even of the same spell.
        testing::news(&mut online, [WorldEvent::SpellEffect(landing)]);
        assert_eq!(follow(&online), [137]);
    }

    #[test]
    fn an_emitter_starts_its_rate_and_its_particles_live_their_life() {
        let emitter = emitter(10.0, 1.0);
        let mut particles = Particles::new(1);
        let mut room = TOTAL_CAP;
        let mut started = 0;
        for _ in 0..10 {
            started += particles.step(&emitter, Vec3::ZERO, 0.05, true, &mut room);
        }
        assert_eq!(started, 5);
        assert_eq!(particles.len(), 5);
        for _ in 0..30 {
            particles.step(&emitter, Vec3::ZERO, 0.05, false, &mut room);
        }
        assert_eq!(particles.len(), 0);
    }

    #[test]
    fn particles_rise_and_fall_by_the_emitters_motion() {
        let emitter = emitter(100.0, 5.0);
        let mut particles = Particles::new(2);
        let mut room = TOTAL_CAP;
        particles.step(&emitter, Vec3::ZERO, 0.02, true, &mut room);
        let first = particles.live[0];
        particles.step(&emitter, Vec3::ZERO, 0.1, false, &mut room);
        // Up in the world is the renderer's Y, so they rise then fall back.
        assert!(particles.live[0].position.y > first.position.y);
        assert!(particles.live[0].velocity.y < first.velocity.y);
    }

    #[test]
    fn the_caps_hold() {
        let emitter = emitter(1_000.0, 30.0);
        let mut particles = Particles::new(3);
        let mut room = TOTAL_CAP;
        particles.step(&emitter, Vec3::ZERO, 1.0, true, &mut room);
        assert_eq!(particles.len(), EMITTER_CAP);
        let mut crowded = Particles::new(4);
        let mut none = 0;
        assert_eq!(crowded.step(&emitter, Vec3::ZERO, 1.0, true, &mut none), 0);
    }

    #[test]
    fn a_quad_faces_the_camera_and_takes_its_cell_by_age() {
        let emitter = emitter(10.0, 1.0);
        let mut particles = Particles::new(5);
        particles.live.push(Particle {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            age: 0.0,
        });
        let quads = particles.quads(&emitter, (Vec3::X, Vec3::Y));
        assert_eq!(quads.positions.len(), 4);
        assert_eq!(quads.indices, [0, 1, 2, 0, 2, 3]);
        assert!(quads.positions.iter().all(|corner| corner[2] == 0.0));
        // The first cell of a 4x4 sheet, at full colour.
        assert_eq!(quads.uvs[2], [0.25, 0.0]);
        assert_eq!(quads.colors[0], [1.0, 1.0, 1.0, 1.0]);
        // Late in life it is a later cell, bluer and fading.
        particles.live[0].age = 0.9;
        let late = particles.quads(&emitter, (Vec3::X, Vec3::Y));
        assert!(late.uvs[0][1] >= 0.75);
        assert!(late.colors[0][3] < 0.5);
        assert!(late.colors[0][0] < late.colors[0][2]);
    }

    #[test]
    fn a_spray_mesh_takes_a_rewrite_after_each_extraction() {
        let emitter = emitter(10.0, 1.0);
        let mut particles = Particles::new(6);
        let mut room = TOTAL_CAP;
        particles.step(&emitter, Vec3::ZERO, 0.5, true, &mut room);
        let empty = Particles::new(7);
        assert!(spray_mesh(empty.quads(&emitter, (Vec3::X, Vec3::Y))).is_none());
        let mut mesh =
            spray_mesh(particles.quads(&emitter, (Vec3::X, Vec3::Y))).expect("it has particles");
        assert!(mesh.get_vertex_buffer_size() > 0);
        for _ in 0..2 {
            rewrite(&mut mesh, particles.quads(&emitter, (Vec3::X, Vec3::Y)));
            // What extraction does each frame: a render-world-only mesh
            // gives its data up, any other is copied.
            if mesh.asset_usage == RenderAssetUsages::RENDER_WORLD {
                mesh.take_gpu_data().expect("the first extraction");
            }
        }
        assert_eq!(mesh.count_vertices(), 4 * particles.len());
    }

    #[test]
    fn a_spray_without_particles_hides_instead_of_emptying_its_mesh() {
        let frame = |rewrite, visibility| Frame {
            rewrite,
            visibility,
        };
        assert_eq!(Frame::of(true, true), frame(true, None));
        assert_eq!(
            Frame::of(true, false),
            frame(false, Some(Visibility::Hidden))
        );
        assert_eq!(Frame::of(false, false), frame(false, None));
        assert_eq!(
            Frame::of(false, true),
            frame(true, Some(Visibility::Inherited))
        );
    }
}
