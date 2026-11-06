//! The installed client's translocate question and Yes/No answer. The
//! network session owns the deadline and follows server movement messages.
//! `EQMac` also declines a new offer if the dialog was already busy.
use crate::{hud::messages::Messages, online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_assets::ui::{InstalledClient, TranslocateQuestion};
use eq_client_core::{ClientCommand, chat::ChatLine, confirmations::ConsentId, world::ClientWorld};

/// What the dialog asks about the offer waiting, in the installed client's
/// words where it has them.
pub(crate) fn question(
    world: &ClientWorld,
    rules: TranslocateQuestion,
    messages: Option<&Messages>,
) -> String {
    let Some(question) = world.translocate() else {
        return String::new();
    };
    let offer = &question.offer;
    if rules.bind_spells.contains(&offer.spell_id) {
        return messages
            .and_then(|messages| messages.formatted(rules.to_bind, &[]))
            .unwrap_or_else(|| "You are being sent to your bind point. Do you accept?".to_owned());
    }
    let caster = eq_client_core::entities::display_name(&offer.caster);
    let zone = eq_client_core::zones::short_name(u32::from(offer.zone_id))
        .and_then(eq_client_core::zones::long_name)
        .filter(|long| !long.is_empty())
        .map_or_else(|| format!("zone {}", offer.zone_id), str::to_owned);
    messages
        .and_then(|messages| messages.formatted(rules.named, &[caster.clone(), zone.clone()]))
        .unwrap_or_else(|| format!("{caster} offers to send you to {zone}. Do you accept?"))
}

/// Answers the offer waiting, Yes (true) or No; after a No, the line the
/// client says then, if `say` and it has one.
pub(crate) fn answer(
    online: &mut OnlineState,
    outbox: &Outbox,
    offer_id: ConsentId,
    accept: bool,
    declined: Option<(u32, &Messages)>,
) -> Option<ChatLine> {
    outbox
        .post(online.world(), |stamp| ClientCommand::AnswerTranslocate {
            session_id: stamp.session_id,
            offer_id,
            accept,
        })
        .ok()?;
    online.dismiss_consent(offer_id);
    let (line, messages) = declined.filter(|_| !accept)?;
    Some(crate::chat::system_line(messages.said(line, &[])))
}

/// Says No to a new Mac-era offer if the dialog already asks something
/// else. The network session separately owns expiry and checks the token.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn decline_unasked(
    (mut online, outbox, asked): (ResMut<OnlineState>, Res<Outbox>, Res<crate::confirm::Asked>),
    settings: Res<crate::ViewerSettings>,
    mut seen: Local<Option<ConsentId>>,
) {
    if settings.0.installed_client != InstalledClient::EqMac {
        return;
    }
    let Some(id) = online.world().translocate().map(|question| question.id) else {
        *seen = None;
        return;
    };
    let new = *seen != Some(id);
    *seen = Some(id);
    let busy = online.world().resurrection().is_some() || asked.question().is_some();
    if new && busy {
        answer(&mut online, &outbox, id, false, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        online::testing,
        windows::{Shown, WindowId},
    };
    use eq_client_core::{
        WorldEvent,
        confirmations::{ConsentKind, ConsentOffer},
    };
    use std::time::{Duration, Instant};

    const RULES: TranslocateQuestion = InstalledClient::EqMac.translocate_offer();

    /// A translocate from `caster` to zone 2 with spell `spell`, as the
    /// Mac-era record lays it out.
    fn offer(caster: &[u8], spell: u32) -> WorldEvent {
        let mut body = vec![0; 92];
        body[..4].copy_from_slice(&2u32.to_le_bytes());
        body[4..8].copy_from_slice(&spell.to_le_bytes());
        body[12..12 + caster.len()].copy_from_slice(caster);
        match eq_client_core::confirmations::eqmac_translocate(&body).unwrap() {
            eq_client_core::confirmations::Translocate::Offer(offer) => {
                WorldEvent::TranslocateOffer(ConsentOffer {
                    id: ConsentId {
                        session_id: 1,
                        kind: ConsentKind::Translocate,
                        sequence: u64::from(spell),
                    },
                    offer,
                    expires_at: Instant::now() + Duration::from_secs(3600),
                })
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
        let id = online.world().translocate().unwrap().id;
        online.dismiss_consent(id);
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
    fn titanium_displays_the_supported_servers_offer_without_an_automatic_busy_decline() {
        let (mut app, sent) = app(InstalledClient::Titanium);
        let mut online = admitted();
        testing::news(&mut online, [offer(b"Tester00", 1)]);
        app.insert_resource(online);
        app.update();
        assert!(open(&app));
        assert!(sent.try_recv().is_err());
    }
}
