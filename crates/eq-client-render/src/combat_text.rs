//! Optional transient numbers from received damage, projected above known targets.
use bevy::prelude::*;
use eq_client_core::{
    combat::{Damage, DamageOutcome},
    world::ClientWorld,
};
use std::collections::VecDeque;

const LIMIT: usize = 64;
const LIFETIME: f32 = 1.4;

#[derive(Resource, Default)]
pub(crate) struct CombatText {
    session: Option<u64>,
    pending: VecDeque<Hit>,
}

struct Hit {
    position: Vec3,
    points: u32,
    incoming: bool,
    invisible: bool,
}

impl CombatText {
    pub(crate) fn hear(&mut self, world: &ClientWorld, damage: &[Damage], show_invisible: bool) {
        if self.session != world.session_id() {
            self.pending.clear();
            self.session = world.session_id();
        }
        if !world.in_world() {
            return;
        }
        for damage in damage {
            let DamageOutcome::Hit(points) = damage.outcome else {
                continue;
            };
            if points == 0 {
                continue;
            }
            let player = world
                .player()
                .filter(|player| player.spawn_id == damage.target_id);
            let anchor = player
                .map(|player| (player.position, player.size, false))
                .or_else(|| {
                    world
                        .spawn(damage.target_id)
                        .filter(|spawn| !spawn.state.invisible || show_invisible)
                        .map(|spawn| {
                            (
                                spawn.state.position,
                                spawn.state.size,
                                spawn.state.invisible,
                            )
                        })
                });
            let Some((position, size, invisible)) = anchor else {
                continue;
            };
            let top = if size.is_finite() && size > 0.0 {
                size
            } else {
                6.0
            };
            let position =
                Vec3::from_array(eq_client_core::render_position(position)) + Vec3::Y * top;
            if !position.is_finite() {
                continue;
            }
            if self.pending.len() == LIMIT {
                self.pending.pop_front();
            }
            self.pending.push_back(Hit {
                position,
                points,
                incoming: player.is_some(),
                invisible,
            });
        }
    }
}

#[derive(Component)]
pub(crate) struct Number {
    session: u64,
    position: Vec3,
    age: f32,
    invisible: bool,
}

/// Numbers expire, disappear on disable/reset, and never generate combat outcomes.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    online: Res<crate::online::OnlineState>,
    options: Res<crate::options::OptionsState>,
    mut state: ResMut<CombatText>,
    time: Res<Time>,
    scale: Option<Res<UiScale>>,
    collision: Option<Res<crate::Collision>>,
    cameras: Query<(&Camera, &Transform, Option<&Projection>), With<crate::OrbitCamera>>,
    mut numbers: Query<(
        Entity,
        &mut Number,
        &mut Node,
        &mut TextColor,
        &mut Visibility,
    )>,
) {
    let world = online.world();
    let session = world.session_id();
    let enabled = options
        .options
        .qol
        .on(eq_client_core::qol::Fix::FloatingCombatText)
        && world.in_world();
    let show_invisible = options
        .options
        .qol
        .on(eq_client_core::qol::Fix::ShowInvisible);
    if !enabled || state.session != session {
        state.pending.clear();
        state.session = session;
    }
    for (entity, mut number, mut node, mut color, mut visibility) in &mut numbers {
        number.age += time.delta_secs().max(0.0);
        if !enabled
            || Some(number.session) != session
            || number.age >= LIFETIME
            || (number.invisible && !show_invisible)
        {
            commands.entity(entity).despawn();
            continue;
        }
        let place = cameras
            .single()
            .ok()
            .and_then(|(camera, view, projection)| {
                if projection.is_some_and(|projection| {
                    crate::camera::past_far(projection, view, number.position)
                }) {
                    return None;
                }
                let offset = number.position - view.translation;
                let distance = offset.length();
                if collision
                    .as_ref()
                    .and_then(|collision| collision.0.as_ref())
                    .is_some_and(|geometry| {
                        distance > 0.5
                            && geometry
                                .ray_distance(view.translation, offset / distance, distance - 0.5)
                                .is_some()
                    })
                {
                    return None;
                }
                let at = camera
                    .world_to_viewport(&GlobalTransform::from(*view), number.position)
                    .ok()?;
                Some(at / scale.as_ref().map_or(1.0, |scale| scale.0))
            });
        *visibility = if let Some(at) = place {
            node.left = px(at.x);
            node.top = px(at.y - 32.0 * number.age);
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        color.0 = color
            .0
            .with_alpha((1.0 - number.age / LIFETIME).clamp(0.0, 1.0));
    }
    let Some(session) = session.filter(|_| enabled) else {
        return;
    };
    let room = LIMIT.saturating_sub(numbers.iter().count());
    for hit in state
        .pending
        .drain(..)
        .filter(|hit| !hit.invisible || show_invisible)
        .take(room)
    {
        draw(&mut commands, session, &hit);
    }
}

fn draw(commands: &mut Commands, session: u64, hit: &Hit) {
    commands.spawn((
        Number {
            session,
            position: hit.position,
            age: 0.0,
            invisible: hit.invisible,
        },
        Text::new(hit.points.to_string()),
        crate::theme::font(crate::theme::Size::Body),
        TextColor(if hit.incoming {
            Color::srgb(1.0, 0.3, 0.25)
        } else {
            Color::srgb(1.0, 0.9, 0.3)
        }),
        TextShadow {
            offset: Vec2::ONE,
            color: Color::BLACK,
        },
        TextLayout::new(Justify::Center, LineBreak::NoWrap),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
        GlobalZIndex(2),
        bevy::ui::FocusPolicy::Pass,
        Visibility::Hidden,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};

    fn hit(target_id: u16, outcome: DamageOutcome) -> Damage {
        Damage {
            source_id: 7,
            target_id,
            kind: 1,
            spell_id: None,
            outcome,
            push: None,
        }
    }

    #[test]
    fn only_positive_received_hits_on_known_targets_are_drawn_and_queue_is_bounded() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::spawns(&mut online, vec![testing::pet(9, 0)]);
        let mut state = CombatText::default();
        state.hear(
            online.world(),
            &[
                hit(9, DamageOutcome::Miss),
                hit(9, DamageOutcome::Hit(0)),
                hit(99, DamageOutcome::Hit(10)),
            ],
            false,
        );
        assert!(state.pending.is_empty());
        state.hear(
            online.world(),
            &vec![hit(9, DamageOutcome::Hit(12)); LIMIT + 1],
            false,
        );
        assert_eq!(state.pending.len(), LIMIT);
        assert!(
            state
                .pending
                .iter()
                .all(|hit| hit.points == 12 && !hit.incoming)
        );
        testing::admit(&mut online, 2, testing::player(7));
        state.hear(online.world(), &[hit(7, DamageOutcome::Hit(3))], false);
        assert_eq!(state.pending.len(), 1);
        assert!(state.pending[0].incoming);
    }

    #[test]
    fn disabling_or_changing_admission_removes_drawn_numbers() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let mut app = App::new();
        app.insert_resource(online)
            .init_resource::<crate::options::OptionsState>()
            .init_resource::<CombatText>()
            .init_resource::<Time>()
            .add_systems(Update, update);
        let number = app
            .world_mut()
            .spawn((
                Number {
                    session: 1,
                    position: Vec3::ZERO,
                    age: 0.0,
                    invisible: false,
                },
                Node::default(),
                TextColor::default(),
                Visibility::Inherited,
            ))
            .id();
        app.update();
        assert!(app.world().get_entity(number).is_err());
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(eq_client_core::qol::Fix::FloatingCombatText, true);
        let old = app
            .world_mut()
            .spawn((
                Number {
                    session: 99,
                    position: Vec3::ZERO,
                    age: 0.0,
                    invisible: false,
                },
                Node::default(),
                TextColor::default(),
                Visibility::Inherited,
            ))
            .id();
        app.update();
        assert!(app.world().get_entity(old).is_err());
    }
}
