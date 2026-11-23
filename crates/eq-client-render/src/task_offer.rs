//! The tasks an NPC offers, in this client's own window until the skin's
//! task select window (`TaskSelectWnd`) is listed: the offered tasks, the
//! chosen one's duration, description and shown steps, and Accept and
//! Decline (checklist 6.560).
//!
//! Only the player's click on this window answers an offer: no script,
//! social or hotbutton reaches Accept or Decline. Where the admission
//! doesn't offer [`Capability::Tasks`] the offer is shown read only, with a
//! line saying it can't be answered here. An answer, the next offer and the
//! zone's end each close it. The task's words are shown here only, never
//! logged; a step's words without its own and the duration's are ours
//! (inferred, R49-38).
use crate::{
    clicks::Clicks,
    online::OnlineState,
    outbox::Outbox,
    theme::{self, Size},
    windows::{self, Shown, Stack, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    Capability, ClientCommand,
    tasks::{self, OfferedTask, TaskOffer},
    world::ClientWorld,
};

/// What a button of the window does to the offer it was drawn for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Answer {
    /// Shows this offered task, by its place in the offer.
    Choose(usize),
    /// Takes this task.
    Accept(u32),
    /// Declines the offer.
    Decline,
    /// Closes a read-only offer, answering nothing.
    Close,
}

#[derive(Clone, Copy, Component)]
pub(super) struct OfferButton {
    serial: u64,
    answer: Answer,
}

#[derive(Component)]
pub(super) struct OfferBody;

/// The task chosen in the open offer.
#[derive(Resource, Default)]
pub(super) struct Chosen {
    serial: u64,
    index: usize,
}

impl Chosen {
    fn of(&self, offer: &TaskOffer) -> usize {
        if self.serial == offer.serial && self.index < offer.offers.tasks.len() {
            self.index
        } else {
            0
        }
    }
}

/// Answers the open offer as one of its buttons says. Accept and Decline go
/// through the outbox with the current admission and close the offer once
/// posted; a refusal leaves it open and says why.
pub(super) fn answer(
    online: &mut OnlineState,
    outbox: &Outbox,
    chosen: &mut Chosen,
    serial: u64,
    answer: Answer,
) -> Result<(), &'static str> {
    let world = online.world();
    let offer = world.task_offer().ok_or("No task is offered")?;
    if offer.serial != serial {
        return Err("The task offer changed");
    }
    match answer {
        Answer::Choose(index) => {
            if index >= offer.offers.tasks.len() {
                return Err("The task offer changed");
            }
            *chosen = Chosen { serial, index };
            return Ok(());
        }
        Answer::Close => {}
        Answer::Accept(task_id) => {
            if !offer
                .offers
                .tasks
                .iter()
                .any(|task| task.task_id == task_id)
            {
                return Err("The task offer changed");
            }
            post(world, outbox, |stamp| ClientCommand::AcceptTask {
                session_id: stamp.session_id,
                task_id,
                created: stamp.created,
            })?;
        }
        Answer::Decline => {
            post(world, outbox, |stamp| ClientCommand::DeclineTasks {
                session_id: stamp.session_id,
            })?;
        }
    }
    online.close_task_offer(serial);
    Ok(())
}

fn post(
    world: &ClientWorld,
    outbox: &Outbox,
    command: impl FnOnce(crate::outbox::Stamp) -> ClientCommand,
) -> Result<(), &'static str> {
    if !world.can(Capability::Tasks) {
        return Err("Tasks can't be answered on this server");
    }
    outbox
        .try_post(world, command)
        .map_err(crate::outbox::Refusal::text)
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn buttons(
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    mut chosen: ResMut<Chosen>,
    mut chat: ResMut<crate::chat::ChatState>,
    clicks: Clicks<OfferButton>,
) {
    if let Some(button) = clicks.iter().next().copied()
        && let Err(reason) = answer(
            &mut online,
            &outbox,
            &mut chosen,
            button.serial,
            button.answer,
        )
    {
        chat.refuse(reason);
    }
}

/// What the drawn window shows: the offer, the chosen task and whether it
/// can be answered.
type Drawn = Option<((u64, usize, bool), Entity)>;

#[allow(clippy::needless_pass_by_value)]
pub(super) fn present(
    mut commands: Commands,
    online: Res<OnlineState>,
    chosen: Res<Chosen>,
    (mut shown, mut stack): (ResMut<Shown>, ResMut<Stack>),
    mut drawn: Local<Drawn>,
) {
    let world = online.world();
    let offer = world.task_offer();
    let key = offer.map(|offer| (offer.serial, chosen.of(offer), world.can(Capability::Tasks)));
    if key == drawn.as_ref().map(|(key, _)| *key) {
        return;
    }
    let opened = drawn.as_ref().map(|((serial, ..), _)| *serial);
    if let Some((_, entity)) = drawn.take() {
        commands.entity(entity).despawn();
    }
    let id = WindowId::TaskOffer;
    if let (Some(key), Some(offer)) = (key, offer) {
        shown.open(id);
        if opened != Some(offer.serial) {
            stack.raise(id);
        }
        *drawn = Some((key, window(&mut commands, offer, key.1, key.2)));
    } else {
        shown.close(id);
    }
}

fn window(commands: &mut Commands, offer: &TaskOffer, chosen: usize, answerable: bool) -> Entity {
    let panel = windows::frame(
        commands,
        WindowId::TaskOffer,
        Node {
            width: px(520),
            max_width: percent(92),
            max_height: percent(80),
            height: px(400),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            padding: UiRect::all(px(12)),
            ..default()
        },
    );
    let serial = offer.serial;
    let button = move |answer| OfferButton { serial, answer };
    commands.entity(panel).with_children(|body| {
        body.spawn(Node {
            flex_grow: 1.0,
            min_height: px(0),
            column_gap: px(8),
            ..default()
        })
        .with_children(|columns| {
            columns
                .spawn((
                    Node {
                        width: px(180),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    windows::pointer::TakesWheel,
                    ScrollPosition::default(),
                    OfferBody,
                ))
                .with_children(|list| {
                    for (index, task) in offer.offers.tasks.iter().enumerate() {
                        let mark = if index == chosen { "> " } else { "" };
                        theme::button_with(
                            list,
                            button(Answer::Choose(index)),
                            &format!("{mark}{}", task.title),
                            Size::Small,
                        );
                    }
                });
            if let Some(task) = offer.offers.tasks.get(chosen) {
                columns
                    .spawn((
                        Node {
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4),
                            padding: UiRect::all(px(6)),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        BackgroundColor(theme::WELL),
                        windows::pointer::TakesWheel,
                        ScrollPosition::default(),
                        OfferBody,
                    ))
                    .with_children(|details| detail(details, task));
            }
        });
        if !answerable {
            body.spawn(theme::text(
                "Tasks can't be answered on this server.",
                Size::Small,
                theme::INK_DIM,
            ));
        }
        body.spawn(Node {
            column_gap: px(8),
            justify_content: JustifyContent::End,
            ..default()
        })
        .with_children(|buttons| {
            if answerable {
                if let Some(task) = offer.offers.tasks.get(chosen) {
                    theme::button_with(
                        buttons,
                        button(Answer::Accept(task.task_id)),
                        "Accept",
                        Size::Body,
                    );
                }
                theme::button_with(buttons, button(Answer::Decline), "Decline", Size::Body);
            } else {
                theme::button_with(buttons, button(Answer::Close), "Close", Size::Body);
            }
        });
    });
    panel
}

fn detail(details: &mut ChildSpawnerCommands, task: &OfferedTask) {
    details.spawn(theme::text(&task.title, Size::Heading, theme::INK_WARM));
    details.spawn(theme::text(
        format!(
            "Duration: {}",
            tasks::duration_words(task.duration_seconds, task.duration_code)
        ),
        Size::Small,
        theme::INK,
    ));
    if !task.description.trim().is_empty() {
        details.spawn(theme::text(
            task.description.trim(),
            Size::Body,
            theme::INK_BRIGHT,
        ));
    }
    for step in &task.steps {
        details.spawn(theme::text(
            format!("- {}", tasks::offered_step_words(step)),
            Size::Body,
            theme::INK,
        ));
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    wheel: Res<windows::pointer::Wheel>,
    mut bodies: Query<(&ComputedNode, &mut ScrollPosition), With<OfferBody>>,
) {
    if let Some(entity) = wheel.surface
        && let Ok((node, mut position)) = bodies.get_mut(entity)
    {
        windows::scroll_by(&mut position, node, wheel.pixels);
    }
}

#[cfg(test)]
mod tests;
