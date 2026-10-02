//! Day and night: where a zone has a sky, the scene's brightness, the sun,
//! the sky's color and the fog follow the time in Norrath; below ground the
//! light stays as it is. Every zone's fog closes in from its header's
//! distances, in its color. Zones and models carry their own lighting and
//! draw unlit, so the night darkens the whole picture, as a camera's
//! exposure would.
use super::OrbitCamera;
use bevy::{
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
    render::view::ColorGrading,
};
use eq_client_core::{
    clock::{GameTime, ZoneSky},
    daylight::PROVISIONAL_DAY_NIGHT,
};

/// The light the time of day sets: the sun by day, the moon by night.
#[derive(Component)]
pub(super) struct Sun;

/// The sun's illuminance at noon, as the scene is lit without a time.
pub(super) const DAY_SUN: f32 = 12_000.0;
/// The moon's.
const NIGHT_SUN: f32 = 600.0;
/// The ambient brightness by day, as the scene is lit without a time.
pub(super) const DAY_AMBIENT: f32 = 150.0;
/// And by night.
const NIGHT_AMBIENT: f32 = 40.0;
/// The ambient color by day, as the scene is lit without a time.
pub(super) const DAY_AMBIENT_COLOR: [f32; 3] = [0.62, 0.68, 0.8];
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

/// How a zone is lit at a time, by the provisional look of day and night; a
/// zone without a sky keeps the day's light, and a time not yet given
/// counts as noon.
pub(super) fn lighting(sky: &ZoneSky, time: Option<GameTime>) -> Lighting {
    let look = PROVISIONAL_DAY_NIGHT;
    let outdoors = sky.sky != 0;
    let (light, dusk) = match time {
        Some(time) if outdoors => (look.daylight(time), look.twilight(time)),
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

/// Lights the scene for the zone and the time of day.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    mut commands: Commands,
    online: Res<super::online::OnlineState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    clear: Option<ResMut<ClearColor>>,
    mut suns: Query<&mut DirectionalLight, With<Sun>>,
    mut cameras: Cameras,
) {
    let world = online.world();
    // Offline, or before the zone has said, the scene stays as it was lit.
    let Some(sky) = world.sky() else {
        return;
    };
    let lit = lighting(&sky, world.game_time(std::time::Instant::now()));
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
        let noon = lighting(&outside, Some(at(12)));
        let midnight = lighting(&outside, Some(at(0)));
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
        assert_eq!(lighting(&outside, None), noon);
        let below = zone(0, [0, 0, 0]);
        assert_eq!(
            lighting(&below, Some(at(0))),
            lighting(&below, Some(at(12)))
        );
        assert_eq!(lighting(&below, Some(at(0))).sky, [0.0; 3]);
        // No fog distances, no fog.
        let mut clear = outside;
        clear.fog[0].far = 0.0;
        assert_eq!(lighting(&clear, Some(at(12))).fog, None);
    }
}
