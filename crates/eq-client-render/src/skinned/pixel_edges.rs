//! A perimeter's shared endpoints use one physical-pixel grid.
//! Taffy rounds local positions but cumulative sizes. Accumulating those
//! local positions can move a strip while retaining its cumulative width.
//! Keep the frame's actual placement and use differences of cumulative
//! rounded endpoints for its leaf pictures, before Bevy computes clipping.
use bevy::{prelude::*, ui::ui_surface::UiSurface};
use std::collections::BTreeMap;

#[derive(Component)]
pub(super) struct Frame;

#[derive(Component)]
pub(crate) struct Piece(pub(super) Rect);

type Relations<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static ChildOf>,
        Option<&'static LayoutConfig>,
        Option<&'static UiTransform>,
    ),
>;

#[derive(Clone, Copy)]
struct Grid {
    origin: Vec2,
    cumulative: Vec2,
    scale: f32,
}

impl Grid {
    fn point(self, point: Vec2) -> Vec2 {
        self.origin + (self.cumulative + point * self.scale).round() - self.cumulative.round()
    }
}

/// Applies only to the plain, rounded layout used by installed skin frames.
/// An explicit unrounded layout or UI transform keeps Bevy's own geometry.
fn grid(
    entity: Entity,
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    relations: &Relations<'_, '_>,
    surface: &mut UiSurface,
) -> Option<Grid> {
    let origin = transform.translation - node.size() * 0.5;
    let mut path = Vec::new();
    let mut ancestor = Some(entity);
    while let Some(entity) = ancestor {
        let (parent, _, transform) = relations.get(entity).ok()?;
        if transform.is_some_and(|transform| *transform != UiTransform::IDENTITY) {
            return None;
        }
        path.push(entity);
        ancestor = parent.map(ChildOf::parent);
    }
    let mut cumulative = origin;
    let mut rounding = true;
    for entity in path.into_iter().rev() {
        let Ok((unrounded, _)) = surface.get_layout(entity, false) else {
            continue;
        };
        if let Some(config) = relations.get(entity).ok()?.1 {
            rounding = config.use_rounding;
        }
        if rounding {
            let (rounded, _) = surface.get_layout(entity, true).ok()?;
            cumulative += Vec2::new(
                unrounded.location.x - rounded.location.x,
                unrounded.location.y - rounded.location.y,
            );
        }
    }
    rounding.then_some(Grid {
        origin,
        cumulative,
        scale: node.inverse_scale_factor().recip(),
    })
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(
        PostUpdate,
        snap.after(bevy::ui::UiSystems::Layout)
            .before(bevy::ui::UiSystems::PostLayout),
    );
}

type Pictures<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Piece,
        &'static ChildOf,
        &'static mut ComputedNode,
        &'static mut UiGlobalTransform,
    ),
    Without<Frame>,
>;

pub(super) fn snap(
    frames: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<Frame>>,
    mut pictures: Pictures,
    relations: Relations,
    mut surface: ResMut<UiSurface>,
) {
    let grids: BTreeMap<_, _> = frames
        .iter()
        .filter_map(|(entity, node, transform)| {
            grid(entity, node, transform, &relations, &mut surface).map(|grid| (entity, grid))
        })
        .collect();
    for (entity, piece, parent, mut node, mut transform) in &mut pictures {
        let Ok((_, config, style)) = relations.get(entity) else {
            continue;
        };
        if config.is_some_and(|config| !config.use_rounding)
            || style.is_some_and(|style| *style != UiTransform::IDENTITY)
        {
            continue;
        }
        if let Some(grid) = grids.get(&parent.parent()) {
            let rect = Rect::from_corners(grid.point(piece.0.min), grid.point(piece.0.max));
            node.size = rect.size();
            *transform = UiGlobalTransform::from_translation(rect.center());
        }
    }
}
