//! Guild-window actions use the existing guild commands. A roster selection
//! and confirmation bind the canonical member, actor, guild and admission;
//! only a subsequent received roster change establishes an effect.
use crate::{
    confirm::{Asked, Question},
    online::OnlineState,
    outbox::Outbox,
    theme::{self, Size},
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand, listing::GuildRank, world::ClientWorld};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// These controls do not transfer the guild's leadership or remove oneself.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Remove,
    Promote,
    Demote,
}

impl Action {
    const fn label(self) -> &'static str {
        match self {
            Self::Remove => "Remove member",
            Self::Promote => "Promote officer",
            Self::Demote => "Demote member",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Remove => "Remove",
            Self::Promote => "Promote",
            Self::Demote => "Demote",
        }
    }

    fn command(self, selected: &Selection) -> ClientCommand {
        let session_id = selected.session;
        let name = selected.member.clone();
        match self {
            Self::Remove => ClientCommand::GuildRemove { session_id, name },
            Self::Promote => ClientCommand::GuildInvite {
                session_id,
                name,
                officer: true,
            },
            Self::Demote => ClientCommand::GuildDemote { session_id, name },
        }
    }
}

/// A row carries the identity/revision it displayed, rather than a row index
/// that a reordered roster could silently assign to another member.
#[derive(Component)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) session: Option<u64>,
    pub(crate) revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Selection {
    session: u64,
    guild: u32,
    actor: String,
    actor_rank: GuildRank,
    revision: u64,
    member: String,
    rank: GuildRank,
}

impl Selection {
    fn new(world: &ClientWorld, name: &str) -> Option<Self> {
        if !world.connected() {
            return None;
        }
        let player = world.player()?;
        let member = world
            .guild_members()?
            .iter()
            .find(|member| member.name == name)?;
        Some(Self {
            session: world.session_id()?,
            guild: player.listing.guild?,
            actor: player.name.clone(),
            actor_rank: player.listing.guild_rank?,
            revision: world.guild_membership_revision(),
            member: member.name.clone(),
            rank: member.rank,
        })
    }

    fn current(&self, world: &ClientWorld) -> bool {
        Self::new(world, &self.member).as_ref() == Some(self)
    }

    fn same_admission(&self, world: &ClientWorld) -> bool {
        world.connected()
            && world.session_id() == Some(self.session)
            && world.player().is_some_and(|player| {
                player.name == self.actor && player.listing.guild == Some(self.guild)
            })
    }
}

struct Ticket {
    id: u64,
    action: Action,
    selected: Selection,
}
struct Pending {
    action: Action,
    selected: Selection,
    refusal: u64,
    since: Instant,
}

#[derive(Resource, Default)]
pub(crate) struct State {
    selected: Option<Selection>,
    asking: Option<Ticket>,
    pending: Option<Pending>,
    serial: u64,
    note: String,
}

impl State {
    pub(crate) fn question(&self, id: u64) -> String {
        self.asking.as_ref().filter(|ticket| ticket.id == id).map_or_else(String::new, |ticket| {
            let name = &ticket.selected.member;
            match ticket.action {
                Action::Remove => format!("Remove {name} from your guild? Choose Yes to request removal of this exact member, or No to cancel."),
                Action::Promote => format!("Offer {name} the Officer rank? They must accept the promotion. Choose Yes to send the offer, or No to cancel."),
                Action::Demote => format!("Demote {name} from Officer to Member? Choose Yes to request this rank change, or No to cancel."),
            }
        })
    }

    fn reason(&self, world: &ClientWorld, action: Action) -> Result<(), &'static str> {
        if self.pending.is_some() {
            return Err("Waiting for the previous guild request; no retry is sent.");
        }
        if self.asking.is_some() {
            return Err("Answer the current confirmation first.");
        }
        let selected = self
            .selected
            .as_ref()
            .ok_or("Select a received guild member first.")?;
        eligible(world, selected, action)
    }

    fn withdraw(&mut self, asked: &mut Asked) {
        if let Some(ticket) = self.asking.take() {
            let question = Question::GuildManagement {
                request_id: ticket.id,
            };
            if asked.question() == Some(question) {
                asked.withdraw();
            }
            let _ = asked.answer(question);
        }
    }

    fn advance(&mut self, world: &ClientWorld, asked: &mut Asked, visible: bool, now: Instant) {
        if let Some(pending) = &self.pending {
            let outcome = if !pending.selected.same_admission(world) {
                Some(
                    "Guild request no longer belongs to this admission; its result is unconfirmed."
                        .into(),
                )
            } else if world.guild_refusal().0 != pending.refusal {
                Some(format!(
                    "Guild refusal received while waiting: {}. Check the roster; no change confirmed.",
                    world.guild_refusal().1
                ))
            } else if confirmed(world, pending) {
                Some(format!(
                    "Server roster confirms: {} — {}.",
                    pending.selected.member,
                    pending.action.label()
                ))
            } else if now.saturating_duration_since(pending.since) >= Duration::from_secs(30) {
                Some("No matching guild change confirmed. Check the received roster and server messages before trying again.".into())
            } else {
                None
            };
            if let Some(note) = outcome {
                self.note = note;
                self.pending = None;
            }
        }
        if !visible
            || self
                .selected
                .as_ref()
                .is_some_and(|selected| !selected.current(world))
        {
            if self.asking.is_some() {
                self.note =
                    "Guild selection changed; confirmation cancelled. Select the member again."
                        .into();
            }
            self.withdraw(asked);
            self.selected = None;
        }
    }

    fn begin(&mut self, world: &ClientWorld, asked: &mut Asked, action: Action) {
        if let Err(reason) = self.reason(world, action) {
            self.note = reason.into();
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        let question = Question::GuildManagement {
            request_id: self.serial,
        };
        if asked.has_pending_answer() || !asked.ask(question) {
            self.note = "Answer the existing question first.".into();
            return;
        }
        self.asking = Some(Ticket {
            id: self.serial,
            action,
            selected: self.selected.clone().expect("eligible selection"),
        });
        self.note = "Confirm the named member and action, or choose No to cancel.".into();
    }

    fn answer(&mut self, world: &ClientWorld, asked: &mut Asked, outbox: &Outbox, now: Instant) {
        let Some(ticket) = &self.asking else {
            return;
        };
        let Some(yes) = asked.answer(Question::GuildManagement {
            request_id: ticket.id,
        }) else {
            return;
        };
        let ticket = self.asking.take().expect("answer owns ticket");
        if !yes {
            self.note = "Cancelled; no guild request sent.".into();
            return;
        }
        if let Err(reason) = eligible(world, &ticket.selected, ticket.action) {
            self.note = reason.into();
            return;
        }
        match outbox.send(world, ticket.action.command(&ticket.selected)) {
            Ok(()) => {
                self.note = "Request queued; awaiting a matching server roster change.".into();
                self.pending = Some(Pending {
                    action: ticket.action,
                    selected: ticket.selected,
                    refusal: world.guild_refusal().0,
                    since: now,
                });
            }
            Err(error) => self.note = format!("Guild request not sent: {error}"),
        }
    }
}

fn eligible(world: &ClientWorld, selected: &Selection, action: Action) -> Result<(), &'static str> {
    if !selected.current(world) {
        return Err("Guild selection changed. Select the member again.");
    }
    if !world.can(Capability::GuildManagement) {
        return Err("Guild management is unavailable in this session.");
    }
    if !world.guild_members().is_some_and(|members| {
        members
            .iter()
            .any(|member| member.name == selected.actor && member.rank == selected.actor_rank)
    }) {
        return Err("Waiting for your received roster rank and character rank to agree.");
    }
    if selected.actor.eq_ignore_ascii_case(&selected.member) {
        return Err("Select another member; these controls do not change your own membership.");
    }
    match action {
        Action::Remove
            if !matches!(selected.actor_rank, GuildRank::Officer | GuildRank::Leader) =>
        {
            Err("Only an Officer or Guild Master can remove another member.")
        }
        Action::Promote | Action::Demote if selected.actor_rank != GuildRank::Leader => {
            Err("Only the Guild Master can change another member's rank.")
        }
        Action::Remove if !matches!(selected.rank, GuildRank::Member | GuildRank::Officer) => {
            Err("These controls cannot remove a Guild Master or an unknown rank.")
        }
        Action::Promote if selected.rank != GuildRank::Member => {
            Err("Select a Member to offer the Officer rank.")
        }
        Action::Promote if !world.player_in_zone(&selected.member) => {
            Err("Promotion requires the member to be present in your zone to accept the offer.")
        }
        Action::Demote if selected.rank != GuildRank::Officer => {
            Err("Select an Officer to demote to Member.")
        }
        _ => Ok(()),
    }
}

fn confirmed(world: &ClientWorld, pending: &Pending) -> bool {
    if world.guild_membership_revision() == pending.selected.revision {
        return false;
    }
    let Some(members) = world.guild_members() else {
        return false;
    };
    let member = members
        .iter()
        .find(|member| member.name == pending.selected.member);
    match pending.action {
        Action::Remove => member.is_none(),
        Action::Promote => member.is_some_and(|member| member.rank == GuildRank::Officer),
        Action::Demote => member.is_some_and(|member| member.rank == GuildRank::Member),
    }
}

#[derive(Component)]
pub(crate) struct SelectedLabel;
#[derive(Component)]
pub(crate) struct StatusLabel;

pub(crate) fn controls(parent: &mut ChildSpawnerCommands, width: f32, height: f32) {
    for (selected, y) in [(true, height - 96.0), (false, height - 46.0)] {
        let mut label = parent.spawn((
            theme::text("", Size::Small, theme::INK_BRIGHT),
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
            crate::skinned::at(8.0, y, width - 16.0, 20.0),
        ));
        if selected {
            label.insert(SelectedLabel);
        } else {
            label.insert(StatusLabel);
        }
    }
    for (x, action) in
        [8.0, 150.0, 292.0]
            .into_iter()
            .zip([Action::Remove, Action::Promote, Action::Demote])
    {
        parent
            .spawn((
                Button,
                action,
                crate::skinned::SkinId {
                    window: WindowId::Guild,
                    id: action.id().into(),
                },
                crate::tooltip::Tooltip(String::new()),
                Node {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..crate::skinned::at(x, height - 72.0, 136.0, 24.0)
                },
                BackgroundColor(theme::BUTTON_OFF),
            ))
            .with_child(theme::text(action.label(), Size::Small, theme::INK_BRIGHT));
    }
}

/// The same click route serves the authored and fallback window and scripts.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn input(
    online: Res<OnlineState>,
    shown: Res<Shown>,
    outbox: Res<Outbox>,
    mut asked: ResMut<Asked>,
    mut state: ResMut<State>,
    members: crate::clicks::Clicks<Member>,
    actions: crate::clicks::Clicks<Action>,
) {
    let world = online.world();
    let visible = shown.displayed(WindowId::Guild);
    let now = Instant::now();
    state.advance(world, &mut asked, visible, now);
    if !visible {
        return;
    }
    for member in members.iter() {
        if member.session == world.session_id()
            && member.revision == world.guild_membership_revision()
        {
            state.withdraw(&mut asked);
            state.selected = Selection::new(world, &member.name);
            if state.pending.is_none() {
                state.note.clear();
            }
        }
    }
    for action in actions.iter() {
        state.begin(world, &mut asked, *action);
    }
    state.answer(world, &mut asked, &outbox, now);
}

#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(crate) fn present(
    online: Res<OnlineState>,
    state: Res<State>,
    mut labels: Query<
        (&mut Text, Has<SelectedLabel>),
        Or<(With<SelectedLabel>, With<StatusLabel>)>,
    >,
    mut actions: Query<(
        &Action,
        &Interaction,
        &mut BackgroundColor,
        &mut crate::tooltip::Tooltip,
    )>,
    mut members: Query<(&Member, &Interaction, &mut BackgroundColor), Without<Action>>,
) {
    let selected = state.selected.as_ref().map_or_else(
        || "Select a member to manage. The server decides permissions and results.".into(),
        |selected| format!("Selected: {} — {:?}", selected.member, selected.rank),
    );
    for (mut text, is_selected) in &mut labels {
        text.0
            .clone_from(if is_selected { &selected } else { &state.note });
    }
    for (action, interaction, mut color, mut tip) in &mut actions {
        let reason = state.reason(online.world(), *action);
        color.0 = theme::button(reason.is_ok(), false, *interaction);
        tip.0 = reason
            .err()
            .unwrap_or("Confirm this action for the selected member; the server must accept it.")
            .into();
    }
    for (member, interaction, mut color) in &mut members {
        color.0 = theme::button(
            true,
            state
                .selected
                .as_ref()
                .is_some_and(|selected| selected.member == member.name),
            *interaction,
        );
    }
}
