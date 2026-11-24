//! A Bard's `/melody` (Titanium eqgame.exe 0x472156, read R49-33 c, plan
//! `plan-melody-r49-33c-20261123.md`): the command and each step, through
//! the gem press a click makes. The list is the world's
//! (`ClientWorld::start_melody`); the session adds nothing for it.

use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand};

/// The Bard's class number, as the program's song test reads it.
const BARD: u32 = 8;

/// Answers a typed `/melody` and makes each owed step. A step is the
/// program's two presses at the player's own gem refresh: the song stop,
/// then the next gem, cycling. A refused press says its line and the next
/// refresh plays on; with no refresh the melody stalls, as it has no timer
/// or retry (reviewer Y1, [I]).
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn melody(
    map: Res<crate::keys::KeyMap>,
    mut chat: ResMut<crate::chat::ChatState>,
    mut online: ResMut<crate::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    definitions: (
        Res<crate::spellbook::SpellNames>,
        Res<super::messages::Messages>,
        Res<crate::ViewerSettings>,
    ),
    options: Option<Res<crate::options::OptionsState>>,
) {
    let definitions = (&*definitions.0, &*definitions.1, &*definitions.2);
    if let Some(words) = chat.melody_asked.take()
        && let Some(gem) = start(&words, &mut chat, &mut online, &outbox)
    {
        super::press_gem(
            (gem, false),
            (&mut chat, &online, &outbox),
            definitions,
            (&map, options.as_deref()),
        );
    }
    if let Some(gem) = online.take_melody_step() {
        stop_song(&online, &outbox);
        super::press_gem(
            (gem, false),
            (&mut chat, &online, &outbox),
            definitions,
            (&map, options.as_deref()),
        );
    }
}

/// `/melody`'s own steps: where the server type takes it, a Bard's clears
/// the list, then stops the song while one is sung, or else starts the
/// list and gives the first gem to press. Anyone else's does nothing and
/// says nothing, as the program's. Where it isn't offered it answers as an
/// unknown game command does here.
fn start(
    words: &str,
    chat: &mut crate::chat::ChatState,
    online: &mut crate::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) -> Option<u8> {
    let world = online.world();
    if !world.can(Capability::Melody) {
        let typed = if words.is_empty() {
            "/melody".to_owned()
        } else {
            format!("/melody {words}")
        };
        chat.refuse(format!("{typed} is not a game command"));
        return None;
    }
    if world.player()?.class != Some(BARD) {
        return None;
    }
    online.clear_melody();
    if online.world().casting().singing.is_some() {
        stop_song(online, outbox);
        return None;
    }
    online.start_melody(eq_client_core::world::melody_gems(words))
}

/// The program's song stop, as `/stopsong` sends it.
fn stop_song(online: &crate::online::OnlineState, outbox: &crate::outbox::Outbox) {
    let _ = outbox.post(online.world(), |stamp| ClientCommand::StopSong {
        session_id: stamp.session_id,
        created: stamp.created,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::{SpellUpdate, WorldEvent};
    use std::sync::mpsc::Receiver;

    /// A Bard with songs 73, 74 and 75 in gems 1 to 3, admitted in session
    /// 7 where the server type offers `/melody` or not.
    fn bard_app(class: u32, offered: bool) -> (App, Receiver<ClientCommand>) {
        let mut player = crate::hud::tests::caster();
        player.class = Some(class);
        player.memorized_spells[1] = Some(74);
        player.memorized_spells[2] = Some(75);
        let mut online = OnlineState::new(true);
        let capabilities = Capability::ALL
            .into_iter()
            .filter(|capability| offered || *capability != Capability::Melody)
            .collect();
        testing::enter_with(&mut online, 7, player, None, capabilities);
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        let (tx, rx) = std::sync::mpsc::sync_channel(16);
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(tx)))
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::spellbook::SpellNames>()
            .init_resource::<crate::ViewerSettings>()
            .init_resource::<super::super::messages::Messages>()
            .add_systems(Update, melody);
        (app, rx)
    }

    fn typed(app: &mut App, words: &str) {
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .melody_asked = Some(words.into());
        app.update();
    }

    fn news(app: &mut App, event: WorldEvent) {
        testing::news(&mut app.world_mut().resource_mut::<OnlineState>(), [event]);
    }

    fn refreshed(app: &mut App, gem: u32, spell_id: u32) {
        news(
            app,
            WorldEvent::Spell(SpellUpdate::BarRefresh {
                slot: gem,
                spell_id,
                reduction_ms: 0,
            }),
        );
        app.update();
    }

    /// What went out, as `s` for a stop and the gem's number for a cast.
    fn sent(rx: &Receiver<ClientCommand>) -> String {
        rx.try_iter()
            .map(|command| match command {
                ClientCommand::StopSong { .. } => "s".to_owned(),
                ClientCommand::CastSpell { gem, .. } => (gem + 1).to_string(),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn gems(app: &App) -> Vec<u8> {
        app.world()
            .resource::<OnlineState>()
            .world()
            .casting()
            .melody
            .gems()
            .to_vec()
    }

    #[test]
    fn a_bards_melody_presses_the_first_gem_then_a_stop_and_the_next_at_each_refresh() {
        let (mut app, rx) = bard_app(BARD, true);
        typed(&mut app, "1 3 2");
        assert_eq!(sent(&rx), "1");
        // Nothing moves it but the player's own gem refresh.
        app.update();
        assert_eq!(sent(&rx), "");
        refreshed(&mut app, 0, 73);
        assert_eq!(sent(&rx), "s 3");
        refreshed(&mut app, 2, 75);
        assert_eq!(sent(&rx), "s 2");
        refreshed(&mut app, 1, 74);
        assert_eq!(sent(&rx), "s 1");
        // A refresh for no gem of the player's makes no step.
        refreshed(&mut app, 5, 999);
        assert_eq!(sent(&rx), "");
        // A new admission starts with no melody.
        testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            8,
            crate::hud::tests::caster(),
        );
        assert_eq!(gems(&app), Vec::<u8>::new());
    }

    #[test]
    fn a_melody_while_singing_only_stops_the_song_and_an_empty_one_only_clears() {
        let (mut app, rx) = bard_app(BARD, true);
        typed(&mut app, "1 2");
        assert_eq!(sent(&rx), "1");
        news(
            &mut app,
            WorldEvent::Singing {
                session_id: 7,
                spell_id: Some(73),
            },
        );
        typed(&mut app, "2 3");
        assert_eq!(sent(&rx), "s");
        assert_eq!(gems(&app), Vec::<u8>::new());
        refreshed(&mut app, 0, 73);
        assert_eq!(sent(&rx), "");
        news(
            &mut app,
            WorldEvent::Singing {
                session_id: 7,
                spell_id: None,
            },
        );
        typed(&mut app, "1 2");
        assert_eq!(sent(&rx), "1");
        typed(&mut app, "");
        assert_eq!(sent(&rx), "");
        assert_eq!(gems(&app), Vec::<u8>::new());
    }

    #[test]
    fn a_typed_or_scripted_melody_line_waits_for_this_system() {
        // A script's `slash("melody 1 2")` runs the line as typed.
        let mut chat = crate::chat::ChatState::default();
        assert_eq!(
            crate::chat::client_request_in_scope("/melody 1 2", &mut chat, None),
            Some(Ok(()))
        );
        assert_eq!(chat.melody_asked.as_deref(), Some("1 2"));
    }

    #[test]
    fn only_a_bard_where_the_server_type_offers_it_plays_a_melody() {
        // Anyone else: nothing sent and nothing said, as the program.
        let (mut app, rx) = bard_app(2, true);
        let said = |app: &App| {
            app.world()
                .resource::<crate::chat::ChatState>()
                .history
                .lines(eq_client_core::chat::ChatTab::All)
                .len()
        };
        let before = said(&app);
        typed(&mut app, "1 2");
        assert_eq!(sent(&rx), "");
        assert_eq!(said(&app), before);
        // Where it isn't offered, it answers as an unknown game command.
        let (mut app, rx) = bard_app(BARD, false);
        typed(&mut app, "1 2");
        assert_eq!(sent(&rx), "");
        assert!(
            app.world()
                .resource::<crate::chat::ChatState>()
                .newest()
                .contains("/melody 1 2 is not a game command")
        );
    }
}
