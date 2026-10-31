//! Being translocated, as the installed client asks it where its rules are
//! read ([`eq_client_assets::ui::InstalledClient::translocate_offer`]): an
//! offer opens the skin's confirmation dialog (`confirm`) with the
//! client's question, and Yes or No answers it. The client says No by
//! itself when the offer waits too long, or at once when the dialog already
//! asks something else as the offer comes. On Yes the server moves the
//! player. A client whose rules are unread leaves offers unanswered.
use crate::{hud::messages::Messages, online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_assets::ui::TranslocateQuestion;
use eq_client_core::{ClientCommand, chat::ChatLine, world::ClientWorld};
use std::time::{Duration, Instant};

/// What the dialog asks about the offer waiting, in the installed client's
/// words where it has them.
pub(crate) fn question(
    world: &ClientWorld,
    rules: TranslocateQuestion,
    messages: Option<&Messages>,
) -> String {
    let Some((offer, _)) = world.translocate() else {
        return String::new();
    };
    if rules.bind_spells.contains(&offer.spell_id) {
        return messages.map_or_else(
            || "You are being sent to your bind point. Do you accept?".to_owned(),
            |messages| messages.format(rules.to_bind, &[]),
        );
    }
    let caster = eq_client_core::entities::display_name(&offer.caster);
    let zone = eq_client_core::zones::short_name(u32::from(offer.zone_id))
        .and_then(eq_client_core::zones::long_name)
        .filter(|long| !long.is_empty())
        .map_or_else(|| format!("zone {}", offer.zone_id), str::to_owned);
    messages.map_or_else(
        || format!("{caster} offers to send you to {zone}. Do you accept?"),
        |messages| messages.format(rules.named, &[caster.clone(), zone.clone()]),
    )
}

/// Answers the offer waiting, Yes (true) or No; after a No, the line the
/// client says then, if `say` and it has one.
pub(crate) fn answer(
    online: &mut OnlineState,
    outbox: &Outbox,
    accept: bool,
    declined: Option<(u32, &Messages)>,
) -> Option<ChatLine> {
    online.world().translocate()?;
    outbox
        .post(online.world(), |stamp| ClientCommand::AnswerTranslocate {
            session_id: stamp.session_id,
            accept,
        })
        .ok()?;
    online.answer_translocate();
    let (line, messages) = declined.filter(|_| !accept)?;
    Some(crate::chat::system_line(messages.said(line, &[])))
}

/// Whether an offer that came at `came` has waited too long by `now`.
pub(crate) fn expired(rules: TranslocateQuestion, came: Instant, now: Instant) -> bool {
    now.saturating_duration_since(came) >= Duration::from_millis(rules.expires_ms)
}

/// Says No for the player where the client does: to an offer that waited
/// too long (with the line a No says), and at once, without it, to one that
/// came while the dialog asked something else. Whether the program says the
/// line on that second No is unread (6.253).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn decline_unasked(
    (mut online, outbox, asked): (ResMut<OnlineState>, Res<Outbox>, Res<crate::confirm::Asked>),
    (settings, messages, mut chat): (
        Res<crate::ViewerSettings>,
        Option<Res<Messages>>,
        ResMut<crate::chat::ChatState>,
    ),
    mut seen: Local<Option<Instant>>,
) {
    let Some(rules) = settings.0.installed_client.translocate_offer() else {
        return;
    };
    let Some((_, came)) = online.world().translocate() else {
        *seen = None;
        return;
    };
    let new = *seen != Some(came);
    *seen = Some(came);
    let busy = online.world().resurrection().is_some() || asked.question().is_some();
    let said = if new && busy {
        answer(&mut online, &outbox, false, None)
    } else if expired(rules, came, Instant::now()) {
        let declined = rules.declined.zip(messages.as_deref());
        answer(&mut online, &outbox, false, declined)
    } else {
        None
    };
    if let Some(line) = said {
        chat.history.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        online::testing,
        windows::{Shown, WindowId},
    };
    use eq_client_assets::ui::InstalledClient;
    use eq_client_core::WorldEvent;

    const RULES: TranslocateQuestion = InstalledClient::EqMac.translocate_offer().unwrap();

    /// A translocate from `caster` to zone 2 with spell `spell`, as the
    /// Mac-era record lays it out.
    fn offer(caster: &[u8], spell: u32) -> WorldEvent {
        let mut body = vec![0; 92];
        body[..4].copy_from_slice(&2u32.to_le_bytes());
        body[4..8].copy_from_slice(&spell.to_le_bytes());
        body[12..12 + caster.len()].copy_from_slice(caster);
        match eq_client_core::confirmations::eqmac_translocate(&body).unwrap() {
            eq_client_core::confirmations::Translocate::Offer(offer) => {
                WorldEvent::TranslocateOffer(offer)
            }
            eq_client_core::confirmations::Translocate::Go(_) => unreachable!(),
        }
    }

    fn admitted() -> OnlineState {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        online
    }

    #[test]
    fn the_question_names_the_caster_and_zone_or_the_bind_point() {
        let mut online = admitted();
        assert_eq!(question(online.world(), RULES, None), "");
        testing::news(&mut online, [offer(b"Tester00", 1)]);
        let zone = eq_client_core::zones::short_name(2)
            .and_then(eq_client_core::zones::long_name)
            .unwrap();
        assert_eq!(
            question(online.world(), RULES, None),
            format!("Tester offers to send you to {zone}. Do you accept?")
        );
        testing::news(&mut online, [offer(b"Tester00", 1334)]);
        assert!(question(online.world(), RULES, None).contains("bind point"));
    }

    #[test]
    fn an_offer_waits_until_its_time_runs_out() {
        let came = Instant::now();
        assert!(!expired(RULES, came, came + Duration::from_secs(3599)));
        assert!(expired(RULES, came, came + Duration::from_secs(3600)));
    }

    fn app(client: InstalledClient) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(Outbox::new(Some(tx)))
            .init_resource::<crate::confirm::Asked>()
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, (decline_unasked, crate::confirm::window).chain());
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = client;
        (app, rx)
    }

    fn open(app: &App) -> bool {
        app.world()
            .resource::<Shown>()
            .is_open(WindowId::Confirmation)
    }

    #[test]
    fn an_offer_opens_the_dialog_until_it_is_answered() {
        let (mut app, sent) = app(InstalledClient::EqMac);
        let mut online = admitted();
        testing::news(&mut online, [offer(b"Tester00", 1)]);
        app.insert_resource(online);
        app.update();
        app.update();
        assert!(open(&app));
        assert!(sent.try_recv().is_err());
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        assert!(online.world().translocate().is_some());
        online.answer_translocate();
        app.update();
        assert!(!open(&app));
    }

    #[test]
    fn an_offer_while_the_dialog_asks_something_else_is_declined_at_once() {
        let (mut app, sent) = app(InstalledClient::EqMac);
        let question = crate::confirm::Question::DeleteSpell { slot: 0, spell: 0 };
        assert!(
            app.world_mut()
                .resource_mut::<crate::confirm::Asked>()
                .ask(question)
        );
        let revision = app
            .world()
            .resource::<crate::chat::ChatState>()
            .history
            .revision();
        let mut online = admitted();
        testing::news(&mut online, [offer(b"Tester00", 1)]);
        app.insert_resource(online);
        app.update();
        assert!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .translocate()
                .is_none()
        );
        assert!(matches!(
            sent.try_recv(),
            Ok(ClientCommand::AnswerTranslocate { accept: false, .. })
        ));
        assert_eq!(
            app.world()
                .resource::<crate::chat::ChatState>()
                .history
                .revision(),
            revision
        );
    }

    #[test]
    fn a_client_whose_rules_are_unread_leaves_the_offer_alone() {
        let (mut app, sent) = app(InstalledClient::Titanium);
        let question = crate::confirm::Question::DeleteSpell { slot: 0, spell: 0 };
        assert!(
            app.world_mut()
                .resource_mut::<crate::confirm::Asked>()
                .ask(question)
        );
        let mut online = admitted();
        testing::news(&mut online, [offer(b"Tester00", 1)]);
        app.insert_resource(online);
        app.update();
        assert!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .translocate()
                .is_some()
        );
        assert!(sent.try_recv().is_err());
    }
}
