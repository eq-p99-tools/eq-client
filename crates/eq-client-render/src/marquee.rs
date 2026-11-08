//! Transient, non-interactive banners received from the admitted zone stream.
use crate::{chat::ChatLook, loading::Loading, theme};
use bevy::{picking::Pickable, prelude::*};
use eq_client_core::Marquee;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const WAITING: usize = 4;
const MAX_WAIT: Duration = Duration::from_secs(10);
const MAX_HOLD: Duration = Duration::from_secs(60);
const MAX_FADE: Duration = Duration::from_secs(10);
const MAX_CHARS: usize = 512;

#[cfg(test)]
mod preview;

/// At most one visible banner and a short queue behind it.
#[derive(Default, Resource)]
pub(super) struct Banners {
    waiting: VecDeque<(Instant, Marquee)>,
    active: Option<(Instant, Marquee)>,
}

impl Banners {
    /// Keeps the newest waiting banners under load, without interrupting one
    /// already on screen.
    pub(super) fn receive(&mut self, banner: &Marquee, now: Instant) {
        if banner.priority == 0 || banner.duration.is_zero() || banner.text.trim().is_empty() {
            return;
        }
        if self.waiting.len() == WAITING {
            self.waiting.pop_front();
        }
        let text = banner.text.chars().take(MAX_CHARS).collect();
        self.waiting.push_back((
            now,
            Marquee {
                text,
                duration: banner.duration.min(MAX_HOLD),
                fade_in: banner.fade_in.min(MAX_FADE),
                fade_out: banner.fade_out.min(MAX_FADE),
                ..*banner
            },
        ));
    }

    /// Zoning, disconnecting or dying makes both visible and queued text stale.
    pub(super) fn clear(&mut self) {
        self.waiting.clear();
        self.active = None;
    }

    fn advance(&mut self, now: Instant, covered: bool) -> Option<(&Marquee, f32)> {
        self.waiting
            .retain(|(received, _)| now.duration_since(*received) <= MAX_WAIT);
        if self
            .active
            .as_ref()
            .is_some_and(|(since, banner)| now.duration_since(*since) >= lifetime(banner))
        {
            self.active = None;
        }
        if covered {
            return None;
        }
        if self.active.is_none() {
            self.active = self.waiting.pop_front().map(|(_, banner)| (now, banner));
        }
        self.active
            .as_ref()
            .map(|(since, banner)| (banner, opacity(banner, now.duration_since(*since))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banner(text: &str) -> Marquee {
        Marquee {
            message_type: 15,
            priority: 510,
            fade_in: Duration::from_secs(1),
            fade_out: Duration::from_secs(2),
            duration: Duration::from_secs(5),
            text: text.to_owned(),
        }
    }

    #[test]
    fn hold_begins_after_fade_in_and_precedes_fade_out() {
        let mut queue = Banners::default();
        let now = Instant::now();
        queue.receive(&banner("Synthetic"), now);
        for (millis, expected) in [(0, 0.0), (500, 0.5), (1000, 1.0), (6000, 1.0), (7000, 0.5)] {
            let (shown, alpha) = queue
                .advance(now + Duration::from_millis(millis), false)
                .unwrap();
            assert_eq!(shown.text, "Synthetic");
            assert!((alpha - expected).abs() < f32::EPSILON);
        }
        assert!(queue.advance(now + Duration::from_secs(8), false).is_none());
    }

    #[test]
    fn packed_opacity_does_not_choose_queue_order() {
        let mut faint = banner("Faint");
        faint.priority = 128;
        assert!((opacity(&faint, Duration::from_secs(2)) - 128.0 / 255.0).abs() < f32::EPSILON);
        let mut queue = Banners::default();
        let now = Instant::now();
        queue.receive(&faint, now);
        queue.receive(&banner("Bright"), now);
        assert_eq!(queue.advance(now, false).unwrap().0.text, "Faint");
    }

    #[test]
    fn a_flood_cannot_replace_the_current_banner_and_reset_clears_everything() {
        let mut queue = Banners::default();
        let now = Instant::now();
        queue.receive(&banner("First"), now);
        queue.advance(now, false);
        for index in 0..=WAITING {
            queue.receive(&banner(&index.to_string()), now);
        }
        assert_eq!(queue.waiting.len(), WAITING);
        assert_eq!(queue.advance(now, false).unwrap().0.text, "First");
        assert_eq!(
            queue
                .advance(now + Duration::from_secs(8), false)
                .unwrap()
                .0
                .text,
            "1",
        );
        queue.clear();
        assert!(queue.advance(now + Duration::from_secs(8), false).is_none());
    }

    #[test]
    fn stale_waiting_text_does_not_appear_when_loading_ends() {
        let mut queue = Banners::default();
        let now = Instant::now();
        queue.receive(&banner("Behind loading"), now);
        assert!(queue.advance(now, true).is_none());
        assert!(
            queue
                .advance(now + MAX_WAIT + Duration::from_secs(1), false)
                .is_none()
        );
    }
}

/// The native overlay API's hold argument follows two separate fades.
/// Apply that timing to the wire fields as an inference.
fn lifetime(banner: &Marquee) -> Duration {
    banner.fade_in + banner.duration + banner.fade_out
}

fn opacity(banner: &Marquee, elapsed: Duration) -> f32 {
    let ramp = |time: Duration, fade: Duration| {
        if fade.is_zero() {
            1.0
        } else {
            (time.as_secs_f32() / fade.as_secs_f32()).min(1.0)
        }
    };
    // EQEmu documents this field as packed opacity and extra blinks.
    // A positive multiple of 255 is fully opaque. Blink timing is unknown.
    let packed = banner
        .priority
        .checked_sub(1)
        .map_or(0, |bits| bits % 255 + 1);
    let peak = f32::from(u8::try_from(packed).expect("packed alpha is at most 255")) / 255.0;
    peak * ramp(elapsed, banner.fade_in).min(ramp(
        lifetime(banner).saturating_sub(elapsed),
        banner.fade_out,
    ))
}

#[derive(Component)]
pub(super) struct BannerNode;

/// Shows the active banner over the scene while leaving input to the normal UI.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(super) fn present(
    mut commands: Commands,
    mut banners: ResMut<Banners>,
    loading: Option<Res<Loading>>,
    chat: Option<Res<ChatLook>>,
    mut nodes: Query<(Entity, &mut Text, &mut TextColor, &mut BackgroundColor), With<BannerNode>>,
) {
    let shown = banners.advance(
        Instant::now(),
        loading.is_some_and(|loading| loading.covered()),
    );
    let Some((banner, alpha)) = shown else {
        for (entity, ..) in &nodes {
            commands.entity(entity).despawn();
        }
        return;
    };
    let color = chat
        .as_ref()
        .and_then(|chat| chat.message_color(banner.message_type))
        .map_or(theme::INK_WARM, |[r, g, b]| Color::srgb_u8(r, g, b));
    let ink = color.with_alpha(alpha);
    let ground = theme::SCRIM.with_alpha(alpha * 0.83);
    if let Ok((_, mut text, mut color, mut background)) = nodes.single_mut() {
        if text.0 != banner.text {
            text.0.clone_from(&banner.text);
        }
        color.0 = ink;
        background.0 = ground;
    } else {
        commands.spawn((
            BannerNode,
            theme::text(&banner.text, theme::Size::Display, ink),
            TextLayout::new(Justify::Center, LineBreak::WordOrCharacter),
            Node {
                position_type: PositionType::Absolute,
                top: percent(12),
                left: percent(18),
                right: percent(18),
                max_height: percent(38),
                padding: UiRect::axes(px(12), px(8)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(ground),
            GlobalZIndex(80),
            bevy::ui::FocusPolicy::Pass,
            Pickable::IGNORE,
        ));
    }
}
