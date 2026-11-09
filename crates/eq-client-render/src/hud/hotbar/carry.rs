//! Hotkeys on the cursor, as the official client puts a control on a
//! hotbutton: holding the left button on a spell gem, a hotbutton, an
//! Actions window button or a worn or carried item with a click effect picks
//! its hotkey up, and a click on a hotbutton puts it down there.
//!
//! Titanium dispatches its held-button notification after more than 750ms
//! (0x4a9741, 0x5b0050, 0x5bb040). Its hotbar handler 0x51550d picks up a
//! nonempty stored shortcut and clears/persists its source slot; placing one
//! replaces the destination and carries its old shortcut, independent of whether
//! either action can activate. Native snippets and installed art stay external.
//!
//! Releasing on the original hotbutton keeps the shortcut on the cursor;
//! releasing on another places it there. Dragging a hotbar shortcut off the bar,
//! clicking elsewhere with one carried, or Escape discards only the shortcut.
//! Changing profile/layout or losing input ownership cancels the gesture. The
//! cursor artwork is this client's presentation and still needs visual acceptance.
//!
//! Each control says what a hold picks up from it where it is built, with
//! [`Pickable`], and the systems that act on its clicks read them through
//! [`crate::clicks::Clicks`]. A quick click on such a control acts when the
//! button is let go, so a hold never clicks too. A press with something else
//! on the cursor acts at once, putting it down, as on an item's place; a
//! press with Shift, Ctrl or Alt, or on a control with nothing to pick up,
//! clicks as every button does, when let go over it, or at once on a control
//! that acts on the press ([`crate::clicks::OnPress`]).
use super::{Action, Bindings};
use crate::abilities::AbilityButton;
use bevy::{prelude::*, ui::RelativeCursorPosition};
use eq_client_core::{inventory::InventorySlot, world::ClientWorld};
use std::time::{Duration, Instant};

/// How long the left button is held on a control before its hotkey comes
/// onto the cursor. Titanium dispatches strictly after 750ms (0x4a9741).
pub(crate) const HOLD: Duration = Duration::from_millis(750);

/// The keys that bypass hotkey pickup, such as Shift-click on a stack.
const MODIFIERS: [KeyCode; 6] = [
    KeyCode::ShiftLeft,
    KeyCode::ShiftRight,
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::AltLeft,
    KeyCode::AltRight,
];

/// What a control gives the action bar, held or bound with Ctrl and a
/// number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// A spell gem, from 0: its spell, while it holds one.
    Gem(u8),
    /// A hotbutton, from 0: its stored shortcut, even when its action is
    /// temporarily unusable. Picking it up removes only this binding.
    Slot(usize),
    /// An Actions window ability button: the ability it holds now.
    Ability(AbilityButton),
    /// An inventory place: the item there, while it is worn or carried (not
    /// in the bank, on the cursor, in a trade or a world container) and has a
    /// click effect.
    Item(InventorySlot),
    /// An Actions window button that always does the same.
    Fixed(Action),
}

impl Source {
    /// What the control gives now, if anything: the one rule for what goes
    /// on the action bar, whether a hold picks it up or a key binds it.
    pub(crate) fn binding(self, world: &ClientWorld, bindings: &Bindings) -> Option<Action> {
        match self {
            Self::Gem(gem) => world.gem(usize::from(gem)).map(|_| Action::Gem(gem)),
            Self::Slot(index) => bindings.0.get(index).copied().flatten(),
            Self::Ability(button) => crate::abilities::assigned(world, button),
            Self::Item(slot) => world
                .inventory()
                .items()
                .get(&slot)
                .filter(|item| {
                    (slot.is_equipment() || slot.is_carried()) && item.activation.effect.is_some()
                })
                .map(|item| Action::Item {
                    slot,
                    id: item.details.id,
                }),
            Self::Fixed(action) => Some(action),
        }
    }
}

/// A control a hold picks a hotkey up from, and what it gives.
#[derive(Component, Clone, Copy, Debug)]
#[require(RelativeCursorPosition)]
pub(crate) struct Pickable(pub(crate) Source);

/// A press under way on a control a hold picks up from.
#[derive(Clone, Copy, Debug)]
struct Held {
    entity: Entity,
    source: Source,
    hotkey: Action,
    since: Instant,
    /// Whether the hold has picked the hotkey up.
    picked: bool,
}

/// What the left button does to the controls a hold picks up from, frame by
/// frame ([`route`]).
#[derive(Resource, Default, Debug)]
pub(crate) struct Presses {
    held: Option<Held>,
    /// The control clicked this frame.
    clicked: Option<Entity>,
    taken: bool,
    editing: Option<Editing>,
}

/// The admission/profile and exact layout that own a pending/cursor operation.
/// A replacement cannot retarget the captured shortcut to a different layout.
#[derive(Debug)]
struct Editing {
    admission: Option<u64>,
    profile: Option<crate::profile_files::CharacterProfile>,
    slots: [Option<Action>; 10],
    layout_revision: u64,
}

impl Editing {
    fn capture(
        world: &ClientWorld,
        profile: &crate::profile_files::Profile,
        bindings: &Bindings,
    ) -> Self {
        Self {
            admission: world.session_id(),
            profile: profile.key(),
            slots: bindings.0,
            layout_revision: bindings.revision(),
        }
    }

    fn matches(
        &self,
        world: &ClientWorld,
        profile: &crate::profile_files::Profile,
        bindings: &Bindings,
    ) -> bool {
        self.admission == world.session_id()
            && self.layout_revision == bindings.revision()
            && self.slots == bindings.0
            && self
                .profile
                .as_ref()
                .map(|key| (key.character.as_str(), key.world.as_str()))
                == profile.names()
    }
}

impl Presses {
    /// Whether this frame's press went to the hotkey on the cursor, which it
    /// put down or threw away, so it does nothing else.
    pub(crate) const fn taken(&self) -> bool {
        self.taken
    }

    /// A mouse hold which Escape can cancel before any binding changes.
    pub(crate) const fn pending(&self) -> bool {
        self.held.is_some()
    }

    fn cancel(&mut self, carry: &mut Carry, button: &mut crate::clicks::LeftButton) {
        self.taken = true;
        self.held = None;
        self.clicked = None;
        self.editing = None;
        carry.0 = None;
        button.cancel();
    }

    /// Whether the hold rule clicked this control this frame.
    pub(crate) fn clicked(&self, control: Entity) -> bool {
        self.clicked == Some(control)
    }
}

#[cfg(test)]
impl Carry {
    /// This hotkey on the cursor, as a hold puts it there.
    pub(crate) const fn holding(hotkey: Action) -> Self {
        Self(Some(hotkey))
    }
}

#[cfg(test)]
impl Presses {
    /// Takes the press under way back past the hold time, as if held that
    /// long.
    pub(crate) fn hold_past(&mut self) {
        if let Some(held) = self.held.as_mut() {
            held.since = held
                .since
                .checked_sub(HOLD + Duration::from_millis(1))
                .unwrap();
        }
    }
}

/// The hotkey riding the cursor.
#[derive(Resource, Default, Debug)]
pub(crate) struct Carry(Option<Action>);

impl Carry {
    /// The hotkey on the cursor, if one is.
    pub(crate) const fn hotkey(&self) -> Option<Action> {
        self.0
    }
}

/// A picture a hotkey shows on the cursor, by its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picture {
    /// A spell's icon.
    Spell(u32),
    /// An item's icon.
    Item(u32),
}

impl Picture {
    /// The picture, from the installation's art.
    pub(crate) fn image(self, art: &mut crate::sheets::Art) -> Option<ImageNode> {
        match self {
            Self::Spell(icon) => art.spell(icon),
            Self::Item(icon) => art.item(icon),
        }
    }
}

/// What the cursor shows of a hotkey: a gem's spell, with its icon, the
/// item it uses, with its picture, or else the words a hotbutton shows for
/// it. Inferred: what the official client shows there is not checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Face {
    pub(crate) picture: Option<Picture>,
    pub(crate) words: String,
}

impl Face {
    /// How a hotkey looks on the cursor now.
    pub(crate) fn of(
        hotkey: Action,
        world: &ClientWorld,
        names: &crate::spellbook::SpellNames,
    ) -> Self {
        let spell = match hotkey {
            Action::Gem(gem) => world.gem(usize::from(gem)),
            _ => None,
        };
        let item = match hotkey {
            Action::Item { slot, id } => world
                .inventory()
                .items()
                .get(&slot)
                .filter(|item| item.details.id == id),
            _ => None,
        };
        if let Some(spell) = spell {
            Self {
                picture: names.icon(spell).map(Picture::Spell),
                words: names.label(spell),
            }
        } else if let Some(item) = item {
            Self {
                picture: item.details.icon.map(Picture::Item),
                words: item.details.name.clone(),
            }
        } else {
            Self {
                picture: None,
                words: match hotkey {
                    Action::Gem(gem) => format!("Spell gem {}", gem + 1),
                    Action::Item { .. } => "Item shortcut".into(),
                    _ => super::caption_text(Some(hotkey), world),
                },
            }
        }
    }
}

/// The controls, with what a hold picks up from each that can.
type Controls<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Interaction,
        Option<&'static Pickable>,
        Has<crate::clicks::OnPress>,
        Option<&'static RelativeCursorPosition>,
        Has<crate::tooltip::ScriptPressed>,
    ),
>;

/// Decides what this frame's left button does with hotkeys: a hold picks one
/// up, a press with one on the cursor puts it down on a hotbutton or throws
/// it away, and a quick press says which control it clicked. Anything else
/// coming onto the cursor (an item, coins, a spell from the book) throws the
/// hotkey away, as a spell from the book gives way to them, and so does
/// leaving the world.
///
/// It reads the button's state, not `Interaction`: Bevy lets a held
/// control's `Interaction` go when the pointer leaves the window, and marks
/// the control under the pointer hovered when the button is let go, which is
/// how a hotbutton under a released hold is found.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
#[allow(
    clippy::too_many_lines,
    reason = "keep ownership, pickup and release decisions in gesture order"
)]
pub(crate) fn route(
    keys: crate::keys::Keys,
    mouse: Res<ButtonInput<MouseButton>>,
    (online, hand): (
        Res<crate::online::OnlineState>,
        Res<crate::spellbook::BookHand>,
    ),
    (profile, escape): (
        Res<crate::profile_files::Profile>,
        Res<crate::escape::Escape>,
    ),
    mut bindings: ResMut<Bindings>,
    (mut carry, mut presses, mut button): (
        ResMut<Carry>,
        ResMut<Presses>,
        ResMut<crate::clicks::LeftButton>,
    ),
    mut controls: Controls,
) {
    let now = Instant::now();
    presses.clicked = None;
    presses.taken = false;
    let world = online.world();
    let active = carry.0.is_some() || presses.pending();
    let busy = world
        .inventory()
        .items()
        .contains_key(&InventorySlot::CURSOR)
        || crate::coins::on_cursor(world).is_some()
        || hand.held.is_some();
    let replaced = presses
        .editing
        .as_ref()
        .is_some_and(|editing| !editing.matches(world, &profile, &bindings));
    let focused = keys.window_focused();
    let can_edit = world.connected() && world.player().is_some();
    let unavailable = !focused || !can_edit;
    if active && (busy || replaced || unavailable || *escape == crate::escape::Escape::Hotbar) {
        presses.cancel(&mut carry, &mut button);
        return;
    }
    if !focused {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        let under = controls
            .iter()
            .filter(|(_, interaction, ..)| **interaction == Interaction::Pressed)
            .find_map(|(entity, _, pickable, on_press, ..)| {
                pickable.map(|pickable| (entity, pickable.0, on_press))
            });
        presses.held = None;
        presses.editing = None;
        if let Some(hotkey) = carry.0 {
            presses.taken = true;
            carry.0 = match under {
                Some((_, Source::Slot(index), _)) => put_down(&mut bindings, world, index, hotkey),
                _ => None,
            };
            if carry.0.is_some() {
                presses.editing = Some(Editing::capture(world, &profile, &bindings));
            }
            button.cancel();
            for (_, mut interaction, ..) in &mut controls {
                if *interaction == Interaction::Pressed {
                    *interaction = Interaction::Hovered;
                }
            }
        } else if let Some((entity, source, on_press)) = under {
            let plain = !keys.input.any_pressed(MODIFIERS);
            if plain
                && can_edit
                && !busy
                && let Some(hotkey) = source.binding(world, &bindings)
            {
                button.claim(entity);
                presses.held = Some(Held {
                    entity,
                    source,
                    hotkey,
                    since: now,
                    picked: false,
                });
                presses.editing = Some(Editing::capture(world, &profile, &bindings));
            } else if on_press || busy {
                button.claim(entity);
                presses.clicked = Some(entity);
            }
        }
    }
    let Some(mut held) = presses.held else {
        return;
    };
    let over_source =
        controls
            .get(held.entity)
            .ok()
            .map(|(_, interaction, _, _, relative, scripted)| {
                *interaction != Interaction::None
                    && (scripted || relative.is_some_and(RelativeCursorPosition::cursor_over))
            });
    if over_source.is_none() {
        presses.cancel(&mut carry, &mut button);
        return;
    }
    let released = mouse.just_released(MouseButton::Left);
    // A reset/capture of the physical button is not a release and cannot click.
    if !mouse.pressed(MouseButton::Left) && !released {
        presses.cancel(&mut carry, &mut button);
        return;
    }
    // Also check the deadline on a release frame: a slow frame must not turn a
    // long hold into activation merely because no held frame was rendered.
    if !held.picked && expired(held.since, now) {
        if over_source != Some(true) || held.source.binding(world, &bindings) != Some(held.hotkey) {
            presses.cancel(&mut carry, &mut button);
            return;
        }
        held.picked = true;
        if let Source::Slot(index) = held.source {
            bindings.0[index] = None;
        }
        carry.0 = Some(held.hotkey);
        presses.editing = Some(Editing::capture(world, &profile, &bindings));
    }
    if !released {
        presses.held = Some(held);
        return;
    }
    presses.held = None;
    if !held.picked {
        if over_source == Some(true) {
            presses.clicked = Some(held.entity);
        }
        presses.editing = None;
        return;
    }
    presses.taken = true;
    button.cancel();
    if let Some(hotkey) = carry.0 {
        let over = controls
            .iter()
            .find_map(
                |(_, interaction, pickable, _, relative, scripted)| match pickable {
                    Some(Pickable(Source::Slot(index)))
                        if *interaction != Interaction::None
                            && (scripted
                                || relative.is_some_and(RelativeCursorPosition::cursor_over)) =>
                    {
                        Some(*index)
                    }
                    _ => None,
                },
            );
        match over {
            Some(index) if held.source != Source::Slot(index) => {
                carry.0 = put_down(&mut bindings, world, index, hotkey);
            }
            None if matches!(held.source, Source::Slot(_)) => carry.0 = None,
            _ => (),
        }
    }
    presses.editing = carry
        .0
        .map(|_| Editing::capture(world, &profile, &bindings));
}

fn expired(since: Instant, now: Instant) -> bool {
    now.saturating_duration_since(since) > HOLD
}

/// Places a shortcut and carries the exact previous binding, including one
/// whose current action cannot activate. This never moves an inventory item.
fn put_down(
    bindings: &mut Bindings,
    world: &ClientWorld,
    index: usize,
    hotkey: Action,
) -> Option<Action> {
    let lifted = Source::Slot(index).binding(world, bindings);
    *bindings.0.get_mut(index)? = Some(hotkey);
    lifted
}

#[cfg(test)]
pub(crate) mod pointer_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clicks::{
        Clicks,
        testing::{press, release},
    };
    use crate::online::{OnlineState, testing};

    /// A control the tests press.
    #[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
    struct Control(u8);

    /// The controls clicked so far, in order.
    #[derive(Resource, Default)]
    struct Clicked(Vec<u8>);

    #[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
    fn record(clicks: Clicks<Control>, mut clicked: ResMut<Clicked>) {
        clicked.0.extend(clicks.iter().map(|control| control.0));
    }

    /// The viewer's app with a player in the world whose first gem holds a
    /// spell, the default hotbar and the hold rule.
    fn app() -> App {
        let mut app = crate::testing::app();
        app.insert_resource(world())
            .init_resource::<Clicked>()
            .add_systems(Update, (route, record).chain());
        app
    }

    /// A player in the world whose first gem holds a spell.
    fn world() -> OnlineState {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(1));
        testing::spell(
            &mut online,
            eq_client_core::SpellUpdate::Slot {
                slot: 0,
                spell_id: 73,
                mode: 1,
            },
        );
        online
    }

    fn control(app: &mut App, number: u8, source: Option<Source>) -> Entity {
        let mut control = app.world_mut().spawn((Control(number), Interaction::None));
        if let Some(source) = source {
            control.insert((
                Pickable(source),
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
            ));
        }
        control.id()
    }

    fn hold(app: &mut App) {
        app.world_mut().resource_mut::<Presses>().hold_past();
        app.update();
    }

    fn clicked(app: &App) -> &[u8] {
        &app.world().resource::<Clicked>().0
    }

    fn hotkey(app: &App) -> Option<Action> {
        app.world().resource::<Carry>().hotkey()
    }

    fn slots(app: &App) -> [Option<Action>; 10] {
        app.world().resource::<Bindings>().0
    }

    /// Picks a gem's hotkey up with a hold, and lets go over the gem.
    fn pick_up(app: &mut App, gem: Entity) {
        press(app, Some(gem));
        hold(app);
        release(app, Some(gem));
    }

    #[test]
    fn a_quick_press_clicks_when_let_go_and_a_hold_picks_up_without_clicking() {
        let mut app = app();
        let gem = control(&mut app, 1, Some(Source::Gem(0)));
        let plain = control(&mut app, 2, None);
        press(&mut app, Some(gem));
        assert_eq!(clicked(&app), [] as [u8; 0]);
        release(&mut app, Some(gem));
        assert_eq!(clicked(&app), [1]);
        // A control a hold can't pick up from clicks as it is let go, as
        // every button does.
        press(&mut app, Some(plain));
        assert_eq!(clicked(&app), [1]);
        release(&mut app, Some(plain));
        assert_eq!(clicked(&app), [1, 2]);
        assert_eq!(hotkey(&app), None);
        press(&mut app, Some(gem));
        app.update();
        assert_eq!(hotkey(&app), None);
        hold(&mut app);
        assert_eq!(hotkey(&app), Some(Action::Gem(0)));
        // Let go over the gem, the hotkey stays on the cursor and nothing is
        // clicked; the gem keeps its spell, and the bar is unchanged.
        release(&mut app, Some(gem));
        assert_eq!(hotkey(&app), Some(Action::Gem(0)));
        assert_eq!(clicked(&app), [1, 2]);
        assert_eq!(slots(&app), Bindings::default().0);
    }

    #[test]
    fn a_quick_press_let_go_off_its_control_clicks_nothing() {
        let mut app = app();
        let gem = control(&mut app, 1, Some(Source::Gem(0)));
        let other = control(&mut app, 2, Some(Source::Slot(1)));
        press(&mut app, Some(gem));
        release(&mut app, Some(other));
        press(&mut app, Some(gem));
        release(&mut app, None);
        assert_eq!(clicked(&app), [] as [u8; 0]);
        assert_eq!(hotkey(&app), None);
        assert_eq!(slots(&app), Bindings::default().0);
    }

    #[test]
    fn a_press_that_could_not_pick_up_clicks_when_let_go_or_puts_the_cursor_down_at_once() {
        let mut app = app();
        let empty = control(&mut app, 1, Some(Source::Gem(1)));
        let gem = control(&mut app, 2, Some(Source::Gem(0)));
        // An empty gem has nothing to pick up: it clicks as a button does.
        press(&mut app, Some(empty));
        assert_eq!(clicked(&app), [] as [u8; 0]);
        release(&mut app, Some(empty));
        assert_eq!(clicked(&app), [1]);
        // Let go off it, it clicks nothing.
        press(&mut app, Some(empty));
        release(&mut app, None);
        assert_eq!(clicked(&app), [1]);
        // A modifier bypasses the hold rule and leaves the ordinary click.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        press(&mut app, Some(gem));
        assert_eq!(clicked(&app), [1]);
        release(&mut app, Some(gem));
        assert_eq!(clicked(&app), [1, 2]);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ShiftLeft);
        // A spell from the book on the cursor goes into the gem at once.
        app.world_mut()
            .resource_mut::<crate::spellbook::BookHand>()
            .held = Some(crate::spellbook::Entry { slot: 3, spell: 9 });
        press(&mut app, Some(gem));
        assert_eq!(clicked(&app), [1, 2, 2]);
        hold(&mut app);
        assert_eq!(hotkey(&app), None);
        release(&mut app, Some(gem));
        assert_eq!(clicked(&app), [1, 2, 2]);
    }

    #[test]
    fn a_hotkey_put_down_on_a_hotbutton_swaps_with_what_it_held() {
        let mut app = app();
        let gem = control(&mut app, 1, Some(Source::Gem(0)));
        let ninth = control(&mut app, 9, Some(Source::Slot(8)));
        let fifth = control(&mut app, 5, Some(Source::Slot(4)));
        app.world_mut().resource_mut::<Bindings>().0[4] = None;
        pick_up(&mut app, gem);
        press(&mut app, Some(ninth));
        assert_eq!(slots(&app)[8], Some(Action::Gem(0)));
        assert_eq!(hotkey(&app), Some(Action::Sit));
        release(&mut app, Some(ninth));
        press(&mut app, Some(fifth));
        assert_eq!(slots(&app)[4], Some(Action::Sit));
        assert_eq!(hotkey(&app), None);
        release(&mut app, Some(fifth));
        // The presses that put hotkeys down clicked nothing.
        assert_eq!(clicked(&app), [] as [u8; 0]);
    }

    #[test]
    fn replacing_an_unusable_shortcut_carries_its_stored_binding() {
        let mut app = app();
        let gem = control(&mut app, 1, Some(Source::Gem(0)));
        let fifth = control(&mut app, 5, Some(Source::Slot(4)));
        let sixth = control(&mut app, 6, Some(Source::Slot(5)));
        // Hotbuttons 5 and 6 start as gems 5 and 6, which hold no spell.
        assert_eq!(slots(&app)[4], Some(Action::Gem(4)));
        assert_eq!(slots(&app)[5], Some(Action::Gem(5)));
        // A click swaps the stored shortcut, even without a memorized spell.
        pick_up(&mut app, gem);
        press(&mut app, Some(fifth));
        release(&mut app, Some(fifth));
        assert_eq!(slots(&app)[4], Some(Action::Gem(0)));
        assert_eq!(hotkey(&app), Some(Action::Gem(4)));
        press(&mut app, None);
        release(&mut app, None);
        // Letting go of a hold over another shortcut also swaps it.
        press(&mut app, Some(gem));
        hold(&mut app);
        release(&mut app, Some(sixth));
        assert_eq!(slots(&app)[5], Some(Action::Gem(0)));
        assert_eq!(hotkey(&app), Some(Action::Gem(5)));
        assert_eq!(clicked(&app), [] as [u8; 0]);
    }

    #[test]
    fn a_hold_can_pick_up_a_stored_shortcut_whose_gem_is_empty() {
        let mut app = app();
        let fifth = control(&mut app, 5, Some(Source::Slot(4)));
        press(&mut app, Some(fifth));
        assert_eq!(clicked(&app), [] as [u8; 0]);
        hold(&mut app);
        assert_eq!(hotkey(&app), Some(Action::Gem(4)));
        release(&mut app, Some(fifth));
        assert_eq!(clicked(&app), [] as [u8; 0]);
        assert_eq!(slots(&app)[4], None);
        assert_eq!(hotkey(&app), Some(Action::Gem(4)));
    }

    #[test]
    fn a_hotbutton_held_leaves_its_slot_and_drops_where_the_hold_is_let_go() {
        let mut app = app();
        let first = control(&mut app, 1, Some(Source::Slot(0)));
        let ninth = control(&mut app, 9, Some(Source::Slot(8)));
        press(&mut app, Some(first));
        hold(&mut app);
        assert_eq!(slots(&app)[0], None);
        assert_eq!(hotkey(&app), Some(Action::Gem(0)));
        // Let go over the hotbutton it came from, it stays on the cursor.
        release(&mut app, Some(first));
        assert_eq!(slots(&app)[0], None);
        assert_eq!(hotkey(&app), Some(Action::Gem(0)));
        // Put back, then held and let go over the ninth: they swap.
        press(&mut app, Some(first));
        release(&mut app, Some(first));
        assert_eq!(slots(&app)[0], Some(Action::Gem(0)));
        press(&mut app, Some(first));
        hold(&mut app);
        release(&mut app, Some(ninth));
        assert_eq!(slots(&app)[8], Some(Action::Gem(0)));
        assert_eq!(hotkey(&app), Some(Action::Sit));
        assert_eq!(clicked(&app), [] as [u8; 0]);
    }

    #[test]
    fn a_click_elsewhere_throws_the_hotkey_away_and_does_nothing_else() {
        let mut app = app();
        let gem = control(&mut app, 1, Some(Source::Gem(0)));
        let plain = control(&mut app, 2, None);
        pick_up(&mut app, gem);
        press(&mut app, Some(plain));
        assert_eq!(hotkey(&app), None);
        assert_eq!(clicked(&app), [] as [u8; 0]);
        assert_eq!(
            app.world().get::<Interaction>(plain),
            Some(&Interaction::Hovered)
        );
        release(&mut app, Some(plain));
        // A click on the world, over no control, throws it away too, and
        // says so for the world's own click.
        pick_up(&mut app, gem);
        press(&mut app, None);
        assert_eq!(hotkey(&app), None);
        assert!(app.world().resource::<Presses>().taken());
        release(&mut app, None);
        assert!(!app.world().resource::<Presses>().taken());
        // A click on a gem with no hotkey left casts again.
        press(&mut app, Some(gem));
        release(&mut app, Some(gem));
        assert_eq!(clicked(&app), [1]);
    }

    #[test]
    fn anything_else_on_the_cursor_or_leaving_the_world_ends_the_hotkey() {
        let mut app = app();
        let gem = control(&mut app, 1, Some(Source::Gem(0)));
        pick_up(&mut app, gem);
        app.world_mut()
            .resource_mut::<crate::spellbook::BookHand>()
            .held = Some(crate::spellbook::Entry { slot: 3, spell: 9 });
        app.update();
        assert_eq!(hotkey(&app), None);
        app.world_mut()
            .resource_mut::<crate::spellbook::BookHand>()
            .held = None;
        pick_up(&mut app, gem);
        let mut item = crate::preview::items().remove(0);
        item.slot = InventorySlot::CURSOR;
        testing::inventory(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            eq_client_core::inventory::InventoryUpdate::Snapshot(vec![item]),
        );
        app.update();
        assert_eq!(hotkey(&app), None);
        app.insert_resource(world());
        pick_up(&mut app, gem);
        assert_eq!(hotkey(&app), Some(Action::Gem(0)));
        app.insert_resource(OnlineState::new(true));
        app.update();
        assert_eq!(hotkey(&app), None);
    }

    #[test]
    fn what_each_control_gives_follows_what_it_holds_now() {
        let online = world();
        let world = online.world();
        let bindings = Bindings::default();
        assert_eq!(
            Source::Gem(0).binding(world, &bindings),
            Some(Action::Gem(0))
        );
        assert_eq!(Source::Gem(1).binding(world, &bindings), None);
        // A hotbutton gives its stored shortcut independently of usability.
        assert_eq!(
            Source::Slot(0).binding(world, &bindings),
            Some(Action::Gem(0))
        );
        assert_eq!(
            Source::Slot(1).binding(world, &bindings),
            Some(Action::Gem(1))
        );
        assert_eq!(Source::Slot(8).binding(world, &bindings), Some(Action::Sit));
        assert_eq!(
            Source::Fixed(Action::Camp).binding(world, &bindings),
            Some(Action::Camp)
        );
    }

    #[test]
    fn the_cursor_shows_a_gems_spell_an_items_picture_or_a_hotbuttons_words() {
        let mut online = world();
        let mut item = crate::preview::items().remove(0);
        item.slot = InventorySlot(23);
        item.details.icon = Some(500);
        let (id, name) = (item.details.id, item.details.name.clone());
        testing::inventory(
            &mut online,
            eq_client_core::inventory::InventoryUpdate::Snapshot(vec![item]),
        );
        let mut fields = vec!["0"; 145];
        fields[0] = "73";
        fields[1] = "Synthetic spell";
        fields[144] = "36";
        let names = crate::spellbook::SpellNames::parse(&fields.join("^"));
        let face = |hotkey| Face::of(hotkey, online.world(), &names);
        assert_eq!(
            face(Action::Gem(0)),
            Face {
                picture: Some(Picture::Spell(36)),
                words: "Synthetic spell".into(),
            }
        );
        let slot = InventorySlot(23);
        assert_eq!(
            face(Action::Item { slot, id }),
            Face {
                picture: Some(Picture::Item(500)),
                words: name,
            }
        );
        // Missing action data still leaves visible shortcut feedback.
        assert_eq!(
            face(Action::Item { slot, id: id + 1 }).words,
            "Item shortcut"
        );
        assert_eq!(face(Action::Gem(1)).words, "Spell gem 2");
        assert_eq!(
            face(Action::Camp),
            Face {
                picture: None,
                words: "Camp".into(),
            }
        );
    }

    #[test]
    fn only_a_worn_or_carried_item_with_a_click_effect_is_given() {
        let mut online = world();
        let mut items = Vec::new();
        for slot in [0, 21, 22, 29, 251, 330, 30, 331, 2000, 2031, 3000, 4000] {
            let mut item = crate::preview::items().remove(0);
            item.slot = InventorySlot(slot);
            item.activation.effect = Some(eq_client_core::inventory::ClickEffect {
                spell_id: 73,
                kind: eq_client_core::inventory::ClickKind::Click,
                required_level: 1,
                effect_level: 1,
                cast_time_ms: 1000,
                recast_delay_seconds: 0,
                recast_type: 0,
            });
            items.push(item);
        }
        let mut plain = crate::preview::items().remove(0);
        plain.slot = InventorySlot(23);
        plain.activation.effect = None;
        items.push(plain);
        testing::inventory(
            &mut online,
            eq_client_core::inventory::InventoryUpdate::Snapshot(items),
        );
        let world = online.world();
        let given = |slot| Source::Item(InventorySlot(slot)).binding(world, &Bindings::default());
        for slot in [0, 21, 22, 29, 251, 330] {
            assert!(
                matches!(given(slot), Some(Action::Item { slot: at, .. }) if at.0 == slot),
                "{slot}"
            );
        }
        // Not the cursor or a bag on it, the bank, a trade or a world
        // container, nor a carried item without a click effect.
        for slot in [30, 331, 2000, 2031, 3000, 4000, 23] {
            assert_eq!(given(slot), None, "{slot}");
        }
        // Editing a stored item shortcut does not depend on the item still
        // being usable. Activation checks remain separate.
        let id = world.inventory().items()[&InventorySlot(22)].details.id;
        let bound = |slot, id| {
            let mut bindings = Bindings::default();
            bindings.0[0] = Some(Action::Item {
                slot: InventorySlot(slot),
                id,
            });
            Source::Slot(0).binding(world, &bindings)
        };
        assert_eq!(
            bound(22, id),
            Some(Action::Item {
                slot: InventorySlot(22),
                id,
            })
        );
        for (slot, id) in [(22, id + 1), (23, id), (24, id), (2000, id)] {
            assert_eq!(
                bound(slot, id),
                Some(Action::Item {
                    slot: InventorySlot(slot),
                    id
                }),
                "{slot}"
            );
        }
    }
}
