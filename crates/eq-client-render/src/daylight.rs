//! Day and night: where a zone has a sky, the scene's brightness, the sun,
//! the sky's color and the fog follow the time in Norrath; below ground the
//! light stays as it is. Every zone's fog closes in from its header's
//! distances, in its color. Zones and models carry their own lighting and
//! draw unlit, so the night darkens the whole picture, as a camera's
//! exposure would. The hour the sky follows, when dawn and dusk come and
//! which zones keep one hour are the installed client's
//! ([`eq_client_assets::clock::ClientClock`]).
use super::OrbitCamera;
use bevy::{
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
    render::view::ColorGrading,
};
use eq_client_assets::clock::ClientClock;
use eq_client_core::{
    clock::{GameTime, ZoneSky},
    daylight::{DayNight, PROVISIONAL_DAY_NIGHT},
};

/// The light the time of day sets: the sun by day, the moon by night.
#[derive(Component)]
pub(super) struct Sun;

/// The sun's illuminance at noon, as the scene is lit without a time.
pub(super) const DAY_SUN: f32 = 12_000.0;
/// The moon's.
const NIGHT_SUN: f32 = 600.0;
/// The ambient brightness by day, as the scene is lit without a time.
const DAY_AMBIENT: f32 = 150.0;
/// And by night.
const NIGHT_AMBIENT: f32 = 40.0;
/// The ambient color by day, as the scene is lit without a time.
const DAY_AMBIENT_COLOR: [f32; 3] = [0.62, 0.68, 0.8];
const NIGHT_AMBIENT_COLOR: [f32; 3] = [0.25, 0.3, 0.5];
const MOONLIGHT: [f32; 3] = [0.6, 0.7, 1.0];
const SUNSET: [f32; 3] = [1.0, 0.6, 0.4];

fn mix(from: [f32; 3], to: [f32; 3], amount: f32) -> [f32; 3] {
    std::array::from_fn(|index| from[index] + (to[index] - from[index]) * amount)
}

fn color([red, green, blue]: [f32; 3]) -> Color {
    Color::srgb(red, green, blue)
}

/// How the scene is lit at a time in a zone: its exposure, color
/// temperature and saturation, the sun's illuminance and color, the ambient
/// brightness and color, the sky's color, and the fog's color and distances
/// when the zone has fog.
#[derive(Debug, PartialEq)]
pub(super) struct Lighting {
    pub exposure: f32,
    pub temperature: f32,
    pub saturation: f32,
    pub sun: (f32, [f32; 3]),
    pub ambient: (f32, [f32; 3]),
    pub sky: [f32; 3],
    pub fog: Option<([f32; 3], f32, f32)>,
}

/// The look of day and night for the installed client: the provisional
/// look, with the client's own dawn and dusk where they are known.
pub(super) fn day_night(clock: ClientClock) -> DayNight {
    clock
        .twilight
        .map_or(PROVISIONAL_DAY_NIGHT, |twilight| DayNight {
            dawn: twilight.dawn,
            dusk: twilight.dusk,
            ..PROVISIONAL_DAY_NIGHT
        })
}

/// The minute after 12 AM that a zone's sky follows: the hour the zone
/// keeps, if the client keeps one for it, or else the time as the client's
/// sky takes it; None while the time is not yet given.
pub(super) fn sky_minute(
    clock: ClientClock,
    zone: Option<u16>,
    time: Option<GameTime>,
) -> Option<u16> {
    zone.and_then(|zone| clock.sky_hour(zone))
        .map(|hour| u16::from(hour) * 60)
        .or_else(|| {
            time.map(|time| u16::from(clock.sky_hour_of(time.hour)) * 60 + u16::from(time.minute))
        })
}

/// How a zone is lit at a minute after the sky hour's 12 AM, by a look of day and
/// night; a zone without a sky keeps the day's light, and a time not yet
/// given counts as noon.
pub(super) fn lighting(sky: &ZoneSky, minute: Option<u16>, look: &DayNight) -> Lighting {
    let outdoors = sky.sky != 0;
    let (light, dusk) = match minute {
        Some(minute) if outdoors => (look.daylight(minute), look.twilight(minute)),
        _ => (1.0, 0.0),
    };
    let sky_color = if outdoors {
        mix(
            mix(look.night_sky, look.day_sky, light),
            look.twilight_sky,
            dusk * 0.6,
        )
    } else {
        [0.0; 3]
    };
    let fog = sky.fog[0];
    let fog_color = if fog.color == [0; 3] {
        sky_color
    } else {
        let zone = fog.color.map(|channel| f32::from(channel) / 255.0);
        if outdoors {
            mix(zone, sky_color, 1.0 - light)
        } else {
            zone
        }
    };
    let night = 1.0 - light;
    Lighting {
        exposure: look.night_exposure * night,
        temperature: look.night_temperature * night + look.twilight_temperature * dusk,
        saturation: 1.0 - (1.0 - look.night_saturation) * night,
        sun: (
            NIGHT_SUN + (DAY_SUN - NIGHT_SUN) * light,
            mix(mix(MOONLIGHT, [1.0; 3], light), SUNSET, dusk * 0.5),
        ),
        ambient: (
            NIGHT_AMBIENT + (DAY_AMBIENT - NIGHT_AMBIENT) * light,
            mix(NIGHT_AMBIENT_COLOR, DAY_AMBIENT_COLOR, light),
        ),
        sky: sky_color,
        fog: (fog.far > fog.near && fog.far > 0.0).then_some((fog_color, fog.near, fog.far)),
    }
}

/// The fog, closed in so the scene fades out before the Far Clip Plane cuts
/// it: the zone's own fog ends no farther than the clip, and a zone with no
/// fog gets one in the sky's colour over the last fifth once the clip is
/// nearer than the zone's far clip.
fn closed_in(
    fog: Option<([f32; 3], f32, f32)>,
    clip: Option<f32>,
    far_clip: Option<f32>,
    sky: [f32; 3],
) -> Option<([f32; 3], f32, f32)> {
    let Some(clip) = clip else {
        return fog;
    };
    match fog {
        Some((tint, start, end)) => Some((tint, start.min(clip * 0.8), end.min(clip))),
        None if far_clip.is_some_and(|far| clip < far) => Some((sky, clip * 0.8, clip)),
        None => None,
    }
}

/// The scene's camera, with the fog and grading the time of day sets.
type Cameras<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Option<&'static mut DistanceFog>,
        Option<&'static mut ColorGrading>,
    ),
    With<OrbitCamera>,
>;

/// The ambient light the scene has at launch, before any zone has said how
/// it is lit.
pub(super) fn launch_ambient() -> GlobalAmbientLight {
    let [red, green, blue] = DAY_AMBIENT_COLOR;
    GlobalAmbientLight {
        color: Color::srgb(red, green, blue),
        brightness: DAY_AMBIENT,
        affects_lightmapped_meshes: true,
    }
}

/// Lights the scene for the zone and the time of day.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    online: Res<super::online::OnlineState>,
    options: Res<super::options::OptionsState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: Option<ResMut<ClearColor>>,
    mut suns: Query<&mut DirectionalLight, With<Sun>>,
    mut cameras: Cameras,
    mut lit: Local<bool>,
    settings: Res<crate::ViewerSettings>,
) {
    let world = online.world();
    // Offline, or before the zone has said, the scene stays as it was lit.
    // Leaving the world, as a camp back to character select does, forgets
    // the zone's sky: the scene goes back to the light it had at launch, so
    // the character shown at select (item 6.256) stands in the same light
    // after a camp as after a fresh login, not in the last zone's.
    let Some(sky) = world.sky() else {
        if std::mem::take(&mut *lit) {
            for mut sun in &mut suns {
                sun.illuminance = DAY_SUN;
                sun.color = DirectionalLight::default().color;
            }
            *ambient = launch_ambient();
            if let Some(clear) = clear.as_mut() {
                clear.0 = ClearColor::default().0;
            }
            for (camera, fog, grading) in &mut cameras {
                if fog.is_some() {
                    commands.entity(camera).remove::<DistanceFog>();
                }
                if let Some(mut grading) = grading {
                    *grading = ColorGrading::default();
                }
            }
        }
        return;
    };
    *lit = true;
    let clock = settings.0.installed_client.clock();
    let minute = sky_minute(
        clock,
        eq_client_core::zones::number(world.zone()),
        world.game_time(std::time::Instant::now()),
    );
    let mut lit = lighting(&sky, minute, &day_night(clock));
    lit.fog = closed_in(
        lit.fog,
        options.options.clip_distance(world.far_clip()),
        world.far_clip(),
        lit.sky,
    );
    for mut sun in &mut suns {
        sun.illuminance = lit.sun.0;
        sun.color = color(lit.sun.1);
    }
    ambient.brightness = lit.ambient.0;
    ambient.color = color(lit.ambient.1);
    if let Some(mut clear) = clear {
        clear.0 = color(lit.sky);
    }
    for (camera, fog, grading) in &mut cameras {
        let grade = |grading: &mut ColorGrading| {
            grading.global.exposure = lit.exposure;
            grading.global.temperature = lit.temperature;
            grading.global.post_saturation = lit.saturation;
        };
        if let Some(mut grading) = grading {
            grade(&mut grading);
        } else {
            let mut grading = ColorGrading::default();
            grade(&mut grading);
            commands.entity(camera).insert(grading);
        }
        match (lit.fog, fog) {
            (Some((tint, start, end)), Some(mut fog)) => {
                fog.color = color(tint);
                fog.falloff = FogFalloff::Linear { start, end };
            }
            (Some((tint, start, end)), None) => {
                commands.entity(camera).insert(DistanceFog {
                    color: color(tint),
                    directional_light_color: Color::NONE,
                    directional_light_exponent: 8.0,
                    falloff: FogFalloff::Linear { start, end },
                });
            }
            (None, Some(_)) => {
                commands.entity(camera).remove::<DistanceFog>();
            }
            (None, None) => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_the_world_puts_back_the_launch_lighting() {
        let mut app = App::new();
        app.insert_resource(crate::online::OnlineState::new(true))
            .init_resource::<crate::options::OptionsState>()
            .insert_resource(launch_ambient())
            .init_resource::<ClearColor>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .add_systems(Update, update);
        let sun = app
            .world_mut()
            .spawn((
                Sun,
                DirectionalLight {
                    illuminance: DAY_SUN,
                    ..default()
                },
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn(OrbitCamera {
                focus: Vec3::ZERO,
                radius: 120.0,
                yaw: 0.0,
                pitch: -0.5,
                pulled: None,
            })
            .id();
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            [
                eq_client_core::WorldEvent::Sky(zone(1, [120, 140, 160])),
                eq_client_core::WorldEvent::TimeOfDay(at(0)),
            ],
        );
        app.update();
        // A zone at midnight: its night sky, fog and moonlight.
        assert_ne!(
            app.world().resource::<ClearColor>().0,
            ClearColor::default().0
        );
        assert!(app.world().get::<DistanceFog>(camera).is_some());
        assert!(
            (app.world().resource::<GlobalAmbientLight>().brightness - DAY_AMBIENT).abs() > 1.0
        );
        // A camp forgets the zone's sky.
        crate::online::testing::set_world(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::world::ClientWorld::default(),
        );
        app.update();
        assert_eq!(
            app.world().resource::<ClearColor>().0,
            ClearColor::default().0
        );
        assert!(app.world().get::<DistanceFog>(camera).is_none());
        assert_eq!(
            app.world()
                .get::<ColorGrading>(camera)
                .map(|grading| grading.global.exposure),
            Some(ColorGrading::default().global.exposure)
        );
        let ambient = app.world().resource::<GlobalAmbientLight>();
        assert_eq!(
            (ambient.color, ambient.brightness),
            (launch_ambient().color, launch_ambient().brightness)
        );
        let light = app.world().get::<DirectionalLight>(sun).unwrap();
        assert_eq!(
            (light.illuminance, light.color),
            (DAY_SUN, DirectionalLight::default().color)
        );
    }

    #[test]
    fn the_fog_closes_in_with_the_clip_plane() {
        let zone_fog = Some(([0.5; 3], 300.0, 900.0));
        // At the zone's own clip, its fog stays as it is.
        assert_eq!(
            closed_in(zone_fog, Some(2400.0), Some(2400.0), [0.0; 3]),
            zone_fog
        );
        // Nearer, it ends at the clip.
        assert_eq!(
            closed_in(zone_fog, Some(600.0), Some(2400.0), [0.0; 3]),
            Some(([0.5; 3], 300.0, 600.0))
        );
        // A zone without fog gets the sky's once the clip comes in.
        assert_eq!(closed_in(None, Some(2400.0), Some(2400.0), [0.2; 3]), None);
        assert_eq!(
            closed_in(None, Some(1000.0), Some(2400.0), [0.2; 3]),
            Some(([0.2; 3], 800.0, 1000.0))
        );
        // Without a clip, nothing changes.
        assert_eq!(closed_in(zone_fog, None, None, [0.0; 3]), zone_fog);
    }
    use eq_client_core::clock::Fog;

    fn zone(sky: u8, fog: [u8; 3]) -> ZoneSky {
        ZoneSky {
            sky,
            time_type: 2,
            fog: [Fog {
                color: fog,
                near: 50.0,
                far: 600.0,
            }; 4],
        }
    }

    /// A minute after the sky hour's 12 AM, on the hour.
    #[allow(clippy::unnecessary_wraps)] // Stands for a time that is given.
    fn shown(hour: u16) -> Option<u16> {
        Some(hour * 60)
    }

    #[test]
    fn the_sky_follows_the_wire_hour_and_keeps_the_zones_hours() {
        use eq_client_assets::clock::{EQMAC, TITANIUM};
        // Wire hour 13, the calendar's 12: both skies are at 1 PM.
        let time = Some(GameTime {
            minute: 30,
            ..at(12)
        });
        for clock in [EQMAC, TITANIUM] {
            assert_eq!(sky_minute(clock, Some(1), time), Some(13 * 60 + 30));
            // Zone 71 (`airplane`) keeps 6 PM, given a time or not.
            assert_eq!(sky_minute(clock, Some(71), time), Some(18 * 60));
            assert_eq!(sky_minute(clock, Some(71), None), Some(18 * 60));
            assert_eq!(sky_minute(clock, None, None), None);
        }
        // Both clients' dawn runs 3:00 to 9:00 and dusk 15:00 to 21:00.
        assert_eq!(day_night(TITANIUM), day_night(EQMAC));
        let mac = day_night(EQMAC);
        assert!(mac.daylight(2 * 60 + 59).abs() < f32::EPSILON);
        assert!((mac.daylight(6 * 60) - 0.5).abs() < f32::EPSILON);
        assert!((mac.daylight(9 * 60) - 1.0).abs() < f32::EPSILON);
        assert!((mac.daylight(18 * 60) - 0.5).abs() < f32::EPSILON);
        assert!(mac.daylight(21 * 60).abs() < f32::EPSILON);
        assert!((mac.night_exposure - PROVISIONAL_DAY_NIGHT.night_exposure).abs() < f32::EPSILON);
    }

    fn at(hour: u8) -> GameTime {
        GameTime {
            hour,
            minute: 0,
            day: 1,
            month: 1,
            year: 3100,
        }
    }

    #[test]
    fn noon_is_bright_midnight_dark_and_dungeons_keep_their_light() {
        let outside = zone(1, [120, 140, 160]);
        let noon = lighting(&outside, shown(12), &PROVISIONAL_DAY_NIGHT);
        let midnight = lighting(&outside, shown(0), &PROVISIONAL_DAY_NIGHT);
        assert!((noon.sun.0 - DAY_SUN).abs() < f32::EPSILON);
        assert!(noon.exposure.abs() < f32::EPSILON);
        assert!((midnight.exposure - PROVISIONAL_DAY_NIGHT.night_exposure).abs() < f32::EPSILON);
        assert!(midnight.saturation < noon.saturation);
        assert!(midnight.sun.0 < noon.sun.0 / 10.0);
        assert!(midnight.ambient.0 < noon.ambient.0);
        assert!(midnight.sky.iter().sum::<f32>() < noon.sky.iter().sum::<f32>() / 5.0);
        // The zone's fog by day, the night sky's by night.
        let (day_fog, start, end) = noon.fog.unwrap();
        assert!((day_fog[0] - 120.0 / 255.0).abs() < 1e-6);
        assert_eq!((start, end), (50.0, 600.0));
        let night_fog = midnight.fog.unwrap().0;
        assert!(
            night_fog
                .iter()
                .zip(midnight.sky)
                .all(|(fog, sky)| (fog - sky).abs() < 1e-6)
        );
        // No time yet counts as noon; below ground, every hour is the same.
        assert_eq!(lighting(&outside, None, &PROVISIONAL_DAY_NIGHT), noon);
        let below = zone(0, [0, 0, 0]);
        assert_eq!(
            lighting(&below, shown(0), &PROVISIONAL_DAY_NIGHT),
            lighting(&below, shown(12), &PROVISIONAL_DAY_NIGHT)
        );
        assert_eq!(
            lighting(&below, shown(0), &PROVISIONAL_DAY_NIGHT).sky,
            [0.0; 3]
        );
        // No fog distances, no fog.
        let mut clear = outside;
        clear.fog[0].far = 0.0;
        assert_eq!(
            lighting(&clear, shown(12), &PROVISIONAL_DAY_NIGHT).fog,
            None
        );
    }
}
