use super::*;
use crate::online::testing;
use eq_client_core::{SpellUpdate, WorldEvent};
use std::time::Duration;

#[test]
fn native_fade_steps_once_per_poll_and_reverses_using_native_modes() {
    let mut fade = Fade::default();
    fade.poll(false);
    assert_eq!(fade.frame, 0);
    for frame in 1..=10 {
        fade.poll(true);
        assert_eq!(fade.frame, frame);
    }
    fade.poll(true);
    assert_eq!(fade.frame, 10);
    for frame in (0..10).rev() {
        fade.poll(false);
        assert_eq!(fade.frame, frame);
    }
    fade.poll(false);
    assert!(fade.mode == Mode::Idle);
    fade.poll(true);
    assert_eq!(fade.frame, 1);
    fade.poll(false);
    assert_eq!(fade.frame, 9); // Restoration resets to 10 before its poll.
    fade.poll(false);
    assert_eq!(fade.frame, 8); // Repeated ready updates do not restart it.
    fade.poll(true);
    assert_eq!(fade.frame, 9); // Blocking during restoration retains its frame.
    fade.poll(false);
    assert_eq!(fade.frame, 9);
}

#[test]
fn native_palette_dims_rgb_and_icon_alpha_but_retains_highlight_and_press() {
    let draw = Some(GemDraw { hover: 0x0020_2020 });
    let holds = Holds::Spell(Some(0xffde_2121));
    for frame in 0..=10 {
        let paint = |piece, interaction| drawn(draw, piece, holds, interaction, frame);
        let red = 222 - 20 * frame;
        let other = 33 - 3 * frame;
        assert_eq!(
            paint(GemPiece::Background, Interaction::None),
            Some(Color::srgba_u8(red, other, other, 255))
        );
        assert_eq!(
            paint(GemPiece::Icon, Interaction::None),
            Some(Color::srgba_u8(255, 255, 255, 255 - 21 * frame))
        );
        assert_eq!(
            paint(GemPiece::Highlight, Interaction::None),
            Some(Color::WHITE)
        );
        assert_eq!(paint(GemPiece::Holder, Interaction::None), None);
        assert_eq!(
            paint(GemPiece::Background, Interaction::Pressed),
            Some(Color::WHITE)
        );
        assert_eq!(
            paint(GemPiece::Background, Interaction::Hovered),
            Some(Color::srgba_u8(red + 32, other + 32, other + 32, 255))
        );
    }
    assert_eq!(
        drawn(
            draw,
            GemPiece::Background,
            Holds::Spell(None),
            Interaction::None,
            10
        ),
        Some(Color::WHITE)
    );
}

#[derive(Resource)]
struct Clock(Instant);

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn poll(
    clock: Res<Clock>,
    settings: Res<crate::ViewerSettings>,
    online: Res<OnlineState>,
    names: Res<SpellNames>,
    mut gems: Query<(&SpellGem, &mut Refresh)>,
) {
    refresh_at(
        settings.0.installed_client,
        online.world(),
        &names,
        &mut gems,
        clock.0,
    );
}

fn app(now: Instant, client: InstalledClient) -> App {
    let mut online = OnlineState::new(true);
    testing::generation(&mut online, crate::entities::dialect(client));
    let mut player = testing::player(7);
    player.class = Some(2);
    player.memorized_spells = [Some(100); 8];
    testing::admit(&mut online, 1, player);
    let mut fields = vec!["0"; 16];
    fields[0] = "100";
    fields[1] = "Synthetic spell";
    fields[14] = "1500";
    fields[15] = "6000";
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    app.insert_resource(online)
        .insert_resource(Clock(now))
        .insert_resource(SpellNames::parse(&fields.join("^")))
        .insert_resource(crate::ViewerSettings(crate::ViewerConfig {
            installed_client: client,
            ..default()
        }))
        .add_systems(Update, (poll, crate::hud::spell_details, pieces).chain());
    app
}

fn event(app: &mut App, event: SpellUpdate, now: Instant) {
    app.world_mut()
        .resource_scope(|world, names: Mut<SpellNames>| {
            let mut online = world.resource_mut::<OnlineState>();
            testing::news_at(&mut online, [WorldEvent::Spell(event)], now);
            online.tick(now, &*names);
        });
}

fn step(app: &mut App, now: Instant) {
    app.world_mut().resource_mut::<Clock>().0 = now;
    app.update();
}

fn shown_gems(app: &mut App) -> Vec<(Entity, Entity, Entity, bool)> {
    let mut gems = Vec::new();
    for index in 0..8 {
        // Both presentation paths share the gem slot, not just its spell ID.
        for fallback in [true, false] {
            let root = app
                .world_mut()
                .spawn((
                    SpellGem(index),
                    Interaction::None,
                    BackgroundColor(Color::NONE),
                ))
                .id();
            if fallback {
                app.world_mut()
                    .entity_mut(root)
                    .insert(crate::hud::FallbackGem);
            }
            let icon = app
                .world_mut()
                .spawn((GemPiece::Icon, Node::default(), ImageNode::default()))
                .id();
            let highlight = app
                .world_mut()
                .spawn((GemPiece::Highlight, Node::default(), ImageNode::default()))
                .id();
            app.world_mut()
                .entity_mut(root)
                .add_children(&[icon, highlight]);
            gems.push((root, icon, highlight, fallback));
        }
    }
    gems
}

#[test]
fn normal_and_skinned_gems_follow_global_then_individual_readiness_and_equality() {
    let now = Instant::now();
    let mut app = app(now, InstalledClient::Titanium);
    let gems = shown_gems(&mut app);
    event(
        &mut app,
        SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 100,
            reduction_ms: 1000,
        },
        now,
    );
    for _ in 0..10 {
        step(&mut app, now);
    }
    let all_frames = |app: &App| {
        gems.iter()
            .map(|(root, ..)| app.world().get::<Refresh>(*root).unwrap().fade.frame)
            .collect::<Vec<_>>()
    };
    assert_eq!(all_frames(&app), vec![10; 16]);
    for (root, icon, highlight, fallback) in &gems {
        assert_eq!(
            app.world().get::<ImageNode>(*icon).unwrap().color,
            Color::srgba_u8(255, 255, 255, 45)
        );
        assert_eq!(
            app.world().get::<ImageNode>(*highlight).unwrap().color,
            Color::WHITE
        );
        assert_eq!(
            *app.world().get::<Interaction>(*root).unwrap(),
            Interaction::None
        );
        if *fallback {
            assert_ne!(
                app.world().get::<BackgroundColor>(*root).unwrap().0,
                crate::theme::READY
            );
        }
    }
    // Global painting is strictly after its endpoint, for every gem.
    let global = now + Duration::from_millis(1500);
    step(&mut app, global);
    assert_eq!(all_frames(&app), vec![10; 16]);
    let after_global = global + Duration::from_nanos(1);
    for _ in 0..10 {
        step(&mut app, after_global);
    }
    assert_eq!(all_frames(&app), [vec![10; 2], vec![0; 14]].concat());
    // A long stall advances the own-gem restoration just once; no catch-up.
    let own = now + Duration::from_secs(5);
    step(&mut app, own);
    assert_eq!(&all_frames(&app)[..2], &[9, 9]);
    let casting = app.world().resource::<OnlineState>().world().casting();
    assert_eq!(casting.gem_wait(0, 100, None, own), Some(Duration::ZERO));
    assert_eq!(
        casting.gem_cast_wait(0, 100, None, own),
        Some(Duration::from_nanos(1))
    );
    for _ in 0..9 {
        step(&mut app, own);
    }
    assert_eq!(all_frames(&app), vec![0; 16]);
    for (root, icon, _, fallback) in gems {
        assert_eq!(
            app.world().get::<ImageNode>(icon).unwrap().color,
            Color::WHITE
        );
        if fallback {
            assert_eq!(
                app.world().get::<BackgroundColor>(root).unwrap().0,
                crate::theme::READY
            );
        }
    }
}

#[test]
fn unread_clients_keep_their_previous_gem_presentation() {
    for client in [InstalledClient::EqMac, InstalledClient::Rof2] {
        let now = Instant::now();
        let mut app = app(now, client);
        let gem = app.world_mut().spawn((SpellGem(0), Interaction::None)).id();
        event(
            &mut app,
            SpellUpdate::Began {
                caster_id: 7,
                spell_id: 100,
                duration_ms: 1500,
            },
            now,
        );
        for _ in 0..15 {
            step(&mut app, now);
        }
        assert_eq!(app.world().get::<Refresh>(gem).unwrap().fade.frame, 0);
    }
}

#[test]
fn replacing_emptying_or_readmitting_a_gem_discards_its_old_fade() {
    let mut refresh = Refresh::default();
    for _ in 0..10 {
        refresh.poll(Some((Some(1), 100)), true);
    }
    refresh.poll(Some((Some(1), 101)), false);
    assert_eq!(refresh.fade.frame, 0);
    refresh.poll(Some((Some(1), 101)), true);
    refresh.poll(None, false);
    assert_eq!(refresh.fade.frame, 0);
    refresh.poll(Some((Some(1), 101)), true);
    refresh.poll(Some((Some(2), 101)), true);
    assert_eq!(refresh.fade.frame, 1);
}

#[test]
fn missing_timing_dims_conservatively_and_a_missing_icon_stays_hidden() {
    let now = Instant::now();
    let mut app = app(now, InstalledClient::Titanium);
    app.insert_resource(SpellNames::default());
    let gem = app.world_mut().spawn((SpellGem(0), Interaction::None)).id();
    let icon = app
        .world_mut()
        .spawn((
            GemPiece::Icon,
            Node {
                display: Display::None,
                ..default()
            },
            ImageNode::default(),
        ))
        .id();
    app.world_mut().entity_mut(gem).add_child(icon);
    event(
        &mut app,
        SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 100,
            reduction_ms: 0,
        },
        now,
    );
    step(&mut app, now + Duration::from_secs(300));
    assert_eq!(app.world().get::<Refresh>(gem).unwrap().fade.frame, 1);
    assert_eq!(
        app.world().get::<Node>(icon).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .casting()
            .gem_wait(0, 100, None, now + Duration::from_secs(300)),
        None
    );
}

#[test]
fn an_active_cast_stays_dim_after_its_visual_duration_until_authoritative_completion() {
    let now = Instant::now();
    let mut app = app(now, InstalledClient::Titanium);
    let gem = app.world_mut().spawn((SpellGem(0), Interaction::None)).id();
    event(
        &mut app,
        SpellUpdate::Began {
            caster_id: 7,
            spell_id: 100,
            duration_ms: 1500,
        },
        now,
    );
    for _ in 0..10 {
        step(&mut app, now + Duration::from_secs(20));
    }
    assert_eq!(app.world().get::<Refresh>(gem).unwrap().fade.frame, 10);
    assert!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .casting()
            .cast
            .is_some()
    );
    event(
        &mut app,
        SpellUpdate::Mana {
            spell_id: 100,
            keep_casting: false,
        },
        now + Duration::from_secs(20),
    );
    step(&mut app, now + Duration::from_secs(20));
    assert_eq!(app.world().get::<Refresh>(gem).unwrap().fade.frame, 9);
}
