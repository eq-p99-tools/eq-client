//! Names over heads: each character's name above them, worded by
//! `eq_client_core::names::label`, while the Options window's Show PC Names
//! and Show NPC Names allow them. They follow the models through the frame
//! and draw under every window; one whose head leaves the view is clipped
//! at the window's edge. A name whose place has zone geometry between it
//! and the camera is hidden with the body behind it (inferred, item 6.234).
//! Each is drawn in the installed client's colour for it
//! ([`eq_client_assets::names::NameColours`]), the target's pulsing.
use bevy::prelude::*;
use eq_client_core::names::{ShowNames, is_pc, label};
use std::collections::BTreeMap;

/// How far above the head a name sits, in world units.
const ABOVE_HEAD: f32 = 0.6;

/// How many times a second the target's name steps through its pulse: the
/// program steps it each time it draws the name, at a pace not read, so
/// this one is this client's (recording checklist 6.354).
const PULSE_STEPS_PER_SECOND: f32 = 10.0;

/// How far short of a name's place geometry must stop to hide it, as the
/// camera keeps off the walls it pulls in against: a place touching a
/// ceiling or wall face is not hidden by the surface it touches.
const SIGHT_MARGIN: f32 = 0.5;

/// The top of what a model root draws, in the root's own units: where a
/// name over it sits. Whatever draws a root sets it, and an animated model
/// moves it with the head each pose.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Overhead(pub(crate) f32);

/// A name drawn over the root it follows.
#[derive(Component)]
pub(crate) struct NameTag;

/// The names drawn, by the root each follows.
#[derive(Resource, Default)]
pub(crate) struct NameTags(BTreeMap<Entity, Entity>);

/// Sets how much of players' names shows, as a `/shownames` asked, and says
/// so as the official client does: in its words where the installation has
/// them.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn request(
    mut chat: ResMut<crate::chat::ChatState>,
    mut options: ResMut<crate::options::OptionsState>,
    messages: Option<Res<crate::hud::messages::Messages>>,
) {
    let say = |(id, fallback): (u32, &str)| {
        messages.as_deref().map_or_else(
            || crate::chat::Said::own(fallback),
            |messages| messages.said_text(id, fallback),
        )
    };
    if let Some(level) = chat.show_names.take() {
        options.options.show_names = level;
        let line = say(level.announcement());
        chat.history.push(crate::chat::system_line(line));
    }
    if std::mem::take(&mut chat.show_names_usage) {
        use eq_client_core::names::{SHOW_NAMES_USAGE, SHOW_NAMES_USAGE_TEXT};
        let line = say((SHOW_NAMES_USAGE, SHOW_NAMES_USAGE_TEXT));
        chat.history.push(crate::chat::system_line(line));
    }
}

/// Draws a name over the player's head and every drawn spawn's, and takes
/// away those of spawns gone or hidden by the options.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters.
pub(crate) fn update(
    mut commands: Commands,
    (online, nearby, options, scale): (
        Res<crate::online::OnlineState>,
        Res<crate::entities::NearbyEntities>,
        Res<crate::options::OptionsState>,
        Option<Res<UiScale>>,
    ),
    (settings, time): (Option<Res<crate::ViewerSettings>>, Res<Time>),
    collision: Option<Res<crate::Collision>>,
    mut drawn: ResMut<NameTags>,
    player: Query<Entity, With<crate::Player>>,
    roots: Query<(&Transform, &Overhead)>,
    cameras: Query<(&Camera, &Transform, Option<&Projection>), With<crate::OrbitCamera>>,
    mut tags: Query<(&mut Node, &mut Text, &mut TextColor, &mut Visibility), With<NameTag>>,
) {
    let world = online.world();
    let options = options.options;
    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    let painter = Painter::new(client, world, time.elapsed_secs());
    let shown = |kind| {
        if is_pc(kind) {
            options.pc_names && options.show_names != ShowNames::Off
        } else {
            options.npc_names
        }
    };
    let words = |name: &str, kind, parts, listing: &eq_client_core::listing::Listing| {
        let guild = listing.guild.and_then(|guild| world.guild_name(guild));
        label(name, kind, (parts, listing), guild, options.show_names)
    };
    let mut wanted = BTreeMap::new();
    let me = eq_client_core::SpawnKind::Player;
    if let (Ok(root), Some(player)) = (player.single(), world.player())
        && shown(me)
    {
        let said = words(&player.name, me, &player.name_parts, &player.listing);
        let drawn = painter.paint(player.spawn_id, true, player.race, &player.listing);
        wanted.insert(root, (said, drawn));
    }
    for (id, root) in &nearby.rendered {
        if let Some(spawn) = world
            .spawn(*id)
            .map(|spawn| &spawn.state)
            .filter(|spawn| shown(spawn.kind))
            .filter(|spawn| {
                !spawn.invisible || options.qol.on(eq_client_core::qol::Fix::ShowInvisible)
            })
        {
            let said = words(&spawn.name, spawn.kind, &spawn.name_parts, &spawn.listing);
            let said = if spawn.invisible {
                format!("({said})")
            } else {
                said
            };
            let player = spawn.kind == eq_client_core::SpawnKind::Player;
            let drawn = painter.paint(*id, player, spawn.race, &spawn.listing);
            wanted.insert(*root, (said, drawn));
        }
    }
    drawn.0.retain(|root, tag| {
        let keep = wanted.contains_key(root);
        if !keep {
            commands.entity(*tag).despawn();
        }
        keep
    });
    let factor = scale.map_or(1.0, |scale| scale.0);
    // The camera and the roots are top-level, so their own transforms place
    // them: reading those rather than the propagated ones keeps the names in
    // step with this frame's moves.
    let camera = cameras.single().ok();
    let geometry = collision
        .as_deref()
        .and_then(|collision| collision.0.as_ref());
    for (root, (words, drawn_in)) in wanted {
        // Where the head's top shows, in the UI's pixels from the window's
        // left and bottom edges; none behind the camera, past its far plane
        // (where the model is left out too) or behind a wall. How far the
        // program draws names is unread (recording checklist 6.342).
        let place = camera.and_then(|(camera, view, projection)| {
            let (transform, Overhead(top)) = roots.get(root).ok()?;
            let above = transform.transform_point(Vec3::Y * *top) + Vec3::Y * ABOVE_HEAD;
            if projection.is_some_and(|projection| crate::camera::past_far(projection, view, above))
                || behind_geometry(geometry, view.translation, above)
            {
                return None;
            }
            let at = camera
                .world_to_viewport(&GlobalTransform::from(*view), above)
                .ok()?;
            let size = camera.logical_viewport_size()?;
            Some(Vec2::new(at.x, size.y - at.y) / factor)
        });
        let Some(&tag) = drawn.0.get(&root) else {
            drawn
                .0
                .insert(root, spawn_tag(&mut commands, words, drawn_in));
            continue;
        };
        let Ok((mut node, mut text, mut colour, mut visibility)) = tags.get_mut(tag) else {
            continue;
        };
        if text.0 != words {
            text.0 = words;
        }
        colour.set_if_neq(TextColor(drawn_in));
        let Some(at) = place else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let (left, bottom) = (px(at.x), px(at.y));
        if node.left != left {
            node.left = left;
        }
        if node.bottom != bottom {
            node.bottom = bottom;
        }
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// What colours the names this frame: the installed client's rule, the
/// world's player-versus-player ruleset, and the target, whose name pulses.
struct Painter {
    colours: eq_client_assets::names::NameColours,
    pvp_rule: u32,
    target: Option<u16>,
    /// Where the target's name is in its pulse, 0 to 10.
    step: u8,
}

impl Painter {
    fn new(
        client: eq_client_assets::ui::InstalledClient,
        world: &eq_client_core::world::ClientWorld,
        seconds: f32,
    ) -> Self {
        // The target's name steps from white to its colour, over and over.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0 to 10.
        let step = ((seconds * PULSE_STEPS_PER_SECOND) % 11.0) as u8;
        Self {
            colours: client.name_colours(),
            pvp_rule: world.pvp_rule(),
            target: world.target().selected,
            step,
        }
    }

    /// The colour of spawn `id`'s name: a player or not, of this race and
    /// listing.
    fn paint(
        &self,
        id: u16,
        player: bool,
        race: u32,
        listing: &eq_client_core::listing::Listing,
    ) -> Color {
        let named = eq_client_assets::names::Named {
            player,
            race,
            roleplaying: listing.anonymity.wire() & 2 != 0,
            pvp_rule: self.pvp_rule,
        };
        let colour = self.colours.colour(named);
        let [r, g, b] = if self.target == Some(id) {
            self.colours.target(colour, self.step)
        } else {
            colour
        };
        Color::srgb_u8(r, g, b)
    }
}

/// Whether zone geometry stands between the camera and a name's place.
/// Without the zone's geometry nothing is.
fn behind_geometry(
    geometry: Option<&eq_client_core::movement::CollisionWorld>,
    camera: Vec3,
    place: Vec3,
) -> bool {
    let offset = place - camera;
    let distance = offset.length();
    geometry.is_some_and(|geometry| {
        distance > SIGHT_MARGIN
            && geometry
                .ray_distance(camera, offset / distance, distance - SIGHT_MARGIN)
                .is_some()
    })
}

/// A name that shows once it is placed.
fn spawn_tag(commands: &mut Commands, words: String, colour: Color) -> Entity {
    commands
        .spawn((
            NameTag,
            Text::new(words),
            crate::theme::font(crate::theme::Size::Label),
            TextColor(colour),
            TextShadow {
                offset: Vec2::splat(1.0),
                color: Color::BLACK,
            },
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            // Centred over the head, however wide.
            UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
            // Above the world, under every window: the HUD's layer is 10.
            GlobalZIndex(1),
            bevy::ui::FocusPolicy::Pass,
            Visibility::Hidden,
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::{SpawnKind, SpawnState, WorldEvent};

    /// A spawn of this kind, named as the server names it, in this guild.
    fn spawn(spawn_id: u16, name: &str, kind: SpawnKind, guild: Option<u32>) -> SpawnState {
        SpawnState {
            name: name.into(),
            kind,
            listing: eq_client_core::listing::Listing { guild, ..default() },
            ..testing::pet(spawn_id, 0)
        }
    }

    /// The words of every name drawn, in order.
    fn shown(app: &mut App) -> Vec<String> {
        let mut words: Vec<String> = app
            .world_mut()
            .query_filtered::<&Text, With<NameTag>>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        words.sort();
        words
    }

    /// An app drawing the player, a guildmate, a gnoll, the guildmate's
    /// corpse and a gnoll pup, none of them placed yet.
    fn app() -> App {
        let mut app = crate::testing::app();
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            let mut me = testing::player(1);
            me.listing.guild = Some(7);
            testing::admit(&mut online, 1, me);
            testing::news(
                &mut online,
                [WorldEvent::GuildNames(vec![(7, "Example Guild".into())])],
            );
            testing::spawns(
                &mut online,
                vec![
                    spawn(2, "Examplar", SpawnKind::Player, Some(7)),
                    spawn(3, "a_gnoll001", SpawnKind::Npc, None),
                    spawn(4, "Examplar's_corpse0", SpawnKind::PlayerCorpse, Some(7)),
                    // Creatures have no guild to show, whatever they carry.
                    spawn(5, "a_gnoll_pup002", SpawnKind::Npc, Some(7)),
                ],
            );
        }
        app.world_mut()
            .spawn((crate::Player, Transform::default(), Overhead(3.0)));
        for id in 2..=5 {
            let root = app
                .world_mut()
                .spawn((Transform::default(), Overhead(3.0)))
                .id();
            app.world_mut()
                .resource_mut::<crate::entities::NearbyEntities>()
                .rendered
                .insert(id, root);
        }
        app.add_systems(Update, (request, update).chain());
        app.update();
        app
    }

    #[test]
    fn names_follow_the_drawn_spawns_and_the_options() {
        let mut app = app();
        let everyone = [
            "Examplar\n<Example Guild>",
            "Examplar's corpse",
            "Example\n<Example Guild>",
            "a gnoll",
            "a gnoll pup",
        ];
        assert_eq!(shown(&mut app), everyone);
        // Without a camera to place them by, none shows yet.
        let mut visible = app
            .world_mut()
            .query_filtered::<&Visibility, With<NameTag>>();
        assert!(
            visible
                .iter(app.world())
                .all(|visibility| *visibility == Visibility::Hidden)
        );

        // Show PC Names governs players and their corpses, Show NPC Names
        // the rest.
        let set = |app: &mut App, pc: bool, npc: bool| {
            let mut options = app
                .world_mut()
                .resource_mut::<crate::options::OptionsState>();
            options.options.pc_names = pc;
            options.options.npc_names = npc;
        };
        set(&mut app, false, true);
        app.update();
        assert_eq!(shown(&mut app), ["a gnoll", "a gnoll pup"]);
        set(&mut app, true, false);
        app.update();
        assert_eq!(shown(&mut app), everyone[..3]);
        set(&mut app, true, true);
        app.update();
        assert_eq!(shown(&mut app), everyone);

        // A spawn no longer drawn loses its name.
        app.world_mut()
            .resource_mut::<crate::entities::NearbyEntities>()
            .rendered
            .remove(&3);
        app.update();
        assert_eq!(
            shown(&mut app),
            [everyone[0], everyone[1], everyone[2], everyone[4]]
        );
    }

    #[test]
    fn revealed_names_are_marked_without_granting_invisible_targeting() {
        let mut app = app();
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Visibility {
                spawn_id: 3,
                invisible: true,
            }],
        );
        app.update();
        assert!(
            !shown(&mut app)
                .iter()
                .any(|name| name.contains("a gnoll") && !name.contains("pup"))
        );
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(eq_client_core::qol::Fix::ShowInvisible, true);
        app.update();
        assert!(shown(&mut app).contains(&"(a gnoll)".to_owned()));
        assert_eq!(
            crate::target::unavailable(app.world().resource::<OnlineState>(), 3),
            Some("This entity is invisible")
        );
        assert!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .spawn(3)
                .unwrap()
                .state
                .invisible
        );
    }

    #[test]
    fn names_take_the_installed_clients_colours() {
        let mut app = app();
        let colour = |app: &mut App, words: &str| {
            app.world_mut()
                .query_filtered::<(&Text, &TextColor), With<NameTag>>()
                .iter(app.world())
                .find(|(text, _)| text.0.starts_with(words))
                .map(|(_, colour)| colour.0)
                .unwrap()
        };
        // Titanium's: players blue, their corpses and creatures teal.
        assert_eq!(colour(&mut app, "Examplar\n"), Color::srgb_u8(61, 107, 220));
        assert_eq!(colour(&mut app, "Examplar's"), Color::srgb_u8(23, 184, 173));
        assert_eq!(colour(&mut app, "a gnoll"), Color::srgb_u8(23, 184, 173));
        // A roleplaying player turns purple.
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Listing {
                spawn_id: 2,
                change: eq_client_core::listing::ListingChange::Anonymity(
                    eq_client_core::listing::Anonymity::Roleplaying,
                ),
            }],
        );
        app.update();
        assert_eq!(colour(&mut app, "Examplar\n"), Color::srgb_u8(133, 72, 156));
        // The Mac-era program uses the same supported categories. Guildmates
        // retain their player colour; corpses and pets are non-players.
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = eq_client_assets::ui::InstalledClient::EqMac;
        let mut pet = testing::pet(5, 1);
        pet.name = "Synthetic_pet001".into();
        testing::spawn_entry(&mut app.world_mut().resource_mut::<OnlineState>(), 5, pet);
        app.update();
        assert_eq!(colour(&mut app, "Examplar\n"), Color::srgb_u8(133, 72, 156));
        assert_eq!(colour(&mut app, "Example\n"), Color::srgb_u8(61, 107, 220));
        assert_eq!(colour(&mut app, "Examplar's"), Color::srgb_u8(23, 184, 173));
        assert_eq!(colour(&mut app, "a gnoll"), Color::srgb_u8(23, 184, 173));
        assert_eq!(
            colour(&mut app, "Synthetic pet"),
            Color::srgb_u8(23, 184, 173)
        );
    }

    #[test]
    fn a_name_behind_zone_geometry_hides_with_its_body() {
        let mut app = app();
        // A camera looking down -Z from behind everyone, onto an 800 by 600
        // window.
        let mut camera = Camera::default();
        camera.computed.clip_from_view =
            Mat4::perspective_infinite_reverse_rh(std::f32::consts::FRAC_PI_4, 4.0 / 3.0, 0.1);
        camera.computed.target_info = Some(bevy::camera::RenderTargetInfo {
            physical_size: UVec2::new(800, 600),
            scale_factor: 1.0,
        });
        app.world_mut().spawn((
            camera,
            Transform::from_xyz(0.0, 3.6, 20.0).looking_at(Vec3::new(0.0, 3.6, 0.0), Vec3::Y),
            crate::OrbitCamera {
                focus: Vec3::ZERO,
                radius: 20.0,
                yaw: 0.0,
                pitch: 0.0,
                pulled: None,
            },
        ));
        // The gnoll stands behind a wall; everyone else in the open.
        let gnoll = app
            .world()
            .resource::<crate::entities::NearbyEntities>()
            .rendered[&3];
        *app.world_mut().get_mut::<Transform>(gnoll).unwrap() =
            Transform::from_xyz(0.0, 0.0, -10.0);
        let wall = |z: f32| {
            let corner = |x: f32, y: f32| [x, y, z];
            [
                [corner(-5.0, -10.0), corner(5.0, -10.0), corner(5.0, 10.0)],
                [corner(-5.0, -10.0), corner(5.0, 10.0), corner(-5.0, 10.0)],
            ]
        };
        app.insert_resource(crate::Collision(Some(
            eq_client_core::movement::CollisionWorld::new(wall(-5.0)).unwrap(),
        )));
        let visible = |app: &mut App| {
            let mut tags: Vec<(String, bool)> = app
                .world_mut()
                .query_filtered::<(&Text, &Visibility), With<NameTag>>()
                .iter(app.world())
                .map(|(text, visibility)| (text.0.clone(), *visibility == Visibility::Inherited))
                .collect();
            tags.sort();
            tags
        };
        app.update();
        let shown = visible(&mut app);
        assert!(
            shown
                .iter()
                .all(|(words, shown)| *shown == (words != "a gnoll")),
            "{shown:?}"
        );
        // Without the zone's geometry, nothing stands in the way.
        app.insert_resource(crate::Collision(None));
        app.update();
        let shown = visible(&mut app);
        assert!(shown.iter().all(|(_, shown)| *shown), "{shown:?}");
        // With the far plane 25 deep, the gnoll 30 deep is past it, where
        // its model is left out, and so is its name; the rest, 20 deep, show.
        let camera = app
            .world_mut()
            .query_filtered::<Entity, With<crate::OrbitCamera>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .entity_mut(camera)
            .insert(Projection::Perspective(PerspectiveProjection {
                far: 25.0,
                ..default()
            }));
        app.update();
        let shown = visible(&mut app);
        assert!(
            shown
                .iter()
                .all(|(words, shown)| *shown == (words != "a gnoll")),
            "{shown:?}"
        );
    }

    #[test]
    fn show_names_and_new_last_names_reword_players() {
        let mut app = app();
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::LastName {
                name: "Examplar".into(),
                last_name: "Exemplum".into(),
            }],
        );
        app.update();
        assert_eq!(shown(&mut app)[0], "Examplar Exemplum\n<Example Guild>");
        let ask = |app: &mut App, input: &str| {
            let mut chat = app.world_mut().resource_mut::<crate::chat::ChatState>();
            crate::chat::client_request(input, &mut chat)
        };
        assert_eq!(ask(&mut app, "/shownames 1"), Some(Ok(())));
        app.update();
        assert_eq!(
            shown(&mut app),
            [
                "Examplar",
                "Examplar's corpse",
                "Example",
                "a gnoll",
                "a gnoll pup"
            ]
        );
        let options = app.world().resource::<crate::options::OptionsState>();
        assert_eq!(options.options.show_names, ShowNames::First);
        // Off hides players and their corpses alone.
        assert_eq!(ask(&mut app, "/shownames off"), Some(Ok(())));
        app.update();
        assert_eq!(shown(&mut app), ["a gnoll", "a gnoll pup"]);
        // A level the client cannot read gets the usage line.
        assert_eq!(ask(&mut app, "/shownames 9"), Some(Ok(())));
        app.update();
        let chat = app.world().resource::<crate::chat::ChatState>();
        let said: Vec<String> = chat
            .history
            .lines(eq_client_core::chat::ChatTab::All)
            .into_iter()
            .map(|(_, line)| line.message.text.clone())
            .collect();
        for line in [
            ShowNames::First.announcement().1,
            ShowNames::Off.announcement().1,
            eq_client_core::names::SHOW_NAMES_USAGE_TEXT,
        ] {
            assert!(said.contains(&line.to_owned()), "{said:?}");
        }
    }
}
