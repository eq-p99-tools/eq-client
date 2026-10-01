//! Which UI skin the windows follow: the one named on the command line, else the
//! one the character last chose in the official client, else the default skin.
use bevy::prelude::*;
use eq_client_assets::ui::{DEFAULT_SKIN, chosen_skin};

/// The skin windows read their layouts from.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub(super) struct UiSkin(pub String);

impl Default for UiSkin {
    fn default() -> Self {
        Self(DEFAULT_SKIN.into())
    }
}

/// Keeps [`UiSkin`] current, before the windows that draw with it.
pub(super) fn register(app: &mut App) {
    app.add_systems(Update, follow.in_set(super::Stage::Scene));
}

/// Picks the skin again when the character or world changes, reading the
/// character's UI settings file only then.
#[allow(clippy::needless_pass_by_value)]
fn follow(
    online: Res<super::online::OnlineState>,
    settings: Res<super::ViewerSettings>,
    mut skin: ResMut<UiSkin>,
    mut chosen_for: Local<Option<(String, String)>>,
) {
    if let Some(name) = &settings.0.ui_skin {
        if skin.0 != *name {
            skin.0.clone_from(name);
        }
        return;
    }
    let current = online
        .world()
        .player()
        .map(|player| player.name.as_str())
        .zip(online.world().world_name());
    if chosen_for
        .as_ref()
        .map(|(character, world)| (character.as_str(), world.as_str()))
        == current
    {
        return;
    }
    *chosen_for = current.map(|(character, world)| (character.to_owned(), world.to_owned()));
    let chosen = current
        .zip(settings.0.eq_directory.as_deref())
        .and_then(|((character, world), directory)| chosen_skin(directory, character, world))
        .unwrap_or_else(|| DEFAULT_SKIN.into());
    if skin.0 != chosen {
        info!("UI skin: {chosen}");
        skin.0 = chosen;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn app(install: &Path, ui_skin: Option<&str>) -> App {
        let mut app = App::new();
        app.init_resource::<UiSkin>()
            .insert_resource(super::super::online::OnlineState::new(true))
            .insert_resource(super::super::ViewerSettings(super::super::ViewerConfig {
                eq_directory: Some(install.to_path_buf()),
                ui_skin: ui_skin.map(Into::into),
                ..Default::default()
            }))
            .add_systems(Update, follow);
        app
    }

    fn enter(app: &mut App, name: &str) {
        crate::online::testing::admit(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            1,
            eq_client_core::PlayerState {
                name: name.into(),
                base_attributes: None,
                deity: None,
                class: Some(1),
                spawn_id: 7,
                race: 1,
                gender: 0,
                level: 1,
                position: eq_client_core::WorldPosition::default(),
                mana: 0,
                endurance: None,
                skills: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 0.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hp_percent: None,
                appearance: eq_client_core::outfit::Appearance::default(),
                listing: eq_client_core::listing::Listing::default(),
            },
        );
    }

    fn skin(app: &App) -> &str {
        &app.world().resource::<UiSkin>().0
    }

    #[test]
    fn a_characters_chosen_skin_applies_once_its_world_is_known() {
        let install = std::env::temp_dir().join(format!("eq-skin-follow-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(
            install.join("UI_Example_ExampleWorld.ini"),
            "[Main]\nUISkin=velious\n",
        )
        .unwrap();
        let mut app = app(&install, None);
        enter(&mut app, "Example");
        app.update();
        assert_eq!(skin(&app), DEFAULT_SKIN);
        crate::online::testing::news(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            [eq_client_core::WorldEvent::WorldName {
                short_name: "ExampleWorld".into(),
            }],
        );
        app.update();
        let chosen = skin(&app).to_owned();
        // Another character without settings goes back to the default skin.
        enter(&mut app, "Other");
        app.update();
        let other = skin(&app).to_owned();
        let mut overridden = self::app(&install, Some("custom"));
        enter(&mut overridden, "Example");
        overridden.update();
        std::fs::remove_dir_all(&install).unwrap();
        assert_eq!(chosen, "velious");
        assert_eq!(other, DEFAULT_SKIN);
        // A skin named on the command line wins.
        assert_eq!(skin(&overridden), "custom");
    }
}
