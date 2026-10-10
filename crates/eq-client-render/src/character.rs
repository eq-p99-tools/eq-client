//! CPU skinning for the small classic models; asset and pose math stay renderer independent.

use bevy::{asset::RenderAssetUsages, prelude::*};
use eq_client_assets::characters::CharacterAsset;
use std::sync::Arc;

use super::{create_render_primitives, create_texture_images};

#[derive(Component)]
pub(super) struct AnimatedCharacter {
    pub(super) asset: Arc<CharacterAsset>,
    /// Per drawn primitive, its asset index and this instance's posed mesh.
    meshes: Vec<(usize, Handle<Mesh>)>,
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
    previous_position: Vec3,
    clip: (&'static str, bool),
    /// The swings the server sent for the spawn drawn.
    swings: Swings,
}

/// A corpse: drawn with its race's model, lying as it fell.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct Corpse;

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
    asset: CharacterAsset,
    feet_offset: f32,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let prepared = prepare(asset, images, meshes, materials);
    spawn_prepared(commands, parent, &prepared, feet_offset, meshes);
    commands.entity(parent).insert(Model(prepared));
}

/// Shared immutable model, texture, and material data for nearby instances.
#[derive(Clone)]
pub(super) struct PreparedCharacter {
    asset: Arc<CharacterAsset>,
    /// Drawable primitives, each with the index of its asset primitive, which
    /// also indexes its pose and base material name.
    primitives: Vec<(usize, Handle<Mesh>, Handle<StandardMaterial>)>,
}

impl PreparedCharacter {
    pub fn height(&self) -> f32 {
        self.asset.height()
    }
}

/// Where the meshes of models hung under a spawn, its body and the items it
/// holds, keep their data: in the main world as well as for drawing, since
/// clicks are picked against them there and bodies are posed from it.
pub(super) const ON_A_SPAWN: RenderAssetUsages =
    RenderAssetUsages::MAIN_WORLD.union(RenderAssetUsages::RENDER_WORLD);

/// Uploads model textures once; posed vertex buffers remain per-instance.
pub(super) fn prepare(
    asset: CharacterAsset,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> PreparedCharacter {
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
    PreparedCharacter {
        asset: Arc::new(asset),
        primitives: drawn
            .into_iter()
            .zip(rendered)
            .map(|(index, (mesh, material))| (index, mesh, material))
            .collect(),
    }
}

/// Creates an independently animated instance sharing its model's textures/materials.
pub(super) fn spawn_prepared(
    commands: &mut Commands,
    parent: Entity,
    prepared: &PreparedCharacter,
    feet_offset: f32,
    meshes: &mut Assets<Mesh>,
) {
    spawn_on_layers(
        commands,
        parent,
        prepared,
        feet_offset,
        meshes,
        &bevy::camera::visibility::RenderLayers::default(),
    );
}

/// As [`spawn_prepared`], drawn only by cameras that see one of `layers`.
pub(super) fn spawn_on_layers(
    commands: &mut Commands,
    parent: Entity,
    prepared: &PreparedCharacter,
    feet_offset: f32,
    meshes: &mut Assets<Mesh>,
    layers: &bevy::camera::visibility::RenderLayers,
) {
    let asset = prepared.asset.clone();
    let primitives: Vec<_> = prepared
        .primitives
        .iter()
        .filter_map(|(index, handle, material)| {
            let mesh = meshes.get(handle)?.clone();
            Some((*index, meshes.add(mesh), material.clone()))
        })
        .collect();
    let handles = primitives
        .iter()
        .map(|(index, mesh, _)| (*index, mesh.clone()))
        .collect();
    let (bottom, top) = asset.span().unwrap_or_default();
    // Until gear says otherwise, only the base body and bare head show.
    let shown: Vec<bool> = asset.pieces.iter().map(|piece| piece.base()).collect();
    let mut parts = Vec::with_capacity(primitives.len());
    let child = commands
        .spawn((
            // Classic character meshes face heading 0 as stored, like all WLD
            // geometry (installed HUM/ERM/ELM/DWM toes extend along it), not
            // the movement root's +Z. Keep protocol heading on the root and
            // turn the model once here.
            Transform::from_xyz(0.0, -feet_offset - bottom, 0.0)
                .with_rotation(Quat::from_rotation_y(eq_client_core::model_yaw())),
            Visibility::Inherited,
        ))
        .with_children(|parent| {
            for (index, mesh, material) in primitives {
                let visibility = if shown.get(index).copied().unwrap_or(true) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                let part = parent
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material.clone()),
                        visibility,
                        layers.clone(),
                    ))
                    .id();
                parts.push(Part {
                    index,
                    entity: part,
                    base: material,
                });
            }
        })
        .id();
    commands
        .entity(parent)
        .add_child(child)
        // The model hangs with its soles at the feet, so its top is this far
        // above the root.
        .insert(super::names::Overhead(top - bottom - feet_offset))
        .insert(AnimatedCharacter {
            asset,
            meshes: handles,
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
            previous_position: Vec3::ZERO,
            clip: ("P01", false),
            swings: Swings::default(),
        });
}

/// The characters the animation poses, with what decides their clip: the
/// spawn they draw, whether they are the player, and whether a corpse.
type Animated<'w, 's> = Query<
    'w,
    's,
    (
        &'static Transform,
        &'static mut AnimatedCharacter,
        Option<&'static super::entities::RemoteEntity>,
        Has<super::Player>,
        Has<Corpse>,
    ),
>;

/// Updates local animation only; it cannot send movement or gameplay commands.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn animate(
    time: Res<Time>,
    (online, spells): (
        Res<super::online::OnlineState>,
        Res<super::spellbook::SpellNames>,
    ),
    mut characters: Animated,
    mut held_items: HeldItems,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let now = std::time::Instant::now();
    let world = online.world();
    for (transform, mut character, remote, own, corpse) in &mut characters {
        let id = remote.map(|entity| entity.id).or_else(|| {
            own.then(|| world.player().map(|player| player.spawn_id))
                .flatten()
        });
        let posture = id.and_then(|id| world.posture(id));
        let moving = transform
            .translation
            .distance_squared(character.previous_position)
            > 0.0001;
        character.previous_position = transform.translation;
        let dt = time.delta_secs().min(0.1);
        character
            .swings
            .follow(id.and_then(|id| world.motion_of(id)), dt);
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
        let cast = id
            .and_then(|id| world.cast_by(id, now))
            .and_then(|spell| spells.casting_animation(spell))
            .and_then(action_clip);
        let gesture = character
            .swings
            .gesture(cast, |clip| character.asset.clip_seconds(clip));
        let (selected, pinned) = chosen_clip(corpse, posture, character.moving_for > 0.0, gesture);
        if selected != character.clip {
            character.clip = selected;
            character.elapsed = 0.0;
        }
        if let Some(seconds) = pinned {
            character.elapsed = seconds;
        }
        let (clip, held) = selected;
        let (mut poses, attachments) =
            character
                .asset
                .pose_with_attachments(clip, character.elapsed, !held, &character.shown);
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
        for (index, handle) in &character.meshes {
            let Some(pose) = poses.get_mut(*index).and_then(Option::take) else {
                continue;
            };
            if let Some(mut mesh) = meshes.get_mut(handle) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pose.positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, pose.normals);
            }
        }
    }
}

/// What a character plays over its stance and walk: a swing the server
/// sent, once, or the gesture of a cast under way, for as long as it lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    /// A swing's clip, and how far into it the character is.
    Swing {
        /// The clip, such as `C05`.
        clip: &'static str,
        /// Seconds since the swing came.
        since: f32,
    },
    /// The casting gesture's clip, looped.
    Cast(&'static str),
}

/// The swings the server sent for one character, as its animation follows
/// them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Swings {
    /// Whether the character has followed the world's motions yet.
    followed: bool,
    /// The latest motion's count and animation number, and the seconds
    /// since it came.
    latest: Option<(u64, u16, f32)>,
}

impl Swings {
    /// Follows the world's latest motion for the character over a frame of
    /// `dt` seconds. A new one starts from its beginning, two alike in a row
    /// included; the one already there when the character was first drawn
    /// came before it and is over.
    fn follow(&mut self, motion: Option<eq_client_core::world::Motion>, dt: f32) {
        let latest = self
            .latest
            .map(|(count, action, since)| (count, action, since + dt));
        self.latest = match motion {
            Some(motion) if latest.is_none_or(|(count, ..)| count != motion.count) => {
                let since = if self.followed { 0.0 } else { f32::INFINITY };
                Some((motion.count, motion.action, since))
            }
            _ => latest,
        };
        self.followed = true;
    }

    /// What the character plays over its stance: the latest swing until its
    /// clip ends, else the casting gesture. A model without the clip
    /// (`length` gives none) plays neither.
    fn gesture(
        &self,
        cast: Option<&'static str>,
        length: impl Fn(&str) -> Option<f32>,
    ) -> Option<Gesture> {
        let swing = self.latest.and_then(|(_, action, since)| {
            let clip = action_clip(action)?;
            (since < length(clip)?).then_some(Gesture::Swing { clip, since })
        });
        swing.or_else(|| {
            cast.filter(|clip| length(clip).is_some())
                .map(Gesture::Cast)
        })
    }
}

/// The clip a character shows, and the time in it when that is pinned
/// rather than run on from when the clip was chosen. A corpse lies as it
/// fell, in the death clip's last frame, rather than falling each time it is
/// drawn (a held clip stays on its last frame). Sitting, lying, looting and
/// ducking show their posture; otherwise a swing plays once over the stance
/// and the walk alike, its clips being the whole body's (inferred), and a
/// cast loops its gesture.
fn chosen_clip(
    corpse: bool,
    posture: Option<eq_client_core::PostureState>,
    moving: bool,
    gesture: Option<Gesture>,
) -> ((&'static str, bool), Option<f32>) {
    use eq_client_core::PostureState;
    if corpse {
        return (DEATH, Some(1.0e6));
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
        Some(Gesture::Swing { clip, since }) if !posed => ((clip, true), Some(since)),
        Some(Gesture::Cast(clip)) if !posed => ((clip, false), None),
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

/// WLD clip names documented by `EQEmu`; presentation never changes network position.
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
mod tests {
    use super::*;

    #[test]
    fn a_corpse_lies_in_the_death_clips_last_frame_whatever_its_posture() {
        use eq_client_core::PostureState;
        let swing = Gesture::Swing {
            clip: "C05",
            since: 0.1,
        };
        let (clip, pinned) = chosen_clip(true, Some(PostureState::Standing), true, Some(swing));
        assert_eq!(clip, ("D05", true));
        assert!(pinned.is_some_and(|seconds| seconds > 1000.0));
        assert_eq!(
            chosen_clip(false, Some(PostureState::Sitting), false, None),
            (("P02", true), None)
        );
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
    fn a_swing_plays_once_over_the_stance_and_walk_but_not_over_a_posture() {
        use eq_client_core::PostureState;
        let swing = Some(Gesture::Swing {
            clip: "C05",
            since: 0.25,
        });
        for moving in [false, true] {
            assert_eq!(
                chosen_clip(false, Some(PostureState::Standing), moving, swing),
                (("C05", true), Some(0.25))
            );
        }
        assert_eq!(
            chosen_clip(false, None, true, swing),
            (("C05", true), Some(0.25))
        );
        for (posture, shown) in [
            (PostureState::Sitting, ("P02", true)),
            (PostureState::Lying, ("D05", true)),
            (PostureState::Looting, ("P05", true)),
            (PostureState::Ducking, ("L08", true)),
        ] {
            assert_eq!(
                chosen_clip(false, Some(posture), false, swing),
                (shown, None)
            );
            assert_eq!(
                chosen_clip(false, Some(posture), false, Some(Gesture::Cast("T05"))),
                (shown, None)
            );
        }
        // A cast loops its gesture; with nothing to play the stance shows.
        assert_eq!(
            chosen_clip(
                false,
                Some(PostureState::Standing),
                true,
                Some(Gesture::Cast("T04"))
            ),
            (("T04", false), None)
        );
        assert_eq!(
            chosen_clip(false, Some(PostureState::Standing), true, None),
            (("L01", false), None)
        );
    }

    fn motion(count: u64, action: u16) -> eq_client_core::world::Motion {
        eq_client_core::world::Motion {
            action,
            speed: 1.0,
            count,
        }
    }

    #[test]
    fn a_new_swing_plays_from_its_start_until_its_clip_ends() {
        let length = |clip: &str| match clip {
            "C05" => Some(0.8),
            "T05" => Some(1.2),
            _ => None,
        };
        // The swing already there when the character is first drawn is over.
        let mut swings = Swings::default();
        swings.follow(Some(motion(3, 5)), 0.0);
        assert_eq!(swings.gesture(None, length), None);
        // A new one plays from its start, and a second alike restarts it.
        swings.follow(Some(motion(4, 5)), 0.1);
        assert_eq!(
            swings.gesture(None, length),
            Some(Gesture::Swing {
                clip: "C05",
                since: 0.0
            })
        );
        swings.follow(Some(motion(4, 5)), 0.5);
        swings.follow(Some(motion(5, 5)), 0.1);
        assert_eq!(
            swings.gesture(None, length),
            Some(Gesture::Swing {
                clip: "C05",
                since: 0.0
            })
        );
        // It ends with its clip, and the cast's gesture shows again.
        swings.follow(Some(motion(5, 5)), 0.5);
        assert!(matches!(
            swings.gesture(Some("T05"), length),
            Some(Gesture::Swing { since, .. }) if (since - 0.5).abs() < 0.0001
        ));
        swings.follow(Some(motion(5, 5)), 0.3);
        assert_eq!(swings.gesture(None, length), None);
        assert_eq!(
            swings.gesture(Some("T05"), length),
            Some(Gesture::Cast("T05"))
        );
        // A model without the clip plays neither a swing nor the gesture.
        swings.follow(Some(motion(6, 8)), 0.1);
        assert_eq!(swings.gesture(Some("T04"), length), None);
        // A character first drawn before any motion plays the first one.
        let mut fresh = Swings::default();
        fresh.follow(None, 0.1);
        fresh.follow(Some(motion(7, 5)), 0.1);
        assert!(matches!(
            fresh.gesture(None, length),
            Some(Gesture::Swing { clip: "C05", .. })
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
