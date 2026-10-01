//! Every window the client draws, described in one place: its title, where it
//! opens, which layer it draws in, how the player opens it, whether Escape
//! closes it and whether its placement is kept. A new window is a new
//! [`WindowId`], and each rule below must say what it does for it.
use bevy::prelude::*;

/// A window the client draws. The id keys its saved placement, its place in
/// the stack of floating windows, its toggle and its Escape rule.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum WindowId {
    /// The zone, the status line and the interaction prompt.
    Status,
    /// The target's name and health.
    Target,
    /// The player's name and bars.
    Player,
    /// The memorized spell gems.
    Spells,
    /// The bound action slots.
    Actions,
    /// The bar that shows a cast or a request under way.
    CastBar,
    /// The player's lasting effects.
    Effects,
    /// Chat.
    Chat,
    /// The inventory and the bank.
    Inventory,
    /// The spellbook.
    Spellbook,
    /// An inspected item.
    Item,
    /// A corpse being looted.
    Loot,
    /// A merchant's wares.
    Merchant,
    /// The buttons that open the other windows.
    Selector,
    /// The character list.
    CharacterSelect,
}

/// Where a window opens before the player moves it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Placement {
    /// Laid out in its row of the bottom HUD.
    Docked,
    /// This far from the left and top edges, in logical pixels.
    TopLeft(f32, f32),
    /// This far from the right and top edges.
    TopRight(f32, f32),
    /// This far from the left and bottom edges.
    BottomLeft(f32, f32),
    /// Centred across the screen, this far from the top; the window's width
    /// centres it.
    TopCentre { top: f32, width: f32 },
    /// Centred across the screen, this far from the bottom.
    BottomCentre { bottom: f32, width: f32 },
    /// Over the whole screen.
    Fill,
}

impl Placement {
    /// Writes the placement's edges into a node.
    pub(crate) fn apply(self, node: &mut Node) {
        let absolute = |node: &mut Node| {
            node.position_type = PositionType::Absolute;
            node.left = Val::Auto;
            node.top = Val::Auto;
            node.right = Val::Auto;
            node.bottom = Val::Auto;
            node.margin = UiRect::ZERO;
        };
        match self {
            Self::Docked => (),
            Self::TopLeft(left, top) => {
                absolute(node);
                node.left = px(left);
                node.top = px(top);
            }
            Self::TopRight(right, top) => {
                absolute(node);
                node.right = px(right);
                node.top = px(top);
            }
            Self::BottomLeft(left, bottom) => {
                absolute(node);
                node.left = px(left);
                node.bottom = px(bottom);
            }
            Self::TopCentre { top, width } => {
                absolute(node);
                node.left = percent(50);
                node.top = px(top);
                node.margin = UiRect::left(px(-width / 2.0));
            }
            Self::BottomCentre { bottom, width } => {
                absolute(node);
                node.left = percent(50);
                node.bottom = px(bottom);
                node.margin = UiRect::left(px(-width / 2.0));
            }
            Self::Fill => {
                absolute(node);
                node.left = px(0);
                node.top = px(0);
                node.width = percent(100);
                node.height = percent(100);
            }
        }
    }
}

/// The drawing layer a window belongs to; the stack orders the floating
/// windows within theirs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Layer {
    /// The always-there HUD: status, target, the bottom row.
    Hud,
    /// Windows the player opens and moves; the clicked one comes to the front.
    Floating,
    /// Windows that sit above the rest while open, such as an inspected item.
    Popup,
    /// Screens that replace the game, such as the character list.
    Screen,
}

impl Layer {
    /// The first global z-index of the layer.
    pub(crate) const fn base(self) -> i32 {
        match self {
            Self::Hud => 10,
            Self::Floating => 20,
            Self::Popup => 60,
            Self::Screen => 500,
        }
    }
}

/// What the client knows about a window before it is drawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Description {
    /// The title bar's text; empty for a window without one.
    pub title: &'static str,
    pub placement: Placement,
    pub layer: Layer,
    /// Whether the player opens and closes it, from the selector and its key.
    pub toggled: bool,
    /// The key that opens and closes it.
    pub key: Option<KeyCode>,
    /// Whether Escape closes it, the top one first.
    pub closes_on_escape: bool,
    /// Whether its placement is kept between runs.
    pub persists: bool,
    /// The names its placement was saved under before windows had ids.
    pub saved_as: &'static [&'static str],
}

/// A window of the always-there HUD.
const fn hud(
    title: &'static str,
    placement: Placement,
    saved_as: &'static [&'static str],
) -> Description {
    Description {
        title,
        placement,
        layer: Layer::Hud,
        toggled: false,
        key: None,
        closes_on_escape: false,
        persists: true,
        saved_as,
    }
}
/// A window the player opens, moves and closes.
const fn floating(
    title: &'static str,
    placement: Placement,
    toggled: bool,
    saved_as: &'static [&'static str],
) -> Description {
    Description {
        title,
        placement,
        layer: Layer::Floating,
        toggled,
        key: None,
        closes_on_escape: true,
        persists: true,
        saved_as,
    }
}

impl WindowId {
    /// Every window, in the order the selector lists the toggled ones.
    pub(crate) const ALL: [Self; 15] = [
        Self::Status,
        Self::Target,
        Self::Player,
        Self::Spells,
        Self::Actions,
        Self::CastBar,
        Self::Effects,
        Self::Chat,
        Self::Inventory,
        Self::Spellbook,
        Self::Item,
        Self::Loot,
        Self::Merchant,
        Self::Selector,
        Self::CharacterSelect,
    ];

    /// The name its placement is saved under.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Target => "target",
            Self::Player => "player",
            Self::Spells => "spells",
            Self::Actions => "actions",
            Self::CastBar => "cast-bar",
            Self::Effects => "effects",
            Self::Chat => "chat",
            Self::Inventory => "inventory",
            Self::Spellbook => "spellbook",
            Self::Item => "item",
            Self::Loot => "loot",
            Self::Merchant => "merchant",
            Self::Selector => "selector",
            Self::CharacterSelect => "character-select",
        }
    }

    /// The window whose placement is saved under this name, now or before
    /// windows had ids.
    pub(crate) fn saved_under(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|id| id.key() == name || id.describe().saved_as.contains(&name))
    }

    /// Where the window opens and how it behaves. The default placements are
    /// laid out so that no two windows that can be open together cover each
    /// other on a 1280 by 720 screen.
    pub(crate) const fn describe(self) -> Description {
        match self {
            Self::Status => hud("", Placement::TopLeft(16.0, 16.0), &["POSITION"]),
            Self::Target => hud(
                "",
                Placement::TopCentre {
                    top: 16.0,
                    width: 280.0,
                },
                &["TARGET"],
            ),
            Self::Player => hud("", Placement::Docked, &["CHARACTER"]),
            Self::Spells => hud("SPELLS", Placement::Docked, &["SPELLS"]),
            Self::Actions => hud("ACTIONS", Placement::Docked, &["ACTIONS"]),
            Self::CastBar => Description {
                persists: false,
                ..hud(
                    "",
                    Placement::BottomCentre {
                        bottom: 232.0,
                        width: 300.0,
                    },
                    &[],
                )
            },
            Self::Chat => hud("CHAT", Placement::BottomLeft(20.0, 16.0), &["CHAT"]),
            Self::Selector => hud("", Placement::TopRight(16.0, 16.0), &[]),
            // The official client leaves the effects window up; Escape does
            // not close it.
            Self::Effects => Description {
                closes_on_escape: false,
                ..floating(
                    "EFFECTS",
                    Placement::TopRight(340.0, 96.0),
                    true,
                    &["BUFFS"],
                )
            },
            Self::Inventory => Description {
                key: Some(KeyCode::KeyI),
                ..floating(
                    "INVENTORY",
                    Placement::TopLeft(16.0, 100.0),
                    true,
                    &["INVENTORY"],
                )
            },
            Self::Spellbook => Description {
                key: Some(KeyCode::KeyB),
                ..floating(
                    "SPELLBOOK",
                    Placement::TopRight(16.0, 56.0),
                    true,
                    &["SPELLBOOK [B]"],
                )
            },
            // Between the inventory and the spellbook, below the effects; a
            // merchant open at the same time sits a little lower, so both
            // titles show.
            Self::Loot => floating("LOOT", Placement::TopLeft(632.0, 206.0), false, &["LOOT"]),
            Self::Merchant => floating(
                "MERCHANT",
                Placement::TopLeft(644.0, 236.0),
                false,
                &["MERCHANT"],
            ),
            Self::Item => Description {
                layer: Layer::Popup,
                ..floating(
                    "ITEM",
                    Placement::TopCentre {
                        top: 100.0,
                        width: 310.0,
                    },
                    false,
                    &["ITEM"],
                )
            },
            Self::CharacterSelect => Description {
                layer: Layer::Screen,
                closes_on_escape: false,
                persists: false,
                ..floating("", Placement::Fill, false, &[])
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_window_has_one_key_and_old_names_find_it() {
        let mut keys: Vec<_> = WindowId::ALL.iter().map(|id| id.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), WindowId::ALL.len());
        for id in WindowId::ALL {
            assert_eq!(WindowId::saved_under(id.key()), Some(id));
            for old in id.describe().saved_as {
                assert_eq!(WindowId::saved_under(old), Some(id), "{old}");
            }
        }
        assert_eq!(
            WindowId::saved_under("SPELLBOOK [B]"),
            Some(WindowId::Spellbook)
        );
        assert_eq!(WindowId::saved_under("unknown"), None);
    }
}
