//! Window borders keep each authored corner and side cap at its own size.
//! Titanium's frame rectangles (0x5c04f0) and client extents (0x5c03e0)
//! distinguish those sizes from the widths/heights of the stretched middles.
//! Source alpha margins are never trimmed to determine layout.
use super::{
    at, pixel_edges,
    reflow::{Axis, Geometry},
    to_f32,
};
use bevy::prelude::*;
use eq_client_assets::sidl::{Border, Piece};

fn size(piece: Option<&Piece>) -> Vec2 {
    piece.map_or(Vec2::ZERO, |piece| {
        Vec2::new(to_f32(piece.width), to_f32(piece.height))
    })
}

/// A missing cap has no rectangle; a present zero-size cap still has an
/// anchor. In particular, it can start a side below the matching corner.
fn cap(piece: Option<&Piece>, position: Vec2, absent: Vec2) -> Rect {
    if piece.is_some() {
        Rect::from_corners(position, position + size(piece))
    } else {
        Rect::from_corners(absent, absent)
    }
}

/// Draws the twelve pieces within the border's outer box. At sizes too
/// small for its fixed corners/caps, inverted middle spans draw nothing,
/// and the fixed artwork is clipped to that box instead of leaking out.
#[allow(clippy::too_many_lines)] // One geometry table for the twelve perimeter pieces.
pub(super) fn draw(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    look: &Border,
    (width, height): (f32, f32),
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let outer = Vec2::new(width, height);
    let tl = cap(look.top_left.as_ref(), Vec2::ZERO, Vec2::ZERO);
    let tr = cap(
        look.top_right.as_ref(),
        Vec2::new(width - size(look.top_right.as_ref()).x, 0.0),
        Vec2::new(width, 0.0),
    );
    let bl = cap(
        look.bottom_left.as_ref(),
        Vec2::new(0.0, height - size(look.bottom_left.as_ref()).y),
        Vec2::new(0.0, height),
    );
    let br = cap(
        look.bottom_right.as_ref(),
        outer - size(look.bottom_right.as_ref()),
        outer,
    );
    let lt = cap(look.left_top.as_ref(), Vec2::new(0.0, tl.max.y), Vec2::ZERO);
    let rt = cap(
        look.right_top.as_ref(),
        Vec2::new(width - size(look.right_top.as_ref()).x, tr.max.y),
        Vec2::ZERO,
    );
    let lb = cap(
        look.left_bottom.as_ref(),
        Vec2::new(0.0, bl.min.y - size(look.left_bottom.as_ref()).y),
        Vec2::new(0.0, height),
    );
    let rb = cap(
        look.right_bottom.as_ref(),
        Vec2::new(
            width - size(look.right_bottom.as_ref()).x,
            br.min.y - size(look.right_bottom.as_ref()).y,
        ),
        outer,
    );
    // Rect::new normalizes inverted bounds. Keep the authored span signed
    // until rejecting it, so a too-small border cannot flip a middle over.
    let pieces = [
        (look.top_left.as_ref(), tl.min, tl.max),
        (
            look.top.as_ref(),
            Vec2::new(tl.max.x, 0.0),
            Vec2::new(tr.min.x, size(look.top.as_ref()).y),
        ),
        (look.top_right.as_ref(), tr.min, tr.max),
        (look.right_top.as_ref(), rt.min, rt.max),
        (
            look.right.as_ref(),
            Vec2::new(width - size(look.right.as_ref()).x, rt.max.y),
            Vec2::new(width, rb.min.y),
        ),
        (look.right_bottom.as_ref(), rb.min, rb.max),
        (look.bottom_right.as_ref(), br.min, br.max),
        (
            look.bottom.as_ref(),
            Vec2::new(bl.max.x, height - size(look.bottom.as_ref()).y),
            Vec2::new(br.min.x, height),
        ),
        (look.bottom_left.as_ref(), bl.min, bl.max),
        (look.left_top.as_ref(), lt.min, lt.max),
        (
            look.left.as_ref(),
            Vec2::new(0.0, lt.max.y),
            Vec2::new(size(look.left.as_ref()).x, lb.min.y),
        ),
        (look.left_bottom.as_ref(), lb.min, lb.max),
    ];
    parent
        .spawn((
            pixel_edges::Frame,
            Geometry::inset(0.0, 0.0, 0.0, 0.0),
            Node {
                overflow: Overflow::clip(),
                ..at(0.0, 0.0, width, height)
            },
        ))
        .with_children(|parent| {
            for (index, (piece, min, max)) in pieces.into_iter().enumerate() {
                let Some(piece) = piece else {
                    continue;
                };
                let span = max - min;
                if piece.width > 0
                    && piece.height > 0
                    && let Some(image) = art.cut(piece)
                {
                    parent.spawn((
                        image,
                        at(min.x, min.y, span.x, span.y),
                        pixel_edges::Piece(Rect {
                            min,
                            max: min + span.max(Vec2::ZERO),
                        }),
                        Geometry::from_area(
                            eq_client_assets::ui::Area {
                                x: min.x,
                                y: min.y,
                                width: span.x,
                                height: span.y,
                            },
                            outer,
                            if matches!(index, 1 | 7) {
                                Axis::Stretch
                            } else if matches!(index, 2..=6) {
                                Axis::End
                            } else {
                                Axis::Start
                            },
                            if matches!(index, 4 | 10) {
                                Axis::Stretch
                            } else if matches!(index, 5..=8 | 11) {
                                Axis::End
                            } else {
                                Axis::Start
                            },
                        ),
                    ));
                }
            }
        });
}
