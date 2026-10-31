//! A spell gem's skin pieces, drawn as the installed client's program draws
//! them where that is read ([`eq_client_assets::ui::InstalledClient::gem_draw`]).
use crate::{hud::SpellGem, online::OnlineState, spellbook::SpellNames};
use bevy::prelude::*;
use eq_client_assets::ui::GemDraw;

/// Which of the skin's gem pieces a gem's child is.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GemPiece {
    Holder,
    Background,
    Highlight,
}

/// What a gem holds, as its pieces see it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Holds {
    Nothing,
    /// A spell, with its gem tint, if known.
    Spell(Option<u32>),
}

/// The colour a piece is drawn in, or None where it is not drawn.
fn drawn(
    draw: Option<GemDraw>,
    piece: GemPiece,
    holds: Holds,
    interaction: Interaction,
) -> Option<Color> {
    let Some(draw) = draw else {
        // Unread: every piece the skin gives but the highlight, untinted,
        // as before.
        return (piece != GemPiece::Highlight).then_some(Color::WHITE);
    };
    match (piece, holds) {
        (GemPiece::Holder, Holds::Nothing) | (GemPiece::Highlight, Holds::Spell(_)) => {
            Some(Color::WHITE)
        }
        (GemPiece::Background | GemPiece::Highlight, Holds::Nothing)
        | (GemPiece::Holder, Holds::Spell(_)) => None,
        (GemPiece::Background, Holds::Spell(tint)) => Some(match (interaction, tint) {
            (Interaction::Pressed, _) | (_, None) => Color::WHITE,
            (Interaction::Hovered, Some(tint)) => argb(tint.wrapping_add(draw.hover)),
            (Interaction::None, Some(tint)) => argb(tint),
        }),
    }
}

fn argb(colour: u32) -> Color {
    let [alpha, red, green, blue] = colour.to_be_bytes();
    Color::srgba_u8(red, green, blue, alpha)
}

/// Shows, hides and tints each gem's skin pieces for what the gem holds.
pub(crate) fn pieces(
    (settings, online, names): (
        Res<crate::ViewerSettings>,
        Res<OnlineState>,
        Res<SpellNames>,
    ),
    gems: Query<(&SpellGem, &Interaction, &Children)>,
    mut pieces: Query<(&GemPiece, &mut Node, &mut ImageNode)>,
) {
    let draw = settings.0.installed_client.gem_draw();
    for (SpellGem(index), interaction, children) in &gems {
        let holds = online
            .world()
            .gem(usize::from(*index))
            .map_or(Holds::Nothing, |spell| Holds::Spell(names.gem_tint(spell)));
        for child in children {
            let Ok((piece, mut node, mut image)) = pieces.get_mut(*child) else {
                continue;
            };
            let colour = drawn(draw, *piece, holds, *interaction);
            let display = if colour.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
            if let Some(colour) = colour.filter(|colour| image.color != *colour) {
                image.color = colour;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TITANIUM: Option<GemDraw> = Some(GemDraw { hover: 0x0020_2020 });

    #[test]
    fn an_empty_gem_draws_its_holder_alone() {
        let empty = |piece| drawn(TITANIUM, piece, Holds::Nothing, Interaction::None);
        assert_eq!(empty(GemPiece::Holder), Some(Color::WHITE));
        assert_eq!(empty(GemPiece::Background), None);
        assert_eq!(empty(GemPiece::Highlight), None);
    }

    #[test]
    fn a_gem_with_a_spell_tints_its_background_and_draws_its_highlight() {
        let red = Holds::Spell(Some(0xffde_2121));
        let with = |piece, interaction| drawn(TITANIUM, piece, red, interaction);
        assert_eq!(with(GemPiece::Holder, Interaction::None), None);
        assert_eq!(
            with(GemPiece::Highlight, Interaction::None),
            Some(Color::WHITE)
        );
        assert_eq!(
            with(GemPiece::Background, Interaction::None),
            Some(Color::srgba_u8(0xde, 0x21, 0x21, 0xff))
        );
        assert_eq!(
            with(GemPiece::Background, Interaction::Hovered),
            Some(Color::srgba_u8(0xfe, 0x41, 0x41, 0xff))
        );
        assert_eq!(
            with(GemPiece::Background, Interaction::Pressed),
            Some(Color::WHITE)
        );
        assert_eq!(
            drawn(
                TITANIUM,
                GemPiece::Background,
                Holds::Spell(None),
                Interaction::None
            ),
            Some(Color::WHITE)
        );
    }

    #[test]
    fn a_client_whose_gem_drawing_is_unread_keeps_the_skins_pieces() {
        for holds in [Holds::Nothing, Holds::Spell(Some(0xffde_2121))] {
            let unread = |piece| drawn(None, piece, holds, Interaction::Hovered);
            assert_eq!(unread(GemPiece::Holder), Some(Color::WHITE));
            assert_eq!(unread(GemPiece::Background), Some(Color::WHITE));
            assert_eq!(unread(GemPiece::Highlight), None);
        }
    }
}
