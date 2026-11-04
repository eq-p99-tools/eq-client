//! Live player placement and travel arrows, separate from facing and map fitting.
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

/// A travel arrow is a child of its dot, so clipping and position follow it.
#[derive(Component)]
pub(crate) struct Travel;

pub(super) fn arrow() -> impl Bundle {
    (
        Travel,
        ImageNode::default(),
        UiTransform::default(),
        bevy::ui::FocusPolicy::Pass,
        Node {
            position_type: PositionType::Absolute,
            width: px(super::ARROW),
            height: px(super::ARROW),
            display: Display::None,
            ..default()
        },
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
    if !matches!(state.kind, SpawnKind::Player | SpawnKind::Npc)
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

/// The texture points up; UI rotation is clockwise with screen Y down.
fn rotation(direction: Vec2) -> Rot2 {
    Rot2::radians(direction.x.atan2(-direction.y))
}

/// Update all arrows every frame, including stop/stale reports and search opacity.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(crate) fn draw(
    online: Res<OnlineState>,
    settings: Res<crate::ViewerSettings>,
    motion: Res<MapMotion>,
    mut images: ResMut<Assets<Image>>,
    mut art: Local<Option<Handle<Image>>>,
    markers: Query<(&Marker, &Node, &BackgroundColor), Without<Travel>>,
    mut arrows: Query<(&ChildOf, &mut Node, &mut UiTransform, &mut ImageNode), With<Travel>>,
) {
    if arrows.is_empty() {
        return;
    }
    let art = art.get_or_insert_with(|| images.add(super::arrow_picture([255; 4], [0, 0, 0, 255])));
    let world = online.world();
    let dialect = crate::entities::dialect(settings.0.installed_client);
    let now = Instant::now();
    for (parent, mut node, mut transform, mut image) in &mut arrows {
        let Ok((marker, dot, color)) = markers.get(parent.parent()) else {
            continue;
        };
        let own = world
            .player()
            .is_some_and(|player| player.spawn_id == marker.0);
        let direction = if own {
            motion.direction
        } else if world.is_player(marker.0) {
            None // A player's marker already shows facing, including at rest.
        } else {
            travel(world, marker.0, dialect, now)
        };
        let Some(direction) = direction else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        let Val::Px(diameter) = dot.width else {
            continue;
        };
        let at = Vec2::splat(diameter / 2.0) + direction * (diameter / 2.0 + 9.0);
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
        let left = px(at.x - super::HALF_ARROW);
        let top = px(at.y - super::HALF_ARROW);
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
        if image.image != *art {
            image.image = art.clone();
        }
        let color = if own {
            super::entities::PLAYER
        } else {
            color.0
        };
        if image.color != color {
            image.color = color;
        }
    }
}

#[cfg(test)]
mod tests;
