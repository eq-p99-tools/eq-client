//! Retained rectangle relationships. Updating a size never replaces widgets.
use super::at;
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{Anchor, Anchors},
    ui::Area,
};
use std::collections::BTreeMap;

#[derive(Component, Clone, Copy)]
pub(crate) struct Geometry(pub Anchors);

#[derive(Clone, Copy)]
pub(super) enum Axis {
    Start,
    Stretch,
    End,
}

impl Geometry {
    /// An inset client box, backdrop, or other box stretched with its parent.
    pub(super) fn inset(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self(Anchors {
            left: Anchor::Start(left),
            top: Anchor::Start(top),
            right: Anchor::End(right),
            bottom: Anchor::End(bottom),
        })
    }

    /// Keep the same authored offsets as the parent changes size.
    pub(super) fn from_area(area: Area, parent: Vec2, horizontal: Axis, vertical: Axis) -> Self {
        let edge = |point: f32, length: f32, end: bool| {
            if end {
                Anchor::End(length - point)
            } else {
                Anchor::Start(point)
            }
        };
        Self(Anchors {
            left: edge(area.x, parent.x, matches!(horizontal, Axis::End)),
            top: edge(area.y, parent.y, matches!(vertical, Axis::End)),
            right: edge(
                area.x + area.width,
                parent.x,
                !matches!(horizontal, Axis::Start),
            ),
            bottom: edge(
                area.y + area.height,
                parent.y,
                !matches!(vertical, Axis::Start),
            ),
        })
    }
}

type SizedNodes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Node,
        Option<&'static crate::windows::Frame>,
    ),
>;

/// Uses explicit logical rectangle sizes before Bevy's layout pass. Resolve
/// containers before descendants, including nested anchored views and clips.
/// Natural flex content below these boxes stays with Bevy's layout owner.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    mut nodes: ParamSet<(SizedNodes, Query<&mut Node>)>,
    rules: Query<(Entity, &Geometry, &ChildOf)>,
    mut pixel_pieces: Query<&mut super::pixel_edges::Piece>,
) {
    let mut sizes: BTreeMap<_, _> = nodes
        .p0()
        .iter()
        .filter_map(|(entity, node, frame)| {
            let (Val::Px(width), Val::Px(mut height)) = (node.width, node.height) else {
                return None;
            };
            if let Some(frame) = frame {
                height = frame.full_height(height);
            }
            Some((entity, Vec2::new(width, height)))
        })
        .collect();
    let mut pending: BTreeMap<_, _> = rules
        .iter()
        .map(|(entity, rule, parent)| (entity, (rule.0, parent.parent())))
        .collect();
    let mut resolved = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let before = pending.len();
        let ready: Vec<_> = pending
            .iter()
            .filter_map(|(entity, (rule, parent))| {
                (!pending.contains_key(parent))
                    .then(|| {
                        sizes
                            .get(parent)
                            .map(|size| (*entity, rule.within(size.x, size.y)))
                    })
                    .flatten()
            })
            .collect();
        for (entity, area) in ready {
            pending.remove(&entity);
            sizes.insert(entity, Vec2::new(area.width, area.height));
            resolved.push((entity, area));
        }
        if pending.len() == before {
            break;
        }
    }
    let mut nodes = nodes.p1();
    for (entity, area) in resolved {
        if let Ok(mut node) = nodes.get_mut(entity) {
            let wanted = at(area.x, area.y, area.width, area.height);
            let height = if node.height == Val::Auto {
                Val::Auto
            } else {
                wanted.height
            };
            if (node.left, node.top, node.width, node.height)
                != (wanted.left, wanted.top, wanted.width, height)
            {
                node.left = wanted.left;
                node.top = wanted.top;
                node.width = wanted.width;
                node.height = height;
            }
        }
        if let Ok(mut piece) = pixel_pieces.get_mut(entity) {
            let min = Vec2::new(area.x, area.y);
            piece.0 = Rect::from_corners(min, min + Vec2::new(area.width, area.height));
        }
    }
}

/// Identity for a particular effective skin and complete authored layout.
/// Stored only as a number in the user's own settings, never a global size.
pub(super) fn scope(skin: &str, screen: &eq_client_assets::sidl::Screen) -> u64 {
    format!("{skin}:{screen:?}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
        })
}

pub(super) fn eligible_scope(
    skin: &str,
    id: super::WindowId,
    screen: &eq_client_assets::sidl::Screen,
) -> Option<u64> {
    (id == super::WindowId::Chat && screen.sizable == eq_client_assets::sidl::Sizable::Enabled)
        .then(|| scope(skin, screen))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_eligibility_and_preferences_are_scoped_to_authored_chat_layout() {
        use eq_client_assets::sidl::{Library, Sizable};
        let mut screen = Library::parse("<XML/>", "<XML/>").unwrap().screen("<XML><Screen item=\"Example\"><Size><CX>400</CX><CY>250</CY></Size></Screen></XML>", "Example").unwrap();
        for sizable in [
            Sizable::Omitted,
            Sizable::Invalid,
            Sizable::Fixed,
            Sizable::Enabled,
        ] {
            screen.sizable = sizable;
            for id in [
                super::super::WindowId::Chat,
                super::super::WindowId::Inventory,
                super::super::WindowId::Spellbook,
            ] {
                assert_eq!(
                    eligible_scope("synthetic", id, &screen).is_some(),
                    id == super::super::WindowId::Chat && sizable == Sizable::Enabled
                );
            }
        }
        let first = scope("synthetic", &screen);
        assert_ne!(first, scope("other", &screen));
        screen.area.width += 1.0;
        assert_ne!(first, scope("synthetic", &screen));
    }
}
