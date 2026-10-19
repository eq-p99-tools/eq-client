//! Skinning for the small classic models: the GPU places each vertex by its
//! bone's joint, so animating moves only the joints and never a mesh; asset
//! and pose math stay renderer independent.

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::DynamicSkinnedMeshBounds,
    mesh::{
        VertexAttributeValues,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    prelude::*,
};
use eq_client_assets::characters::CharacterAsset;
use std::sync::Arc;

use super::{create_render_primitives, create_texture_images};

#[derive(Component)]
pub(super) struct AnimatedCharacter {
    pub(super) asset: Arc<CharacterAsset>,
    /// The entities its bones' joints are, in bone order: what the GPU
    /// places its vertices by.
    joints: Vec<Entity>,
    /// The entities drawing the primitives.
    pub(super) parts: Vec<Part>,
    /// The gear last drawn and whether its helm showed, if dressed yet.
    pub(super) dressed: Option<(eq_client_core::outfit::Appearance, bool)>,
    /// The entity the parts hang from; held items hang from it too.
    pub(super) model: Entity,
    /// The render layers the model draws on.
    pub(super) layers: bevy::camera::visibility::RenderLayers,
    /// Items in the primary and secondary hands.
    pub(super) held: [Option<Held>; 2],
    /// Where held items attach in the latest pose, in `Attachment` order.
    pub(super) attachments: [Option<Mat4>; 3],
    /// Per asset primitive, whether its piece is drawn; hidden ones are not
    /// posed.
    pub(super) shown: Vec<bool>,
    elapsed: f32,
    since_pose: f32,
    moving_for: f32,
    /// Where it stood when last posed; none before its first pose, so a
    /// character newly drawn at its place is not taken for moving there.
    previous_position: Option<Vec3>,
    clip: (&'static str, bool),
    /// The swings the server sent for the spawn drawn.
    swings: Swings,
    /// How far the model hangs above its root: what lifts a pose's top to
    /// the root's [`super::names::Overhead`].
    lift: f32,
}

/// A corpse: drawn with its race's model, falling from when it died if the
/// player saw it die, else lying as it fell.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct Corpse {
    /// When it died, for a death the player saw.
    pub(super) fell: Option<std::time::Instant>,
}

impl Corpse {
    /// Seconds since it fell: for one that arrived dead, a time long past,
    /// so it lies rather than falling each time it is drawn.
    fn lying(self, now: std::time::Instant) -> f32 {
        self.fell.map_or(1.0e6, |fell| {
            now.saturating_duration_since(fell).as_secs_f32()
        })
    }
}

/// The death clip, whose last frame is the body lying on the ground.
const DEATH: (&str, bool) = ("D05", true);

/// How high over a corpse's position its name shows.
pub(super) const CORPSE_NAME_HEIGHT: f32 = 1.5;

/// One drawn primitive of a character instance.
pub(super) struct Part {
    /// Index into the asset's primitives and base material names.
    pub(super) index: usize,
    /// The entity drawing it.
    pub(super) entity: Entity,
    /// Its material in the base look.
    pub(super) base: Handle<StandardMaterial>,
}

/// An item drawn in a hand.
pub(super) struct Held {
    /// The item's model number, such as 10 for `IT10`.
    pub(super) number: u32,
    /// The entity drawing it.
    pub(super) entity: Entity,
    /// Where it attaches.
    pub(super) point: eq_client_assets::characters::Attachment,
}

/// Marks a held item's entity, which follows its attachment point.
#[derive(Component)]
pub(super) struct HeldItem;

/// Marks a bone's joint, which animation moves.
#[derive(Component)]
pub(super) struct Joint;

/// Joints, moved by their character's animation.
type Joints<'w, 's> = Query<
    'w,
    's,
    &'static mut Transform,
    (With<Joint>, Without<AnimatedCharacter>, Without<HeldItem>),
>;

/// Held items, placed and shown by their character's animation.
type HeldItems<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut Visibility),
    (With<HeldItem>, Without<AnimatedCharacter>),
>;

/// The model a movement root wears, for views that draw it again elsewhere.
#[derive(Component)]
pub(super) struct Model(pub PreparedCharacter);

/// Attaches the model to the movement root while keeping feet at terrain height.
pub(super) fn spawn(
    commands: &mut Commands,
    parent: Entity,
    prepared: PreparedCharacter,
    feet_offset: f32,
) {
    spawn_prepared(commands, parent, &prepared, feet_offset);
    commands.entity(parent).insert(Model(prepared));
}

/// Shared immutable model, mesh, texture, and material data for nearby
/// instances: every instance draws the same meshes, each placed by its own
/// joints.
#[derive(Clone)]
pub(super) struct PreparedCharacter {
    asset: Arc<CharacterAsset>,
    /// Drawable primitives, each with the index of its asset primitive, which
    /// also indexes its skin and base material name.
    primitives: Vec<(usize, Handle<Mesh>, Handle<StandardMaterial>)>,
    /// The model's inverse bind poses: identities, as its vertices are
    /// already bone-local.
    bindposes: Handle<SkinnedMeshInverseBindposes>,
}

/// The stores a model's textures, meshes, materials and bind poses go in.
pub(super) type ModelStores<'a> = (
    &'a mut Assets<Image>,
    &'a mut Assets<Mesh>,
    &'a mut Assets<StandardMaterial>,
    &'a mut Assets<SkinnedMeshInverseBindposes>,
);

impl PreparedCharacter {
    pub fn height(&self) -> f32 {
        self.asset.height()
    }
}

/// Where the meshes of models hung under a spawn, its body and the items it
/// holds, keep their data: in the main world as well as for drawing, since
/// clicks are picked against them there.
pub(super) const ON_A_SPAWN: RenderAssetUsages =
    RenderAssetUsages::MAIN_WORLD.union(RenderAssetUsages::RENDER_WORLD);

/// Uploads a model's textures and meshes once, each mesh holding its
/// vertices bone-local with the bone that places them. None, with a
/// warning, for a model the GPU cannot skin: more bones than a skin may
/// have joints, or a skin that does not match its mesh.
pub(super) fn prepare(
    asset: CharacterAsset,
    (images, meshes, materials, bindposes): ModelStores,
) -> Option<PreparedCharacter> {
    let bones = asset.bone_count();
    if bones > bevy::pbr::MAX_JOINTS {
        warn!(
            model = asset.name,
            bones, "Model not drawn: more bones than a skin may have joints"
        );
        return None;
    }
    let textures = create_texture_images(asset.textures.clone(), images);
    // Empty primitives draw nothing; the rest keep their asset index.
    let drawn: Vec<usize> = (0..asset.primitives.len())
        .filter(|index| {
            let primitive = &asset.primitives[*index];
            !primitive.indices.is_empty() && !primitive.positions.is_empty()
        })
        .collect();
    let rendered = create_render_primitives(
        drawn
            .iter()
            .map(|index| asset.primitives[*index].clone())
            .collect(),
        &textures,
        meshes,
        materials,
        true,
        ON_A_SPAWN,
    );
    for (index, (handle, _)) in drawn.iter().zip(&rendered) {
        let skinned = meshes
            .get_mut(handle)
            .and_then(|mut mesh| asset.skin(*index).and_then(|skin| bind(&mut mesh, skin)));
        if skinned.is_none() {
            warn!(
                model = asset.name,
                primitive = index,
                "Model not drawn: a skin does not match its mesh"
            );
            for (handle, _) in &rendered {
                meshes.remove(handle);
            }
            return None;
        }
    }
    Some(PreparedCharacter {
        asset: Arc::new(asset),
        primitives: drawn
            .into_iter()
            .zip(rendered)
            .map(|(index, (mesh, material))| (index, mesh, material))
            .collect(),
        bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(vec![
            Mat4::IDENTITY;
            bones
        ])),
    })
}

/// Gives a mesh its skin: its vertices bone-local, each with its one bone
/// at full weight, and the bounds the joints carry it within. None when the
/// skin and the mesh differ in their vertices.
fn bind(mesh: &mut Mesh, skin: eq_client_assets::characters::SkinBind) -> Option<()> {
    let vertices = mesh.count_vertices();
    let normals = mesh.contains_attribute(Mesh::ATTRIBUTE_NORMAL);
    if skin.positions.len() != vertices
        || skin.bones.len() < vertices
        || (normals && skin.normals.len() != vertices)
    {
        return None;
    }
    let joints = skin.bones[..vertices]
        .iter()
        .map(|bone| Some([u16::try_from(*bone).ok()?, 0, 0, 0]))
        .collect::<Option<Vec<_>>>()?;
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, skin.positions.to_vec());
    if normals {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, skin.normals.to_vec());
    }
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(joints),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_WEIGHT,
        vec![[1.0_f32, 0.0, 0.0, 0.0]; vertices],
    );
    mesh.generate_skinned_mesh_bounds().ok()
}

/// Creates an independently animated instance sharing its model's meshes,
/// textures and materials.
pub(super) fn spawn_prepared(
    commands: &mut Commands,
    parent: Entity,
    prepared: &PreparedCharacter,
    feet_offset: f32,
) {
    spawn_on_layers(
        commands,
        parent,
        prepared,
        feet_offset,
        &bevy::camera::visibility::RenderLayers::default(),
    );
}

/// As [`spawn_prepared`], drawn only by cameras that see one of `layers`.
pub(super) fn spawn_on_layers(
    commands: &mut Commands,
    parent: Entity,
    prepared: &PreparedCharacter,
    feet_offset: f32,
    layers: &bevy::camera::visibility::RenderLayers,
) {
    let asset = prepared.asset.clone();
    let (bottom, top) = asset.span().unwrap_or_default();
    let lift = -feet_offset - bottom;
    // Until gear says otherwise, only the base body and bare head show.
    let shown: Vec<bool> = asset.pieces.iter().map(|piece| piece.base()).collect();
    let mut parts = Vec::with_capacity(prepared.primitives.len());
    let mut joints = Vec::with_capacity(asset.bone_count());
    let child = commands
        .spawn((
            // Classic character meshes face heading 0 as stored, like all WLD
            // geometry (installed HUM/ERM/ELM/DWM toes extend along it), not
            // the movement root's +Z. Keep protocol heading on the root and
            // turn the model once here.
            Transform::from_xyz(0.0, lift, 0.0)
                .with_rotation(Quat::from_rotation_y(eq_client_core::model_yaw())),
            Visibility::Inherited,
        ))
        .with_children(|parent| {
            // The joints hang from the model beside its parts, in the base
            // pose until the first pose moves them.
            for joint in asset.joints("", 0.0, true) {
                joints.push(parent.spawn((Joint, Transform::from_matrix(joint))).id());
            }
            for (index, mesh, material) in &prepared.primitives {
                let visibility = if shown.get(*index).copied().unwrap_or(true) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                let part = parent
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        SkinnedMesh {
                            inverse_bindposes: prepared.bindposes.clone(),
                            joints: joints.clone(),
                        },
                        DynamicSkinnedMeshBounds,
                        visibility,
                        layers.clone(),
                    ))
                    .id();
                parts.push(Part {
                    index: *index,
                    entity: part,
                    base: material.clone(),
                });
            }
        })
        .id();
    commands
        .entity(parent)
        .add_child(child)
        // The model hangs with its soles at the feet, so its top is this far
        // above the root until the first pose places it.
        .insert(super::names::Overhead(top + lift))
        .insert(AnimatedCharacter {
            asset,
            joints,
            parts,
            dressed: None,
            model: child,
            layers: layers.clone(),
            held: [None, None],
            attachments: [None; 3],
            shown,
            elapsed: 0.0,
            since_pose: 0.0,
            moving_for: 0.0,
            previous_position: None,
            clip: ("P01", false),
            swings: Swings::default(),
            lift,
        });
}

/// The characters the animation poses, with what decides their clip: the
/// spawn they draw, whether they are the player, and whether a corpse and
/// when it fell; and
/// where their name goes.
type Animated<'w, 's> = Query<
    'w,
    's,
    (
        &'static Transform,
        &'static mut AnimatedCharacter,
        Option<&'static mut super::names::Overhead>,
        Option<&'static super::entities::RemoteEntity>,
        Has<super::Player>,
        Option<&'static Corpse>,
    ),
>;

/// Updates local animation only; it cannot send movement or gameplay commands.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn animate(
    time: Res<Time>,
    (online, spells, controls): (
        Res<super::online::OnlineState>,
        Res<super::spellbook::SpellNames>,
        Option<Res<super::motion::Controls>>,
    ),
    mut characters: Animated,
    mut held_items: HeldItems,
    mut joints: Joints,
) {
    let now = std::time::Instant::now();
    let world = online.world();
    for (transform, mut character, overhead, remote, own, corpse) in &mut characters {
        let id = remote.map(|entity| entity.id).or_else(|| {
            own.then(|| world.player().map(|player| player.spawn_id))
                .flatten()
        });
        let posture = id.and_then(|id| world.posture(id));
        let moving = character
            .previous_position
            .is_some_and(|previous| transform.translation.distance_squared(previous) > 0.0001);
        character.previous_position = Some(transform.translation);
        let dt = time.delta_secs().min(0.1);
        // The latest of the spawn's swing and its completed cast, which
        // plays its spell's casting gesture.
        let latest = id.and_then(|id| {
            let swing = world
                .motion_of(id)
                .map(|motion| (motion.count, motion.action));
            let cast = world.completed_cast_of(id).and_then(|done| {
                Some((
                    done.count,
                    spells.casting_animation(u32::from(done.spell_id))?,
                ))
            });
            swing.max(cast)
        });
        character.swings.follow(latest, dt);
        if posture == Some(eq_client_core::PostureState::Frozen) {
            continue;
        }
        character.elapsed += dt;
        character.since_pose += dt;
        character.moving_for = if moving {
            0.15
        } else {
            (character.moving_for - dt).max(0.0)
        };
        if character.since_pose < 1.0 / 30.0 {
            continue;
        }
        character.since_pose = 0.0;
        let gesture = character
            .swings
            .gesture(|clip| character.asset.clip_seconds(clip));
        // A corpse seen dying falls from its death; one that arrived dead
        // lies in the death clip's last frame.
        let lying = corpse.map(|corpse| corpse.lying(now));
        let (selected, pinned) = chosen_clip(lying, posture, character.moving_for > 0.0, gesture);
        let selected = paced(
            selected,
            character.clip,
            &character.asset,
            (world, id, own, controls.as_deref()),
        );
        if selected != character.clip {
            character.clip = selected;
            character.elapsed = 0.0;
        }
        if let Some(seconds) = pinned {
            character.elapsed = seconds;
        }
        let (clip, held) = selected;
        let posed = character.asset.joints(clip, character.elapsed, !held);
        let attachments = character.asset.attachments(&posed);
        // The name rides just over the head as the pose moves it (inferred,
        // a recording item); a corpse's stays where its lying body puts it.
        if let (Some(mut overhead), Some(top), None) = (
            overhead,
            character.asset.top(&posed, &character.shown),
            lying,
        ) {
            overhead.set_if_neq(super::names::Overhead(top + character.lift));
        }
        // Held items follow their hand or shield point, and stay hidden on a
        // skeleton without one rather than sitting at the model's origin.
        character.attachments = attachments;
        for item in character.held.iter().flatten() {
            let Ok((mut placed, mut shown)) = held_items.get_mut(item.entity) else {
                continue;
            };
            let point = attachments[item.point.index()];
            if let Some(point) = point {
                *placed = Transform::from_matrix(point);
            }
            let visibility = if point.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            shown.set_if_neq(visibility);
        }
        // The GPU places the vertices by the joints; no mesh changes.
        for (entity, joint) in character.joints.iter().zip(&posed) {
            if let Ok(mut transform) = joints.get_mut(*entity) {
                transform.set_if_neq(Transform::from_matrix(*joint));
            }
        }
    }
}

impl AnimatedCharacter {
    /// The casting gesture it is playing, if any: the seconds since it began
    /// and the seconds left of its clip.
    pub(super) fn casting_gesture(&self) -> Option<(f32, f32)> {
        let (_, action, _) = self.swings.latest?;
        if !(42..=44).contains(&action) {
            return None;
        }
        let length = |clip: &str| self.asset.clip_seconds(clip);
        let gesture = self.swings.gesture(length)?;
        Some((gesture.since, length(gesture.clip)? - gesture.since))
    }
}

/// What a character plays once over its stance and walk: a swing the
/// server sent, or the casting gesture of a cast it completed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Gesture {
    /// The clip, such as `C05`.
    clip: &'static str,
    /// Seconds since it came.
    since: f32,
}

/// The swings the server sent for one character and the casts it completed,
/// as its animation follows them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Swings {
    /// Whether the character has followed the world's motions yet.
    followed: bool,
    /// The latest swing's or completed cast's count and animation number,
    /// and the seconds since it came.
    latest: Option<(u64, u16, f32)>,
}

impl Swings {
    /// Follows the world's latest swing or completed cast for the
    /// character, its count and animation number, over a frame of `dt`
    /// seconds. A new one starts from its beginning, two alike in a row
    /// included; the one already there when the character was first drawn
    /// came before it and is over.
    fn follow(&mut self, news: Option<(u64, u16)>, dt: f32) {
        let latest = self
            .latest
            .map(|(count, action, since)| (count, action, since + dt));
        self.latest = match news {
            Some((count, action)) if latest.is_none_or(|(last, ..)| last != count) => {
                let since = if self.followed { 0.0 } else { f32::INFINITY };
                Some((count, action, since))
            }
            _ => latest,
        };
        self.followed = true;
    }

    /// What the character plays over its stance: the latest swing or
    /// casting gesture until its clip ends, in the clip the model has for it
    /// (`length` gives none for a clip the model lacks). Nothing plays
    /// during a cast, only once it completes, as the official client does
    /// (checked against it, 10-14).
    fn gesture(&self, length: impl Fn(&str) -> Option<f32>) -> Option<Gesture> {
        let (_, action, since) = self.latest?;
        let clip = played_clip(action, |clip| length(clip).is_some())?;
        (since < length(clip)?).then_some(Gesture { clip, since })
    }
}

/// The clip a character shows, and the time in it when that is pinned
/// rather than run on from when the clip was chosen. A corpse plays the
/// death clip from `corpse` seconds since it fell, staying on its last
/// frame, the body lying on the ground (a held clip stays there); one that
/// arrived dead is given a time long past, so it lies rather than falling
/// each time it is drawn (the fall is inferred). Sitting, lying, looting and
/// ducking show their posture; otherwise a swing or a completed cast's
/// gesture plays once over the stance and the walk alike, its clips being
/// the whole body's (inferred).
fn chosen_clip(
    corpse: Option<f32>,
    posture: Option<eq_client_core::PostureState>,
    moving: bool,
    gesture: Option<Gesture>,
) -> ((&'static str, bool), Option<f32>) {
    use eq_client_core::PostureState;
    if let Some(since) = corpse {
        return (DEATH, Some(since));
    }
    let posed = matches!(
        posture,
        Some(
            PostureState::Sitting
                | PostureState::Lying
                | PostureState::Looting
                | PostureState::Ducking
        )
    );
    match gesture {
        Some(Gesture { clip, since }) if !posed => ((clip, true), Some(since)),
        _ => (posture_clip(posture, moving), None),
    }
}

/// The clip each of the servers' animation numbers plays, from 1, as
/// `EQEmu` documents them (eqemu-docs-v2, docs/server/npc/animations.md);
/// TAKP numbers its animations the same. The codes are the models' own clip
/// names. Inferred: which clip the official client plays for a number is a
/// recording item.
#[rustfmt::skip]
const ACTION_CLIPS: [&str; 73] = [
    // 1 to 11: the weapon swings, kicks and the bow.
    "C01", "C02", "C03", "C04", "C05", "C06", "C07", "C08", "C09", "C10", "C11",
    // 12 to 16: struck, drowning and dying.
    "D01", "D02", "D03", "D04", "D05",
    // 17 to 26: moving about, and standing.
    "L01", "L02", "L03", "L04", "L05", "L06", "L07", "L08", "L09", "O01",
    // 27 to 31: socials.
    "S01", "S02", "S03", "S04", "S05",
    // 32 to 38: stances.
    "P01", "P02", "P03", "P04", "P05", "P06", "P07",
    // 39 to 47: the instruments, the three casting gestures and the monk
    // attacks.
    "T01", "T02", "T03", "T04", "T05", "T06", "T07", "T08", "T09",
    // 48 to 70: socials.
    "S06", "S07", "S08", "S09", "S10", "S11", "S12", "S13", "S14", "S15", "S16", "S17",
    "S18", "S19", "S20", "S21", "S22", "S23", "S24", "S25", "S26", "S27", "S28",
    // 71 to 73: stances.
    "P08", "O02", "O03",
];

/// The clip a server animation number plays; None for a number without one.
fn action_clip(action: u16) -> Option<&'static str> {
    ACTION_CLIPS
        .get(usize::from(action).checked_sub(1)?)
        .copied()
}

/// The clip a model plays for a server animation number: the table's, or
/// where the model lacks it, the lowest-numbered clip of its kind the model
/// has. An attack (1 to 11) stands in with another attack, and a casting
/// gesture (42 to 44) with another casting gesture; anything else the model
/// lacks plays nothing. Inferred, a recording item: `EQEmu`'s documentation
/// notes that a number plays differently from one race's model to
/// another's, and the gnoll's model has no clip for its own hand-to-hand
/// swing, kick or bash.
fn played_clip(action: u16, has: impl Fn(&str) -> bool) -> Option<&'static str> {
    let clip = action_clip(action)?;
    if has(clip) {
        return Some(clip);
    }
    let kin = match action {
        1..=11 => 1..=11,
        42..=44 => 42..=44,
        _ => return None,
    };
    kin.filter_map(action_clip).find(|clip| has(clip))
}

/// The clip a character plays at its pace ([`paced_clip`]); a change into
/// the walk or the run logs the pace it was chosen by.
fn paced(
    selected: (&'static str, bool),
    current: (&'static str, bool),
    asset: &CharacterAsset,
    (world, id, own, controls): (
        &eq_client_core::world::ClientWorld,
        Option<u16>,
        bool,
        Option<&super::motion::Controls>,
    ),
) -> (&'static str, bool) {
    use eq_client_core::movement::{record_speed, runs};
    let (speed, walk_speed, run_speed) = pace(world, id, own, controls);
    // The player moves at the pace it chose: its run, or its walk while
    // the walk toggle is on.
    let runs = if own {
        !controls.is_some_and(|controls| controls.walking)
    } else {
        runs(speed, walk_speed)
    };
    let selected = paced_clip(selected, runs, |clip| asset.clip_moves_body(clip));
    if selected != current && matches!(selected.0, "L01" | "L02") {
        debug!(
            spawn = id,
            own,
            clip = selected.0,
            speed,
            walk = record_speed(walk_speed),
            run = record_speed(run_speed),
            "Moving pace"
        );
    }
    selected
}

/// The pace a drawn character moves at, in units per second, with its
/// record's walk and run speeds: another spawn's by the speed of its latest
/// report, from which [`eq_client_core::movement::runs`] decides; the
/// player's by the speed its movement asks for (its run, or its walk while
/// walking), logged beside its choice.
fn pace(
    world: &eq_client_core::world::ClientWorld,
    id: Option<u16>,
    own: bool,
    controls: Option<&super::motion::Controls>,
) -> (f32, f32, f32) {
    use eq_client_core::movement::record_speed;
    if own {
        let walking = controls.is_some_and(|controls| controls.walking);
        let (walk, run) = world
            .player()
            .map_or((0.0, 0.0), |player| (player.walk_speed, player.run_speed));
        // The record's own speeds where it has them; else what the session
        // lets the player move at.
        let asked = if walk > 0.0 && run > walk {
            record_speed(if walking { walk } else { run })
        } else {
            controls
                .and_then(|controls| {
                    if walking {
                        controls.walk_speed
                    } else {
                        controls.speed
                    }
                })
                .unwrap_or(0.0)
        };
        return (asked, walk, run);
    }
    id.and_then(|id| world.spawn(id))
        .map_or((0.0, 0.0, 0.0), |spawn| {
            let [x, y, _] = spawn.state.velocity;
            (x.hypot(y), spawn.state.walk_speed, spawn.state.run_speed)
        })
}

/// The moving clip a character plays at its pace: the run (`L02`) when it
/// `runs`, else the walk (`L01`); either one the model has only for its root
/// gives way to the other, and a ducking walk (`L06`) the model has only for
/// its root gives way to the ducked stance (`L08`). `moves` says whether the
/// model's clip moves the body. A model lacking both moving clips shows what
/// it chose. Other clips pass unchanged. Inferred, recording checklist item
/// 6.232.
fn paced_clip(
    (clip, held): (&'static str, bool),
    runs: bool,
    moves: impl Fn(&str) -> bool,
) -> (&'static str, bool) {
    match clip {
        "L01" => {
            let (chosen, other) = if runs { ("L02", "L01") } else { ("L01", "L02") };
            let clip = if !moves(chosen) && moves(other) {
                other
            } else {
                chosen
            };
            (clip, held)
        }
        "L06" if !moves("L06") => ("L08", true),
        _ => (clip, held),
    }
}

/// WLD clip names documented by `EQEmu`; presentation never changes network position.
/// A moving character's walk (`L01`) is paced into its run by [`paced_clip`].
fn posture_clip(
    posture: Option<eq_client_core::PostureState>,
    moving: bool,
) -> (&'static str, bool) {
    use eq_client_core::PostureState;
    match posture {
        Some(PostureState::Sitting) => ("P02", true),
        Some(PostureState::Ducking) if moving => ("L06", false),
        Some(PostureState::Ducking) => ("L08", true),
        Some(PostureState::Looting) => ("P05", true),
        Some(PostureState::Lying) => ("D05", true),
        _ if moving => ("L01", false),
        _ => ("P01", false),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// The stores a model goes in, empty, on an app or world.
    pub(crate) fn stores(world: &mut World) {
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<SkinnedMeshInverseBindposes>>();
    }

    /// The test model, prepared in the world's stores.
    pub(crate) fn prepared(world: &mut World) -> PreparedCharacter {
        world
            .run_system_once(
                |mut images: ResMut<Assets<Image>>,
                 mut meshes: ResMut<Assets<Mesh>>,
                 mut materials: ResMut<Assets<StandardMaterial>>,
                 mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>| {
                    prepare(
                        CharacterAsset::testing(),
                        (&mut images, &mut meshes, &mut materials, &mut bindposes),
                    )
                },
            )
            .unwrap()
            .unwrap()
    }

    /// Draws the prepared model under a new root; the root.
    pub(crate) fn drawn(world: &mut World, prepared: &PreparedCharacter) -> Entity {
        let prepared = prepared.clone();
        world
            .run_system_once(move |mut commands: Commands| {
                let root = commands.spawn(Transform::IDENTITY).id();
                spawn_prepared(&mut commands, root, &prepared, 0.0);
                root
            })
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_skinned_once_and_its_instances_share_its_meshes() {
        let mut world = World::new();
        testing::stores(&mut world);
        let prepared = testing::prepared(&mut world);
        let first = testing::drawn(&mut world, &prepared);
        let second = testing::drawn(&mut world, &prepared);
        let parts = |world: &mut World, root: Entity| {
            let character = world.get::<AnimatedCharacter>(root).unwrap();
            let joints = character.joints.clone();
            let parts: Vec<_> = character.parts.iter().map(|part| part.entity).collect();
            let meshes: Vec<_> = parts
                .iter()
                .map(|part| world.get::<Mesh3d>(*part).unwrap().0.clone())
                .collect();
            let skinned = world.get::<SkinnedMesh>(parts[0]).unwrap().joints.clone();
            (joints, meshes, skinned)
        };
        let (first_joints, first_meshes, first_skin) = parts(&mut world, first);
        let (second_joints, second_meshes, _) = parts(&mut world, second);
        // One mesh per piece, shared; each instance has joints of its own.
        assert_eq!(first_meshes, second_meshes);
        assert_eq!(world.resource::<Assets<Mesh>>().len(), 1);
        assert_eq!(first_joints.len(), 2);
        assert_eq!(first_skin, first_joints);
        assert!(
            first_joints
                .iter()
                .all(|joint| !second_joints.contains(joint))
        );
        // The mesh holds its vertices bone-local, each with its bone at full
        // weight, and the bounds the joints carry it within.
        let meshes = world.resource::<Assets<Mesh>>();
        let mesh = meshes.get(&first_meshes[0]).unwrap();
        let Some(VertexAttributeValues::Uint16x4(joints)) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
        else {
            panic!("no joint indices");
        };
        assert_eq!(joints, &vec![[1, 0, 0, 0]; 3]);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        assert_eq!(positions[2], [0.0, 1.0, 0.0]);
        assert!(mesh.skinned_mesh_bounds().is_some());
    }

    #[test]
    fn animation_moves_the_joints_and_leaves_the_meshes_alone() {
        let mut app = crate::testing::app();
        testing::stores(app.world_mut());
        app.init_resource::<Time>().add_systems(Update, animate);
        let prepared = testing::prepared(app.world_mut());
        let root = testing::drawn(app.world_mut(), &prepared);
        let (joint, handle) = {
            let world = app.world();
            let character = world.get::<AnimatedCharacter>(root).unwrap();
            let part = character.parts[0].entity;
            (
                character.joints[1],
                world.get::<Mesh3d>(part).unwrap().0.clone(),
            )
        };
        let positions = |app: &App| {
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&handle)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(VertexAttributeValues::as_float3)
                .unwrap()
                .to_vec()
        };
        let before = positions(&app);
        let rest = *app.world().get::<Transform>(joint).unwrap();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(100));
        app.update();
        // The stance turns the bone a tenth of its quarter turn in its first
        // tenth of a second; the mesh keeps its vertices as they were.
        let turned = *app.world().get::<Transform>(joint).unwrap();
        assert!(turned.rotation.angle_between(rest.rotation) > 0.1);
        assert_eq!(turned.translation, rest.translation);
        assert_eq!(positions(&app), before);
    }

    #[test]
    fn a_skin_that_does_not_fit_its_mesh_is_refused() {
        fn skin(bones: &[usize]) -> eq_client_assets::characters::SkinBind<'_> {
            eq_client_assets::characters::SkinBind {
                positions: &[[0.0; 3]; 3],
                normals: &[],
                bones,
            }
        }
        let mut meshes = Assets::<Mesh>::default();
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0_f32; 3]; 3]);
        let handle = meshes.add(mesh);
        // A bone past what a joint index holds, or too few bones, is refused.
        assert!(bind(&mut meshes.get_mut(&handle).unwrap(), skin(&[0, 70_000, 1])).is_none());
        assert!(bind(&mut meshes.get_mut(&handle).unwrap(), skin(&[0, 1])).is_none());
        assert!(bind(&mut meshes.get_mut(&handle).unwrap(), skin(&[0, 1, 1])).is_some());
    }

    #[test]
    fn a_corpse_falls_from_its_death_or_lies_in_the_death_clips_last_frame() {
        use eq_client_core::PostureState;
        let swing = Gesture {
            clip: "C05",
            since: 0.1,
        };
        // One that arrived dead lies; one seen dying falls from its death.
        let now = std::time::Instant::now();
        let arrived = Corpse { fell: None }.lying(now);
        let (clip, pinned) = chosen_clip(
            Some(arrived),
            Some(PostureState::Standing),
            true,
            Some(swing),
        );
        assert_eq!(clip, ("D05", true));
        assert!(pinned.is_some_and(|seconds| seconds > 1000.0));
        let seen = Corpse {
            fell: now.checked_sub(std::time::Duration::from_millis(400)),
        };
        assert_eq!(
            chosen_clip(Some(seen.lying(now)), None, false, None),
            (("D05", true), Some(0.4))
        );
        assert_eq!(
            chosen_clip(None, Some(PostureState::Sitting), false, None),
            (("P02", true), None)
        );
    }

    #[test]
    fn a_moving_character_walks_or_runs_in_the_clip_that_moves_its_body() {
        let full = |clip: &str| clip != "missing";
        // Its pace picks the walk or the run.
        assert_eq!(paced_clip(("L01", false), false, full), ("L01", false));
        assert_eq!(paced_clip(("L01", false), true, full), ("L02", false));
        // A clip moving only the root gives way to the other, as a gnoll's
        // walk to its run.
        let root_walk = |clip: &str| clip != "L01";
        assert_eq!(paced_clip(("L01", false), false, root_walk), ("L02", false));
        let root_run = |clip: &str| clip != "L02";
        assert_eq!(paced_clip(("L01", false), true, root_run), ("L01", false));
        // A model with neither shows what it chose.
        let neither = |_: &str| false;
        assert_eq!(paced_clip(("L01", false), true, neither), ("L02", false));
        // A ducking walk with only the root's track ducks in place.
        assert_eq!(paced_clip(("L06", false), false, neither), ("L08", true));
        assert_eq!(paced_clip(("L06", false), true, full), ("L06", false));
        // Standing and the other clips are left as they are.
        assert_eq!(paced_clip(("P01", false), true, neither), ("P01", false));
        assert_eq!(paced_clip(("C05", true), true, full), ("C05", true));
    }

    #[test]
    fn the_servers_animation_numbers_play_eqemus_documented_clips() {
        assert_eq!(action_clip(5), Some("C05"));
        assert_eq!(action_clip(8), Some("C08"));
        assert_eq!(action_clip(43), Some("T05"));
        assert_eq!(action_clip(1), Some("C01"));
        assert_eq!(action_clip(16), Some("D05"));
        assert_eq!(action_clip(26), Some("O01"));
        assert_eq!(action_clip(48), Some("S06"));
        assert_eq!(action_clip(71), Some("P08"));
        assert_eq!(action_clip(73), Some("O03"));
        for none in [0, 74, 76, u16::MAX] {
            assert_eq!(action_clip(none), None, "{none}");
        }
        // The postures this module plays agree with the table.
        for (action, posture) in [(32, "P01"), (33, "P02"), (36, "P05"), (17, "L01")] {
            assert_eq!(action_clip(action), Some(posture));
        }
    }

    #[test]
    fn a_model_lacking_an_attack_or_gesture_plays_the_lowest_of_its_kind_it_has() {
        // A model like the Qeynos Hills gnoll's: C05, C09 and C10 its only
        // attacks, and T06 its only casting gesture.
        let gnoll = |clip: &str| ["C05", "C09", "C10", "D02", "P01", "T06"].contains(&clip);
        // Its own hand-to-hand swing, kick and bash stand in with its lowest
        // attack; the attacks it has play as they are.
        for action in [8, 1, 7, 5] {
            assert_eq!(played_clip(action, gnoll), Some("C05"), "{action}");
        }
        assert_eq!(played_clip(9, gnoll), Some("C09"));
        // Every casting gesture plays the one it has.
        for action in [42, 43, 44] {
            assert_eq!(played_clip(action, gnoll), Some("T06"), "{action}");
        }
        // Anything else it lacks, a monk's attack included, plays nothing,
        // and what it has plays.
        for action in [12, 27, 33, 39, 46, 74] {
            assert_eq!(played_clip(action, gnoll), None, "{action}");
        }
        assert_eq!(played_clip(13, gnoll), Some("D02"));
        // The lowest-numbered of a kind stands in.
        let clawed = |clip: &str| ["C02", "C03", "T05", "T06"].contains(&clip);
        assert_eq!(played_clip(8, clawed), Some("C02"));
        assert_eq!(played_clip(42, clawed), Some("T05"));
        // A model with every clip plays the table's.
        assert_eq!(played_clip(8, |_| true), Some("C08"));
        assert_eq!(played_clip(44, |_| true), Some("T06"));
        // One with no attack or gesture at all plays nothing for them.
        assert_eq!(played_clip(8, |clip| clip == "P01"), None);
        assert_eq!(played_clip(43, |clip| clip == "P01"), None);
    }

    #[test]
    fn a_swing_plays_once_over_the_stance_and_walk_but_not_over_a_posture() {
        use eq_client_core::PostureState;
        let swing = Some(Gesture {
            clip: "C05",
            since: 0.25,
        });
        for moving in [false, true] {
            assert_eq!(
                chosen_clip(None, Some(PostureState::Standing), moving, swing),
                (("C05", true), Some(0.25))
            );
        }
        assert_eq!(
            chosen_clip(None, None, true, swing),
            (("C05", true), Some(0.25))
        );
        for (posture, shown) in [
            (PostureState::Sitting, ("P02", true)),
            (PostureState::Lying, ("D05", true)),
            (PostureState::Looting, ("P05", true)),
            (PostureState::Ducking, ("L08", true)),
        ] {
            assert_eq!(
                chosen_clip(None, Some(posture), false, swing),
                (shown, None)
            );
        }
        // With nothing to play the stance or walk shows.
        assert_eq!(
            chosen_clip(None, Some(PostureState::Standing), true, None),
            (("L01", false), None)
        );
    }

    #[test]
    fn a_new_swing_or_completed_cast_plays_from_its_start_until_its_clip_ends() {
        let length = |clip: &str| match clip {
            "C05" => Some(0.8),
            "T05" => Some(1.2),
            _ => None,
        };
        let playing = |clip, since| Some(Gesture { clip, since });
        // The swing already there when the character is first drawn is over.
        let mut swings = Swings::default();
        swings.follow(Some((3, 5)), 0.0);
        assert_eq!(swings.gesture(length), None);
        // A new one plays from its start, and a second alike restarts it.
        swings.follow(Some((4, 5)), 0.1);
        assert_eq!(swings.gesture(length), playing("C05", 0.0));
        swings.follow(Some((4, 5)), 0.5);
        swings.follow(Some((5, 5)), 0.1);
        assert_eq!(swings.gesture(length), playing("C05", 0.0));
        // It ends with its clip, and the stance shows again.
        swings.follow(Some((5, 5)), 0.5);
        assert!(matches!(
            swings.gesture(length),
            Some(Gesture { since, .. }) if (since - 0.5).abs() < 0.0001
        ));
        swings.follow(Some((5, 5)), 0.3);
        assert_eq!(swings.gesture(length), None);
        // A completed cast plays its gesture once the same way.
        swings.follow(Some((6, 43)), 0.1);
        assert_eq!(swings.gesture(length), playing("T05", 0.0));
        swings.follow(Some((6, 43)), 1.3);
        assert_eq!(swings.gesture(length), None);
        // A model without the clip plays the lowest of its kind it has.
        swings.follow(Some((7, 8)), 0.1);
        assert_eq!(swings.gesture(length), playing("C05", 0.0));
        swings.follow(Some((8, 42)), 0.1);
        assert_eq!(swings.gesture(length), playing("T05", 0.0));
        // One with none of its kind plays nothing for it.
        let bare = |clip: &str| (clip == "P01").then_some(1.0);
        swings.follow(Some((9, 8)), 0.1);
        assert_eq!(swings.gesture(bare), None);
        // A character first drawn before any swing or cast plays the first.
        let mut fresh = Swings::default();
        fresh.follow(None, 0.1);
        fresh.follow(Some((10, 5)), 0.1);
        assert!(matches!(
            fresh.gesture(length),
            Some(Gesture { clip: "C05", .. })
        ));
    }

    #[test]
    fn posture_overrides_walk_and_transition_clips_hold_their_final_pose() {
        use eq_client_core::PostureState;
        assert_eq!(
            posture_clip(Some(PostureState::Sitting), true),
            ("P02", true)
        );
        assert_eq!(
            posture_clip(Some(PostureState::Ducking), false),
            ("L08", true)
        );
        assert_eq!(
            posture_clip(Some(PostureState::Ducking), true),
            ("L06", false)
        );
        assert_eq!(
            posture_clip(Some(PostureState::Standing), true),
            ("L01", false)
        );
        assert_eq!(
            posture_clip(Some(PostureState::Unknown(999)), false),
            ("P01", false)
        );
    }
}
