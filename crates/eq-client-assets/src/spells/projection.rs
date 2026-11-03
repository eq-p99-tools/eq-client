//! The supported numeric fields copied by P99's spell-table replay.
use super::{Definition, Mechanics, Timing};

/// An explicit source snapshot for spell-table replay. Text and future
/// definition fields are deliberately absent. Missing optional values remain
/// missing when applied; no native address or heap pointer is represented.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayProjection {
    icon: Option<u32>,
    beneficial: Option<bool>,
    gem_icon: Option<u32>,
    gem_tint: Option<u32>,
    gem_tint_field: Option<u32>,
    mana: Option<u32>,
    cast_ms: Option<u32>,
    range: Option<f32>,
    timing: Option<Timing>,
    casting_animation: Option<u16>,
    effect: Option<u32>,
    mechanics: Option<Mechanics>,
}

impl ReplayProjection {
    /// Snapshots the current source's supported copyable fields. This does
    /// not read an installed base row or clone the source definition wholesale.
    pub fn snapshot(source: &Definition) -> Self {
        Self {
            icon: source.icon,
            beneficial: source.beneficial,
            gem_icon: source.gem_icon,
            gem_tint: source.gem_tint,
            gem_tint_field: source.gem_tint_field,
            mana: source.mana,
            cast_ms: source.cast_ms,
            range: source.range,
            timing: source.timing,
            casting_animation: source.casting_animation,
            effect: source.effect,
            mechanics: source.mechanics.clone(),
        }
    }

    /// Replaces only the supported numeric fields, preserving destination
    /// strings and every definition field not explicitly named here.
    pub fn apply(&self, destination: &mut Definition) {
        destination.icon = self.icon;
        destination.beneficial = self.beneficial;
        destination.gem_icon = self.gem_icon;
        destination.gem_tint = self.gem_tint;
        destination.gem_tint_field = self.gem_tint_field;
        destination.mana = self.mana;
        destination.cast_ms = self.cast_ms;
        destination.range = self.range;
        destination.timing = self.timing;
        destination.casting_animation = self.casting_animation;
        destination.effect = self.effect;
        destination.mechanics.clone_from(&self.mechanics);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_projection_preserves_destination_text_and_unknown_values() {
        let source = Definition {
            name: Some("Source".into()),
            fades: Some("Source fades".into()),
            mana: Some(27),
            icon: Some(11),
            beneficial: Some(true),
            ..Definition::default()
        };
        let mut destination = Definition {
            name: Some("Destination".into()),
            fades: Some("Destination fades".into()),
            mana: Some(9),
            icon: Some(2),
            beneficial: Some(false),
            cast_ms: Some(1200),
            ..Definition::default()
        };
        ReplayProjection::snapshot(&source).apply(&mut destination);
        assert_eq!(destination.name.as_deref(), Some("Destination"));
        assert_eq!(destination.fades.as_deref(), Some("Destination fades"));
        assert_eq!(destination.mana, Some(27));
        assert_eq!(destination.icon, Some(11));
        assert_eq!(destination.beneficial, Some(true));
        assert_eq!(destination.cast_ms, None);
        assert_eq!(destination.mechanics, None);
    }
}
