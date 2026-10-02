//! Every window the client draws, described in one place: its title, where it
//! opens, which layer it draws in, how the player opens it, whether Escape
//! closes it and whether its placement is kept. A new window is a new
//! [`WindowId`], and each rule below must say what it does for it.
use std::borrow::Cow;

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
    /// The inventory and, drawn by the client's own chrome, the bank.
    Inventory,
    /// The bank, drawn from the skin, while a banker is near.
    Bank,
    /// A bag's contents, drawn from the skin: the bag in this inventory
    /// slot (Titanium's numbering).
    Bag(i32),
    /// The spellbook.
    Spellbook,
    /// An inspected item.
    Item,
    /// A corpse being looted.
    Loot,
    /// A merchant's wares.
    Merchant,
    /// The give window: what the player hands an NPC, drawn from the skin.
    Give,
    /// The skin's Actions window: its Main page's sit, stand and camp, and
    /// the abilities on its Combat and Abilities pages.
    ActionsWindow,
    /// The skin's Pet Info window: the pet's health, its commands and its
    /// buffs, open while the player has a pet.
    PetInfo,
    /// The skin's Options window: the options the player sets, on its pages,
    /// and a page of the options only this client has.
    Options,
    /// The skin's Training window: the skills a guildmaster teaches, open
    /// while the player trains.
    Training,
    /// The skin's Skills window: the player's skills and their values,
    /// opened from the inventory's Skills button.
    Skills,
    /// The skin's confirmation dialog: a question to answer Yes or No, such
    /// as a resurrection's.
    Confirmation,
    /// The skin's note window: a note or scroll being read.
    Note,
    /// The skin's book window: a book being read, two pages at a time.
    Book,
    /// The skin's container window for the world container open for the
    /// player, such as a forge.
    WorldContainer,
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
    /// Whether the player opens and closes it, from the selector and its key
    /// in the key map.
    pub toggled: bool,
    /// Whether Escape closes it, the top one first.
    pub closes_on_escape: bool,
    /// Whether its placement is kept between runs.
    pub persists: bool,
    /// The names its placement was saved under before windows had ids.
    pub saved_as: &'static [&'static str],
}

/// Where a bag opens until it is moved: carried bags side by side right of
/// the inventory, below the target window; bank bags side by side right of
/// the bank, five to a row, each row a little lower.
const fn bag_placement(slot: i32) -> Placement {
    // A skin's bag window is about 100 pixels wide.
    const STEP: f32 = 98.0;
    #[allow(clippy::cast_precision_loss)] // Slot offsets are small.
    if slot >= 2000 {
        let index = (slot - 2000).rem_euclid(16);
        Placement::TopLeft(
            778.0 + (index % 5) as f32 * STEP,
            100.0 + (index / 5) as f32 * 24.0,
        )
    } else {
        Placement::TopLeft(484.0 + (slot - 22).rem_euclid(8) as f32 * STEP, 300.0)
    }
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
        closes_on_escape: true,
        persists: true,
        saved_as,
    }
}

impl WindowId {
    /// Every window, in the order the selector lists the toggled ones.
    /// Bags are left out: there is one for each slot that can hold a bag
    /// (see [`WindowId::bags`]), and none of them is toggled.
    pub(crate) const ALL: [Self; 26] = [
        Self::Status,
        Self::Target,
        Self::Player,
        Self::Spells,
        Self::Actions,
        Self::CastBar,
        Self::Effects,
        Self::Chat,
        Self::Inventory,
        Self::Bank,
        Self::Spellbook,
        Self::Item,
        Self::Loot,
        Self::Merchant,
        Self::Give,
        Self::ActionsWindow,
        Self::PetInfo,
        Self::Options,
        Self::Training,
        Self::Skills,
        Self::Confirmation,
        Self::Note,
        Self::Book,
        Self::WorldContainer,
        Self::Selector,
        Self::CharacterSelect,
    ];

    /// The windows of the bags in the slots that hold one: carried, then
    /// in the bank.
    pub(crate) fn bags() -> impl Iterator<Item = Self> {
        (22..=29).chain(2000..=2015).map(Self::Bag)
    }

    /// The official client's name for the window's screen in the skin's
    /// files; also its section in a character's UI file, except a bag's
    /// (see [`WindowId::section`]).
    pub(crate) const fn official(self) -> Option<&'static str> {
        match self {
            Self::Target => Some("TargetWindow"),
            Self::Player => Some("PlayerWindow"),
            Self::Spells => Some("CastSpellWnd"),
            Self::Actions => Some("HotButtonWnd"),
            Self::CastBar => Some("CastingWindow"),
            Self::Effects => Some("BuffWindow"),
            Self::Chat => Some("ChatWindow"),
            Self::Inventory => Some("InventoryWindow"),
            Self::Bank => Some("BankWnd"),
            Self::Bag(_) | Self::WorldContainer => Some("ContainerWindow"),
            Self::Spellbook => Some("SpellBookWnd"),
            Self::Item => Some("ItemDisplayWindow"),
            Self::Loot => Some("LootWnd"),
            Self::Merchant => Some("MerchantWnd"),
            Self::Give => Some("GiveWnd"),
            Self::ActionsWindow => Some("ActionsWindow"),
            Self::PetInfo => Some("PetInfoWindow"),
            Self::Options => Some("OptionsWindow"),
            Self::Training => Some("TrainWindow"),
            Self::Skills => Some("SkillsWindow"),
            Self::Confirmation => Some("ConfirmationDialogBox"),
            Self::Note => Some("NoteWindow"),
            Self::Book => Some("BookWindow"),
            _ => None,
        }
    }

    /// Its section in a character's UI file: each bag keeps its own place,
    /// `BagInv1` to `BagInv8` carried and `BagBank1` on in the bank.
    pub(crate) fn section(self) -> Option<Cow<'static, str>> {
        match self {
            Self::Bag(slot @ 22..=29) => Some(Cow::Owned(format!("BagInv{}", slot - 21))),
            Self::Bag(slot @ 2000..=2015) => Some(Cow::Owned(format!("BagBank{}", slot - 1999))),
            // The bags' window; a world container keeps a place of its own.
            Self::Bag(_) | Self::WorldContainer => None,
            _ => self.official().map(Cow::Borrowed),
        }
    }

    /// The name its placement is saved under; each bag's names its slot.
    pub(crate) fn key(self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            Self::Status => "status",
            Self::Target => "target",
            Self::Player => "player",
            Self::Spells => "spells",
            Self::Actions => "actions",
            Self::CastBar => "cast-bar",
            Self::Effects => "effects",
            Self::Chat => "chat",
            Self::Inventory => "inventory",
            Self::Bank => "bank",
            Self::Bag(slot) => return Cow::Owned(format!("bag-{slot}")),
            Self::Spellbook => "spellbook",
            Self::Item => "item",
            Self::Loot => "loot",
            Self::Merchant => "merchant",
            Self::Give => "give",
            Self::ActionsWindow => "actions-window",
            Self::PetInfo => "pet-info",
            Self::Options => "options",
            Self::Training => "training",
            Self::Skills => "skills",
            Self::Confirmation => "confirmation",
            Self::Note => "note",
            Self::Book => "book",
            Self::WorldContainer => "world-container",
            Self::Selector => "selector",
            Self::CharacterSelect => "character-select",
        })
    }

    /// The window whose placement is saved under this name, now or before
    /// windows had ids.
    pub(crate) fn saved_under(name: &str) -> Option<Self> {
        if name.starts_with("bag-") {
            return Self::bags().find(|bag| bag.key() == name);
        }
        Self::ALL
            .into_iter()
            .find(|id| id.key() == name || id.describe().saved_as.contains(&name))
    }

    /// Where the window opens and how it behaves. The default placements are
    /// laid out so that no two windows that can be open together cover each
    /// other on a 1280 by 720 screen.
    pub(crate) const fn describe(self) -> Description {
        match self {
            // Clear of the skin's spell gems along the left edge.
            Self::Status => hud("", Placement::TopLeft(56.0, 16.0), &["POSITION"]),
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
            // Named apart from the skin's Actions window; placements saved
            // under its old title still find it.
            Self::Actions => hud("HOTBAR", Placement::Docked, &["ACTIONS"]),
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
                ..floating(
                    "INVENTORY",
                    // Clear of the skin's spell gems along the left edge.
                    Placement::TopLeft(56.0, 100.0),
                    true,
                    &["INVENTORY"],
                )
            },
            // Right of where the skin puts the player and target windows, so
            // the banker stays in view; a banker's window opens and closes
            // with the banker's reach.
            Self::Bank => floating("BANK", Placement::TopLeft(666.0, 100.0), false, &[]),
            // Each bag keeps its own place, as in the official client.
            Self::Bag(slot) => floating("", bag_placement(slot), false, &[]),
            Self::Spellbook => Description {
                ..floating(
                    "SPELLBOOK",
                    Placement::TopRight(16.0, 56.0),
                    true,
                    &["SPELLBOOK [B]"],
                )
            },
            // Loot and merchant open right of where the skin puts the player
            // and target windows; a merchant open at the same time sits a
            // little lower, so both titles show.
            Self::Loot => floating("LOOT", Placement::TopLeft(676.0, 206.0), false, &["LOOT"]),
            Self::Merchant => floating(
                "MERCHANT",
                Placement::TopLeft(688.0, 236.0),
                false,
                &["MERCHANT"],
            ),
            // Right of the effects window, clear of the inventory and its
            // bags, where the items to give come from.
            Self::Give => floating("GIVE", Placement::TopRight(220.0, 100.0), false, &[]),
            // Where the skin places it, right of the player and target windows.
            Self::ActionsWindow => floating("ACTIONS", Placement::TopLeft(516.0, 292.0), true, &[]),
            // Where the skin places it, right of the hotbar; it opens with a
            // pet and closes when the pet is gone.
            Self::PetInfo => floating("PET", Placement::TopLeft(56.0, 160.0), false, &[]),
            // Where the skin places it; Alt+O opens and closes it, as in the
            // official client.
            Self::Options => floating("OPTIONS", Placement::TopLeft(90.0, 47.0), true, &[]),
            // Where the skin places it; it opens with a guildmaster's answer
            // and closes when training ends.
            Self::Training => floating("TRAINING", Placement::TopLeft(120.0, 20.0), false, &[]),
            // Where the skin places it, over the chat; the inventory's
            // Skills button opens it too.
            Self::Skills => floating("SKILLS", Placement::TopLeft(120.0, 445.0), true, &[]),
            // Where the skin places it, above the other windows until it is
            // answered; Escape leaves the question open.
            Self::Confirmation => Description {
                layer: Layer::Popup,
                closes_on_escape: false,
                persists: false,
                ..floating("", Placement::TopLeft(550.0, 350.0), false, &[])
            },
            // Where the skin places them; they open with a text to read.
            Self::Note => floating("", Placement::TopLeft(100.0, 80.0), false, &[]),
            Self::Book => floating("", Placement::TopLeft(120.0, 20.0), false, &[]),
            // Where the skin places its container window (EQUI_Container.xml).
            Self::WorldContainer => floating("", Placement::TopLeft(350.0, 100.0), false, &[]),
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
        let windows: Vec<_> = WindowId::ALL.into_iter().chain(WindowId::bags()).collect();
        let mut keys: Vec<_> = windows.iter().map(|id| id.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), windows.len());
        for id in windows {
            assert_eq!(WindowId::saved_under(&id.key()), Some(id));
            for old in id.describe().saved_as {
                assert_eq!(WindowId::saved_under(old), Some(id), "{old}");
            }
        }
        assert_eq!(
            WindowId::saved_under("SPELLBOOK [B]"),
            Some(WindowId::Spellbook)
        );
        assert_eq!(WindowId::saved_under("unknown"), None);
        assert_eq!(WindowId::saved_under("bag-"), None);
        assert_eq!(WindowId::saved_under("bag-31"), None);
    }

    #[test]
    fn each_bag_has_the_official_clients_section() {
        let section = |slot| WindowId::Bag(slot).section().map(Cow::into_owned);
        assert_eq!(section(22).as_deref(), Some("BagInv1"));
        assert_eq!(section(29).as_deref(), Some("BagInv8"));
        assert_eq!(section(2000).as_deref(), Some("BagBank1"));
        assert_eq!(section(2500), None);
        assert_eq!(
            WindowId::Inventory.section().as_deref(),
            Some("InventoryWindow")
        );
    }
}
