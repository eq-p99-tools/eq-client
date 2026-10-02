//! Names over heads: each character's name above them and a player's guild
//! under it, while the Options window's Show PC Names and Show NPC Names
//! allow them. They follow the models through the frame and draw under
//! every window.
use bevy::prelude::*;
use eq_client_core::SpawnKind;
use std::collections::BTreeMap;

/// How far above the head a name sits, in world units.
const ABOVE_HEAD: f32 = 0.6;

/// The top of what a model root draws, in the root's own units: where a
/// name over it sits. Whatever draws a root sets it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Overhead(pub(crate) f32);

/// A name drawn over the root it follows.
#[derive(Component)]
pub(crate) struct NameTag;

/// The names drawn, by the root each follows.
#[derive(Resource, Default)]
pub(crate) struct NameTags(BTreeMap<Entity, Entity>);

/// What a name over a root says.
#[derive(Debug, PartialEq, Eq)]
struct Label {
    /// The name, then the guild's line when there is one.
    words: String,
    /// Whether Show PC Names governs it; Show NPC Names governs the rest.
    pc: bool,
}

/// Draws a name over the player's head and every drawn spawn's, and takes
/// away those of spawns gone or hidden by the options.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn update(
    mut commands: Commands,
    (online, nearby, options, scale): (
        Res<crate::online::OnlineState>,
        Res<crate::entities::NearbyEntities>,
        Res<crate::options::OptionsState>,
        Option<Res<UiScale>>,
    ),
    mut drawn: ResMut<NameTags>,
    player: Query<Entity, With<crate::Player>>,
    roots: Query<(&Transform, &Overhead)>,
    cameras: Query<(&Camera, &Transform), With<crate::OrbitCamera>>,
    mut tags: Query<(&mut Node, &mut Text, &mut Visibility), With<NameTag>>,
) {
    let world = online.world();
    let mut wanted = BTreeMap::new();
    if let (Ok(root), Some(me)) = (player.single(), world.player()) {
        wanted.insert(root, label(&me.name, SpawnKind::Player, &me.listing, world));
    }
    for (id, root) in &nearby.rendered {
        if let Some(spawn) = world.spawn(*id).map(|spawn| &spawn.state) {
            wanted.insert(*root, label(&spawn.name, spawn.kind, &spawn.listing, world));
        }
    }
    wanted.retain(|_, label| {
        if label.pc {
            options.options.pc_names
        } else {
            options.options.npc_names
        }
    });
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
    for (root, label) in wanted {
        // Where the head's top shows, in the UI's pixels from the window's
        // left and bottom edges; none behind the camera or off the view.
        let place = camera.and_then(|(camera, view)| {
            let (transform, Overhead(top)) = roots.get(root).ok()?;
            let above = transform.transform_point(Vec3::Y * *top) + Vec3::Y * ABOVE_HEAD;
            let at = camera
                .world_to_viewport(&GlobalTransform::from(*view), above)
                .ok()?;
            let size = camera.logical_viewport_size()?;
            (at.cmpge(Vec2::ZERO).all() && at.cmple(size).all())
                .then(|| Vec2::new(at.x, size.y - at.y) / factor)
        });
        let Some(&tag) = drawn.0.get(&root) else {
            drawn.0.insert(root, spawn_tag(&mut commands, label.words));
            continue;
        };
        let Ok((mut node, mut text, mut visibility)) = tags.get_mut(tag) else {
            continue;
        };
        if text.0 != label.words {
            text.0 = label.words;
        }
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

/// A name that shows once it is placed.
fn spawn_tag(commands: &mut Commands, words: String) -> Entity {
    commands
        .spawn((
            NameTag,
            Text::new(words),
            crate::theme::font(crate::theme::Size::Label),
            TextColor(Color::WHITE),
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

/// What the name over a spawn says: its name as the client shows names,
/// and a player's guild under it in angle brackets. Corpses go with the
/// players or creatures they were.
fn label(
    name: &str,
    kind: SpawnKind,
    listing: &eq_client_core::listing::Listing,
    world: &eq_client_core::world::ClientWorld,
) -> Label {
    let name = eq_client_core::entities::display_name(name);
    let guild = listing
        .guild
        .filter(|_| kind == SpawnKind::Player)
        .and_then(|guild| world.guild_name(guild));
    Label {
        words: match guild {
            Some(guild) => format!("{name}\n<{guild}>"),
            None => name,
        },
        pc: matches!(kind, SpawnKind::Player | SpawnKind::PlayerCorpse),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::SpawnState;

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

    #[test]
    fn names_follow_the_drawn_spawns_and_the_options() {
        let mut app = crate::testing::app();
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            let mut me = testing::player(1);
            me.listing.guild = Some(7);
            testing::admit(&mut online, 1, me);
            testing::news(
                &mut online,
                [eq_client_core::WorldEvent::GuildNames(vec![(
                    7,
                    "Example Guild".into(),
                )])],
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
        app.add_systems(Update, update);
        app.update();
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
}
