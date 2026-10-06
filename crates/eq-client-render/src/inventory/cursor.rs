//! Non-interactive display of the actual inventory cursor slot: the skin's
//! cursor attachment where the skin has one, or else the client's own box
//! with the item's name.
use super::{InventorySlot, InventoryState};
use crate::hud::hotbar::carry::{Carry, Face, Picture};
use crate::skinned::CursorPlace;
use crate::theme::{self, Size};
use bevy::{prelude::*, ui::FocusPolicy, window::PrimaryWindow};

#[derive(Component)]
pub(crate) struct Overlay;

pub(super) fn spawn(commands: &mut Commands) {
    commands.spawn((
        Overlay,
        crate::hud::HudRoot,
        GlobalZIndex(100),
        FocusPolicy::Pass,
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            column_gap: px(6),
            align_items: AlignItems::Center,
            max_width: px(260),
            padding: UiRect::all(px(4)),
            ..default()
        },
        BackgroundColor(theme::SCRIM),
    ));
}

/// What rides the cursor: the item, the coins, the spell picked up from the
/// skin's book and how a hotkey looks, of which the first there is shown.
type Riding<'a> = (
    Option<&'a eq_client_core::inventory::InventoryItem>,
    Option<(eq_client_core::money::Coin, u32)>,
    Option<u32>,
    Option<Face>,
);

/// What the overlay was last drawn for: its entity, the window's and the
/// inventory's revisions, the coins on the cursor, the spell picked up from
/// the skin's book and how a hotkey on the cursor looks.
type Drawn = (
    Entity,
    u64,
    u64,
    Option<(eq_client_core::money::Coin, u32)>,
    Option<u32>,
    Option<Face>,
);

/// Follows the pointer using confirmed/predicted inventory state, never a selected slot.
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub(crate) fn update(
    mut commands: Commands,
    state: Res<InventoryState>,
    online: Res<crate::online::OnlineState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    (scale, look): (Option<Res<UiScale>>, Res<crate::skinned::CursorLook>),
    (hand, names, carry): (
        Res<crate::spellbook::BookHand>,
        Res<crate::spellbook::SpellNames>,
        Res<Carry>,
    ),
    controls: Query<(&Interaction, &ComputedNode, &UiGlobalTransform)>,
    mut root: Query<(Entity, &mut Node, &mut BackgroundColor), With<Overlay>>,
    mut stamp: Local<Option<Drawn>>,
    mut art: crate::sheets::Art,
) {
    let Ok((entity, mut node, mut background)) = root.single_mut() else {
        return;
    };
    let inventory = online.world().inventory();
    let item = inventory.items().get(&InventorySlot::CURSOR);
    let coins = crate::coins::on_cursor(online.world());
    // A spell from the skin's book rides the cursor while nothing else does.
    let spell = hand
        .held
        .map(|held| held.spell)
        .filter(|_| item.is_none() && coins.is_none());
    // So does a hotkey picked up for a hotbutton.
    let hotkey = carry
        .hotkey()
        .filter(|_| item.is_none() && coins.is_none() && spell.is_none())
        .map(|hotkey| Face::of(hotkey, online.world(), &names));
    let factor = scale.as_ref().map_or(1.0, |scale| scale.0);
    // A control hovered with no pointer over the window, as a script hovers
    // one, stands in for the pointer at its middle, as for a tooltip.
    let hovered = || {
        controls
            .iter()
            .find(|(interaction, ..)| **interaction != Interaction::None)
            .map(|(_, computed, at)| at.translation * computed.inverse_scale_factor())
    };
    let pointer = windows.single().ok().and_then(|window| {
        let pointer = match window.cursor_position() {
            Some(pointer) => window.focused.then_some(pointer / factor),
            None => hovered(),
        }?;
        Some((pointer, window.size() / factor))
    });
    let carried = item.is_some() || coins.is_some() || spell.is_some() || hotkey.is_some();
    node.display = if carried && pointer.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if let Some((pointer, viewport)) = pointer {
        if look.0.is_some() {
            hang(&mut node, pointer);
        } else {
            place(&mut node, pointer, viewport);
        }
    }
    let current = (
        entity,
        state.revision,
        inventory.revision(),
        coins,
        spell,
        hotkey.clone(),
    );
    if stamp.as_ref() == Some(&current) && !look.is_changed() {
        return;
    }
    *stamp = Some(current);
    commands.entity(entity).despawn_children();
    dress(&mut node, &mut background, look.0.as_ref());
    let riding = (item, coins, spell, hotkey);
    if let Some(place) = &look.0 {
        let picture = picture(place, &riding, &names, &mut art);
        let count = item.map_or(coins.map(|(_, count)| count), |item| {
            item.stack_count.filter(|count| *count > 1)
        });
        // A hotkey without a picture shows its words in the picture's place.
        let words = riding
            .3
            .filter(|_| picture.is_none())
            .map(|face| face.words);
        commands
            .entity(entity)
            .with_children(|parent| attachment(parent, place, picture, (count, words)));
        return;
    }
    let Some((icon, text)) = own_box(riding, inventory.stale(), &names, &mut art) else {
        return;
    };
    commands.entity(entity).with_children(|parent| {
        if let Some(icon) = icon {
            parent.spawn((
                icon,
                FocusPolicy::Pass,
                Node {
                    width: px(32),
                    height: px(32),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
        }
        parent.spawn((
            Text::new(text),
            theme::font(Size::Body),
            TextColor(theme::INK_WARM),
            FocusPolicy::Pass,
        ));
    });
}

/// What the client's own box shows of what rides the cursor: the item's
/// picture and name, with its stack's count; the coins on their own, as in
/// the official client; the name of a spell from the skin's book; or a
/// hotkey's picture and words.
fn own_box(
    (item, coins, spell, hotkey): Riding,
    stale: bool,
    names: &crate::spellbook::SpellNames,
    art: &mut crate::sheets::Art,
) -> Option<(Option<ImageNode>, String)> {
    Some(if let Some(item) = item {
        let mut text = item.details.name.clone();
        if let Some(count) = item.stack_count {
            use std::fmt::Write;
            let _ = write!(text, " x{count}");
        }
        if stale {
            text.push_str("\nAwaiting inventory update");
        }
        (item.details.icon.and_then(|icon| art.item(icon)), text)
    } else if let Some((coin, count)) = coins {
        (None, format!("{count} {}", coin_name(coin)))
    } else if let Some(spell) = spell {
        (None, names.label(spell))
    } else {
        let face = hotkey?;
        (
            face.picture.and_then(|picture| picture.image(art)),
            face.words,
        )
    })
}

/// The picture of what rides the cursor, in the skin's attachment: the
/// item's icon, the spell's or a hotkey's, where the skin puts its picture,
/// or the skin's picture of the coins at its own size.
fn picture(
    place: &CursorPlace,
    (item, coins, spell, hotkey): &Riding,
    names: &crate::spellbook::SpellNames,
    art: &mut crate::sheets::Art,
) -> Option<(ImageNode, Vec2)> {
    let whole = Vec2::new(place.icon.width, place.icon.height);
    match (*item, *coins) {
        (Some(item), _) => item
            .details
            .icon
            .and_then(|icon| art.item(icon))
            .map(|icon| (icon, whole)),
        (None, Some((coin, _))) => place.coin(coin).and_then(|piece| {
            let size = Vec2::new(to_f32(piece.width), to_f32(piece.height));
            art.cut(piece).map(|image| (image, size))
        }),
        (None, None) => spell
            .and_then(|spell| names.icon(spell))
            .map(Picture::Spell)
            .or_else(|| hotkey.as_ref().and_then(|face| face.picture))
            .and_then(|picture| picture.image(art))
            .map(|icon| (icon, whole)),
    }
}

/// Dresses the overlay as the skin's cursor attachment, a box without a
/// background of its own, or as the client's own box.
fn dress(node: &mut Node, background: &mut BackgroundColor, skin: Option<&CursorPlace>) {
    if let Some(place) = skin {
        node.width = px(place.size.x);
        node.height = px(place.size.y);
        node.max_width = Val::Auto;
        node.padding = UiRect::ZERO;
        background.0 = Color::NONE;
    } else {
        node.width = Val::Auto;
        node.height = Val::Auto;
        node.padding = UiRect::all(px(4));
        background.0 = theme::SCRIM;
    }
}

/// What rides the cursor, in the skin's cursor attachment: the item's icon
/// where the skin puts its picture, with its stack's count as a slot shows
/// it, the skin's picture of the coins with their count, or the words of a
/// hotkey without a picture. Whether the official client shows a count
/// there, and what it shows for coins and hotkeys, is not checked yet.
fn attachment(
    parent: &mut ChildSpawnerCommands,
    place: &CursorPlace,
    picture: Option<(ImageNode, Vec2)>,
    (count, words): (Option<u32>, Option<String>),
) {
    let icon = place.icon;
    let at = |x: f32, y: f32, width: f32, height: f32| Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(width),
        height: px(height),
        ..default()
    };
    if let Some((image, size)) = picture {
        // A smaller picture, as a coin's, sits in the middle of the place.
        let size = size.min(Vec2::new(icon.width, icon.height));
        parent.spawn((
            image,
            FocusPolicy::Pass,
            at(
                icon.x + (icon.width - size.x) / 2.0,
                icon.y + (icon.height - size.y) / 2.0,
                size.x,
                size.y,
            ),
        ));
    }
    if let Some(count) = count {
        parent.spawn((
            theme::text(count.to_string(), Size::Body, theme::INK_BRIGHT),
            TextLayout::new(Justify::Right, LineBreak::NoWrap),
            FocusPolicy::Pass,
            at(icon.x, icon.y + icon.height - 14.0, icon.width - 3.0, 13.0),
        ));
    }
    if let Some(words) = words {
        parent
            .spawn((
                FocusPolicy::Pass,
                BackgroundColor(theme::SCRIM),
                Node {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..at(icon.x, icon.y, icon.width, icon.height)
                },
            ))
            .with_child((
                theme::text(words, Size::Caption, theme::INK_BRIGHT),
                TextLayout::new(Justify::Center, LineBreak::WordBoundary),
                FocusPolicy::Pass,
            ));
    }
}

/// A picture's size in pixels, which fits a float exactly.
#[allow(clippy::cast_precision_loss, reason = "a picture's pixels are few")]
const fn to_f32(pixels: u32) -> f32 {
    pixels as f32
}

/// Hangs the skin's cursor attachment from the pointer, its top left corner
/// there; where the official client puts it is not checked yet.
fn hang(node: &mut Node, pointer: Vec2) {
    node.left = px(pointer.x);
    node.top = px(pointer.y);
    node.right = Val::Auto;
    node.bottom = Val::Auto;
}

/// What a kind of coin is called.
const fn coin_name(coin: eq_client_core::money::Coin) -> &'static str {
    use eq_client_core::money::Coin;
    match coin {
        Coin::Platinum => "platinum",
        Coin::Gold => "gold",
        Coin::Silver => "silver",
        Coin::Copper => "copper",
    }
}

/// Anchors away from the nearest edges without waiting for text layout measurements.
fn place(node: &mut Node, pointer: Vec2, viewport: Vec2) {
    let pointer = pointer.clamp(Vec2::ZERO, viewport);
    let offset = 16.0;
    let available_width = if pointer.x <= viewport.x * 0.5 {
        node.left = px((pointer.x + offset).min(viewport.x));
        node.right = Val::Auto;
        viewport.x - pointer.x - offset
    } else {
        node.left = Val::Auto;
        node.right = px((viewport.x - pointer.x + offset).min(viewport.x));
        pointer.x - offset
    };
    node.max_width = px(available_width.clamp(0.0, 260.0));
    if pointer.y <= viewport.y * 0.5 {
        node.top = px((pointer.y + offset).min(viewport.y));
        node.bottom = Val::Auto;
    } else {
        node.top = Val::Auto;
        node.bottom = px((viewport.y - pointer.y + offset).min(viewport.y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skins_attachment_hangs_from_the_pointer_with_the_stacks_count() {
        let mut app = crate::testing::app();
        app.add_systems(Startup, |mut commands: Commands| spawn(&mut commands))
            .add_systems(Update, update);
        app.world_mut()
            .resource_mut::<crate::skinned::CursorLook>()
            .0 = Some(CursorPlace {
            size: Vec2::splat(50.0),
            icon: eq_client_assets::ui::Area {
                x: 5.0,
                y: 5.0,
                width: 40.0,
                height: 40.0,
            },
            coins: Default::default(),
        });
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(100.0, 80.0)));
        let mut items = crate::preview::items();
        for item in &mut items {
            if item.slot == InventorySlot::CURSOR {
                item.stack_count = Some(5);
            }
        }
        crate::online::testing::inventory(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::inventory::InventoryUpdate::Snapshot(items),
        );
        app.update();
        let world = app.world_mut();
        let mut overlays = world.query_filtered::<(&Node, &BackgroundColor), With<Overlay>>();
        let (node, background) = overlays.single(world).unwrap();
        // The box's top left corner is at the pointer, the skin's size, with
        // no background of its own.
        assert_eq!(
            (node.left, node.top, node.width, node.height, node.display),
            (px(100), px(80), px(50), px(50), Display::Flex)
        );
        assert_eq!(background.0, Color::NONE);
        // The stack's count shows as a slot shows it; the name does not.
        let mut texts = world.query::<&Text>();
        let words: Vec<_> = texts.iter(world).map(|text| text.0.clone()).collect();
        assert!(words.iter().any(|text| text == "5"), "{words:?}");
        assert!(
            !words.iter().any(|text| text.contains("lantern")),
            "{words:?}"
        );
    }

    #[test]
    fn a_hotkey_rides_the_cursor_with_its_words_until_something_else_does() {
        let mut app = crate::testing::app();
        app.insert_resource(Carry::holding(crate::hud::hotbar::Action::Camp))
            .add_systems(Startup, |mut commands: Commands| spawn(&mut commands))
            .add_systems(Update, update);
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(100.0, 80.0)));
        let shown = |app: &mut App| {
            let world = app.world_mut();
            let mut overlays = world.query_filtered::<&Node, With<Overlay>>();
            let display = overlays.single(world).unwrap().display;
            let mut texts = world.query::<&Text>();
            let words: Vec<_> = texts.iter(world).map(|text| text.0.clone()).collect();
            (display, words)
        };
        app.update();
        assert_eq!(shown(&mut app), (Display::Flex, vec!["Camp".to_owned()]));
        // The skin's attachment shows the words where its picture goes.
        app.world_mut()
            .resource_mut::<crate::skinned::CursorLook>()
            .0 = Some(CursorPlace {
            size: Vec2::splat(50.0),
            icon: eq_client_assets::ui::Area {
                x: 5.0,
                y: 5.0,
                width: 40.0,
                height: 40.0,
            },
            coins: Default::default(),
        });
        app.update();
        assert_eq!(shown(&mut app), (Display::Flex, vec!["Camp".to_owned()]));
        // An item on the cursor takes its place.
        crate::online::testing::inventory(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::inventory::InventoryUpdate::Snapshot(crate::preview::items()),
        );
        app.update();
        assert!(!shown(&mut app).1.contains(&"Camp".to_owned()));
    }

    #[test]
    fn cursor_tracks_real_slot_when_inventory_is_closed_and_hides_after_clear() {
        let mut app = crate::testing::app();
        app.insert_resource(UiScale(2.0))
            .add_systems(Startup, |mut commands: Commands| spawn(&mut commands))
            .add_systems(Update, update);
        let window_id = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .get_mut::<Window>(window_id)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(100.0, 80.0)));
        crate::online::testing::inventory(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::inventory::InventoryUpdate::Snapshot(crate::preview::items()),
        );
        app.update();
        let world = app.world_mut();
        let mut overlays = world.query_filtered::<(&Node, &FocusPolicy), With<Overlay>>();
        let (node, policy) = overlays.single(world).unwrap();
        assert_eq!(
            (node.left, node.top, node.display),
            (px(66), px(56), Display::Flex)
        );
        assert_eq!(*policy, FocusPolicy::Pass);
        let mut texts = world.query::<&Text>();
        assert!(texts.iter(world).any(|text| text.0 == "Preview lantern"));
        let mut window = world.get_mut::<Window>(window_id).unwrap();
        let edge = window.size() - Vec2::splat(2.0);
        window.set_cursor_position(Some(edge));
        app.update();
        let world = app.world_mut();
        let node = overlays.single(world).unwrap().0;
        assert_eq!((node.left, node.top), (Val::Auto, Val::Auto));
        assert_eq!((node.right, node.bottom), (px(17), px(17)));
        // Moving back resets the opposite anchors instead of stretching the overlay.
        world
            .get_mut::<Window>(window_id)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(100.0, 80.0)));
        app.update();
        let world = app.world_mut();
        let node = overlays.single(world).unwrap().0;
        assert_eq!((node.left, node.top), (px(66), px(56)));
        assert_eq!((node.right, node.bottom), (Val::Auto, Val::Auto));
        // The world forgets the inventory, as when the player camps.
        *world.resource_mut::<crate::online::OnlineState>() =
            crate::online::OnlineState::new(false);
        world.resource_mut::<InventoryState>().forget();
        app.update();
        let world = app.world_mut();
        assert_eq!(overlays.single(world).unwrap().0.display, Display::None);
        assert_eq!(texts.iter(world).count(), 0);
    }
}
