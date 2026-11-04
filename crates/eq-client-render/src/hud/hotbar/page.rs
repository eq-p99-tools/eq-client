//! The first hotbar's ten pages. Active slots remain the one projection read
//! by activation, binding and drag code. Native page wrap: 0x513b6b; direct
//! selection: 0x4619cd, commands 75..84. Selection is runtime state; no native
//! selected-page persistence contract is assumed.
use super::Bindings;
use bevy::prelude::*;
use eq_client_core::hotbar::{Hotbar, PAGES, Pages};

#[derive(Default)]
pub(super) struct State {
    pages: Pages,
    selected: usize,
    revision: u64,
}

impl Bindings {
    pub(crate) const fn page(&self) -> usize {
        self.1.selected
    }
    pub(super) const fn revision(&self) -> u64 {
        self.1.revision
    }

    /// A complete save snapshot, including edits made through active slots.
    pub(super) fn snapshot(&self) -> Pages {
        let mut pages = self.1.pages;
        pages.0[self.page()] = Hotbar(self.0);
        pages
    }

    pub(super) fn load(&mut self, pages: &Pages) {
        // Native binding loading (0x4b0812) replaces the page contents while
        // page selection is separate runtime state. Keep the current view.
        self.0 = pages.0[self.page()].0;
        self.1.pages = *pages;
        self.1.revision = self.1.revision.wrapping_add(1);
    }

    /// Commit the active page before selecting another. Identity changes even
    /// when both pages hold identical bindings, or a page is replaced twice
    /// between pointer frames. Re-selecting the current page is a no-op.
    pub(crate) fn select(&mut self, page: usize) -> bool {
        if page >= PAGES || page == self.page() {
            return false;
        }
        self.1.pages = self.snapshot();
        self.1.selected = page;
        self.0 = self.1.pages.0[page].0;
        self.1.revision = self.1.revision.wrapping_add(1);
        true
    }

    fn turn(&mut self, forward: bool) {
        self.select((self.page() + if forward { 1 } else { PAGES - 1 }) % PAGES);
    }
}

/// A hotbar page arrow. Its ordinary release rule is shared by both skins.
#[derive(Component, Clone, Copy)]
pub(crate) struct Button(pub(crate) bool);
/// The active page number, from 1, in ordinary or skinned chrome.
#[derive(Component)]
pub(crate) struct Number;

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn input(
    keys: crate::keys::Keys,
    online: Res<crate::online::OnlineState>,
    mut bindings: ResMut<Bindings>,
    buttons: crate::clicks::Clicks<Button>,
) {
    if !keys.window_focused() || !online.world().connected() || online.world().player().is_none() {
        return;
    }
    if let Some(page) = (0_u8..10).find(|page| keys.pressed(crate::keys::Act::HotbarPage(*page))) {
        bindings.select(usize::from(page));
    } else if let Some(button) = buttons.iter().next() {
        bindings.turn(button.0);
    }
}

pub(super) fn spawn(commands: &mut Commands, frame: Entity) {
    let row = super::super::row(commands, frame, 4.0);
    for (forward, words) in [(false, "<"), (true, ">")] {
        let button = super::super::slot(commands, row, words, 24.0, false);
        commands.entity(button).insert((
            bevy::prelude::Button,
            Button(forward),
            crate::outbox::Needs::Nothing,
        ));
    }
    let number = super::super::label(
        commands,
        row,
        "1",
        crate::theme::Size::Caption,
        crate::theme::INK,
    );
    commands.entity(number).insert(Number);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::Action;
    use super::super::carry::pointer_tests::{self, Fixture};
    use super::*;

    fn chord(fixture: &mut Fixture, modifiers: &[KeyCode], key: KeyCode) {
        let mut keys = fixture
            .app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        for modifier in modifiers {
            keys.press(*modifier);
        }
        keys.press(key);
        fixture.app.update();
        fixture
            .app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }

    /// Both real factories must expose page controls through the release rule,
    /// and use the selected page for keyboard activation and bind/clear.
    pub(crate) fn controls(mut fixture: Fixture) {
        // Keyboard editing is an explicit user choice, independent of page keys.
        crate::keys::testing::bind(
            &mut fixture.app,
            crate::keys::Act::BindSlot(0),
            crate::keys::Chord::ctrl(KeyCode::Digit1),
        );
        crate::keys::testing::bind(
            &mut fixture.app,
            crate::keys::Act::ClearSlot(0),
            crate::keys::Chord::ctrl_shift(KeyCode::Digit1),
        );
        fixture.app.add_systems(
            Update,
            (super::super::update, super::super::presentation)
                .chain()
                .after(super::super::carry::route),
        );
        fixture.pointer(Vec2::new(30.0, 120.0), Some(true));
        assert_eq!(fixture.app.world().resource::<Bindings>().page(), 0);
        fixture.pointer(Vec2::new(30.0, 120.0), Some(false));
        assert_eq!(fixture.app.world().resource::<Bindings>().page(), 9);
        let world = fixture.app.world_mut();
        assert_eq!(
            world
                .query_filtered::<&Text, With<Number>>()
                .single(world)
                .unwrap()
                .0,
            "10"
        );
        fixture.pointer(Vec2::new(80.0, 120.0), Some(true));
        fixture.pointer(Vec2::new(80.0, 120.0), Some(false));
        assert_eq!(fixture.app.world().resource::<Bindings>().page(), 0);
        chord(&mut fixture, &[KeyCode::ShiftLeft], KeyCode::Digit0);
        assert_eq!(fixture.app.world().resource::<Bindings>().page(), 9);
        fixture
            .app
            .world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        chord(&mut fixture, &[KeyCode::ShiftLeft], KeyCode::Digit1);
        assert_eq!(fixture.app.world().resource::<Bindings>().page(), 9);
        fixture
            .app
            .world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = false;
        fixture.app.world_mut().resource_mut::<Bindings>().0[0] = Some(Action::Camp);
        chord(&mut fixture, &[], KeyCode::Digit1);
        assert_eq!(fixture.requests(), [Action::Camp]);
        fixture.app.world_mut().resource_mut::<Bindings>().0[8] = Some(Action::Sit);
        fixture.pointer(Vec2::new(430.0, 60.0), None);
        chord(&mut fixture, &[KeyCode::ControlLeft], KeyCode::Digit1);
        assert_eq!(fixture.binding(0), Some(Action::Sit));
        chord(
            &mut fixture,
            &[KeyCode::ControlLeft, KeyCode::ShiftLeft],
            KeyCode::Digit1,
        );
        assert_eq!(fixture.binding(0), None);
        assert_eq!(fixture.app.world().resource::<Bindings>().page(), 9);
        chord(&mut fixture, &[KeyCode::ShiftLeft], KeyCode::Digit1);
        assert_eq!(fixture.binding(0), Some(Action::Gem(0)));
        assert_eq!(fixture.requests(), [Action::Camp]);
    }

    pub(crate) fn hold_context(mut fixture: Fixture, picked: bool, round_trip: bool) {
        let mut pages = Pages::default();
        pages.0[1] = pages.0[0];
        fixture
            .app
            .world_mut()
            .resource_mut::<Bindings>()
            .load(&pages);
        fixture.press(0);
        if picked {
            fixture.hold();
        }
        if round_trip {
            {
                let mut bindings = fixture.app.world_mut().resource_mut::<Bindings>();
                bindings.select(1);
                bindings.select(0);
            }
            fixture.app.update();
        } else {
            chord(&mut fixture, &[KeyCode::ShiftLeft], KeyCode::Digit2);
        }
        fixture.release(0);
        let bindings = fixture.app.world().resource::<Bindings>();
        assert_eq!(bindings.page(), usize::from(!round_trip));
        assert_eq!(bindings.snapshot().0[1].0[0], Some(Action::Gem(0)));
        assert_eq!(
            bindings.snapshot().0[0].0[0],
            if picked { None } else { Some(Action::Gem(0)) }
        );
        assert_eq!(fixture.carry(), None);
        assert!(
            !fixture
                .app
                .world()
                .resource::<super::super::carry::Presses>()
                .pending()
        );
        assert_eq!(fixture.requests(), []);
    }

    #[test]
    fn ordinary_controls_select_wrap_activate_bind_and_clear_the_current_page() {
        controls(pointer_tests::ordinary());
    }

    #[test]
    fn ordinary_holds_cannot_cross_identical_pages_or_an_away_and_back_replacement() {
        for picked in [false, true] {
            for round_trip in [false, true] {
                hold_context(pointer_tests::ordinary(), picked, round_trip);
            }
        }
    }

    #[test]
    fn an_unfocused_window_cannot_select_a_page_by_key() {
        let mut fixture = pointer_tests::ordinary();
        let world = fixture.app.world_mut();
        world
            .query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>()
            .single_mut(world)
            .unwrap()
            .focused = false;
        chord(&mut fixture, &[KeyCode::ShiftLeft], KeyCode::Digit0);
        assert_eq!(fixture.app.world().resource::<Bindings>().page(), 0);
        assert_eq!(fixture.requests(), []);
    }

    #[test]
    fn selecting_commits_active_edits_and_wraps_all_ten_pages() {
        let mut bindings = Bindings::default();
        bindings.0[0] = Some(Action::Camp);
        bindings.turn(false);
        assert_eq!(bindings.page(), 9);
        assert_eq!(bindings.0, [None; 10]);
        bindings.0[0] = Some(Action::Sit);
        bindings.turn(true);
        assert_eq!(bindings.page(), 0);
        assert_eq!(bindings.0[0], Some(Action::Camp));
        bindings.select(9);
        assert_eq!(bindings.0[0], Some(Action::Sit));
        let revision = bindings.revision();
        assert!(!bindings.select(10));
        assert!(!bindings.select(9));
        assert_eq!(bindings.revision(), revision);
        assert_eq!(bindings.snapshot().0[0].0[0], Some(Action::Camp));
    }
}
