//! Bind the installed Load Skin window to the same loader used by chat.
use super::{LoadSkin, UiSkin};
use crate::{
    ViewerSettings,
    chat::ChatState,
    clicks::Clicks,
    theme,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;

#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Open,
    Apply,
}

#[derive(Component)]
pub(crate) struct List;

#[derive(Component)]
pub(crate) struct Folder(pub(super) String);

#[derive(Resource, Default)]
pub(crate) struct Selection(Option<String>);

pub(super) fn folders(root: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("uifiles")) else {
        return Vec::new();
    };
    let mut names: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|ty| ty.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| eq_client_assets::ui::skin_file(root, name, "EQUI.xml").is_ok())
        .collect();
    names.sort_by_cached_key(|name| name.to_ascii_lowercase());
    names
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn fill(
    mut commands: Commands,
    lists: Query<Entity, Added<List>>,
    settings: Res<ViewerSettings>,
    selected: Res<Selection>,
    mut rows: Query<(&Folder, &mut BackgroundColor)>,
) {
    for list in &lists {
        if let Some(root) = settings.0.eq_directory.as_deref() {
            commands.entity(list).with_children(|ui| {
                for name in folders(root) {
                    ui.spawn((
                        Button,
                        Folder(name.clone()),
                        Node {
                            width: percent(100),
                            min_height: px(18),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(if selected.0.as_ref() == Some(&name) {
                            theme::INSET
                        } else {
                            Color::NONE
                        }),
                    ))
                    .with_child((
                        theme::text(name, theme::Size::Small, theme::INK_BRIGHT),
                        bevy::ui::FocusPolicy::Pass,
                    ));
                }
            });
        }
    }
    if selected.is_changed() {
        for (folder, mut bg) in &mut rows {
            bg.set_if_neq(BackgroundColor(if selected.0.as_ref() == Some(&folder.0) {
                theme::INSET
            } else {
                Color::NONE
            }));
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn input(
    actions: Clicks<Action>,
    rows: Clicks<Folder>,
    skin: Res<UiSkin>,
    mut selected: ResMut<Selection>,
    mut shown: ResMut<Shown>,
    mut chat: Option<ResMut<ChatState>>,
) {
    if shown.displayed(WindowId::LoadSkin) {
        for folder in rows.iter() {
            selected.0 = Some(folder.0.clone());
        }
    }
    for action in actions.iter() {
        match action {
            Action::Open => {
                selected.0 = Some(skin.0.clone());
                shown.open(WindowId::LoadSkin);
            }
            Action::Apply if shown.displayed(WindowId::LoadSkin) => {
                if let (Some(folder), Some(chat)) = (&selected.0, chat.as_mut()) {
                    chat.load_skin = Some(LoadSkin::Skin(folder.clone()));
                    shown.close(WindowId::LoadSkin);
                }
            }
            Action::Apply => {}
        }
    }
}
