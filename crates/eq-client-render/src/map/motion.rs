//! Live player placement and travel indicators, separate from facing and map fitting.
use super::{entities::Marker, map_point};
use crate::online::OnlineState;
use bevy::prelude::*;
use eq_client_core::{GameDialect, SpawnKind, WorldPosition, world::ClientWorld};
use std::time::{Duration, Instant};

/// A short hold avoids flicker between local interpolation steps.
const LOCAL_HOLD: Duration = Duration::from_millis(150);
/// A lost or quiet connection must not imply movement indefinitely.
const REPORT_FRESHNESS: Duration = Duration::from_secs(5);

/// The player's actual displayed pose, sampled after movement interpolation.
#[derive(Resource, Default)]
pub(crate) struct MapMotion {
    admission: Option<u64>,
    previous: Option<(Entity, WorldPosition)>,
    direction: Option<Vec2>,
    moved: Option<Instant>,
}

impl MapMotion {
    /// The rendered pose when available, otherwise the latest session position.
    pub(super) fn player(&self, world: &ClientWorld) -> Option<WorldPosition> {
        world.player().map(|player| {
            self.previous
                .filter(|_| self.admission == world.session_id())
                .map_or(player.position, |(_, position)| position)
        })
    }

    fn observe(
        &mut self,
        admission: Option<u64>,
        pose: Option<(Entity, WorldPosition)>,
        now: Instant,
    ) {
        if self.admission != admission || self.previous.map(|p| p.0) != pose.map(|p| p.0) {
            *self = Self {
                admission,
                previous: pose,
                ..default()
            };
            return;
        }
        if let (Some((_, before)), Some((_, after))) = (self.previous, pose) {
            let delta = map_point(after) - map_point(before);
            // Corrections and teleports should not be presented as walking.
            if delta.length_squared() > 64.0 * 64.0 {
                self.direction = None;
                self.moved = None;
            } else if let Some(direction) = unit(delta) {
                self.direction = Some(direction);
                self.moved = Some(now);
            }
        }
        if self
            .moved
            .is_none_or(|at| now.saturating_duration_since(at) > LOCAL_HOLD)
        {
            self.direction = None;
        }
        self.previous = pose;
    }
}

/// Follows the local model every frame, without changing the authoritative world.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn sample(
    online: Res<OnlineState>,
    scene: Option<Res<crate::SceneInfo>>,
    players: Query<(Entity, &Transform), With<crate::Player>>,
    mut motion: ResMut<MapMotion>,
) {
    let world = online.world();
    let current = world.connected()
        && !world.is_dead()
        && !online.scene_loading()
        && scene.is_some_and(|scene| scene.zone_name == world.zone());
    let pose = current
        .then(|| players.single().ok())
        .flatten()
        .and_then(|(entity, transform)| {
            let facing = transform.rotation * Vec3::Z;
            let heading = eq_client_core::world_heading(facing.x.atan2(facing.z));
            let position =
                eq_client_core::world_position(transform.translation.to_array(), heading);
            [position.x, position.y, position.z, position.heading]
                .into_iter()
                .all(f32::is_finite)
                .then_some((entity, position))
        });
    motion.observe(world.session_id(), pose, Instant::now());
}

/// A travel indicator follows its marker's position and clipping.
#[derive(Component)]
pub(crate) struct Travel;

const LINE_LENGTH: f32 = 10.0;
const LINE_WIDTH: f32 = 3.0;
const DOT_SIZE: f32 = 5.0;

pub(super) fn indicator() -> impl Bundle {
    (
        Travel,
        BackgroundColor(Color::BLACK),
        BorderColor::all(crate::theme::INK),
        UiTransform::default(),
        bevy::ui::FocusPolicy::Pass,
        Node {
            position_type: PositionType::Absolute,
            width: px(LINE_WIDTH),
            height: px(LINE_LENGTH),
            border: UiRect::horizontal(px(1)),
            display: Display::None,
            ..default()
        },
        children![(
            BackgroundColor(Color::NONE),
            bevy::ui::FocusPolicy::Pass,
            Node {
                position_type: PositionType::Absolute,
                top: px(-1),
                left: px((LINE_WIDTH - DOT_SIZE) / 2.0 - 1.0),
                width: px(DOT_SIZE),
                height: px(DOT_SIZE),
                border_radius: BorderRadius::all(percent(50)),
                ..default()
            },
        )],
    )
}

/// Reuse the dialect's movement calculation for direction, never heading alone:
/// backward movement, strafing and a moving spawn's velocity can differ from it.
fn travel(world: &ClientWorld, id: u16, dialect: GameDialect, now: Instant) -> Option<Vec2> {
    if !world.connected() {
        return None;
    }
    let spawn = world.spawn(id)?;
    let state = &spawn.state;
    if state.kind != SpawnKind::Npc
        || now.saturating_duration_since(spawn.reported) > REPORT_FRESHNESS
        || !state
            .velocity
            .into_iter()
            .chain([state.forward, state.position.heading])
            .all(f32::is_finite)
    {
        return None;
    }
    let next = eq_client_core::entities::carried(dialect, state, Duration::from_millis(50));
    unit(map_point(next) - map_point(state.position))
}

fn unit(delta: Vec2) -> Option<Vec2> {
    (delta.is_finite() && delta.length_squared() > 0.000_001).then(|| delta.normalize())
}

/// The indicator points up; UI rotation is clockwise with screen Y down.
fn rotation(direction: Vec2) -> Rot2 {
    Rot2::radians(direction.x.atan2(-direction.y))
}

/// Update all travel indicators, including stop/stale reports and search opacity.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(crate) fn draw(
    online: Res<OnlineState>,
    settings: Res<crate::ViewerSettings>,
    motion: Res<MapMotion>,
    markers: Query<(&Marker, &Node, &BackgroundColor), Without<Travel>>,
    mut indicators: Query<
        (
            &ChildOf,
            &Children,
            &mut Node,
            &mut UiTransform,
            &mut BorderColor,
            &mut BackgroundColor,
        ),
        With<Travel>,
    >,
    mut dots: Query<&mut BackgroundColor, (Without<Marker>, Without<Travel>)>,
) {
    if indicators.is_empty() {
        return;
    }
    let world = online.world();
    let dialect = crate::entities::dialect(settings.0.installed_client);
    let now = Instant::now();
    let own_has_heading = motion
        .player(world)
        .is_some_and(|pose| pose.heading.is_finite());
    for (parent, children, mut node, mut transform, mut border, mut line) in &mut indicators {
        let Ok((marker, marker_node, fill)) = markers.get(parent.parent()) else {
            continue;
        };
        let own = world.is_player(marker.0);
        let direction = if own {
            motion
                .direction
                .filter(|_| motion.admission == world.session_id())
        } else {
            // Other players keep their facing arrows.
            travel(world, marker.0, dialect, now)
        };
        let Some(direction) = direction else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        let Val::Px(diameter) = marker_node.width else {
            continue;
        };
        let centre = Vec2::splat(diameter / 2.0);
        let visual_diameter = if own && own_has_heading {
            super::OWN_ARROW_SIZE
        } else {
            diameter
        };
        let at = centre + direction * (visual_diameter / 2.0 + LINE_LENGTH / 2.0 + 2.0);
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
        let left = px(at.x - LINE_WIDTH / 2.0);
        let top = px(at.y - LINE_LENGTH / 2.0);
        if node.left != left {
            node.left = left;
        }
        if node.top != top {
            node.top = top;
        }
        let rotation = rotation(direction);
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
        let color = if own { super::entities::PLAYER } else { fill.0 };
        let alpha = color.alpha();
        border.set_if_neq(BorderColor::all(crate::theme::INK.with_alpha(alpha)));
        line.set_if_neq(BackgroundColor(Color::BLACK.with_alpha(alpha)));
        if let Some(&dot) = children.first()
            && let Ok(mut fill) = dots.get_mut(dot)
        {
            fill.set_if_neq(BackgroundColor(color));
        }
    }
}

#[cfg(test)]
mod tests;
