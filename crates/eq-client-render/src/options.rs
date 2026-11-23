//! The player's options: loaded for whoever is playing, saved as they
//! change, and told to the session where it acts on them.
use crate::profile_files::{CharacterProfile, Store};
use bevy::prelude::*;
use eq_client_core::food::AutoEat;
use eq_client_core::options::{Options, Toggle};
use std::time::Instant;

mod session_choices;
pub(crate) use session_choices::MovementSync;
pub(crate) use session_choices::tell as tell_choices;

/// The file a character with none of their own starts from.
const SHARED: &str = "options.txt";

/// The options in effect, and where they are kept.
#[derive(Resource)]
pub(crate) struct OptionsState {
    /// The options in effect.
    pub(crate) options: Options,
    /// What a character with no options of their own starts with.
    defaults: Options,
    store: Store,
    /// The admission and auto-eat options last accepted by the command queue.
    told: Option<(u64, AutoEat)>,
    /// The admission and Show My Helm as the session last knew it.
    helm: Option<(u64, bool)>,
    /// The admission and the server filters the session keeps, as far as
    /// this client told it; none once a character list is back.
    filters: Option<(u64, eq_client_core::filters::ServerFilters)>,
    /// The server filters given at the character list for the character
    /// entering, which its first admission starts from.
    chosen: Option<eq_client_core::filters::ServerFilters>,
}

impl Default for OptionsState {
    fn default() -> Self {
        Self::new(Options::default())
    }
}

impl OptionsState {
    /// Options that start from these defaults until a character's own load.
    pub(crate) fn new(defaults: Options) -> Self {
        Self {
            options: defaults,
            defaults,
            store: Store::default(),
            told: None,
            helm: None,
            filters: None,
            chosen: None,
        }
    }

    /// Whether the toggle is on.
    pub(crate) const fn on(&self, toggle: Toggle) -> bool {
        self.options.get(toggle)
    }

    /// Notes the server filters given at the character list for the
    /// character entering, or none where none were given: the session then
    /// keeps its configured set.
    pub(crate) fn choose_filters(
        &mut self,
        chosen: Option<eq_client_core::filters::ServerFilters>,
    ) {
        self.filters = None;
        self.chosen = chosen;
    }

    /// A character's saved server filters for the generation, read from
    /// their options file, or the shared one where they have none, before
    /// they play: what the character list gives the session for them.
    pub(crate) fn saved_filters(
        &self,
        directory: Option<&std::path::PathBuf>,
        profile: &CharacterProfile,
        dialect: eq_client_core::GameDialect,
    ) -> eq_client_core::filters::ServerFilters {
        // Before the options first load, a store of its own reads the files.
        let mut unopened = Store::default();
        let store = if self.store.loaded {
            &self.store
        } else {
            unopened.directory = directory.cloned();
            &unopened
        };
        let text = store
            .read(&file_name(Some(profile)))
            .or_else(|| store.read(SHARED))
            .unwrap_or_default();
        Options::read(&text, self.defaults)
            .chat_filters
            .for_session(dialect)
    }
}

/// An Options window checkbox: a click turns its option the other way.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OptionCheckbox(pub(crate) Toggle);

/// A Filters page button: a click steps its kind to the next mode the
/// generation's packet takes for it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FilterButton(pub(crate) eq_client_core::filters::FilterCategory);

/// A Filters page label: it says its kind's mode.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FilterValue(pub(crate) eq_client_core::filters::FilterCategory);

/// The Filters page's note: it says when a change reaches this server.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FilterNote;

/// The Filters page's note, for a session that reads a change at once
/// (`ServerFiltersAtOnce` offered) or one that reads it only at zone-in.
pub(crate) const fn filter_note(at_once: bool) -> &'static str {
    if at_once {
        "A change is sent to this server at once."
    } else {
        "A change reaches this server at the next zone."
    }
}

/// Steps a kind's server filter when its Filters page button is clicked.
/// The session hears the change from [`tell_session`].
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn step_filters(
    clicks: crate::clicks::Clicks<FilterButton>,
    online: Res<crate::online::OnlineState>,
    mut state: ResMut<OptionsState>,
) {
    let dialect = online.world().generation();
    for FilterButton(category) in clicks.iter() {
        if let Some(next) = state.options.chat_filters.next(*category, dialect) {
            state.options.chat_filters.set(*category, next);
        }
    }
}

/// Says each kind's mode on the Filters page, and hides the rows of kinds
/// the playing generation's packet has no word for.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show_filters(
    online: Res<crate::online::OnlineState>,
    state: Res<OptionsState>,
    mut buttons: Query<(&FilterButton, &mut Visibility)>,
    mut labels: Query<(&FilterValue, &mut Text)>,
    mut notes: Query<&mut Text, (With<FilterNote>, Without<FilterValue>)>,
) {
    let note = filter_note(
        online
            .world()
            .can(eq_client_core::Capability::ServerFiltersAtOnce),
    );
    for mut text in &mut notes {
        if text.0 != note {
            note.clone_into(&mut text.0);
        }
    }
    let dialect = online.world().generation();
    let taken =
        |category: eq_client_core::filters::FilterCategory| !category.modes(dialect).is_empty();
    for (FilterButton(category), mut visibility) in &mut buttons {
        let wanted = if taken(*category) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    for (FilterValue(category), mut text) in &mut labels {
        let words = if taken(*category) {
            eq_client_core::chat_filters::mode_words(state.options.chat_filters.get(*category))
        } else {
            ""
        };
        if text.0 != words {
            words.clone_into(&mut text.0);
        }
    }
}

/// Turns an option the other way when its checkbox is clicked.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn toggle(
    clicks: crate::clicks::Clicks<OptionCheckbox>,
    online: Res<crate::online::OnlineState>,
    mut state: ResMut<OptionsState>,
) {
    for OptionCheckbox(toggle) in clicks.iter() {
        // A greyed checkbox, for what the session does not offer, stays
        // as it is.
        let offered =
            crate::outbox::Needs::of(*toggle).is_none_or(|needs| needs.offered(online.world()));
        if offered {
            let on = state.on(*toggle);
            state.options.set(*toggle, !on);
        }
    }
}

/// Loads the options of whoever is playing, and saves a change to their file.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn persist(
    settings: Res<crate::ViewerSettings>,
    profile: Res<crate::profile_files::Profile>,
    mut state: ResMut<OptionsState>,
) {
    let changed = state.is_changed();
    state.bypass_change_detection().store.retry(Instant::now());
    if state.store.loaded && !profile.is_changed() {
        if changed {
            save(state.bypass_change_detection());
        }
        return;
    }
    if state.store.loaded {
        save(&mut state);
    } else {
        state.store.open(settings.0.settings_directory.as_ref());
    }
    let state = &mut *state;
    state.store.profile = profile.key();
    // A character with no options of their own starts from the shared file.
    let text = state
        .store
        .read(&file_name(state.store.profile.as_ref()))
        .or_else(|| state.store.read(SHARED))
        .unwrap_or_default();
    state.options = Options::read(&text, state.defaults);
    state.store.loaded_text(state.options.text());
}

/// Writes the options to the profile's file when they changed since last time.
fn save(state: &mut OptionsState) {
    let name = file_name(state.store.profile.as_ref());
    state.store.save(name, state.options.text(), Instant::now());
}

/// The shared file, or the profile's own.
fn file_name(profile: Option<&CharacterProfile>) -> String {
    crate::profile_files::name("options", SHARED, profile)
}

/// Tells each admission what the session may eat and drink on its own, and
/// tells it again when the player changes that.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn tell_session(
    online: Res<crate::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    mut state: ResMut<OptionsState>,
) {
    let Some(session_id) = outbox.peek(online.world()).map(|stamp| stamp.session_id) else {
        return;
    };
    tell_helm(&online, &outbox, &mut state, session_id);
    tell_filters(&online, &outbox, &mut state, session_id);
    if !crate::outbox::offered(online.world(), eq_client_core::Capability::Meals) {
        return;
    }
    let auto_eat = state.options.auto_eat();
    if state.told == Some((session_id, auto_eat)) {
        return;
    }
    let command = eq_client_core::ClientCommand::AutoEat {
        session_id,
        auto_eat,
    };
    if outbox.tell(online.world(), command) {
        state.told = Some((session_id, auto_eat));
    }
}

/// Tells the session each change of Show My Helm while the player is in a
/// zone, where it sends the option on (the Titanium program sends it on
/// every change, sitting 8f). An admission starts from the option as it
/// is, as the server keeps it in the profile; a session that doesn't send
/// it changes only how the player sees themselves.
fn tell_helm(
    online: &crate::online::OnlineState,
    outbox: &crate::outbox::Outbox,
    state: &mut ResMut<OptionsState>,
    session_id: u64,
) {
    let show = state.on(Toggle::ShowHelm);
    let told = match state.helm {
        Some((seen, before)) if seen == session_id && before != show => {
            !crate::outbox::offered(online.world(), eq_client_core::Capability::ShowingHelm)
                || outbox.tell(
                    online.world(),
                    eq_client_core::ClientCommand::ShowHelm { session_id, show },
                )
        }
        Some((seen, _)) if seen == session_id => false,
        _ => true,
    };
    if told {
        state.bypass_change_detection().helm = Some((session_id, show));
    }
}

/// Tells the session the character's server filters when they differ from
/// the set it keeps: at an admission whose set is not the character's (one
/// entered without the character list's choice), and on each change. The
/// session keeps the set across zones and sends it at once only where the
/// server type reads a change mid-zone; elsewhere the next zone-in carries
/// it (recording checklist 6.494c and e).
fn tell_filters(
    online: &crate::online::OnlineState,
    outbox: &crate::outbox::Outbox,
    state: &mut ResMut<OptionsState>,
    session_id: u64,
) {
    let wanted = state
        .options
        .chat_filters
        .for_session(online.world().generation());
    // A zone crossing keeps the set the session already had.
    let held = match &state.filters {
        Some((_, held)) => held.clone(),
        None => state.chosen.clone().unwrap_or_default(),
    };
    let told = held == wanted
        || outbox.tell(
            online.world(),
            eq_client_core::ClientCommand::SetServerFilters {
                session_id,
                filters: wanted.clone(),
            },
        );
    let known = if told { wanted } else { held };
    if state.filters.as_ref() != Some(&(session_id, known.clone())) {
        state.bypass_change_detection().filters = Some((session_id, known));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};

    fn app(directory: &std::path::Path) -> App {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .settings_directory = Some(directory.to_owned());
        app.insert_resource(OptionsState::new(Options::default()))
            .add_systems(
                Update,
                (crate::profile_files::follow, toggle, persist).chain(),
            );
        app
    }

    fn enter(app: &mut App, session_id: u64, name: &str) {
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::WorldName {
                short_name: "ExampleWorld".into(),
            }],
        );
        let mut player = testing::player(7);
        player.name = name.into();
        testing::admit(&mut online, session_id, player);
        app.insert_resource(online);
    }

    #[test]
    fn each_character_keeps_their_own_options() {
        use eq_client_core::options::Level;

        let directory = std::env::temp_dir().join(format!(
            "eq-client-options-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let mut app = app(&directory);
        enter(&mut app, 1, "Example");
        app.update();
        assert!(app.world().resource::<OptionsState>().options.wheel_zoom);
        // Nothing is written until the player changes something.
        assert!(std::fs::read_dir(&directory).is_err());
        let checkbox = app
            .world_mut()
            .spawn((Interaction::None, OptionCheckbox(Toggle::WheelZoom)))
            .id();
        crate::clicks::testing::click(&mut app, checkbox);
        assert!(!app.world().resource::<OptionsState>().options.wheel_zoom);
        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .set_level(Level::RunSpeed, 120);
        app.update();
        let file = directory.join("options-ExampleWorld-Example.txt");
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(saved.contains("wheel_zoom = false"));
        assert!(saved.contains("run_boost = 120"));
        // Another character starts from the defaults again.
        app.world_mut().despawn(checkbox);
        enter(&mut app, 2, "Another");
        app.update();
        assert!(app.world().resource::<OptionsState>().options.wheel_zoom);
        assert_eq!(
            app.world()
                .resource::<OptionsState>()
                .options
                .level(Level::RunSpeed),
            100
        );
        let mut restarted = self::app(&directory);
        enter(&mut restarted, 1, "Example");
        restarted.update();
        let restored = &restarted.world().resource::<OptionsState>().options;
        assert!(!restored.wheel_zoom);
        assert_eq!(restored.level(Level::RunSpeed), 120);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_new_characters_choices_load_before_the_worker_is_told() {
        use eq_client_core::{ClientCommand, qol::Fix};
        let directory = std::env::temp_dir().join(format!(
            "eq-client-qol-profile-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut saved = Options::default();
        saved.qol.set(Fix::Fly, true);
        saved.qol.set(Fix::AutoLoot, true);
        std::fs::write(
            directory.join("options-ExampleWorld-Example.txt"),
            saved.text(),
        )
        .unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let mut app = app(&directory);
        app.insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, tell_choices.after(persist));
        enter(&mut app, 1, "Example");
        app.update();
        assert!(receiver.try_iter().any(|command| matches!(command, ClientCommand::SetMovementOptions { options, .. } if options.fly)));
        enter(&mut app, 2, "Another");
        app.update();
        let commands: Vec<_> = receiver.try_iter().collect();
        let options = &app.world().resource::<OptionsState>().options;
        assert!(!options.qol.on(Fix::Fly));
        assert!(!options.qol.on(Fix::AutoLoot));
        // A new admission already starts with the defaults.
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, ClientCommand::SetMovementOptions { .. }))
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    fn meal_admission(app: &mut App, session_id: u64, meals: bool) {
        use eq_client_core::{Capability, WorldEvent};
        let mut online = OnlineState::new(true);
        let mut capabilities = vec![Capability::Inventory];
        if meals {
            capabilities.push(Capability::Meals);
        }
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities,
                choices: Vec::new(),
                session_id,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        testing::connect(&mut online, true);
        app.insert_resource(online);
    }

    #[test]
    fn meal_preferences_wait_for_support_and_survive_admission_changes() {
        use eq_client_core::{ClientCommand, qol::Fix};
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let mut app = crate::testing::app();
        let mut options = Options::default();
        options.set(Toggle::Qol(Fix::SkipModifiedFood), false);
        let saved = options.text();
        app.insert_resource(OptionsState::new(options))
            .insert_resource(crate::outbox::Outbox::new(Some(tx)))
            .add_systems(Update, tell_session);
        meal_admission(&mut app, 1, false);
        for _ in 0..2 {
            app.update();
        }
        assert!(rx.try_recv().is_err());
        let state = app.world().resource::<OptionsState>();
        assert_eq!(state.told, None);
        assert_eq!(state.options.text(), saved);
        assert_eq!(
            Options::read(&saved, Options::default()).auto_eat(),
            AutoEat::Anything
        );
        meal_admission(&mut app, 2, true);
        app.update();
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::AutoEat {
                session_id: 2,
                auto_eat: AutoEat::Anything
            }
        );
        app.update();
        assert!(
            rx.try_recv().is_err(),
            "one update per admission and setting"
        );
        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .set(Toggle::Qol(Fix::SkipModifiedFood), true);
        app.update();
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::AutoEat {
                session_id: 2,
                auto_eat: AutoEat::Plain
            }
        );
        meal_admission(&mut app, 3, false);
        app.update();
        assert!(rx.try_recv().is_err());
        assert_eq!(
            app.world().resource::<OptionsState>().options.auto_eat(),
            AutoEat::Plain
        );
    }

    #[test]
    fn a_full_queue_retries_the_current_meal_preference() {
        use eq_client_core::ClientCommand;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let mut app = crate::testing::app();
        app.insert_resource(OptionsState::default())
            .insert_resource(crate::outbox::Outbox::new(Some(tx)))
            .add_systems(Update, tell_session);
        meal_admission(&mut app, 4, true);
        assert!(app.world().resource::<crate::outbox::Outbox>().tell(
            app.world().resource::<OnlineState>().world(),
            ClientCommand::AutoEat {
                session_id: 4,
                auto_eat: AutoEat::Anything
            },
        ));
        app.update();
        assert_eq!(app.world().resource::<OptionsState>().told, None);
        rx.try_recv().unwrap();
        app.update();
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::AutoEat {
                session_id: 4,
                auto_eat: AutoEat::Plain
            }
        );
    }

    #[test]
    fn show_my_helm_is_told_on_each_change_where_the_session_sends_it() {
        use eq_client_core::{Capability, ClientCommand};
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let mut app = crate::testing::app();
        app.insert_resource(OptionsState::new(Options::default()))
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, (toggle, tell_session).chain());
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::connect(&mut online, true);
        app.insert_resource(online);
        let helm = |receiver: &std::sync::mpsc::Receiver<ClientCommand>| {
            receiver
                .try_iter()
                .filter(|command| matches!(command, ClientCommand::ShowHelm { .. }))
                .collect::<Vec<_>>()
        };
        // An admission starts from the option as it is.
        app.update();
        assert_eq!(helm(&receiver), []);
        let shown = app.world().resource::<OptionsState>().on(Toggle::ShowHelm);
        let checkbox = app
            .world_mut()
            .spawn((Interaction::None, OptionCheckbox(Toggle::ShowHelm)))
            .id();
        crate::clicks::testing::click(&mut app, checkbox);
        assert_eq!(
            helm(&receiver),
            [ClientCommand::ShowHelm {
                session_id: 1,
                show: !shown
            }]
        );
        // Told once.
        app.update();
        assert_eq!(helm(&receiver), []);
        assert!(crate::outbox::offered(
            app.world().resource::<OnlineState>().world(),
            Capability::ShowingHelm
        ));
    }

    #[test]
    fn the_server_filters_are_told_when_they_differ_from_what_the_session_keeps() {
        use eq_client_core::{
            ClientCommand,
            filters::{FilterCategory, FilterMode},
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let mut app = crate::testing::app();
        let mut options = Options::default();
        options
            .chat_filters
            .set(FilterCategory::Shouts, FilterMode::Hide);
        app.insert_resource(OptionsState::new(options))
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, tell_session);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::connect(&mut online, true);
        app.insert_resource(online);
        let told = |receiver: &std::sync::mpsc::Receiver<ClientCommand>| {
            receiver
                .try_iter()
                .filter_map(|command| match command {
                    ClientCommand::SetServerFilters {
                        session_id,
                        filters,
                    } => Some((session_id, filters.get(FilterCategory::Shouts))),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // Entered without the list's choice, the session keeps the default.
        app.update();
        assert_eq!(told(&receiver), [(1, FilterMode::Hide)]);
        app.update();
        assert_eq!(told(&receiver), []);
        // A change is told once.
        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .chat_filters
            .set(FilterCategory::Shouts, FilterMode::Show);
        app.update();
        app.update();
        assert_eq!(told(&receiver), [(1, FilterMode::Show)]);
        // An entry whose choice the list gave already is not told again.
        let mut chosen = eq_client_core::filters::ServerFilters::default();
        chosen.set(FilterCategory::Shouts, FilterMode::Hide);
        {
            let mut state = app.world_mut().resource_mut::<OptionsState>();
            state
                .options
                .chat_filters
                .set(FilterCategory::Shouts, FilterMode::Hide);
            state.choose_filters(Some(chosen));
        }
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 2, testing::player(7));
        testing::connect(&mut online, true);
        app.insert_resource(online);
        app.update();
        assert_eq!(told(&receiver), []);
    }

    #[test]
    fn a_filters_page_click_steps_its_kind_and_its_label_says_the_mode() {
        use eq_client_core::filters::{FilterCategory, FilterMode};
        let mut app = crate::testing::app();
        app.insert_resource(OptionsState::new(Options::default()))
            .add_systems(Update, (step_filters, show_filters).chain());
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        app.insert_resource(online);
        let button = app
            .world_mut()
            .spawn((
                Interaction::None,
                FilterButton(FilterCategory::BardSongs),
                Visibility::Inherited,
            ))
            .id();
        let label = app
            .world_mut()
            .spawn((FilterValue(FilterCategory::BardSongs), Text::default()))
            .id();
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "Show");
        crate::clicks::testing::click(&mut app, button);
        assert_eq!(
            app.world()
                .resource::<OptionsState>()
                .options
                .chat_filters
                .get(FilterCategory::BardSongs),
            FilterMode::MineOnly
        );
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "Mine only");
        assert_eq!(
            *app.world().get::<Visibility>(button).unwrap(),
            Visibility::Inherited
        );
    }

    #[test]
    fn the_filters_note_says_when_a_change_reaches_this_server() {
        use eq_client_core::Capability;
        let mut app = crate::testing::app();
        app.insert_resource(OptionsState::new(Options::default()))
            .insert_resource(OnlineState::new(true))
            .add_systems(Update, show_filters);
        let note = app.world_mut().spawn((FilterNote, Text::default())).id();
        let says = |app: &App| app.world().get::<Text>(note).unwrap().0.clone();
        // Before an admission nothing is read until the zone-in.
        app.update();
        assert_eq!(says(&app), filter_note(false));
        assert!(says(&app).contains("next zone"));
        // A session that offers ServerFiltersAtOnce reads it at once.
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        app.insert_resource(online);
        app.update();
        assert_eq!(says(&app), filter_note(true));
        assert!(says(&app).contains("at once"));
        // One that does not offer it, as Quarm and TAKP, waits.
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Entered {
                capabilities: Capability::ALL
                    .into_iter()
                    .filter(|capability| *capability != Capability::ServerFiltersAtOnce)
                    .collect(),
                choices: Vec::new(),
                session_id: 2,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        app.insert_resource(online);
        app.update();
        assert_eq!(says(&app), filter_note(false));
    }

    #[test]
    fn a_characters_saved_filters_read_before_they_play() {
        use eq_client_core::filters::{FilterCategory, FilterMode};
        let directory = std::env::temp_dir().join(format!(
            "eq-client-filters-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut saved = Options::default();
        saved
            .chat_filters
            .set(FilterCategory::DamageOverTime, FilterMode::Hide);
        saved
            .chat_filters
            .set(FilterCategory::Ooc, FilterMode::Hide);
        std::fs::write(
            directory.join("options-ExampleWorld-Example.txt"),
            saved.text(),
        )
        .unwrap();
        let state = OptionsState::new(Options::default());
        let profile = CharacterProfile {
            world: "ExampleWorld".into(),
            character: "Example".into(),
        };
        let titanium = state.saved_filters(
            Some(&directory),
            &profile,
            eq_client_core::GameDialect::Titanium,
        );
        assert_eq!(
            titanium.get(FilterCategory::DamageOverTime),
            FilterMode::Hide
        );
        assert_eq!(titanium.get(FilterCategory::Ooc), FilterMode::Hide);
        // EQMac's packet has no damage-over-time word.
        let eqmac = state.saved_filters(
            Some(&directory),
            &profile,
            eq_client_core::GameDialect::EqMac,
        );
        assert_eq!(eqmac.get(FilterCategory::DamageOverTime), FilterMode::Show);
        assert_eq!(eqmac.get(FilterCategory::Ooc), FilterMode::Hide);
        // A character with no file of their own reads the defaults.
        let other = CharacterProfile {
            world: "ExampleWorld".into(),
            character: "Another".into(),
        };
        assert_eq!(
            state.saved_filters(
                Some(&directory),
                &other,
                eq_client_core::GameDialect::Titanium
            ),
            eq_client_core::filters::ServerFilters::default()
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_setting_for_what_the_session_does_not_offer_stays_as_it_is() {
        use eq_client_core::{Capability, WorldEvent, qol::Fix};
        let mut app = crate::testing::app();
        app.insert_resource(OptionsState::new(Options::default()))
            .add_systems(Update, toggle);
        // A session without meals eats nothing on its own, so the
        // food setting is greyed out there.
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: vec![Capability::Talking, Capability::Inventory],
                choices: Vec::new(),
                session_id: 1,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        testing::connect(&mut online, true);
        app.insert_resource(online);
        let food = Toggle::Qol(Fix::SkipModifiedFood);
        let checkbox = app
            .world_mut()
            .spawn((Interaction::None, OptionCheckbox(food)))
            .id();
        crate::clicks::testing::click(&mut app, checkbox);
        assert!(app.world().resource::<OptionsState>().on(food));
        // Where the session offers it, a click turns it off.
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 2, testing::player(7));
        app.insert_resource(online);
        let checkbox = app
            .world_mut()
            .spawn((Interaction::None, OptionCheckbox(food)))
            .id();
        crate::clicks::testing::click(&mut app, checkbox);
        assert!(!app.world().resource::<OptionsState>().on(food));
    }
}
