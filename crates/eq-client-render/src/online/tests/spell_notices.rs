use super::*;
use eq_client_core::spell_tables::{Binding, Context, Event, Fact, Policy, Stage, Update};
use eq_network_game::spell_tables::{DecodeError, Kind, Unavailable};

fn fixture() -> (App, std::sync::mpsc::Sender<WorldUpdate>, Binding) {
    let (spells, facts) = spell_provider_fixture();
    let WorldUpdate::SpellTable(first) = &facts[0] else {
        panic!()
    };
    let binding = first.binding();
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut app = App::new();
    app.insert_resource(OnlineState::new(true))
        .insert_resource(spells)
        .insert_resource(Updates(Mutex::new(Some(receiver))))
        .insert_resource(ViewerSettings(crate::ViewerConfig::default()))
        .insert_resource(crate::loading::Loading::waiting_for_gesture(true))
        .init_resource::<hud::HudState>()
        .init_resource::<crate::motion::Controls>()
        .init_resource::<crate::chat::ChatState>()
        .init_resource::<crate::notices::Lines>()
        .init_resource::<crate::combat::CombatState>()
        .init_resource::<crate::trade::TradeState>()
        .init_resource::<crate::items::ItemState>()
        .init_resource::<crate::inventory::InventoryState>()
        .init_resource::<crate::sounds::Sounds>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, receive);
    for fact in facts {
        sender.send(fact).unwrap();
    }
    app.update();
    (app, sender, binding)
}

fn failure(binding: Binding, run_id: u64, revision: u64) -> WorldUpdate {
    WorldUpdate::SpellTable(binding.envelope(Fact::Table(Event::new(
        Context::new(run_id, revision, Stage::World),
        Update::Unavailable {
            kind: Kind::Registration,
            reason: Unavailable::Malformed(DecodeError::MissingCount),
        },
    ))))
}

fn notices(app: &App) -> usize {
    app.world()
        .resource::<crate::chat::ChatState>()
        .history
        .lines(eq_client_core::chat::ChatTab::System)
        .iter()
        .filter(|(_, line)| line.message.text == SPELL_DATA_UNAVAILABLE_NOTICE)
        .count()
}

#[test]
fn spell_provider_failure_notice_bypasses_held_admission_without_spam() {
    let (mut app, sender, binding) = fixture();
    assert_eq!(notices(&app), 0, "normal synchronization is not a failure");
    sender
        .send(WorldUpdate::Chat(crate::chat::system_line(
            "Another line still arrives",
        )))
        .unwrap();
    sender
        .send(WorldUpdate::Game(WorldEvent::Entered {
            capabilities: Vec::new(),
            choices: Vec::new(),
            session_id: 2,
            zone: "example".into(),
            player: Box::new(testing::player(7)),
            far_clip: None,
            spell_landing_flag: None,
        }))
        .unwrap();
    let failed = failure(binding, 7, 3);
    for _ in 0..3 {
        sender.send(failed.clone()).unwrap();
    }
    app.update();
    assert_eq!(world(&app).session_id(), None, "admission is still held");
    assert_eq!(notices(&app), 1);
    assert_eq!(
        app.world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::System)
            .len(),
        2
    );
    app.update();
    assert_eq!(notices(&app), 1);
    sender
        .send(WorldUpdate::SpellTable(binding.envelope(
            Fact::RunStarted {
                run_id: 8,
                attempt: 2,
            },
        )))
        .unwrap();
    sender.send(failure(binding, 8, 1)).unwrap();
    app.update();
    assert_eq!(
        notices(&app),
        1,
        "a retry does not recover a sticky provider failure"
    );
}

#[test]
fn spell_provider_failure_notice_rearms_only_for_a_new_provider_lifetime() {
    let (mut app, sender, old) = fixture();
    sender.send(failure(old, 7, 3)).unwrap();
    app.update();
    assert_eq!(notices(&app), 1);
    let replacement = Binding::new(Policy::Project1999);
    app.world_mut()
        .resource_mut::<crate::spellbook::SpellNames>()
        .bind(replacement);
    sender.send(failure(old, 7, 4)).unwrap();
    app.update();
    assert_eq!(
        notices(&app),
        1,
        "old producer facts cannot notify the replacement"
    );
    sender
        .send(WorldUpdate::SpellTable(replacement.envelope(
            Fact::RunStarted {
                run_id: 9,
                attempt: 1,
            },
        )))
        .unwrap();
    let failed = failure(replacement, 9, 1);
    sender.send(failed.clone()).unwrap();
    sender.send(failed).unwrap();
    app.update();
    assert_eq!(
        notices(&app),
        2,
        "failed initial synchronization notifies once"
    );
    let installed = Binding::new(Policy::Installed);
    app.world_mut()
        .resource_mut::<crate::spellbook::SpellNames>()
        .bind(installed);
    sender.send(failure(installed, 10, 1)).unwrap();
    app.update();
    assert_eq!(
        notices(&app),
        2,
        "installed-only policies do not inherit this failure"
    );
}

#[test]
fn spell_provider_unknown_run_notifies_once_and_keeps_other_chat_processing() {
    let (mut app, sender, binding) = fixture();
    let missing = WorldUpdate::SpellTable(binding.envelope(Fact::RunUnavailable { attempt: 2 }));
    sender.send(missing.clone()).unwrap();
    sender.send(missing).unwrap();
    sender
        .send(WorldUpdate::Chat(crate::chat::system_line(
            "Another line still arrives",
        )))
        .unwrap();
    app.update();
    assert_eq!(notices(&app), 1);
    assert!(
        !app.world()
            .resource::<crate::spellbook::SpellNames>()
            .ready()
    );
    assert_eq!(
        app.world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::System)
            .len(),
        2
    );
    app.update();
    assert_eq!(notices(&app), 1);
}
