//! The skin's Skills window: the skills the player has, by name, with their
//! values, opened from the inventory's Skills button as in the official
//! client. A skill the player has not learned yet is left out, since the
//! client does not know which ones the class may learn.
use crate::online::OnlineState;
use bevy::prelude::*;
use eq_client_core::world::ClientWorld;

/// The Skills window's list, which the client fills from the player's
/// skills.
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct SkillsList {
    /// Each column's width, from the left.
    columns: Vec<f32>,
    /// The rows last drawn, to redraw only when one changes.
    drawn: Option<Vec<(String, u32)>>,
    /// Inputs last projected; None also covers a newly created window.
    checked: Option<u64>,
}

impl SkillsList {
    pub(crate) fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
            checked: None,
        }
    }
}

/// Values servers use to mark a skill as reset rather than learned.
pub(crate) const RESET: u32 = 254;

/// The player's learned skills by name, with their values.
fn rows(world: &ClientWorld) -> Vec<(String, u32)> {
    let skills = world
        .player()
        .and_then(|player| player.skills.as_deref())
        .unwrap_or_default();
    let mut rows: Vec<(String, u32)> = skills
        .iter()
        .enumerate()
        .filter(|(_, value)| (1..RESET).contains(*value))
        .filter_map(|(skill, value)| {
            let skill = u32::try_from(skill).ok()?;
            Some((eq_client_core::skills::name(skill)?.to_owned(), *value))
        })
        .collect();
    rows.sort();
    rows
}

/// The height of a row of the list.
const ROW_HEIGHT: f32 = 16.0;

/// Fills the Skills window's list again whenever a skill or its value
/// changes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut lists: Query<(Entity, &mut SkillsList, Option<&Children>)>,
) {
    let input = online.world().skills_revision();
    // No list, or every existing list is current: no row allocation or sorting.
    if lists.iter().all(|(_, list, _)| list.checked == Some(input)) {
        return;
    }
    let wanted = rows(online.world());
    for (entity, mut list, children) in &mut lists {
        if list.checked == Some(input) {
            continue;
        }
        list.checked = Some(input);
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        if let Some(children) = children {
            for child in children {
                commands.entity(*child).despawn();
            }
        }
        for (name, value) in &wanted {
            // The official client's rank for a value is not known yet.
            let cells = [name.clone(), String::new(), value.to_string()];
            let line = commands
                .spawn((
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ChildOf(entity),
                ))
                .id();
            crate::list_cells::text(&mut commands, line, cells, list.columns.iter().copied());
        }
        list.drawn = Some(wanted.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;

    #[test]
    fn the_list_names_each_learned_skill_with_its_value() {
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        let mut skills = vec![0; 100];
        skills[30] = 20;
        skills[10] = 5;
        skills[29] = RESET;
        player.skills = Some(skills);
        testing::admit(&mut online, 1, player);
        assert_eq!(
            rows(online.world()),
            [("Bash".to_owned(), 5), ("Kick".to_owned(), 20)]
        );
    }

    #[test]
    fn skill_lists_refresh_on_changes_and_recreation_but_not_unrelated_news() {
        use eq_client_core::WorldEvent;
        let mut app = crate::testing::app();
        app.add_systems(Update, fill);
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.skills = Some(vec![1; 100]);
        testing::admit(&mut online, 1, player);
        app.insert_resource(online);
        let list = app.world_mut().spawn(SkillsList::new(vec![100.0; 3])).id();
        app.update();
        let children = app.world().get::<Children>(list).unwrap().to_vec();
        let revision = app.world().get::<SkillsList>(list).unwrap().checked;
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Coins(eq_client_core::Coins::default())],
        );
        app.update();
        assert_eq!(
            app.world().get::<Children>(list).unwrap().to_vec(),
            children
        );
        assert_eq!(
            app.world().get::<SkillsList>(list).unwrap().checked,
            revision
        );

        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Skill {
                skill_id: 30,
                value: 7,
            }],
        );
        app.update();
        assert!(
            app.world()
                .get::<SkillsList>(list)
                .unwrap()
                .drawn
                .as_ref()
                .unwrap()
                .contains(&("Kick".to_owned(), 7))
        );
        assert_ne!(
            app.world().get::<Children>(list).unwrap().to_vec(),
            children
        );

        let new_list = app.world_mut().spawn(SkillsList::new(vec![100.0; 3])).id();
        app.update();
        assert_eq!(
            app.world().get::<SkillsList>(new_list).unwrap().drawn,
            app.world().get::<SkillsList>(list).unwrap().drawn
        );
        assert!(app.world().get::<Children>(new_list).is_some());
        app.world_mut().resource_mut::<OnlineState>().restart();
        app.update();
        for list in [list, new_list] {
            assert!(
                app.world()
                    .get::<Children>(list)
                    .is_none_or(Children::is_empty)
            );
        }
    }
}
