//! Samples local air from the player's head, independently of camera and HP.
pub(super) mod head;
mod lava;
mod modifiers;
#[cfg(test)]
mod motion_tests;

use crate::{
    Player,
    motion::LiquidBasis,
    online::OnlineState,
    options::OptionsState,
    outbox::Outbox,
    spellbook::SpellNames,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_assets::regions::Medium;
use eq_client_core::{
    Capability, ClientCommand, GameDialect, PlayerState,
    breath::{Air, Breath, Sample, titanium_capacity, water_breathing_race},
    hazards::{LiquidHazard, LiquidObservation},
    qol::Fix,
};
use std::time::Instant;

/// Admission-scoped adapter around the typed air counter.
#[derive(Resource, Default)]
pub(super) struct State {
    air: AirState,
    heat: lava::Heat,
}

/// Why the current frame could not supply an air observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Unavailable {
    OutsideAdmission,
    Gills,
    OutOfWorld,
    MovementPending,
    MovementUnconfirmed,
    SceneLoading,
    MissingHeadBasis,
    ModelChanged,
    ModelMismatch,
    MissingZoneScale,
    InvalidHeadOffset,
    UnresolvedMedium,
    MissingProfile,
    AlternateAdvancement,
    Equipment,
    Food,
    BuffSlots,
    Effects,
    UnslottedModifier,
    Capacity,
}

type Probe = (Option<Medium>, Result<Sample, Unavailable>);
type Observation = (Option<Medium>, Result<(Sample, Option<u32>), Unavailable>);

impl State {
    pub(super) const fn air(&self) -> Option<Air> {
        self.air.breath.air()
    }

    pub(super) const fn probe(&self) -> Option<Probe> {
        self.air.probe
    }
}

#[derive(Default)]
struct AirState {
    admission: Option<(u64, u16)>,
    placement: u64,
    breath: Breath,
    probe: Option<Probe>,
}

impl AirState {
    /// Runs only in an eligible admission. Reporting policy gates damage
    /// amounts independently of the air counter.
    fn advance(
        &mut self,
        now: Instant,
        world: &eq_client_core::world::ClientWorld,
        qol: &eq_client_core::qol::Settings,
        placement: u64,
        movement: LiquidBasis,
        sample: impl FnOnce(&PlayerState) -> Observation,
    ) -> Option<u32> {
        let scope = world.session_id().zip(world.player()).filter(|_| {
            !world.ended() && !world.is_dead() && world.generation() == GameDialect::Titanium
        });
        // Skipping the counter withholds the drowning reports a local EQEmu
        // session takes, so the departure gate applies as it does elsewhere.
        let gills = qol.on(Fix::GillsForDays) && world.can(Capability::GameplayDepartures);
        let Some((admission, player)) = scope.filter(|_| !gills) else {
            *self = Self {
                probe: Some((
                    None,
                    Err(if scope.is_some() {
                        Unavailable::Gills
                    } else {
                        Unavailable::OutsideAdmission
                    }),
                )),
                ..Self::default()
            };
            return None;
        };
        if !world.in_world() {
            self.breath.interrupt();
            self.probe = Some((None, Err(Unavailable::OutOfWorld)));
            return None;
        }
        let can_report = qol.on(Fix::TakeEnvironmentalDamage) && world.can(Capability::Drowning);
        let scope = (admission, player.spawn_id);
        if movement == LiquidBasis::Unconfirmed {
            self.set_scope(scope, placement);
            self.breath.hide_air();
            self.probe = Some((None, Err(Unavailable::MovementUnconfirmed)));
            return None;
        }
        if movement == LiquidBasis::Pending {
            // The worker may already have moved while its result is in flight.
            // Keep the tick due so it can use the next observed position.
            self.set_scope(scope, placement);
            self.probe = Some((None, Err(Unavailable::MovementPending)));
            return None;
        }
        let (medium, result) = sample(player);
        let (sample, maximum) = result.unwrap_or((Sample::Unknown, None));
        let amount = self.observe(
            now,
            scope,
            placement,
            sample,
            maximum.filter(|_| can_report),
        );
        self.probe = Some((medium, result.map(|(sample, _)| sample)));
        amount
    }

    fn observe(
        &mut self,
        now: Instant,
        admission: (u64, u16),
        placement: u64,
        sample: Sample,
        maximum_hp: Option<u32>,
    ) -> Option<u32> {
        self.set_scope(admission, placement);
        self.breath.sample(now, sample, maximum_hp)
    }

    fn set_scope(&mut self, admission: (u64, u16), placement: u64) {
        if self.admission != Some(admission) {
            *self = Self {
                admission: Some(admission),
                placement,
                ..default()
            };
        } else if self.placement != placement {
            self.placement = placement;
            self.breath.interrupt();
        }
    }
}

fn head_medium(
    online: &OnlineState,
    basis: Option<head::Basis>,
    player: &PlayerState,
) -> Result<Medium, Unavailable> {
    if online.scene_loading() {
        return Err(Unavailable::SceneLoading);
    }
    let basis = basis.ok_or(Unavailable::MissingHeadBasis)?;
    let mut head = eq_client_core::render_position(player.position);
    head[1] += basis.for_player(online.world(), player)?;
    online
        .regions
        .medium_at(head)
        .ok_or(Unavailable::UnresolvedMedium)
}

fn observe_head(
    online: &OnlineState,
    basis: Option<head::Basis>,
    spells: &SpellNames,
    player: &PlayerState,
) -> Observation {
    let medium = match head_medium(online, basis, player) {
        Ok(medium) => medium,
        Err(reason) => return (None, Err(reason)),
    };
    let sample = match medium {
        Medium::Air => Ok((Sample::CanBreathe, None)),
        Medium::Water | Medium::Lava | Medium::OtherSubmerging => {
            submerged(online.world(), spells, player)
        }
    };
    (Some(medium), sample)
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    (online, motion): (Res<OnlineState>, Res<crate::motion::Controls>),
    options: Res<OptionsState>,
    spells: Res<SpellNames>,
    body: Query<&head::Basis, With<Player>>,
    outbox: Res<Outbox>,
    mut state: ResMut<State>,
    mut shown: ResMut<Shown>,
) {
    let world = online.world();
    let now = Instant::now();
    let qol = &options.options.qol;
    let movement = motion.liquid_basis(world.session_id(), now);
    let report = |hazard, amount| {
        let Some((revision, player)) = world.liquid_hazard_revision().zip(world.player()) else {
            return;
        };
        let _ = outbox.post(world, |stamp| ClientCommand::LiquidDamage {
            session_id: stamp.session_id,
            hazard,
            amount,
            observation: LiquidObservation {
                revision,
                position: player.position,
                sampled_at: now,
            },
        });
    };
    if let Some(amount) = state.air.advance(
        now,
        world,
        qol,
        online.placement_revision(),
        movement,
        |player| observe_head(&online, body.single().ok().copied(), &spells, player),
    ) {
        report(LiquidHazard::Drowning, amount);
    }
    if let Some(amount) = state.heat.advance(now, &online, qol, &spells, movement) {
        report(LiquidHazard::Lava, amount);
    }
    match (state.air().is_some(), shown.is_open(WindowId::Breath)) {
        (true, false) => shown.open(WindowId::Breath),
        (false, true) => shown.close(WindowId::Breath),
        _ => (),
    }
}

/// Only resolved inventory, spell and HP inputs can produce damage.
fn submerged(
    world: &eq_client_core::world::ClientWorld,
    spells: &SpellNames,
    player: &PlayerState,
) -> Result<(Sample, Option<u32>), Unavailable> {
    let race = world
        .spawn(player.spawn_id)
        .map_or(player.race, |spawn| spawn.state.race);
    if water_breathing_race(race) {
        return Ok((Sample::CanBreathe, None));
    }
    let inputs = modifiers::read(world, spells, player, modifiers::Need::Air)?;
    if inputs.water_breathing {
        return Ok((Sample::CanBreathe, None));
    }
    let capacity = titanium_capacity(race, inputs.stamina, inputs.breath_level)
        .ok_or(Unavailable::Capacity)?;
    let maximum_hp = if inputs.invulnerable {
        None
    } else {
        inputs.maximum_hp(world)
    };
    Ok((Sample::Submerged(capacity), maximum_hp))
}

/// A small meter for skins without a usable `BreathWindow`.
#[derive(Component)]
pub(super) struct Fallback;

/// The installed skin supplied an air gauge for this frame.
#[derive(Component)]
pub(super) struct NativeGauge;

#[allow(clippy::needless_pass_by_value)]
pub(super) fn fallback(
    mut commands: Commands,
    state: Res<State>,
    native: Query<(), With<NativeGauge>>,
    panel: Query<Entity, With<Fallback>>,
) {
    let visible = state.air().is_some() && native.is_empty();
    let mut panels = panel.iter();
    if visible && panels.next().is_none() {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: px(70),
                    left: px(0),
                    right: px(0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                GlobalZIndex(12),
                Fallback,
            ))
            .with_children(|root| {
                root.spawn((
                    Node {
                        width: px(160),
                        padding: UiRect::all(px(4)),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        ..default()
                    },
                    crate::theme::surface(),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Air Remaining"),
                        crate::theme::font(crate::theme::Size::Label),
                        TextColor(crate::theme::INK),
                    ));
                    panel
                        .spawn((
                            Node {
                                height: px(6),
                                width: percent(100),
                                ..default()
                            },
                            BackgroundColor(crate::theme::WELL),
                        ))
                        .with_child((
                            Node {
                                height: percent(100),
                                width: percent(0),
                                ..default()
                            },
                            BackgroundColor(crate::theme::CAST),
                            crate::skinned::Shows::Fill(8),
                        ));
                });
            });
    }
    // Keep the first existing fallback when it is needed, and remove the
    // rest even if a previous frame somehow supplied more than one.
    for panel in panels {
        commands.entity(panel).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{num::NonZeroU32, time::Duration};

    #[test]
    fn missing_head_basis_is_reported_and_interrupts_air_without_damage() {
        use crate::online::testing;
        use eq_client_core::qol::Settings;

        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 5, testing::player(7));
        let mut state = AirState::default();
        let now = Instant::now();
        let wet = Sample::Submerged(NonZeroU32::MIN);
        state.observe(now, (5, 7), 1, wet, Some(80));
        state.observe(now + Duration::from_secs(1), (5, 7), 1, wet, Some(80));
        assert_eq!(state.breath.air().unwrap().remaining, 0);

        let mut qol = Settings::default();
        qol.set(Fix::TakeEnvironmentalDamage, true);
        let spells = SpellNames::default();
        let amount = state.advance(
            now + Duration::from_secs(2),
            online.world(),
            &qol,
            1,
            LiquidBasis::Observed,
            |player| observe_head(&online, None, &spells, player),
        );
        assert_eq!(amount, None);
        assert!(state.breath.air().is_none());
        assert_eq!(
            state.probe,
            Some((None, Err(Unavailable::MissingHeadBasis)))
        );
    }

    #[test]
    fn gills_is_inert_where_the_session_withholds_gameplay_departures() {
        use crate::{online::testing, outbox::Needs};
        use eq_client_core::{WorldEvent, options::Toggle, qol::Settings};

        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                session_id: 5,
                capabilities: vec![Capability::EnvironmentalDamage],
                choices: Vec::new(),
                zone: "Example".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        testing::connect(&mut online, true);
        let needs = Needs::of(Toggle::Qol(Fix::GillsForDays)).unwrap();
        assert!(!needs.offered(online.world()));
        let mut qol = Settings::default();
        qol.set(Fix::GillsForDays, true);
        let mut state = AirState::default();
        let start = Instant::now();
        let wet = |_: &PlayerState| {
            (
                Some(Medium::Water),
                Ok((Sample::Submerged(NonZeroU32::MIN), Some(80))),
            )
        };
        state.advance(start, online.world(), &qol, 1, LiquidBasis::Observed, wet);
        state.advance(
            start + Duration::from_secs(1),
            online.world(),
            &qol,
            1,
            LiquidBasis::Observed,
            wet,
        );
        // The counter ran: the setting changed nothing on this session.
        assert_eq!(state.breath.air().unwrap().remaining, 0);
    }

    #[test]
    fn titanium_air_and_gills_work_without_a_liquid_wire_capability() {
        use crate::{online::testing, outbox::Needs};
        use eq_client_core::{WorldEvent, options::Toggle, qol::Settings};

        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                session_id: 5,
                capabilities: vec![
                    Capability::EnvironmentalDamage,
                    Capability::GameplayDepartures,
                ],
                choices: Vec::new(),
                zone: "Example".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        testing::connect(&mut online, true);
        let needs = Needs::of(Toggle::Qol(Fix::GillsForDays)).unwrap();
        assert!(needs.offered(online.world()));
        assert!(!online.world().can(Capability::Drowning));
        let mut qol = Settings::default();
        let mut state = AirState::default();
        let start = Instant::now();
        let wet = |_: &PlayerState| {
            (
                Some(Medium::Water),
                Ok((Sample::Submerged(NonZeroU32::MIN), Some(80))),
            )
        };
        for seconds in [0, 1, 2] {
            assert_eq!(
                state.advance(
                    start + Duration::from_secs(seconds),
                    online.world(),
                    &qol,
                    1,
                    LiquidBasis::Observed,
                    wet
                ),
                None
            );
        }
        assert_eq!(state.breath.air().unwrap().remaining, 0);
        qol.set(Fix::GillsForDays, true);
        state.advance(
            start,
            online.world(),
            &qol,
            1,
            LiquidBasis::Observed,
            |_| panic!("gills must skip sampling"),
        );
        assert!(state.breath.air().is_none());
        assert_eq!(state.probe, Some((None, Err(Unavailable::Gills))));
    }

    #[test]
    fn the_fallback_follows_air_without_duplicating_a_native_gauge() {
        let mut app = crate::testing::app();
        app.add_systems(Update, fallback);
        let now = Instant::now();
        let visible = |app: &mut App| {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<Fallback>>()
                .iter(world)
                .count()
        };
        let sample = |app: &mut App, sample| {
            app.world_mut()
                .resource_mut::<State>()
                .air
                .breath
                .sample(now, sample, None);
            app.update();
        };
        let wet = Sample::Submerged(NonZeroU32::MIN);
        sample(&mut app, wet);
        assert_eq!(visible(&mut app), 1);
        app.world_mut().spawn(Fallback);
        app.world_mut().spawn(Fallback);
        sample(&mut app, wet);
        assert_eq!(visible(&mut app), 1);

        let native = app.world_mut().spawn(NativeGauge).id();
        app.world_mut().spawn(Fallback);
        app.update();
        assert_eq!(visible(&mut app), 0);
        app.world_mut().entity_mut(native).despawn();
        sample(&mut app, wet);
        assert_eq!(visible(&mut app), 1);
        sample(&mut app, Sample::CanBreathe);
        assert_eq!(visible(&mut app), 0);
    }

    #[test]
    fn placement_and_admission_changes_do_not_replay_drowning_ticks() {
        let mut state = AirState::default();
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let wet = Sample::Submerged(NonZeroU32::MIN);
        let admission = (1, 7);
        assert_eq!(state.observe(at(0), admission, 1, wet, Some(80)), None);
        assert_eq!(state.observe(at(1), admission, 1, wet, Some(80)), None);
        assert_eq!(state.breath.air().unwrap().remaining, 0);
        assert_eq!(state.observe(at(10), admission, 2, wet, Some(80)), None);
        assert_eq!(state.breath.air().unwrap().remaining, 0);
        assert_eq!(state.observe(at(11), admission, 2, wet, Some(80)), Some(10));
        assert_eq!(state.observe(at(13), (2, 7), 3, wet, Some(80)), None);
        assert_eq!(state.breath.air().unwrap().remaining, 1);
    }

    #[test]
    fn environmental_prevention_leaves_air_running_but_gills_skip_sampling() {
        use crate::online::testing;
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 5, testing::player(7));
        let mut qol = eq_client_core::qol::Settings::default();
        let mut state = AirState::default();
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let wet = |_: &PlayerState| {
            (
                Some(Medium::Water),
                Ok((Sample::Submerged(NonZeroU32::MIN), Some(80))),
            )
        };
        qol.set(Fix::TakeEnvironmentalDamage, false);
        for seconds in [0, 1, 2] {
            assert_eq!(
                state.advance(
                    at(seconds),
                    online.world(),
                    &qol,
                    1,
                    LiquidBasis::Observed,
                    wet
                ),
                None
            );
        }
        assert_eq!(state.breath.air().unwrap().remaining, 0);
        qol.set(Fix::TakeEnvironmentalDamage, true);
        assert_eq!(
            state.advance(at(3), online.world(), &qol, 1, LiquidBasis::Observed, wet),
            Some(10)
        );

        testing::link(&mut online, eq_client_core::world::Link::Zoning);
        assert_eq!(
            state.advance(
                at(4),
                online.world(),
                &qol,
                2,
                LiquidBasis::Observed,
                |_| panic!("zoning must skip sampling")
            ),
            None
        );
        assert!(state.breath.air().is_none());
        testing::connect(&mut online, true);
        assert_eq!(
            state.advance(at(8), online.world(), &qol, 2, LiquidBasis::Observed, wet),
            None
        );
        assert_eq!(state.breath.air().unwrap().remaining, 0);

        qol.set(Fix::GillsForDays, true);
        assert_eq!(
            state.advance(
                at(9),
                online.world(),
                &qol,
                2,
                LiquidBasis::Observed,
                |_| panic!("gills must skip sampling")
            ),
            None
        );
        assert!(state.breath.air().is_none());
        qol.set(Fix::GillsForDays, false);
        assert_eq!(
            state.advance(at(10), online.world(), &qol, 2, LiquidBasis::Observed, wet),
            None
        );
        assert_eq!(state.breath.air().unwrap().remaining, 1);

        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::DeathBind { session_id: 5 }],
        );
        assert_eq!(
            state.advance(at(20), online.world(), &qol, 1, LiquidBasis::Observed, wet),
            None
        );
        assert!(state.breath.air().is_none());
    }
}
