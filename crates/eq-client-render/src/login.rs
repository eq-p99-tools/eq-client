//! The login front end: the login screen, the login server's list of
//! worlds, and the sessions they start, before the world's character list.
//! The app offers the login servers and starts sessions through [`Logins`];
//! the viewer shows the screens and hands each session's news to the world,
//! and never sees the network's own types.
//!
//! The screens are the installation's own where it has them
//! ([`official`]), and else the client's own windows, whose look and words
//! are ours.
mod form;
mod official;
mod worlds;

pub(super) use form::form;
pub(super) use official::{LoginLook, login_screen, read as read_look, worlds_screen};
pub(super) use worlds::{LoginStrings, scroll, worlds};

use super::{online::OnlineState, online::Updates, outbox::Outbox, theme};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, WorldUpdate};
use std::sync::mpsc::{Receiver, SyncSender};
use zeroize::{Zeroize, Zeroizing};

/// The app's side of the login front end: the login servers it offers, and
/// how it logs in on one.
pub trait Logins: Send + Sync {
    /// The login servers the login screen offers, in order.
    fn servers(&self) -> Vec<LoginServer>;

    /// Which of them the login screen offers first.
    fn first(&self) -> usize;

    /// Logs in on a login server, by its place, with the account and
    /// password the player typed.
    ///
    /// # Errors
    /// Says why no session could start, in the player's words.
    fn connect(
        &mut self,
        server: usize,
        account: &str,
        password: &str,
    ) -> Result<Connection, String>;

    /// A session on this login server reached a world's character list with
    /// this account, on the world the player chose from the list where they
    /// chose one: what the login screen offers first next time.
    fn played(&mut self, server: usize, account: &str, world: Option<&str>);

    /// The account and password a script's `login` step types, which only
    /// the environment holds.
    fn scripted(&self) -> Option<(String, Zeroizing<String>)>;
}

/// A login server as the login screen offers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginServer {
    /// What the player calls it.
    pub name: String,
    /// The account last logged in with here, for the account box.
    pub account: String,
    /// The world last played on here, which the list highlights first.
    pub world: Option<String>,
    /// Whether this run can log in here.
    pub availability: Availability,
}

/// Whether a run can log in on a login server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    /// The session runs in this client.
    Here,
    /// The server type needs another installation of the official client,
    /// in the folder named, as the player reads it, so Connect opens the
    /// client again with it.
    Reopens(String),
    /// No session can start from here, and why, in the player's words.
    Unavailable(String),
}

/// What logging in started.
pub enum Connection {
    /// A session, in this client.
    Session(Session),
    /// The client opened again with the server type's installation; this one
    /// closes.
    Reopened,
}

/// A session running on its own thread: its news, the queue for the
/// player's requests, and the thread.
pub struct Session {
    /// What the session tells the client.
    pub updates: Receiver<WorldUpdate>,
    /// What the player asks the session to send.
    pub commands: SyncSender<ClientCommand>,
    /// The thread the session runs on.
    pub worker: Box<dyn Worker>,
}

/// The thread a session runs on. Dropping it stops the session and waits
/// for the thread.
pub trait Worker: Send + Sync {
    /// Asks the session to end; it ends shortly after.
    fn stop(&self);

    /// Whether the session has ended.
    fn finished(&self) -> bool;

    /// Why the session ended, in the player's words, once it has; None when
    /// it was asked to, or ended without a reason.
    fn reason(&self) -> Option<String>;
}

/// The screen the player is at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Screen {
    /// The login screen, with no session or one logging in.
    Login,
    /// The login server's list of worlds.
    Servers,
    /// The world's character list.
    Characters,
    /// A zone, or the offline viewer.
    World,
}

/// A login screen box the player types in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Field {
    /// Neither box.
    #[default]
    None,
    /// The account's name.
    Account,
    /// The password.
    Password,
}

/// The login front end's state: what the login screen offers and holds,
/// and the session it started.
#[derive(Resource, Default)]
pub(crate) struct FrontEnd {
    /// The app's side; none offline.
    logins: Option<Box<dyn Logins>>,
    /// The login servers offered, as the app last said.
    pub(crate) servers: Vec<LoginServer>,
    /// The login server chosen, by its place.
    pub(crate) chosen: usize,
    /// The account's name as typed.
    pub(crate) account: String,
    /// The password as typed, wiped once handed to a session.
    pub(crate) password: Zeroizing<String>,
    /// The box the player types in.
    pub(crate) field: Field,
    /// What the login screen says: why the last session ended, or why one
    /// could not start.
    pub(crate) status: String,
    /// The world highlighted on the login server's list, by its place.
    pub(crate) highlighted: Option<usize>,
    /// What a script asked of the screen showing, which it does as a press
    /// of its button would.
    pub(crate) request: Option<Request>,
    /// The session running, if any.
    session: Option<Running>,
    /// Sessions asked to stop, kept until their threads end so that none is
    /// waited for while drawing.
    closing: Vec<Box<dyn Worker>>,
}

/// A session the front end started, or the launch began.
struct Running {
    worker: Box<dyn Worker>,
    /// The login server and the account the player logged in with; none for
    /// the session the launch began.
    login: Option<(usize, String)>,
    /// The list the player last chose from, by its identity, and the world
    /// they chose there, by name.
    chose: Option<(u64, String)>,
    /// The list whose highlight is set, by its identity.
    listed: Option<u64>,
    /// The session reached the world's character list; it never shows the
    /// login screens again.
    past_login: bool,
    /// The player asked Quick Connect: the list's first look plays on the
    /// world last played on, where it takes players.
    quick: bool,
}

impl FrontEnd {
    /// The front end of a run with a login screen.
    pub(crate) fn new(logins: Box<dyn Logins>) -> Self {
        let mut front = Self {
            chosen: logins.first(),
            logins: Some(logins),
            ..Self::default()
        };
        front.refresh();
        front
    }

    /// The front end of a run with a login screen, with the session the
    /// launch began, if any, already running, and that session's channels
    /// for the world.
    pub(crate) fn launched(
        logins: Box<dyn Logins>,
        session: Option<Session>,
    ) -> (
        Self,
        Option<Receiver<WorldUpdate>>,
        Option<SyncSender<ClientCommand>>,
    ) {
        let mut front = Self::new(logins);
        let Some(session) = session else {
            return (front, None, None);
        };
        front.session = Some(Running {
            worker: session.worker,
            login: None,
            chose: None,
            listed: None,
            past_login: false,
            quick: false,
        });
        (front, Some(session.updates), Some(session.commands))
    }

    /// Takes the login servers from the app again, and fills the account
    /// box with the chosen one's account.
    fn refresh(&mut self) {
        let Some(logins) = &self.logins else {
            return;
        };
        self.servers = logins.servers();
        if self.chosen >= self.servers.len() {
            self.chosen = 0;
        }
        self.choose(self.chosen);
    }

    /// Chooses a login server: its account fills the account box, and the
    /// password box waits when there is one.
    pub(crate) fn choose(&mut self, index: usize) {
        let Some(server) = self.servers.get(index) else {
            return;
        };
        self.chosen = index;
        server.account.clone_into(&mut self.account);
        self.password.zeroize();
        self.field = if self.account.is_empty() {
            Field::Account
        } else {
            Field::Password
        };
    }

    /// The login server chosen.
    pub(crate) fn chosen(&self) -> Option<&LoginServer> {
        self.servers.get(self.chosen)
    }

    /// The world last played on the chosen login server, which Quick
    /// Connect and Play Last Server play on.
    pub(crate) fn remembered_world(&self) -> Option<&str> {
        self.chosen().and_then(|server| server.world.as_deref())
    }

    /// Whether the run has a login screen.
    pub(crate) const fn has_login(&self) -> bool {
        self.logins.is_some()
    }

    /// Whether a session runs.
    pub(crate) const fn running(&self) -> bool {
        self.session.is_some()
    }

    /// The screen the player is at. A character list shows whenever one is
    /// up, and once a session has reached one it stays there between
    /// characters; before that, the login server's list shows while it is
    /// up, and the login screen the rest of the time.
    pub(crate) fn screen(&self, online: &OnlineState) -> Screen {
        let world = online.world();
        if world.session_id().is_some() {
            return Screen::World;
        }
        if online.selection.is_some() {
            return Screen::Characters;
        }
        if !online.enabled {
            return Screen::World;
        }
        let Some(running) = &self.session else {
            return Screen::Login;
        };
        if running.past_login {
            Screen::Characters
        } else if world.servers().is_some() {
            Screen::Servers
        } else {
            Screen::Login
        }
    }

    /// Whether the screen has settled, for a screenshot: the login screen
    /// with nothing logging in, a list up, or the player in the world.
    pub(crate) fn settled(&self, online: &OnlineState) -> bool {
        match self.screen(online) {
            Screen::Login => self.session.is_none(),
            Screen::Servers => true,
            Screen::Characters => online.selection.is_some(),
            Screen::World => {
                !online.enabled || (online.world().connected() && online.world().player().is_some())
            }
        }
    }

    /// Logs in on the chosen login server with what the login screen holds.
    /// The password is wiped whatever happens; a refusal stays on the
    /// screen.
    pub(crate) fn connect(&mut self, online: &mut OnlineState, link: &mut Link) -> Connected {
        let password = std::mem::take(&mut self.password);
        if self.session.is_some() {
            return Connected::No;
        }
        let Some(logins) = self.logins.as_mut() else {
            return Connected::No;
        };
        // Opening the client again needs no account; the new one asks.
        let reopens = self
            .servers
            .get(self.chosen)
            .is_some_and(|server| matches!(server.availability, Availability::Reopens(_)));
        if self.account.trim().is_empty() && !reopens {
            "Type your account name first".clone_into(&mut self.status);
            self.field = Field::Account;
            return Connected::No;
        }
        match logins.connect(self.chosen, self.account.trim(), &password) {
            Ok(Connection::Session(session)) => {
                let login = Some((self.chosen, self.account.trim().to_owned()));
                self.begin(session, login, online, link);
                Connected::Session
            }
            Ok(Connection::Reopened) => Connected::Reopened,
            Err(reason) => {
                self.status = reason;
                Connected::No
            }
        }
    }

    /// Logs in as [`Self::connect`] does, then plays on the world last
    /// played on this login server as soon as the list shows it taking
    /// players, as the official login screen's Quick Connect does; where it
    /// does not, the list stays up.
    pub(crate) fn quick_connect(&mut self, online: &mut OnlineState, link: &mut Link) -> Connected {
        let connected = self.connect(online, link);
        if connected == Connected::Session
            && let Some(running) = self.session.as_mut()
        {
            running.quick = true;
        }
        connected
    }

    /// Fills the login screen with the account and password a script types.
    ///
    /// # Errors
    /// Says why a script cannot log in here.
    pub(crate) fn type_scripted(&mut self) -> Result<(), &'static str> {
        let logins = self
            .logins
            .as_ref()
            .ok_or("login needs the login screen; this run is offline")?;
        let (account, password) = logins
            .scripted()
            .ok_or("login needs EQ_ACCOUNT and EQ_PASSWORD in the environment")?;
        self.account = account;
        self.password = password;
        Ok(())
    }

    /// Hands a session's news and requests to the world, which starts over.
    pub(crate) fn begin(
        &mut self,
        session: Session,
        login: Option<(usize, String)>,
        online: &mut OnlineState,
        link: &mut Link,
    ) {
        online.restart();
        link.attach(Some(session.updates), Some(session.commands));
        self.session = Some(Running {
            worker: session.worker,
            login,
            chose: None,
            listed: None,
            past_login: false,
            quick: false,
        });
        self.highlighted = None;
        self.status.clear();
    }

    /// Ends the session: it stops, the world hears nothing more from it,
    /// and the login screen says why it ended, if it knows.
    pub(crate) fn end(&mut self, reason: Option<String>, link: &mut Link) {
        if let Some(running) = self.session.take() {
            self.status = reason
                .or_else(|| running.worker.reason())
                .unwrap_or_default();
            running.worker.stop();
            self.closing.push(running.worker);
        }
        link.attach(None, None);
        self.highlighted = None;
        // The account typed stays for another try, as the official login
        // screen keeps it; the password goes.
        let typed = std::mem::take(&mut self.account);
        self.refresh();
        if !typed.trim().is_empty() {
            self.account = typed;
            self.field = Field::Password;
        }
    }

    /// Goes back to the login screen from past the list, as character
    /// select's Quit does: the session ends as [`Self::end`] ends it, with no
    /// reason to show, and the world starts over. A session asked to stop
    /// sends no news of its end, so the world would otherwise keep its
    /// character list.
    pub(crate) fn back_to_login(&mut self, online: &mut OnlineState, link: &mut Link) {
        self.end(None, link);
        online.restart();
    }

    /// The player chose a world from the login server's list.
    pub(crate) fn chose_world(&mut self, selection_id: u64, name: &str) {
        if let Some(running) = self.session.as_mut() {
            running.chose = Some((selection_id, name.to_owned()));
        }
    }
}

/// What a script asks of the login screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    /// Connect, as the login screen's Connect does.
    Connect,
    /// Play on the highlighted world, as the list's Play does.
    Play,
}

/// What connecting did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Connected {
    /// Nothing: the login screen says why, if anything needs saying.
    No,
    /// A session started.
    Session,
    /// The client opened again; this one closes.
    Reopened,
}

/// The way between a session and the world: its news coming in and the
/// player's requests going out.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Link<'w> {
    updates: Res<'w, Updates>,
    outbox: ResMut<'w, Outbox>,
}

impl Link<'_> {
    /// Sends the player's request to the session, or says why not.
    ///
    /// # Errors
    /// Refuses a request the session cannot take now.
    fn send(
        &self,
        world: &eq_client_core::world::ClientWorld,
        command: ClientCommand,
    ) -> Result<(), crate::outbox::Refusal> {
        self.outbox.send(world, command)
    }

    /// Connects the world to a session's channels, or to none.
    fn attach(
        &mut self,
        updates: Option<Receiver<WorldUpdate>>,
        commands: Option<SyncSender<ClientCommand>>,
    ) {
        if let Ok(mut receiver) = self.updates.0.lock() {
            *receiver = updates;
        }
        self.outbox.connect(commands);
    }
}

/// Follows the running session: once it reaches a world's character list,
/// the app remembers the account and world; once it ends, the login screen
/// shows again with the reason. Stopped sessions are let go once their
/// threads end.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn watch(mut front: ResMut<FrontEnd>, online: Res<OnlineState>, mut link: Link) {
    front.closing.retain(|worker| !worker.finished());
    let world = online.world();
    let front = &mut *front;
    let Some(running) = front.session.as_mut() else {
        return;
    };
    if let Some(list) = world.servers()
        && running.listed != Some(list.selection_id)
    {
        running.listed = Some(list.selection_id);
        let last = front
            .servers
            .get(front.chosen)
            .and_then(|server| server.world.as_deref());
        front.highlighted = worlds::first(&list.servers, last);
        // Quick Connect plays at once on the world last played on, which
        // the list highlights first where it takes players.
        if std::mem::take(&mut running.quick)
            && let (Some(index), Some(last)) = (front.highlighted, last)
            && list.servers[index].name.eq_ignore_ascii_case(last)
        {
            front.request = Some(Request::Play);
        }
    }
    if !running.past_login && (world.characters().is_some() || world.session_id().is_some()) {
        running.past_login = true;
        if let (Some(logins), Some((server, account))) = (front.logins.as_mut(), &running.login) {
            let world = running.chose.as_ref().map(|(_, name)| name.as_str());
            logins.played(*server, account, world);
        }
    }
    if !world.ended() {
        return;
    }
    let reason = running.worker.reason().unwrap_or_else(|| {
        if running.past_login {
            "The connection to the server ended".to_owned()
        } else {
            "The login server ended the session".to_owned()
        }
    });
    front.end(Some(reason), &mut link);
}

/// Character select's Quit goes back to the login screen, the account kept,
/// in a run that has one, as the owner chose; a run without one, such as an
/// offline preview, leaves the game.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn quit(
    mut front: ResMut<FrontEnd>,
    mut online: ResMut<OnlineState>,
    mut link: Link,
    keys: crate::keys::Keys,
    buttons: Query<(Ref<Interaction>, &super::character_select::Action)>,
    mut exit: MessageWriter<AppExit>,
) {
    let pressed = buttons.iter().any(|(interaction, action)| {
        interaction.is_changed()
            && *interaction == Interaction::Pressed
            && matches!(action, super::character_select::Action::Quit)
    });
    if !pressed || !keys.focused() || front.screen(&online) != Screen::Characters {
        return;
    }
    if front.has_login() {
        front.back_to_login(&mut online, &mut link);
    } else {
        exit.write(AppExit::Success);
    }
}

/// Gives the login screens the keyboard while one is up, so that what the
/// player types there, such as an account's name, does nothing else.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn claim(
    front: Res<FrontEnd>,
    online: Res<OnlineState>,
    mut typing: ResMut<crate::keys::Typing>,
) {
    let up = front.has_login() && matches!(front.screen(&online), Screen::Login | Screen::Servers);
    if typing.logging_in != up {
        typing.logging_in = up;
    }
}

/// How a button of the login screens lights.
#[derive(Component, Clone, Copy)]
pub(crate) struct Lit {
    /// Whether pressing it does anything.
    pub(crate) usable: bool,
    /// Whether it is the one chosen.
    pub(crate) on: bool,
}

/// Lights the login screens' buttons as the pointer moves over them.
pub(super) fn light(
    mut buttons: Query<(&Lit, &Interaction, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (lit, interaction, mut color) in &mut buttons {
        color.0 = theme::button(lit.usable, lit.on, *interaction);
    }
}

/// A row or button of the login screens, lit as it stands.
pub(crate) fn lit_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    marker: impl Bundle,
    text: &str,
    lit: Lit,
) -> EntityCommands<'a> {
    let mut button = theme::button_with(parent, marker, text, theme::Size::Large);
    button.insert((
        Node {
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(theme::button(lit.usable, lit.on, Interaction::None)),
        lit,
    ));
    button
}

/// A screen-filling cover with a centred panel, under the skin's windows.
pub(crate) fn cover<'a>(commands: &'a mut Commands, root: impl Bundle) -> EntityCommands<'a> {
    commands.spawn((
        root,
        Button,
        GlobalZIndex(super::windows::Layer::Screen.base() - 1),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme::COVER),
    ))
}

/// The panel on a cover, holding a screen's controls.
pub(crate) fn panel() -> impl Bundle {
    (
        Node {
            width: px(460),
            max_width: percent(95),
            padding: UiRect::all(px(24)),
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(theme::TITLE_BAR),
    )
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    /// A worker whose session ends when told.
    #[derive(Clone, Default)]
    pub(crate) struct Thread {
        pub(crate) stopped: Arc<AtomicBool>,
        pub(crate) reason: Arc<Mutex<Option<String>>>,
    }

    impl Worker for Thread {
        fn stop(&self) {
            self.stopped.store(true, Ordering::SeqCst);
        }

        fn finished(&self) -> bool {
            self.stopped.load(Ordering::SeqCst)
        }

        fn reason(&self) -> Option<String> {
            self.reason.lock().unwrap().clone()
        }
    }

    /// What a test's app heard from the front end.
    #[derive(Default)]
    pub(crate) struct Heard {
        /// Each connect: the server, the account and the password.
        pub(crate) connects: Vec<(usize, String, String)>,
        /// Each played: the server, the account and the world.
        pub(crate) played: Vec<(usize, String, Option<String>)>,
        /// The sessions started, with their channels' other ends.
        pub(crate) sessions: Vec<(
            std::sync::mpsc::SyncSender<WorldUpdate>,
            std::sync::mpsc::Receiver<ClientCommand>,
            Thread,
        )>,
    }

    /// An app with these login servers, which starts a session on each
    /// connect and says so in `heard`.
    pub(crate) struct Fake {
        pub(crate) servers: Vec<LoginServer>,
        pub(crate) heard: Arc<Mutex<Heard>>,
    }

    impl Logins for Fake {
        fn servers(&self) -> Vec<LoginServer> {
            self.servers.clone()
        }

        fn first(&self) -> usize {
            0
        }

        fn connect(
            &mut self,
            server: usize,
            account: &str,
            password: &str,
        ) -> Result<Connection, String> {
            let mut heard = self.heard.lock().unwrap();
            heard
                .connects
                .push((server, account.to_owned(), password.to_owned()));
            match &self.servers[server].availability {
                Availability::Here => (),
                Availability::Reopens(_) => return Ok(Connection::Reopened),
                Availability::Unavailable(reason) => return Err(reason.clone()),
            }
            let (updates, receive) = std::sync::mpsc::sync_channel(64);
            let (commands, queue) = std::sync::mpsc::sync_channel(8);
            let thread = Thread::default();
            heard.sessions.push((updates, queue, thread.clone()));
            Ok(Connection::Session(Session {
                updates: receive,
                commands,
                worker: Box::new(thread),
            }))
        }

        fn played(&mut self, server: usize, account: &str, world: Option<&str>) {
            self.heard.lock().unwrap().played.push((
                server,
                account.to_owned(),
                world.map(str::to_owned),
            ));
        }

        fn scripted(&self) -> Option<(String, Zeroizing<String>)> {
            Some(("scripted".into(), Zeroizing::new("secret".into())))
        }
    }

    /// A login server offered here, with this account remembered.
    pub(crate) fn server(name: &str, account: &str) -> LoginServer {
        LoginServer {
            name: name.into(),
            account: account.into(),
            world: None,
            availability: Availability::Here,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Fake, Heard, Thread, server};
    use super::*;
    use std::sync::{Arc, Mutex, atomic::Ordering};

    /// An app at a world's character list, with character select's Quit
    /// pressed.
    fn app(
        front: FrontEnd,
        updates: Option<Receiver<WorldUpdate>>,
        commands: Option<SyncSender<ClientCommand>>,
    ) -> App {
        let mut online = OnlineState::new(true);
        online.selection = Some(crate::character_select::Selection::new(7, Vec::new()));
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.insert_resource(front)
            .insert_resource(online)
            .insert_resource(crate::online::Updates(Mutex::new(updates)))
            .insert_resource(Outbox::new(commands))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<AppExit>()
            .add_systems(Update, quit);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        app.world_mut()
            .spawn((Interaction::Pressed, crate::character_select::Action::Quit));
        app
    }

    #[test]
    fn quit_at_character_select_goes_back_to_the_login_screen_keeping_the_account() {
        let fake = Fake {
            servers: vec![server("Example", "remembered")],
            heard: Arc::new(Mutex::new(Heard::default())),
        };
        let thread = Thread::default();
        let (_news, updates) = std::sync::mpsc::sync_channel(1);
        let (commands, _queue) = std::sync::mpsc::sync_channel(1);
        let (mut front, updates, commands) = FrontEnd::launched(
            Box::new(fake),
            Some(Session {
                updates,
                commands,
                worker: Box::new(thread.clone()),
            }),
        );
        front.account = "typed".into();
        front.password = Zeroizing::new("secret".into());
        let mut app = app(front, updates, commands);
        app.update();
        // The session stops and the world hears nothing more from it.
        assert!(thread.stopped.load(Ordering::SeqCst));
        assert!(
            app.world()
                .resource::<crate::online::Updates>()
                .0
                .lock()
                .unwrap()
                .is_none()
        );
        // The login screen shows with no reason, the account kept and the
        // password gone, and the character list is no more.
        let front = app.world().resource::<FrontEnd>();
        let online = app.world().resource::<OnlineState>();
        assert!(!front.running());
        assert_eq!(front.screen(online), Screen::Login);
        assert_eq!(front.status, "");
        assert_eq!(front.account, "typed");
        assert_eq!(front.password.as_str(), "");
        assert!(online.selection.is_none());
        assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 0);
    }

    #[test]
    fn quit_at_character_select_without_a_login_screen_leaves_the_game() {
        let mut app = app(FrontEnd::default(), None, None);
        app.update();
        assert_ne!(app.world().resource::<Messages<AppExit>>().len(), 0);
        assert!(app.world().resource::<OnlineState>().selection.is_some());
    }
}
