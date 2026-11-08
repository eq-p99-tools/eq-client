//! Titanium light definitions and a bounded Bevy presentation of classic interiors.
use super::{
    OrbitCamera, Player, PlayerBody, ViewerSettings, daylight::Lighting, entities::NearbyEntities,
    online::OnlineState, options::OptionsState, spellbook::SpellNames,
};
use bevy::prelude::*;
use eq_client_assets::{
    lighting::PlacedLight,
    spells::{BaseDuration, Mechanics},
    ui::InstalledClient,
};
use eq_client_core::{render_position, world::ClientWorld};

const MOST_PLACED: usize = 16;
const MOST_CARRIED: usize = 8;

#[derive(Resource)]
pub(super) struct SceneLights {
    zone: String,
    placed: Vec<PlacedLight>,
    pub(super) surfaces: Vec<Handle<StandardMaterial>>,
}

impl SceneLights {
    pub(super) fn new(zone: String, placed: Vec<PlacedLight>) -> Self {
        Self {
            zone,
            placed,
            surfaces: Vec::new(),
        }
    }
}

/// Selects the no-sky classic presentation when the matching installation
/// supplied usable point lights. No sky by itself is insufficient.
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
        && scene.is_some_and(|scene| scene.zone == world.zone() && !scene.placed.is_empty())
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

/// Native RGB ambient floor constants from eqgame.exe 0x4917dd; the
/// ordinary no-sky baseline uses the 0.1 branch at 0x441295.
fn floor(vision: Vision) -> [f32; 3] {
    match vision {
        Vision::Ordinary => [0.1; 3],
        Vision::Infra => [65.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0],
        Vision::Ultra => [80.0 / 255.0, 80.0 / 255.0, 120.0 / 255.0],
    }
}

/// Maps the native floors onto this renderer's ambient/exposure controls.
pub(super) fn adjust(lit: &mut Lighting, vision: Vision, interior: bool, darkness: f32) {
    let rgb = floor(vision);
    let peak = rgb.into_iter().fold(0.0_f32, f32::max);
    if interior {
        lit.sun.0 = 0.0;
        lit.ambient = (150.0 * peak, rgb.map(|channel| channel / peak));
    } else if vision != Vision::Ordinary {
        // Outdoor terrain uses the existing time-of-day grade. Preserve the
        // sky/fog interpolation and apply only a bounded dark-time lift.
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
    fn point(self) -> PointLight {
        let [red, green, blue] = self.rgb;
        PointLight {
            color: Color::linear_rgb(red, green, blue),
            intensity: self.range * self.range * 30.0 * self.level,
            range: self.range,
            radius: self.range * 0.08,
            ..default()
        }
    }
}

#[derive(Default)]
pub(super) struct ActiveLights {
    lit: bool,
    entities: Vec<Entity>,
}

type Positions<'w, 's> = (
    Query<'w, 's, &'static Transform, (With<OrbitCamera>, Without<Player>, Without<PointLight>)>,
    Query<'w, 's, (&'static Transform, &'static PlayerBody), (With<Player>, Without<PointLight>)>,
    Query<'w, 's, &'static Transform, (Without<OrbitCamera>, Without<Player>, Without<PointLight>)>,
);
type Lights<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut PointLight),
    (Without<Player>, Without<OrbitCamera>),
>;

/// Reuses light entities, and switches only the active zone's terrain and
/// object material handles. Observer visibility remains a separate input.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
pub(super) fn update(
    mut commands: Commands,
    online: Res<OnlineState>,
    settings: Res<ViewerSettings>,
    options: Res<OptionsState>,
    scene: Option<Res<SceneLights>>,
    nearby: Res<NearbyEntities>,
    (camera, player, transforms): Positions,
    mut points: Lights,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut active: Local<ActiveLights>,
) {
    let world = online.world();
    let clearly = options.options.qol.on(eq_client_core::qol::Fix::ClearView);
    let lit = interior(scene.as_deref(), world, &settings, clearly);
    if let Some(scene) = scene
        .as_ref()
        .filter(|scene| scene.is_changed() || active.lit != lit)
    {
        for handle in &scene.surfaces {
            if let Some(mut material) = materials.get_mut(handle)
                && material.alpha_mode != AlphaMode::Add
            {
                material.unlit = !lit;
            }
        }
    }
    active.lit = lit;
    let mut wanted = Vec::new();
    if let (Some(scene), Some(own), Ok((position, body))) = (
        scene.as_deref().filter(|_| lit),
        world.player(),
        player.single(),
    ) {
        let eye = camera
            .single()
            .map_or(position.translation, |camera| camera.translation);
        let mut placed: Vec<_> = scene
            .placed
            .iter()
            .filter_map(|source| {
                let point = Vec3::from_array(source.position);
                let distance = eye.distance(point) - source.radius;
                (distance <= 100.0).then_some((
                    distance,
                    point,
                    Lamp {
                        range: source.radius,
                        rgb: source.rgb,
                        level: source.level,
                    },
                ))
            })
            .collect();
        placed.sort_by(|a, b| a.0.total_cmp(&b.0));
        wanted.extend(
            placed
                .into_iter()
                .take(MOST_PLACED)
                .map(|(_, point, lamp)| (point, lamp)),
        );
        if !world.is_dead() {
            let inventory = world.inventory();
            let light = if inventory.received() {
                inventory.titanium_carried_light()
            } else {
                Some(own.appearance.light)
            };
            if let Some(lamp) = light.and_then(carried) {
                wanted.push((position.translation + Vec3::Y * body.height * 0.65, lamp));
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
                    let distance = eye.distance(point) - lamp.range;
                    (distance <= 100.0).then_some((distance, point, lamp))
                })
                .collect();
            others.sort_by(|a, b| a.0.total_cmp(&b.0));
            wanted.extend(
                others
                    .into_iter()
                    .take(MOST_CARRIED)
                    .map(|(_, point, lamp)| (point, lamp)),
            );
        }
    }
    let kept = wanted.len().min(active.entities.len());
    for entity in active.entities.drain(kept..) {
        commands.entity(entity).despawn();
    }
    for (index, (point, lamp)) in wanted.into_iter().enumerate() {
        let pose = Transform::from_translation(point);
        let light = lamp.point();
        if let Some(entity) = active.entities.get(index).copied()
            && let Ok((mut old_pose, mut old_light)) = points.get_mut(entity)
        {
            if old_pose.translation != point {
                *old_pose = pose;
            }
            if old_light.range.to_bits() != light.range.to_bits()
                || old_light.intensity.to_bits() != light.intensity.to_bits()
                || old_light.color != light.color
            {
                *old_light = light;
            }
        } else {
            let entity = commands.spawn((pose, light)).id();
            if let Some(slot) = active.entities.get_mut(index) {
                *slot = entity;
            } else {
                active.entities.push(entity);
            }
        }
    }
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

    #[test]
    #[allow(clippy::too_many_lines)] // Drive the real scene through the ordered light transitions.
    fn the_scene_tracks_placed_and_carried_lights_and_clear_view() {
        let mut online = OnlineState::new(true);
        let mut own = session::player(7);
        own.appearance.light = 1;
        session::admit(&mut online, 5, own);
        session::news(
            &mut online,
            [WorldEvent::Sky(ZoneSky {
                sky: 0,
                time_type: 0,
                fog: [Fog {
                    color: [0; 3],
                    near: 0.0,
                    far: 0.0,
                }; 4],
            })],
        );
        let mut scene = SceneLights::new(
            online.world().zone().to_owned(),
            vec![PlacedLight {
                position: [8.0, 4.0, 12.0],
                radius: 40.0,
                rgb: [0.9, 0.3, 0.2],
                level: 1.0,
            }],
        );
        let mut materials = Assets::<StandardMaterial>::default();
        let surface = materials.add(StandardMaterial {
            unlit: true,
            ..default()
        });
        scene.surfaces.push(surface.clone());

        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(scene)
            .insert_resource(materials)
            .insert_resource(ViewerSettings(crate::ViewerConfig::default()))
            .init_resource::<OptionsState>()
            .init_resource::<NearbyEntities>()
            .add_systems(Update, update);
        app.world_mut().spawn((
            Player,
            Transform::IDENTITY,
            PlayerBody {
                feet_offset: 0.0,
                height: 6.0,
                size: 6.0,
            },
        ));

        let ranges = |app: &mut App| {
            let world = app.world_mut();
            let mut ranges = world
                .query::<&PointLight>()
                .iter(world)
                .map(|light| light.range.to_bits())
                .collect::<Vec<_>>();
            ranges.sort_unstable();
            ranges
        };
        app.update();
        assert_eq!(ranges(&mut app), [25.0_f32.to_bits(), 40.0_f32.to_bits()]);
        assert!(
            !app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&surface)
                .unwrap()
                .unlit
        );

        for (kind, expected) in [
            (13, vec![40.0_f32.to_bits(), 150.0_f32.to_bits()]),
            (255, vec![40.0_f32.to_bits()]),
        ] {
            session::news(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                [WorldEvent::Appearance {
                    spawn_id: 7,
                    change: AppearanceChange::Light(kind),
                }],
            );
            app.update();
            assert_eq!(ranges(&mut app), expected);
        }
        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .qol
            .set(eq_client_core::qol::Fix::ClearView, true);
        app.update();
        assert_eq!(ranges(&mut app), [] as [u32; 0]);
        assert!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&surface)
                .unwrap()
                .unlit
        );

        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .qol
            .set(eq_client_core::qol::Fix::ClearView, false);
        session::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            6,
            session::player(7),
        );
        app.update();
        assert_eq!(ranges(&mut app), [] as [u32; 0]);
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
                entity_id: 7,
                spell_id: 42,
                slot: 1,
                buff: None,
            })],
        );
        assert_eq!(observer(online.world(), Some(&names)), Vision::Infra);
    }
}
