//! The official client's look for each skinned window's background, as the
//! character last saved it: how opaque it is, the skin's texture in a tint
//! or a flat colour instead, and how it fades while the pointer is away.
//! Read only through `ViewerConfig::official_settings`, so a client
//! generation without a checked reader keeps the skin's look, and nothing is
//! written back.
use super::WindowId;
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_assets::ui::WindowLook;
use std::{collections::BTreeMap, time::Duration};

/// Each window's background as the character saved it.
#[derive(Resource, Default)]
pub(crate) struct Looks {
    /// The character and world they were read for.
    read_for: Option<(String, String)>,
    looks: BTreeMap<WindowId, WindowLook>,
}

/// A skinned window's background, which its look tints and fades.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Backdrop(pub(crate) WindowId);

/// Reads the character's window looks again when the character or world
/// changes, and only then.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn load(
    online: Res<crate::online::OnlineState>,
    settings: Res<crate::ViewerSettings>,
    mut looks: ResMut<Looks>,
) {
    let current = online
        .world()
        .player()
        .map(|player| player.name.as_str())
        .zip(online.world().world_name());
    if looks
        .read_for
        .as_ref()
        .map(|(character, world)| (character.as_str(), world.as_str()))
        == current
    {
        return;
    }
    looks.read_for = current.map(|(character, world)| (character.to_owned(), world.to_owned()));
    let saved = current
        .zip(settings.0.official_settings())
        .map(|((character, world), official)| official.window_looks(character, world))
        .unwrap_or_default();
    looks.looks = WindowId::ALL
        .into_iter()
        .chain(WindowId::bags())
        .filter_map(|id| {
            let section = id.section()?;
            let look = saved
                .iter()
                .find(|look| look.window.eq_ignore_ascii_case(&section))?;
            Some((id, look.clone()))
        })
        .collect();
}

/// The opacity a look gives its window's background: its own while the
/// pointer is on the window; where it fades, the opacity it fades to,
/// reached over its duration once the pointer has been away its delay.
fn opacity(look: &WindowLook, away: Option<Duration>) -> u8 {
    let Some(away) = away.filter(|_| look.fades) else {
        return look.alpha;
    };
    let Some(fading) = away.checked_sub(Duration::from_millis(look.delay_ms.into())) else {
        return look.alpha;
    };
    let duration = Duration::from_millis(look.duration_ms.into());
    let done = if duration.is_zero() {
        1.0
    } else {
        (fading.as_secs_f32() / duration.as_secs_f32()).min(1.0)
    };
    let (from, to) = (f32::from(look.alpha), f32::from(look.fade_to_alpha));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Between two bytes.
    let alpha = (from + (to - from) * done).round().clamp(0.0, 255.0) as u8;
    alpha
}

/// Draws each skinned window's background as its look says, faded while the
/// pointer is away from the window; a window without one keeps the skin's.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn dress(
    (time, looks): (Res<Time<Real>>, Res<Looks>),
    windows: Query<&Window, With<PrimaryWindow>>,
    frames: Query<(&WindowId, &UiGlobalTransform, &ComputedNode), With<crate::windows::Frame>>,
    mut backdrops: Query<(&Backdrop, Option<&mut ImageNode>, &mut BackgroundColor)>,
    mut left: Local<BTreeMap<WindowId, Duration>>,
) {
    let now = time.elapsed();
    let cursor = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position);
    for (Backdrop(id), image, mut background) in &mut backdrops {
        let (texture, fill) = match looks.looks.get(id) {
            Some(look) => {
                let over = cursor.is_some_and(|cursor| {
                    frames.iter().any(|(frame, transform, node)| {
                        frame == id && crate::windows::contains(cursor, transform, node)
                    })
                });
                // Since when the pointer has been away from the window.
                let away = if over {
                    left.remove(id);
                    None
                } else {
                    Some(now.saturating_sub(*left.entry(*id).or_insert(now)))
                };
                let [red, green, blue] = look.tint;
                let tinted = Color::srgba_u8(red, green, blue, opacity(look, away));
                if look.flat {
                    (Color::NONE, tinted)
                } else {
                    (tinted, Color::NONE)
                }
            }
            None => (Color::WHITE, Color::NONE),
        };
        if let Some(mut image) = image
            && image.color != texture
        {
            image.color = texture;
        }
        if background.0 != fill {
            background.0 = fill;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(alpha: u8, fade_to_alpha: u8) -> WindowLook {
        WindowLook {
            window: "MainChat".into(),
            alpha,
            fades: true,
            fade_to_alpha,
            delay_ms: 2000,
            duration_ms: 500,
            flat: true,
            tint: [0, 0, 0],
        }
    }

    #[test]
    fn a_background_fades_once_the_pointer_has_been_away_its_delay() {
        let look = look(255, 55);
        let away = |ms| Some(Duration::from_millis(ms));
        // In use, and for its delay after, it keeps its own opacity.
        assert_eq!(opacity(&look, None), 255);
        assert_eq!(opacity(&look, away(1999)), 255);
        // Halfway through the fade, halfway there; then there.
        assert_eq!(opacity(&look, away(2250)), 155);
        assert_eq!(opacity(&look, away(9000)), 55);
        // A window that does not fade keeps its opacity.
        let still = WindowLook {
            fades: false,
            ..look
        };
        assert_eq!(opacity(&still, away(9000)), 255);
    }
}
