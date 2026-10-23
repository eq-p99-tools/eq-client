//! The skin's Skills window: the skills the player has, by name, with their
//! values, opened from the inventory's Skills button as in the official
//! client. A skill the player has not learned yet is left out, since the
//! client does not know which ones the class may learn.
use crate::{
    online::OnlineState,
    theme::{self, Size},
};
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
}

impl SkillsList {
    pub(crate) fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
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
    let wanted = rows(online.world());
    for (entity, mut list, children) in &mut lists {
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
            for (words, width) in cells.into_iter().zip(&list.columns) {
                commands.spawn((
                    theme::text(words, Size::Small, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    Node {
                        width: px(*width),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ChildOf(line),
                ));
            }
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
}
