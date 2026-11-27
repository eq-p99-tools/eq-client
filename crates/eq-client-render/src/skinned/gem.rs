//! A spell gem's skin pieces, drawn as the installed client's program draws
//! them where that is read ([`eq_client_assets::ui::InstalledClient::gem_draw`]).
use crate::{hud::SpellGem, online::OnlineState, spellbook::SpellNames};
use bevy::prelude::*;
use eq_client_assets::ui::{GemDraw, InstalledClient};
use std::time::Instant;

/// Presentation only: no clock, cast permission or custody is retained here.
/// Titanium advances one of eleven palette entries per UI poll, rather than
/// wiping an image in proportion to the remaining recast time.
#[derive(Component, Default)]
pub(crate) struct Refresh {
    identity: Option<(Option<u64>, u32)>,
    fade: Fade,
}

#[derive(Default)]
struct Fade {
    frame: u8,
    mode: Mode,
}

#[derive(Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Idle,
    Darken,
    Restore,
}

impl Fade {
    fn poll(&mut self, blocked: bool) {
        if blocked {
            self.mode = Mode::Darken;
        } else if self.mode == Mode::Darken {
            // Native restoration starts at the darkest palette entry, even
            // when readiness changed during the darkening transition.
            self.mode = Mode::Restore;
            self.frame = 10;
        }
        match self.mode {
            Mode::Darken => self.frame = (self.frame + 1).min(10),
            Mode::Restore if self.frame > 0 => self.frame -= 1,
            Mode::Restore => self.mode = Mode::Idle,
            Mode::Idle => {}
        }
    }
}

impl Refresh {
    fn poll(&mut self, identity: Option<(Option<u64>, u32)>, blocked: bool) {
        if self.identity != identity {
            self.identity = identity;
            self.fade = Fade::default();
        }
        if identity.is_some() {
            self.fade.poll(blocked);
        }
    }

    /// Fallback chrome has no authored gem background; use its ordinary
    /// ready colour with the same palette attenuation as the native pieces.
    pub(crate) fn background(&self, colour: Color) -> Color {
        if self.fade.frame == 0 {
            return colour;
        }
        let [red, green, blue, alpha] = colour.to_srgba().to_u8_array();
        argb(dim(
            u32::from_be_bytes([alpha, red, green, blue]),
            self.fade.frame,
        ))
    }
}

fn blocked(
    world: &eq_client_core::world::ClientWorld,
    names: &SpellNames,
    gem: u8,
    spell: u32,
    now: Instant,
) -> bool {
    let casting = world.casting();
    casting.cast.is_some()
        || casting.pending.is_some()
        || world.book_request_pending()
        || casting
            .gem_wait(gem, spell, names.linked_timer(spell), now)
            .is_none_or(|wait| !wait.is_zero())
}

/// Both normal and skinned gems consume the existing painted readiness;
/// click admission keeps its separate, stricter equality boundary.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(crate) fn refresh(
    settings: Option<Res<crate::ViewerSettings>>,
    online: Res<OnlineState>,
    names: Res<SpellNames>,
    mut gems: Query<(&SpellGem, &mut Refresh)>,
) {
    let client = settings.map_or_else(InstalledClient::default, |s| s.0.installed_client);
    refresh_at(client, online.world(), &names, &mut gems, Instant::now());
}

fn refresh_at(
    client: InstalledClient,
    world: &eq_client_core::world::ClientWorld,
    names: &SpellNames,
    gems: &mut Query<(&SpellGem, &mut Refresh)>,
    now: Instant,
) {
    for (SpellGem(gem), mut refresh) in gems {
        let spell = client
            .fades_blocked_gems()
            .then(|| world.gem(usize::from(*gem)))
            .flatten();
        let blocked = spell.is_some_and(|spell| blocked(world, names, *gem, spell, now));
        refresh.poll(spell.map(|spell| (world.session_id(), spell)), blocked);
        if world.casting().p99_gem_start() && !blocked {
            // Eligible means visibly eligible now. Stock Titanium's restore
            // tail must not suggest that an admitted P99 press should wait.
            refresh.fade = Fade::default();
        }
    }
}

/// Which of the skin's gem pieces a gem's child is.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GemPiece {
    Holder,
    Background,
    Icon,
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
    frame: u8,
) -> Option<Color> {
    let Some(draw) = draw else {
        // Unread: every piece the skin gives but the highlight, untinted,
        // as before.
        return (piece != GemPiece::Highlight).then_some(Color::WHITE);
    };
    match (piece, holds) {
        (GemPiece::Icon, Holds::Spell(_)) => Some(Color::srgba_u8(255, 255, 255, 255 - 21 * frame)),
        (GemPiece::Holder, Holds::Nothing) | (GemPiece::Highlight, Holds::Spell(_)) => {
            Some(Color::WHITE)
        }
        (GemPiece::Icon | GemPiece::Background | GemPiece::Highlight, Holds::Nothing)
        | (GemPiece::Holder, Holds::Spell(_)) => None,
        (GemPiece::Background, Holds::Spell(tint)) => Some(match (interaction, tint) {
            (Interaction::Pressed, _) | (_, None) => Color::WHITE,
            (Interaction::Hovered, Some(tint)) => argb(dim(tint, frame).wrapping_add(draw.hover)),
            (Interaction::None, Some(tint)) => argb(dim(tint, frame)),
        }),
    }
}

fn dim(colour: u32, frame: u8) -> u32 {
    let [alpha, red, green, blue] = colour.to_be_bytes();
    let dim = |channel: u8| channel - (channel / 11) * frame;
    u32::from_be_bytes([alpha, dim(red), dim(green), dim(blue)])
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
    gems: Query<(&SpellGem, &Refresh, &Interaction, &Children)>,
    mut pieces: Query<(&GemPiece, &mut Node, &mut ImageNode)>,
) {
    let draw = settings.0.installed_client.gem_draw();
    for (SpellGem(index), refresh, interaction, children) in &gems {
        let holds = online
            .world()
            .gem(usize::from(*index))
            .map_or(Holds::Nothing, |spell| Holds::Spell(names.gem_tint(spell)));
        for child in children {
            let Ok((piece, mut node, mut image)) = pieces.get_mut(*child) else {
                continue;
            };
            let colour = drawn(draw, *piece, holds, *interaction, refresh.fade.frame);
            // Artwork alone knows whether an icon's source actually resolved.
            // Never reveal a missing or stale icon while applying its alpha.
            if *piece == GemPiece::Icon {
                image.color = colour.unwrap_or(Color::WHITE);
                continue;
            }
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
#[path = "gem_refresh_tests.rs"]
mod refresh_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const TITANIUM: Option<GemDraw> = Some(GemDraw { hover: 0x0020_2020 });

    #[test]
    fn an_empty_gem_draws_its_holder_alone() {
        let empty = |piece| drawn(TITANIUM, piece, Holds::Nothing, Interaction::None, 0);
        assert_eq!(empty(GemPiece::Holder), Some(Color::WHITE));
        assert_eq!(empty(GemPiece::Background), None);
        assert_eq!(empty(GemPiece::Highlight), None);
    }

    #[test]
    fn a_gem_with_a_spell_tints_its_background_and_draws_its_highlight() {
        let red = Holds::Spell(Some(0xffde_2121));
        let with = |piece, interaction| drawn(TITANIUM, piece, red, interaction, 0);
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
                Interaction::None,
                0
            ),
            Some(Color::WHITE)
        );
    }

    #[test]
    fn a_client_whose_gem_drawing_is_unread_keeps_the_skins_pieces() {
        for holds in [Holds::Nothing, Holds::Spell(Some(0xffde_2121))] {
            let unread = |piece| drawn(None, piece, holds, Interaction::Hovered, 0);
            assert_eq!(unread(GemPiece::Holder), Some(Color::WHITE));
            assert_eq!(unread(GemPiece::Background), Some(Color::WHITE));
            assert_eq!(unread(GemPiece::Highlight), None);
        }
    }
}
