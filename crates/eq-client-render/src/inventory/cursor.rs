//! Non-interactive display of the actual inventory cursor slot.
use super::{InventorySlot, InventoryState};
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

/// What the overlay was last drawn for: its entity, the window's and the
/// inventory's revisions, and the coins on the cursor.
type Drawn = (Entity, u64, u64, Option<(eq_client_core::money::Coin, u32)>);

/// Follows the pointer using confirmed/predicted inventory state, never a selected slot.
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub(crate) fn update(
    mut commands: Commands,
    state: Res<InventoryState>,
    online: Res<crate::online::OnlineState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Option<Res<UiScale>>,
    mut root: Query<(Entity, &mut Node), With<Overlay>>,
    mut stamp: Local<Option<Drawn>>,
    mut art: crate::sheets::Art,
) {
    let Ok((entity, mut node)) = root.single_mut() else {
        return;
    };
    let inventory = online.world().inventory();
    let item = inventory.items().get(&InventorySlot::CURSOR);
    let coins = crate::coins::on_cursor(online.world());
    let pointer = windows
        .single()
        .ok()
        .filter(|window| window.focused)
        .and_then(|window| {
            window
                .cursor_position()
                .map(|pointer| (pointer, window.size()))
        });
    node.display = if (item.is_some() || coins.is_some()) && pointer.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if let Some((pointer, viewport)) = pointer {
        let factor = scale.as_ref().map_or(1.0, |scale| scale.0);
        place(&mut node, pointer / factor, viewport / factor);
    }
    let current = (entity, state.revision, inventory.revision(), coins);
    if *stamp == Some(current) {
        return;
    }
    *stamp = Some(current);
    commands.entity(entity).despawn_children();
    let Some(item) = item else {
        // Coins ride the cursor on their own, as in the official client.
        if let Some((coin, count)) = coins {
            commands.entity(entity).with_child((
                Text::new(format!("{count} {}", coin_name(coin))),
                theme::font(Size::Body),
                TextColor(theme::INK_WARM),
                FocusPolicy::Pass,
            ));
        }
        return;
    };
    let icon = art.item(item.icon);
    let mut text = item.details.name.clone();
    if let Some(count) = item.stack_count {
        use std::fmt::Write;
        let _ = write!(text, " x{count}");
    }
    if inventory.stale() {
        text.push_str("\nAwaiting inventory update");
    }
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
