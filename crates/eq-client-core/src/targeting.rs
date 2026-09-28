//! Deterministic selection helpers shared by keyboard and graphical controls.

/// Cycles a stable nearby-entity order, wrapping in either direction.
pub fn cycle(ids: &[u16], current: Option<u16>, backward: bool) -> Option<u16> {
    if ids.is_empty() {
        return None;
    }
    let index = current.and_then(|id| ids.iter().position(|v| *v == id));
    Some(
        ids[match index {
            Some(i) if backward => (i + ids.len() - 1) % ids.len(),
            Some(i) => (i + 1) % ids.len(),
            None if backward => ids.len() - 1,
            None => 0,
        }],
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycling_wraps_and_recovers_after_despawn() {
        let ids = [7, 9, 3];
        assert_eq!(cycle(&ids, None, false), Some(7));
        assert_eq!(cycle(&ids, Some(7), false), Some(9));
        assert_eq!(cycle(&ids, Some(3), false), Some(7));
        assert_eq!(cycle(&ids, Some(7), true), Some(3));
        assert_eq!(cycle(&ids, Some(99), false), Some(7));
        assert_eq!(cycle(&[], Some(7), false), None);
    }
}
