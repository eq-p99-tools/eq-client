//! Titanium light definitions, and zones without a sky drawn as the
//! graphics engine's base and light passes draw them: each vertex's baked
//! light, the zone ambient by its weight, the floor, and point lights,
//! worked out per vertex here (`zone_light`, `docs/zone-lighting.md`).
use super::{
    OrbitCamera, Player, PlayerBody, ViewerSettings,
    daylight::Lighting,
    entities::NearbyEntities,
    online::OnlineState,
    options::OptionsState,
    spellbook::SpellNames,
    zone_light::{self, BaseInputs, PassLight},
};
use bevy::prelude::*;
use eq_client_assets::{
    lighting::{DayAmbient, PlacedLight},
    spells::{BaseDuration, Mechanics},
    ui::InstalledClient,
};
use eq_client_core::{render_position, world::ClientWorld};

/// How many lights others carry are worked out at once, nearest the camera
/// first (a bound of this client's, not the original's).
const MOST_CARRIED: usize = 8;

/// A placed mesh's world positions and normals, one each per vertex.
pub(super) type Placed = (Vec<[f32; 3]>, Vec<[f32; 3]>);

/// How a scene's meshes were last coloured.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Colouring {
    /// White: the plain textures.
    White,
    /// By the engine's sum at these inputs.
    Lit(BaseInputs),
}

/// One drawn zone mesh whose vertices the zone light colours.
pub(super) struct LitMesh {
    pub mesh: Handle<Mesh>,
    /// One per vertex: its baked light, or no light of its own and the full
    /// ambient weight (inferred) where the file gives none.
    pub light: Vec<[u8; 4]>,
    /// World positions and normals, for a mesh drawn once in one place,
    /// which point lights reach. None for a mesh several placements or a
    /// door share: the base light alone (inferred).
    pub placed_at: Option<Placed>,
    /// Additive geometry, drawn as before (the engine's alpha path is
    /// unread; inferred).
    pub additive: bool,
}

/// What is worked out for a lit mesh: its box, the placed lights that
/// reach it most, its base light at the current inputs, and the lights
/// its colours were last written with.
#[derive(Default)]
struct Worked {
    bounds: Option<([f32; 3], [f32; 3])>,
    placed: Vec<PassLight>,
    base: Vec<[f32; 3]>,
    assigned: Vec<PassLight>,
}

#[derive(Resource)]
pub(super) struct SceneLights {
    zone: String,
    placed: Vec<PlacedLight>,
    /// The installed program's day ambient table, when it holds one.
    day: Option<DayAmbient>,
    pub(super) meshes: Vec<LitMesh>,
    worked: Vec<Worked>,
    /// How the meshes were last coloured; None before the first time.
    applied: Option<Colouring>,
}

impl SceneLights {
    pub(super) fn new(zone: String, placed: Vec<PlacedLight>, day: Option<&DayAmbient>) -> Self {
        Self {
            zone,
            placed,
            day: day.copied(),
            meshes: Vec::new(),
            worked: Vec::new(),
            applied: None,
        }
    }
}

/// Whether the zone draws as the Titanium engine's no-sky scene: the
/// installed client is Titanium, the player is in the zone the scene was
/// built for, and the zone has no sky. The Mac-era engine is unread, and
/// See the World Clearly draws the plain textures.
pub(super) fn interior(
    scene: Option<&SceneLights>,
    world: &ClientWorld,
    settings: &ViewerSettings,
    clearly: bool,
) -> bool {
    !clearly
        && settings.0.installed_client == InstalledClient::Titanium
        && world.connected()
        && world.session_id().is_some()
        && world.pending_transfer().is_none()
        && world.sky().is_some_and(|sky| sky.sky == 0)
        && scene.is_some_and(|scene| scene.zone == world.zone())
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Vision {
    #[default]
    Ordinary,
    Infra,
    Ultra,
}

/// The playable races checked at eqgame.exe 0x41bf92. Unknown races stay
/// unresolved. Received vision effects can still establish a floor.
fn racial_vision(race: u32) -> Option<Vision> {
    Some(match race {
        1..=3 => Vision::Ordinary,
        4 | 5 | 7..=12 | 128 => Vision::Infra,
        6 | 130 | 330 => Vision::Ultra,
        _ => return None,
    })
}

/// A known active spell's low-light ability, from installed/effective SPAs
/// 65 and 66 (eqgame.exe 0x414913, 0x441295).
fn spell_vision(mechanics: &Mechanics, level: u16, landed: bool) -> Vision {
    if landed
        && matches!(
            mechanics.base_duration(level),
            None | Some(BaseDuration::Ticks(0))
        )
    {
        return Vision::Ordinary;
    }
    mechanics
        .effects
        .iter()
        .filter(|effect| {
            matches!(effect.id, 65 | 66)
                && effect.base_magnitude(level).is_some_and(|value| value != 0)
        })
        .map(|effect| match effect.id {
            65 => Vision::Infra,
            66 => Vision::Ultra,
            _ => Vision::Ordinary,
        })
        .max()
        .unwrap_or_default()
}

pub(super) fn observer(world: &ClientWorld, names: Option<&SpellNames>) -> Vision {
    let Some(player) = world
        .player()
        .filter(|_| world.in_world() && !world.is_dead())
    else {
        return Vision::Ordinary;
    };
    let race = world
        .spawn(player.spawn_id)
        .map_or(player.race, |spawn| spawn.state.race);
    let innate = racial_vision(race).unwrap_or_default();
    let Some(names) = names else {
        return innate;
    };
    let slotted = world
        .buffs()
        .slots()
        .into_iter()
        .flat_map(|slots| slots.values())
        .filter_map(|buff| {
            Some(spell_vision(
                names.mechanics(buff.spell_id)?,
                u16::from(buff.caster_level),
                false,
            ))
        });
    let landed = world.buffs().effects().values().filter_map(|effect| {
        Some(spell_vision(
            names.mechanics(u32::from(effect.spell_id))?,
            effect.caster_level,
            true,
        ))
    });
    slotted.chain(landed).fold(innate, Vision::max)
}

/// The zone ambient eqgame.exe 0x441295 picks by the zone header's time
/// type (header offset 424, [0x923b88]) at a minute after the sky's 12 AM:
/// types 1, 2 and 5 follow the program's day table (0x78f708, read from
/// the installed eqgame.exe), an hour's row toward the next by the minute
/// (the hour the sky follows is taken as the table's; inferred); type 4 is
/// 0.5 grey and every other type 0.1 grey. A time not yet given counts as
/// noon. Without the table, types 1, 2 and 5 take 0.1 grey too. A sky
/// object's colour, which replaces the table, belongs to zones with a sky.
fn zone_ambient(time_type: u8, minute: Option<u16>, day: Option<&DayAmbient>) -> [f32; 3] {
    match (time_type, day) {
        (1 | 2 | 5, Some(day)) => {
            let minute = minute.unwrap_or(12 * 60) % (24 * 60);
            let hour = usize::from(minute / 60);
            let part = f32::from(minute % 60) / 60.0;
            let (from, to) = (day[hour], day[(hour + 1) % 24]);
            std::array::from_fn(|channel| {
                (from[channel] + (to[channel] - from[channel]) * part).min(1.0)
            })
        }
        (4, _) => [0.5; 3],
        _ => [0.1; 3],
    }
}

/// The ambient floor eqgame.exe 0x441295 raises the zone ambient to, from
/// the vision globals 0x4917dd sets: 0.08 grey, or the infravision and
/// ultravision colours.
fn floor(vision: Vision) -> [f32; 3] {
    match vision {
        Vision::Ordinary => [0.08; 3],
        Vision::Infra => [65.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0],
        Vision::Ultra => [80.0 / 255.0, 80.0 / 255.0, 120.0 / 255.0],
    }
}

/// A no-sky zone's ambient: the zone ambient raised to the floor per
/// channel (0x441295).
pub(super) fn interior_ambient(
    time_type: u8,
    minute: Option<u16>,
    vision: Vision,
    day: Option<&DayAmbient>,
) -> [f32; 3] {
    let zone = zone_ambient(time_type, minute, day);
    let floor = floor(vision);
    std::array::from_fn(|channel| zone[channel].max(floor[channel]))
}

/// What 0x441295 hands the engine for a no-sky zone: the ambient, and the
/// floor as the special ambient's base.
///
/// Known permissive departure: 0x441295 scales the floor by (1 - fade),
/// where the fade follows the player's own carried light ([0x698a08]) by
/// 0.01 a call. The light value's scale isn't read, so the floor applies
/// unscaled. That is brighter than the original only while the player
/// carries a light, and only where the floor clears the base pass's 0.1
/// offset: with infravision or ultravision.
pub(super) fn interior_inputs(
    time_type: u8,
    minute: Option<u16>,
    vision: Vision,
    day: Option<&DayAmbient>,
) -> BaseInputs {
    BaseInputs::new(
        interior_ambient(time_type, minute, vision, day),
        floor(vision),
    )
}

impl SceneLights {
    /// The installed program's day ambient table, when it holds one.
    pub(super) fn day(&self) -> Option<&DayAmbient> {
        self.day.as_ref()
    }
}

/// Maps the native ambient onto this renderer's ambient/exposure controls,
/// for what is still lit by Bevy's lights in a no-sky zone (boats; the
/// zone's own geometry is coloured per vertex instead).
pub(super) fn adjust(
    lit: &mut Lighting,
    vision: Vision,
    interior: Option<[f32; 3]>,
    darkness: f32,
) {
    if let Some(rgb) = interior {
        let peak = rgb.into_iter().fold(0.0_f32, f32::max).max(f32::EPSILON);
        lit.sun.0 = 0.0;
        lit.ambient = (150.0 * peak, rgb.map(|channel| channel / peak));
    } else if vision != Vision::Ordinary {
        // Outdoor terrain uses the existing time-of-day grade. Preserve the
        // sky/fog interpolation and apply only a bounded dark-time lift.
        let peak = floor(vision).into_iter().fold(0.0_f32, f32::max);
        lit.exposure += darkness * peak;
        lit.temperature += darkness * if vision == Vision::Infra { 0.05 } else { -0.05 };
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Lamp {
    range: f32,
    rgb: [f32; 3],
    level: f32,
}

/// Titanium eqgame.exe 0x43746b definitions and 0x439377 default
/// ranges. Unknown types have no projection, including high-bit variants.
fn carried(kind: u8) -> Option<Lamp> {
    let warm = [0.996, 0.9412, 0.6274];
    let magic = [0.4, 0.996, 0.996];
    let (range, rgb) = match kind {
        1 => (25.0, warm),
        2 => (100.0, warm),
        3 | 9 => (100.0, magic),
        4 => (150.0, warm),
        5 | 10 => (150.0, magic),
        6 => (200.0, warm),
        7 | 11 => (200.0, magic),
        8 => (400.0, warm),
        12 | 14 => (150.0, [0.5, 0.26, 0.0]),
        13 | 15 => (150.0, [0.16, 0.2, 0.5]),
        _ => return None,
    };
    Some(Lamp {
        range,
        rgb,
        level: 1.0,
    })
}

impl Lamp {
    fn pass(self, position: Vec3) -> PassLight {
        PassLight {
            position: position.to_array(),
            radius: self.range,
            rgb: self.rgb.map(|channel| channel * self.level),
        }
    }
}

/// A draw's light list: the lights within their radius of its box, nearest
/// first, of which the draw takes the first [`zone_light::SLOTS`]. The
/// engine builds these lists before drawing, by a rule not yet read; the
/// range, order and count here are inferred.
fn assign(bounds: ([f32; 3], [f32; 3]), lights: impl Iterator<Item = PassLight>) -> Vec<PassLight> {
    let mut reaching: Vec<_> = lights
        .map(|light| (light.distance_squared(bounds), light))
        .filter(|(squared, light)| *squared <= light.radius * light.radius)
        .collect();
    reaching.sort_by(|a, b| a.0.total_cmp(&b.0));
    reaching
        .into_iter()
        .take(zone_light::SLOTS)
        .map(|(_, light)| light)
        .collect()
}

fn bounds(positions: &[[f32; 3]]) -> Option<([f32; 3], [f32; 3])> {
    let (first, rest) = positions.split_first()?;
    Some(rest.iter().fold((*first, *first), |(low, high), point| {
        (
            std::array::from_fn(|axis| low[axis].min(point[axis])),
            std::array::from_fn(|axis| high[axis].max(point[axis])),
        )
    }))
}

/// The lights the player and others nearby carry, nearest the camera first
/// for others. `ShowDynamicLights` is on by default (eqgame.exe 0x4a3341).
fn carried_lights(
    world: &ClientWorld,
    nearby: &NearbyEntities,
    eye: Vec3,
    own: (Vec3, f32),
    transforms: &Query<&Transform, (Without<OrbitCamera>, Without<Player>)>,
) -> Vec<PassLight> {
    let mut lights = Vec::new();
    let Some(player) = world.player() else {
        return lights;
    };
    if world.is_dead() {
        return lights;
    }
    let inventory = world.inventory();
    let light = if inventory.received() {
        inventory.titanium_carried_light()
    } else {
        Some(player.appearance.light)
    };
    if let Some(lamp) = light.and_then(carried) {
        lights.push(lamp.pass(own.0 + Vec3::Y * own.1 * 0.65));
    }
    let mut others: Vec<_> = nearby
        .rendered
        .iter()
        .filter_map(|(id, entity)| {
            let spawn = &world.spawn(*id)?.state;
            if !world.can_see(spawn) {
                return None;
            }
            let lamp = carried(spawn.appearance.light)?;
            let base = transforms.get(*entity).ok().map_or_else(
                || Vec3::from_array(render_position(spawn.position)),
                |pose| pose.translation,
            );
            let point = base + Vec3::Y * spawn.size.max(2.0) * 0.65;
            Some((eye.distance(point), lamp.pass(point)))
        })
        .collect();
    others.sort_by(|a, b| a.0.total_cmp(&b.0));
    lights.extend(
        others
            .into_iter()
            .take(MOST_CARRIED)
            .map(|(_, light)| light),
    );
    lights
}

/// A mesh's vertex colours: its base light and its draw's lights.
fn colours(lit: &LitMesh, worked: &Worked) -> Vec<[f32; 4]> {
    if worked.base.is_empty() {
        return vec![[1.0; 4]; lit.light.len()];
    }
    worked
        .base
        .iter()
        .enumerate()
        .map(|(index, base)| {
            let mut light = *base;
            if let Some((positions, normals)) = &lit.placed_at {
                for (slot, pass) in worked.assigned.iter().enumerate() {
                    let add = zone_light::point_light(slot, pass, positions[index], normals[index]);
                    light = std::array::from_fn(|channel| light[channel] + add[channel]);
                }
            }
            zone_light::vertex_colour(light)
        })
        .collect()
}

type Positions<'w, 's> = (
    Query<'w, 's, &'static Transform, (With<OrbitCamera>, Without<Player>)>,
    Query<'w, 's, (&'static Transform, &'static PlayerBody), (With<Player>, Without<OrbitCamera>)>,
    Query<'w, 's, &'static Transform, (Without<OrbitCamera>, Without<Player>)>,
);

/// Colours the zone's meshes as the engine's single-pass effects light
/// them, or white when the zone isn't drawn so. The base light is worked
/// out once per zone and input; a mesh is rewritten when the lights its
/// draw takes change, as when a carried light moves near it.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    online: Res<OnlineState>,
    settings: Res<ViewerSettings>,
    options: Res<OptionsState>,
    names: Option<Res<SpellNames>>,
    scene: Option<ResMut<SceneLights>>,
    nearby: Res<NearbyEntities>,
    (eye, player, transforms): Positions,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let world = online.world();
    let clearly = options.options.qol.on(eq_client_core::qol::Fix::ClearView);
    let Some(mut scene) = scene else {
        return;
    };
    let lit = interior(Some(&scene), world, &settings, clearly);
    let inputs = world.sky().filter(|_| lit).map(|sky| {
        let minute = crate::daylight::sky_minute(
            settings.0.installed_client.clock(),
            eq_client_core::zones::number(world.zone()),
            world.game_time(std::time::Instant::now()),
        );
        interior_inputs(
            sky.time_type,
            minute,
            observer(world, names.as_deref()),
            scene.day.as_ref(),
        )
    });
    let carried = match (inputs, player.single()) {
        (Some(_), Ok((own, body))) => {
            let eye = eye
                .single()
                .map_or(own.translation, |camera| camera.translation);
            carried_lights(
                world,
                &nearby,
                eye,
                (own.translation, body.height),
                &transforms,
            )
        }
        _ => Vec::new(),
    };
    let scene = &mut *scene;
    if scene.worked.len() != scene.meshes.len() {
        let placed: Vec<_> = scene
            .placed
            .iter()
            .map(|light| PassLight {
                position: light.position,
                radius: light.radius,
                // The light's level scales its colour (inferred).
                rgb: light.rgb.map(|channel| channel * light.level),
            })
            .collect();
        scene.worked = scene
            .meshes
            .iter()
            .map(|lit| {
                let bounds = lit
                    .placed_at
                    .as_ref()
                    .and_then(|(positions, _)| bounds(positions));
                Worked {
                    bounds,
                    placed: bounds
                        .map_or_else(Vec::new, |bounds| assign(bounds, placed.iter().copied())),
                    ..Worked::default()
                }
            })
            .collect();
        scene.applied = None;
    }
    let colouring = inputs.map_or(Colouring::White, Colouring::Lit);
    let refresh = scene.applied != Some(colouring);
    for (lit, worked) in scene.meshes.iter().zip(&mut scene.worked) {
        if refresh {
            worked.base = match inputs {
                Some(inputs) if !lit.additive => lit
                    .light
                    .iter()
                    .map(|baked| zone_light::base_light(*baked, inputs))
                    .collect(),
                _ => Vec::new(),
            };
        }
        let assigned = match (worked.bounds, worked.base.is_empty()) {
            (Some(bounds), false) => assign(bounds, worked.placed.iter().chain(&carried).copied()),
            _ => Vec::new(),
        };
        if !refresh && assigned == worked.assigned {
            continue;
        }
        worked.assigned = assigned;
        if let Some(mut mesh) = meshes.get_mut(&lit.mesh) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours(lit, worked));
        }
    }
    scene.applied = Some(colouring);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing as session;
    use eq_client_core::{
        WorldEvent,
        clock::{Fog, ZoneSky},
    };
    use eq_network_game::appearance::AppearanceChange;

    fn colours_of(app: &App, mesh: &Handle<Mesh>) -> Vec<[f32; 4]> {
        match app
            .world()
            .resource::<Assets<Mesh>>()
            .get(mesh)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_COLOR)
        {
            Some(bevy::mesh::VertexAttributeValues::Float32x4(colours)) => colours.clone(),
            other => panic!("no vertex colours: {other:?}"),
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Drive the real scene through the ordered light transitions.
    fn the_scene_colours_vertices_by_baked_placed_and_carried_light() {
        let mut online = OnlineState::new(true);
        let mut own = session::player(7);
        own.appearance.light = 1;
        session::admit(&mut online, 5, own);
        session::news(
            &mut online,
            [WorldEvent::Sky(ZoneSky {
                sky: 0,
                time_type: 4,
                fog: [Fog {
                    color: [0; 3],
                    near: 0.0,
                    far: 0.0,
                }; 4],
            })],
        );
        let placed = PlacedLight {
            position: [0.0, 10.0, 0.0],
            radius: 20.0,
            rgb: [0.9, 0.3, 0.2],
            level: 1.0,
        };
        let mut scene = SceneLights::new(online.world().zone().to_owned(), vec![placed], None);
        let mut meshes = Assets::<Mesh>::default();
        let positions = vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 0.0, 100.0]];
        let mesh = meshes.add(Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        ));
        // A palace vertex: light of its own, no ambient weight.
        let baked = [100, 50, 0, 0];
        scene.meshes.push(LitMesh {
            mesh: mesh.clone(),
            light: vec![baked; 3],
            placed_at: Some((positions.clone(), vec![[0.0, 1.0, 0.0]; 3])),
            additive: false,
        });

        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(scene)
            .insert_resource(meshes)
            .insert_resource(ViewerSettings(crate::ViewerConfig::default()))
            .init_resource::<OptionsState>()
            .init_resource::<NearbyEntities>()
            .add_systems(Update, update);
        app.world_mut().spawn((
            Player,
            Transform::from_xyz(0.0, 0.0, 0.0),
            PlayerBody {
                feet_offset: 0.0,
                height: 6.0,
                size: 6.0,
            },
        ));
        app.world_mut().spawn(OrbitCamera {
            focus: Vec3::ZERO,
            radius: 120.0,
            yaw: 0.0,
            pitch: -0.5,
            pulled: None,
            view: default(),
            look: 0.0,
        });

        let inputs = interior_inputs(4, None, Vision::Ordinary, None);
        let base = zone_light::base_light(baked, inputs);
        let expect = |lights: &[PassLight], index: usize| {
            let mut light = base;
            for (slot, pass) in lights.iter().enumerate() {
                let add = zone_light::point_light(slot, pass, positions[index], [0.0, 1.0, 0.0]);
                light = std::array::from_fn(|channel| light[channel] + add[channel]);
            }
            zone_light::vertex_colour(light)
        };
        let placed_pass = PassLight {
            position: placed.position,
            radius: placed.radius,
            rgb: placed.rgb,
        };
        let torch = carried(1).unwrap().pass(Vec3::new(0.0, 6.0 * 0.65, 0.0));

        app.update();
        // The torch is nearer the triangle than the lamp: it comes first.
        let lit = colours_of(&app, &mesh);
        for (index, colour) in lit.iter().enumerate() {
            assert_eq!(*colour, expect(&[torch, placed_pass], index));
        }

        // Putting the torch away rewrites the colours with the lamp alone.
        session::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Appearance {
                spawn_id: 7,
                change: AppearanceChange::Light(255),
            }],
        );
        app.update();
        assert_eq!(colours_of(&app, &mesh)[0], expect(&[placed_pass], 0));

        // Seeing the world clearly draws the plain textures.
        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .qol
            .set(eq_client_core::qol::Fix::ClearView, true);
        app.update();
        assert_eq!(colours_of(&app, &mesh), vec![[1.0; 4]; 3]);

        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .qol
            .set(eq_client_core::qol::Fix::ClearView, false);
        app.update();
        assert_eq!(colours_of(&app, &mesh)[0], expect(&[placed_pass], 0));
        // A zone with a sky is drawn as before.
        session::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Sky(ZoneSky {
                sky: 1,
                time_type: 2,
                fog: [Fog {
                    color: [0; 3],
                    near: 0.0,
                    far: 0.0,
                }; 4],
            })],
        );
        app.update();
        assert_eq!(colours_of(&app, &mesh), vec![[1.0; 4]; 3]);
    }

    #[test]
    fn zone_surfaces_stay_unlit_and_fogged() {
        // The colours reach the frame through an unlit material that the
        // zone's distance fog still covers.
        let mut materials = Assets::<StandardMaterial>::default();
        let handle = crate::gpu_assets::create_material(
            eq_client_assets::MaterialMode::Opaque,
            None,
            &mut materials,
            true,
        );
        let material = materials.get(&handle).unwrap();
        assert!(material.unlit);
        assert!(material.fog_enabled);
        assert_eq!(material.base_color, Color::WHITE);
    }

    #[test]
    fn the_interior_ambient_follows_the_time_type_raised_to_the_floor() {
        // 0x441295: type 4 is 0.5 grey and the others without a table 0.1.
        assert_eq!(interior_ambient(4, None, Vision::Ordinary, None), [0.5; 3]);
        for time_type in [0, 3, 6, 255] {
            assert_eq!(
                interior_ambient(time_type, Some(0), Vision::Ordinary, None),
                [0.1; 3]
            );
        }
        // The floor is per channel: infravision lifts red above 0.1 but
        // leaves type 4's 0.5 alone.
        let infra = interior_ambient(0, None, Vision::Infra, None);
        assert_eq!(infra, [65.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0]);
        assert_eq!(interior_ambient(4, None, Vision::Ultra, None), [0.5; 3]);
    }

    #[test]
    fn time_types_one_two_and_five_follow_the_installed_day_table() {
        // A made-up table: row h is h/24 red, a fixed green and blue.
        let day: DayAmbient =
            std::array::from_fn(|hour| [f32::from(u8::try_from(hour).unwrap()) / 24.0, 0.2, 0.05]);
        for time_type in [1, 2, 5] {
            // Noon, and noon when the time isn't given.
            assert_eq!(
                zone_ambient(time_type, Some(12 * 60), Some(&day)),
                [0.5, 0.2, 0.05]
            );
            assert_eq!(zone_ambient(time_type, None, Some(&day)), [0.5, 0.2, 0.05]);
            // Raised to the floor per channel.
            assert_eq!(
                interior_ambient(time_type, Some(0), Vision::Ordinary, Some(&day)),
                [0.08, 0.2, 0.08]
            );
            // Half past six, halfway from the 6 AM row to the 7 AM one.
            let half = zone_ambient(time_type, Some(6 * 60 + 30), Some(&day));
            assert!((half[0] - 6.5 / 24.0).abs() < 1e-6);
            // 11:30 PM goes halfway toward 12 AM's row.
            let late = zone_ambient(time_type, Some(23 * 60 + 30), Some(&day));
            assert!((late[0] - 11.5 / 24.0).abs() < 1e-6);
            // Without the installed table, 0.1 grey.
            assert_eq!(zone_ambient(time_type, Some(12 * 60), None), [0.1; 3]);
        }
    }

    #[test]
    fn an_interior_puts_out_the_sun_and_scales_the_ambient() {
        let lit = |interior| {
            let mut lit = crate::daylight::lighting(
                &ZoneSky {
                    sky: 0,
                    time_type: 4,
                    fog: [Fog {
                        color: [0; 3],
                        near: 0.0,
                        far: 0.0,
                    }; 4],
                },
                None,
                &eq_client_core::daylight::PROVISIONAL_DAY_NIGHT,
            );
            adjust(&mut lit, Vision::Ordinary, interior, 0.0);
            lit
        };
        let palace = lit(Some([0.5; 3]));
        assert_eq!(palace.sun.0, 0.0);
        assert_eq!(palace.ambient, (75.0, [1.0; 3]));
        assert_eq!(lit(Some([0.1; 3])).ambient, (15.0, [1.0; 3]));
        assert!(lit(None).sun.0 > 0.0);
    }

    #[test]
    fn low_light_vision_comes_from_the_known_race_and_current_buffs() {
        let mut fields = ["0"; 183];
        fields[0] = "42";
        fields[16] = "7";
        fields[20] = "1";
        fields[70] = "100";
        fields[86] = "66";
        let names = SpellNames::parse(&fields.join("^"));
        let mut online = OnlineState::new(true);
        let mut own = session::player(7);
        own.race = 4;
        session::admit(&mut online, 5, own);
        assert_eq!(observer(online.world(), Some(&names)), Vision::Infra);
        let buff = eq_client_core::Buff {
            spell_id: 42,
            caster_level: 20,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks: 4,
            counters: 0,
            caster_id: 7,
        };
        session::news(
            &mut online,
            [WorldEvent::BuffSnapshot(vec![None, Some(buff)])],
        );
        assert_eq!(observer(online.world(), Some(&names)), Vision::Ultra);
        session::news(
            &mut online,
            [WorldEvent::Buff(eq_client_core::BuffUpdate {
                applies: eq_client_core::buffs::Applies::Received,
                record_type: 0,
                entity_id: 7,
                spell_id: 42,
                slot: 1,
                buff: None,
            })],
        );
        assert_eq!(observer(online.world(), Some(&names)), Vision::Infra);
    }
}
