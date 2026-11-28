//! Real picker nodes, Bevy layout, cursor hit testing and keyboard messages.
use super::{app, bounds, fixture};
use crate::skinned::color_picker::{self, COMBAT_COLORS, HexEdit, Paint, Part};
use crate::{
    keys::Typing,
    options::OptionsState,
    profile_files::Profile,
    theme,
    windows::{Shown, WindowId},
};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        touch::Touches,
    },
    prelude::*,
    ui::{UiStack, ui_focus_system},
    window::PrimaryWindow,
};
use eq_client_core::options::Options;

struct Form {
    app: App,
    window: Entity,
    square: Entity,
    hue: Entity,
    hex: Entity,
    swatch: Entity,
    anchor: Entity,
    label: Entity,
}

fn stack(world: &World, id: Entity, ordered: &mut Vec<Entity>) {
    ordered.push(id);
    if let Some(children) = world.get::<Children>(id) {
        for child in children {
            stack(world, *child, ordered);
        }
    }
}

fn draw_color_fields(app: &mut App) -> Entity {
    app.world_mut()
        .run_system_once(|mut commands: Commands| {
            commands
                .spawn(super::super::at(0.0, 0.0, 128.0, 128.0))
                .with_children(|ui| {
                    color_picker::draw_swatch(
                        ui,
                        WindowId::Options,
                        color_picker::Binding::Combat(COMBAT_COLORS[1].2),
                        super::super::at(0.0, 0.0, 18.0, 18.0),
                        theme::Size::Label,
                    );
                    color_picker::draw_swatch(
                        ui,
                        WindowId::Options,
                        color_picker::Binding::Chat(2),
                        super::super::at(150.0, 0.0, 18.0, 18.0),
                        theme::Size::Label,
                    );
                })
                .id()
        })
        .unwrap()
}

fn form() -> Form {
    let mut app = app(1.0, fixture().1);
    let options = Options::read(
        "combat_text_incoming_red=20\ncombat_text_incoming_green=40\ncombat_text_incoming_blue=60\n",
        Options::default(),
    );
    app.insert_resource(OptionsState::new(&options))
        .init_resource::<Touches>()
        .init_resource::<UiStack>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<Key>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Shown>()
        .init_resource::<Typing>()
        .init_resource::<Profile>()
        .init_resource::<HexEdit>()
        .add_message::<KeyboardInput>()
        .add_systems(PreUpdate, ui_focus_system)
        .add_systems(
            Update,
            (
                color_picker::manage,
                color_picker::focus,
                color_picker::edit_hex,
                color_picker::pick,
                color_picker::present,
                color_picker::show_swatches,
            )
                .chain(),
        );
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Options);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    let root = draw_color_fields(&mut app);
    app.update();
    let anchor = app.world().get::<Children>(root).unwrap()[0];
    assert_eq!(
        app.world_mut().query::<&Part>().iter(app.world()).count(),
        0
    );
    focus_stack(&mut app);
    let pos = bounds(app.world(), anchor).center();
    press_at(&mut app, window, pos);
    app.update();
    release(&mut app);
    focus_stack(&mut app);
    let world = app.world_mut();
    let (mut sq, mut hue, mut hex) = (None, None, None);
    for (id, part) in world.query::<(Entity, &Part)>().iter(world) {
        match part {
            Part::Square(_) => sq = Some(id),
            Part::Hue(_) => hue = Some(id),
            Part::Hex(_) => hex = Some(id),
        }
    }
    let mut paint = world.query::<(Entity, &Paint)>();
    let swatch = paint
        .iter(world)
        .find(|(_, p)| matches!(p, Paint::Swatch(_)))
        .unwrap()
        .0;
    let label = paint
        .iter(world)
        .find(|(_, p)| matches!(p, Paint::Hex(_)))
        .unwrap()
        .0;
    app.update();
    Form {
        app,
        window,
        square: sq.unwrap(),
        hue: hue.unwrap(),
        hex: hex.unwrap(),
        swatch,
        anchor,
        label,
    }
}

fn focus_stack(app: &mut App) {
    let world = app.world_mut();
    let mut ordered = Vec::new();
    for root in world
        .query_filtered::<Entity, (With<Node>, Without<ChildOf>)>()
        .iter(world)
    {
        stack(world, root, &mut ordered);
    }
    for &id in &ordered {
        world.entity_mut(id).insert(InheritedVisibility::VISIBLE);
    }
    *world.resource_mut::<UiStack>() = UiStack {
        partition: std::iter::once(0..ordered.len()).collect(),
        uinodes: ordered,
    };
}

fn press_at(app: &mut App, window: Entity, pos: Vec2) {
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(pos.as_dvec2()));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
}

fn release(app: &mut App) {
    let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    mouse.clear();
    mouse.release(MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

impl Form {
    fn click(&mut self, id: Entity, fraction: Vec2) {
        let rect = bounds(self.app.world(), id);
        assert!(rect.size().cmpgt(Vec2::ZERO).all());
        let pos = rect.min + rect.size() * fraction;
        press_at(&mut self.app, self.window, pos);
        self.app.update();
        assert_eq!(
            self.app.world().get::<Interaction>(id),
            Some(&Interaction::Pressed)
        );
        release(&mut self.app);
    }

    fn keys(&mut self, text: &str, finish: Key) {
        for (key_code, logical_key, text) in [
            (KeyCode::KeyA, Key::Character("a".into()), Some(text.into())),
            (KeyCode::Enter, finish, None),
        ] {
            self.app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key,
                text,
                state: ButtonState::Pressed,
                repeat: false,
                window: self.window,
            });
        }
        self.app.update();
    }

    fn assert_color(&self, levels: [u16; 3], hex: &str) {
        let options = &self.app.world().resource::<OptionsState>().options;
        assert_eq!(options.combat_text_incoming, levels);
        assert_eq!(
            options.combat_text_outgoing,
            Options::default().combat_text_outgoing
        );
        assert_eq!(
            Options::read(&options.text(), Options::default()).combat_text_incoming,
            levels
        );
        assert_eq!(self.app.world().get::<Text>(self.label).unwrap().0, hex);
        assert_eq!(
            self.app
                .world()
                .get::<BackgroundColor>(self.swatch)
                .unwrap()
                .0
                .to_srgba()
                .to_hex(),
            hex
        );
    }
}

#[test]
fn saved_slider_values_load_and_pointer_picks_update_the_preview_and_profile() {
    let mut form = form();
    form.assert_color([20, 40, 60], "#336699");
    // Select pure green hue and then half value at full saturation.
    form.click(form.hue, Vec2::new(0.5, 1.0 / 3.0));
    form.click(form.square, Vec2::new(0.999, 0.5));
    form.assert_color([0, 50, 0], "#008000");
}

#[test]
fn entering_hex_updates_the_same_swatch_and_escape_discards_an_invalid_edit() {
    let mut form = form();
    form.click(form.hex, Vec2::splat(0.5));
    assert!(form.app.world().resource::<Typing>().color_hex);
    form.keys("#CC33FF", Key::Enter);
    form.assert_color([80, 20, 100], "#CC33FF");
    form.click(form.hex, Vec2::splat(0.5));
    form.keys("zz00zz", Key::Enter);
    assert!(form.app.world().resource::<Typing>().color_hex);
    form.keys("", Key::Escape);
    assert!(form.app.world().resource::<Typing>().escape_consumed);
    form.assert_color([80, 20, 100], "#CC33FF");
}

#[test]
fn outside_click_dismisses_the_editor_and_chat_swatch_edits_byte_precision() {
    let mut form = form();
    let window = form.window;
    let app = &mut form.app;
    let count = |app: &mut App| app.world_mut().query::<&Part>().iter(app.world()).count();
    assert_eq!(count(app), 3);
    assert_eq!(bounds(app.world(), form.anchor).size(), Vec2::splat(18.0));
    press_at(app, window, Vec2::new(300.0, 180.0));
    app.update();
    release(app);
    assert_eq!(count(app), 0);
    focus_stack(app);
    press_at(app, window, Vec2::new(159.0, 9.0));
    app.update();
    release(app);
    assert_eq!(count(app), 3);
    focus_stack(app);
    let hex = app
        .world_mut()
        .query::<(Entity, &Part)>()
        .iter(app.world())
        .find(|(_, p)| matches!(p, Part::Hex(_)))
        .unwrap()
        .0;
    form.click(hex, Vec2::splat(0.5));
    form.keys("#0123FE", Key::Enter);
    let saved = &form.app.world().resource::<OptionsState>().options;
    assert_eq!(saved.user_colors[2], Some([1, 35, 254]));
    assert_eq!(
        Options::read(&saved.text(), Options::default()).user_colors[2],
        Some([1, 35, 254])
    );
    assert_eq!(saved.combat_text_incoming, [20, 40, 60]);
}

#[test]
fn clicking_another_options_control_dismisses_the_open_picker() {
    let mut form = form();
    press_at(&mut form.app, form.window, Vec2::new(159.0, 9.0));
    form.app.update();
    release(&mut form.app);
    let world = form.app.world_mut();
    assert_eq!(world.query::<&Part>().iter(world).count(), 0);
    assert!(world.resource::<Shown>().displayed(WindowId::Options));
}

#[test]
fn physical_click_on_a_stock_list_sample_edits_its_own_chat_color() {
    use crate::skinned::option_lists::{ColorSample, chat_rows};
    let mut form = form();
    let app = &mut form.app;
    let window = form.window;
    // Close the independent fixture popup and create the same list as the Titanium Colors page.
    press_at(app, window, Vec2::new(350.0, 350.0));
    app.update();
    release(app);
    app.world_mut()
        .run_system_once(|mut commands: Commands| {
            let mut list = commands.spawn(Node {
                flex_direction: FlexDirection::Column,
                ..super::super::at(220.0, 0.0, 220.0, 300.0)
            });
            chat_rows(&mut list, &[150.0]);
        })
        .unwrap();
    app.update();
    focus_stack(app);
    let world = app.world_mut();
    let (chip, row) = world
        .query::<(Entity, &ColorSample, &ChildOf)>()
        .iter(world)
        .find(|(_, kind, _)| kind.0 == 5)
        .map(|(id, _, parent)| (id, parent.parent()))
        .unwrap();
    let caption = world.get::<Children>(row).unwrap()[0];
    let middle = bounds(world, chip).center();
    assert!((middle.y - bounds(world, caption).center().y).abs() <= 1.0);
    press_at(app, window, middle);
    app.update();
    release(app);
    let world = app.world_mut();
    let hex = world
        .query::<(Entity, &Part)>()
        .iter(world)
        .find(|(_, part)| matches!(part, Part::Hex(_)))
        .unwrap()
        .0;
    focus_stack(app);
    form.click(hex, Vec2::splat(0.5));
    form.keys("#2468AC", Key::Enter);
    assert_eq!(
        form.app
            .world()
            .resource::<OptionsState>()
            .options
            .user_colors[5],
        Some([0x24, 0x68, 0xac])
    );
}
