//! The game generation the session speaks, for what its official client
//! does on its own that the world follows: one method per question, each
//! answered per generation.
use crate::SpawnKind;
use eq_network_game::GameDialect;

/// The game generation the session speaks: Titanium (stock `EQEmu` and
/// Project 1999) or `EqMac` (Project Quarm and TAKP). It enters the world
/// once, as the session starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Generation(GameDialect);

impl Generation {
    /// The generation of a dialect.
    #[must_use]
    pub const fn new(dialect: GameDialect) -> Self {
        Self(dialect)
    }

    /// The name a spawn that dies in view takes as a corpse, or None where it
    /// keeps its living name.
    ///
    /// Titanium: the name the server gives a corpse first seen dead, so the
    /// two read alike (inferred from `EQEmu`'s `Corpse::CalcCorpseName`; the
    /// official client's own rename is not checked): the living name
    /// without its digits, then `'s corpse` for a player's, or a backtick
    /// and `s_corpse` for a creature's, then the spawn ID, within 63
    /// characters. `EqMac`: not checked on TAKP yet, so the living name
    /// stays. How the official client shows either, with the backtick or an
    /// apostrophe and whether the two cases read the same, is recording
    /// checklist item 6.60.
    #[must_use]
    pub fn corpse_name(self, name: &str, kind: SpawnKind, id: u16) -> Option<String> {
        match self.0 {
            GameDialect::Titanium => Some(titanium_corpse_name(name, kind, id)),
            // EqMac, and a generation still to come, keeps the living name.
            _ => None,
        }
    }
}

/// `EQEmu`'s name for the corpse of a spawn of this name.
fn titanium_corpse_name(name: &str, kind: SpawnKind, id: u16) -> String {
    let suffix = if kind == SpawnKind::PlayerCorpse {
        format!("'s corpse{id}")
    } else {
        format!("`s_corpse{id}")
    };
    let base: String = name
        .chars()
        .filter(|c| !c.is_ascii_digit())
        .take(63usize.saturating_sub(suffix.len()))
        .collect();
    base + &suffix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titanium_names_a_corpse_as_the_server_does_and_eqmac_keeps_the_name() {
        let titanium = Generation::new(GameDialect::Titanium);
        assert_eq!(
            titanium
                .corpse_name("Examplar", SpawnKind::PlayerCorpse, 4)
                .as_deref(),
            Some("Examplar's corpse4")
        );
        // Digits go, wherever they are, and a long name makes room for the end.
        assert_eq!(
            titanium
                .corpse_name("a_gnoll0012", SpawnKind::NpcCorpse, 12)
                .as_deref(),
            Some("a_gnoll`s_corpse12")
        );
        let named = titanium
            .corpse_name(&"a".repeat(80), SpawnKind::NpcCorpse, 300)
            .unwrap();
        assert_eq!(named.len(), 63);
        assert!(named.ends_with("`s_corpse300"));
        assert_eq!(
            Generation::new(GameDialect::EqMac).corpse_name("a_gnoll00", SpawnKind::NpcCorpse, 12),
            None
        );
    }
}
