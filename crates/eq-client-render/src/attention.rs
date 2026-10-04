//! Flash on Tells ([`Fix::FlashOnTells`]): a tell that arrives while the
//! client is not the focused window asks for the player's attention, as the
//! window system shows it: on Windows the client's taskbar button flashes
//! until the client is focused again. The request is taken back once it is,
//! as some window systems keep it until then.
use bevy::{ecs::system::NonSendMarker, prelude::*, window::PrimaryWindow};
use eq_client_core::{
    chat::{ChannelName, ChatTab},
    qol::Fix,
};

/// The player's attention, as the client asks for it.
#[derive(Resource, Default)]
pub(crate) struct Attention {
    /// The newest chat line looked at.
    seen: u64,
    /// Whether the client asked for attention and has not been focused
    /// since.
    asked: bool,
    /// What the window is to do next: ask for attention (true), or take the
    /// request back.
    request: Option<bool>,
}

/// Watches the chat for tells, and has the window ask for attention.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<Attention>()
        .add_systems(Update, (watch, request).chain());
}

/// Asks for the player's attention when a tell arrives while the client is
/// not the focused window, where the player wants that, and takes the
/// request back once the client is focused again. Only a tell to the player
/// asks, not the echo of one they sent.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn watch(
    (chat, options): (
        Res<crate::chat::ChatState>,
        Res<crate::options::OptionsState>,
    ),
    windows: Query<&Window, With<PrimaryWindow>>,
    mut attention: ResMut<Attention>,
) {
    let revision = chat.history.revision();
    let told = revision > attention.seen
        && chat
            .history
            .lines(ChatTab::Tell)
            .iter()
            .any(|(id, line)| *id > attention.seen && line.channel == ChannelName::Tell);
    attention.seen = revision;
    // With no window, as in a test without one, there is nothing to ask.
    if windows.single().ok().is_none_or(|window| window.focused) {
        if attention.asked {
            attention.asked = false;
            attention.request = Some(false);
        }
    } else if told && options.options.qol.on(Fix::FlashOnTells) {
        attention.asked = true;
        attention.request = Some(true);
    }
}

/// Has the window ask for the player's attention, or take the request back,
/// as [`watch`] decided. The window system's windows live on the main
/// thread.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn request(
    mut attention: ResMut<Attention>,
    windows: Query<Entity, With<PrimaryWindow>>,
    _main_thread: NonSendMarker,
) {
    let (Some(asking), Ok(window)) = (attention.request.take(), windows.single()) else {
        return;
    };
    bevy::winit::WINIT_WINDOWS.with_borrow(|winit| {
        if let Some(window) = winit.get_window(window) {
            // winit's default request is its gentler one, Informational: on
            // Windows the taskbar button flashes until the client is
            // focused, where Critical flashes the window too. `bevy::winit`
            // does not re-export its type, hence `Default`.
            window.request_user_attention(asking.then(Default::default));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::chat::{ChatLine, Message, Source};

    fn line(channel: ChannelName) -> ChatLine {
        ChatLine {
            channel,
            message_type: None,
            sender: Some("Examplar".into()),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: "Synthetic line".into(),
                item_links: Vec::new(),
            },
            source: Source::Server,
        }
    }

    #[test]
    fn a_tell_asks_for_attention_only_while_the_client_is_not_focused() {
        let mut app = App::new();
        app.init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::options::OptionsState>()
            .init_resource::<Attention>()
            .add_systems(Update, watch);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: false,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        let say = |app: &mut App, channel| {
            app.world_mut()
                .resource_mut::<crate::chat::ChatState>()
                .history
                .push(line(channel));
            app.update();
            app.world_mut().resource_mut::<Attention>().request.take()
        };
        // Another channel, or the echo of a tell the player sent, asks
        // nothing.
        assert_eq!(say(&mut app, ChannelName::Say), None);
        assert_eq!(say(&mut app, ChannelName::TellEcho), None);
        // A tell asks, and the request is taken back once the client is
        // focused.
        assert_eq!(say(&mut app, ChannelName::Tell), Some(true));
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_eq!(
            app.world_mut().resource_mut::<Attention>().request.take(),
            Some(false)
        );
        // Focused, a tell asks nothing.
        assert_eq!(say(&mut app, ChannelName::Tell), None);
        // With the setting off, nor does one while the client is not
        // focused.
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .set(
                eq_client_core::options::Toggle::Qol(Fix::FlashOnTells),
                false,
            );
        assert_eq!(say(&mut app, ChannelName::Tell), None);
    }

    #[test]
    fn asking_with_no_window_system_does_nothing() {
        let mut app = App::new();
        app.insert_resource(Attention {
            request: Some(true),
            ..default()
        })
        .add_systems(Update, request);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.update();
        assert_eq!(app.world().resource::<Attention>().request, None);
    }
}
