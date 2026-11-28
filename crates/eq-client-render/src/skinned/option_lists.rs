//! The stock Options lists retain the coordinates, columns and scrollbar supplied by the skin.
use super::{
    color_picker::{Binding, Swatch, draw_swatch},
    colors,
};
use crate::{online::OnlineState, options::OptionsState, theme, windows::WindowId};
use bevy::prelude::*;
use eq_client_core::{
    GameDialect,
    chat_filters::{has_row, kind_words},
    filters::FilterCategory,
};

#[derive(Resource)]
pub(crate) struct Selection {
    pub(super) filter: FilterCategory,
    color: u8,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            filter: FilterCategory::Ooc,
            color: 1,
        }
    }
}

pub(super) type ChoiceData<'w> = (
    Option<ResMut<'w, OptionsState>>,
    Option<Res<'w, Selection>>,
    Option<Res<'w, OnlineState>>,
);

#[derive(Component, Default)]
pub(super) struct FilterRows(Option<GameDialect>);
#[derive(Component)]
pub(super) struct ColorRows;
#[derive(Component)]
pub(super) struct ChangeColor;

#[derive(Component, Clone, Copy)]
enum Row {
    Filter(FilterCategory),
    Color(u8),
}

fn row() -> Node {
    Node {
        width: percent(100),
        min_height: px(21),
        flex_shrink: 0.0,
        align_items: AlignItems::Center,
        column_gap: px(5),
        ..default()
    }
}

pub(super) fn chat_rows(rows: &mut EntityCommands) {
    rows.with_children(|list| {
        for (kind, name) in colors::chat_names() {
            list.spawn((Button, Row::Color(kind), row()))
                .with_children(|ui| {
                    draw_swatch(
                        ui,
                        WindowId::Options,
                        Binding::Chat(kind),
                        Node {
                            width: px(18),
                            height: px(18),
                            ..default()
                        },
                        theme::Size::Small,
                    );
                    ui.spawn(theme::text(name, theme::Size::Small, theme::INK_BRIGHT));
                });
        }
    });
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(crate) fn update(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut selected: ResMut<Selection>,
    mut lists: Query<(Entity, &mut FilterRows)>,
    mut rows: Query<(&Row, &Interaction, &mut BackgroundColor)>,
    mut change: Query<&mut Swatch, With<ChangeColor>>,
) {
    let dialect = online.world().generation();
    for (entity, mut last) in &mut lists {
        if last.0 == Some(dialect) {
            continue;
        }
        last.0 = Some(dialect);
        let mut list = commands.entity(entity);
        list.despawn_children();
        list.with_children(|list| {
            for category in FilterCategory::ALL
                .into_iter()
                .filter(|c| has_row(*c) && !c.modes(dialect).is_empty())
            {
                list.spawn((
                    Button,
                    Row::Filter(category),
                    row(),
                    children![theme::text(
                        kind_words(category),
                        theme::Size::Small,
                        theme::INK_BRIGHT
                    )],
                ));
            }
        });
    }
    for (row, interaction, _) in &mut rows {
        if *interaction == Interaction::Pressed {
            match row {
                Row::Color(n) => selected.color = *n,
                Row::Filter(c) => selected.filter = *c,
            }
        }
    }
    for (row, _, mut bg) in &mut rows {
        let on = match row {
            Row::Color(n) => selected.color == *n,
            Row::Filter(c) => selected.filter == *c,
        };
        bg.set_if_neq(BackgroundColor(if on { theme::INSET } else { Color::NONE }));
    }
    for mut field in &mut change {
        field.select(Binding::Chat(selected.color));
    }
}
