//! Breakpoint dots on source lines: placed on statements, kept with their
//! lines as the text around them changes. The executor pauses on them.

use std::collections::BTreeSet;

/// The lines with a dot, one-based.
pub(super) type Dots = BTreeSet<usize>;

/// Toggle the dot for a click on `line`. With a valid script, a line that
/// holds no statement (a comment, a blank, `pass`) puts the dot on the next
/// statement below it; with errors, the dot goes where it was asked.
pub(super) fn toggle(dots: &mut Dots, line: usize, statements: Option<&BTreeSet<usize>>) {
    let target = match statements {
        Some(statements) => match statements.range(line..).next() {
            Some(target) => *target,
            None => return,
        },
        None => line,
    };
    if !dots.remove(&target) {
        dots.insert(target);
    }
}

/// The statement line at or after `line`, for Run to cursor.
pub(super) fn statement_at(line: usize, statements: &BTreeSet<usize>) -> Option<usize> {
    statements.range(line..).next().copied()
}

/// Keep each dot on its line across an edit from `old` to `new`. The edit is
/// the region between the lines both texts share at the start and at the end.
/// Dots before it stay, dots after it move by the change in line count, and
/// in it the first lines keep their dots while removed ones lose theirs.
pub(super) fn follow(dots: &Dots, old: &str, new: &str) -> Dots {
    if dots.is_empty() || old == new {
        return dots.clone();
    }
    let old_lines = old.split('\n').collect::<Vec<_>>();
    let new_lines = new.split('\n').collect::<Vec<_>>();
    let shared = old_lines.len().min(new_lines.len());
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(old, new)| old == new)
        .count();
    let suffix = old_lines
        .iter()
        .rev()
        .zip(new_lines.iter().rev())
        .take(shared - prefix)
        .take_while(|(old, new)| old == new)
        .count();
    let old_middle = old_lines.len() - prefix - suffix;
    let new_middle = new_lines.len() - prefix - suffix;
    dots.iter()
        .filter_map(|&line| {
            let index = line - 1;
            if index < prefix {
                Some(line)
            } else if index < prefix + old_middle {
                (index - prefix < new_middle).then_some(line)
            } else {
                Some(line + new_middle - old_middle)
            }
        })
        .filter(|line| *line <= new_lines.len())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dots(lines: &[usize]) -> Dots {
        lines.iter().copied().collect()
    }

    #[test]
    fn dots_move_with_inserts_and_deletes_and_go_with_removed_lines() {
        let old = "a\nb\nc\nd\n";
        // A line inserted above moves the dots below it.
        assert_eq!(
            follow(&dots(&[1, 3]), old, "a\nnew\nb\nc\nd\n"),
            dots(&[1, 4])
        );
        // A deleted dotted line loses its dot; the rest close up.
        assert_eq!(follow(&dots(&[2, 4]), old, "a\nc\nd\n"), dots(&[3]));
        // Typing in a dotted line keeps its dot.
        assert_eq!(follow(&dots(&[2]), old, "a\nbb\nc\nd\n"), dots(&[2]));
        // A line added after a dotted one leaves its dot and moves the next.
        assert_eq!(
            follow(&dots(&[2, 3]), old, "a\nb\nx\nc\nd\n"),
            dots(&[2, 4])
        );
        // Splitting a dotted line keeps the dot on its first part.
        assert_eq!(
            follow(&dots(&[2, 3]), old, "a\nb1\nb2\nc\nd\n"),
            dots(&[2, 4])
        );
        // Re-indenting a line into a block keeps its dot.
        assert_eq!(
            follow(
                &dots(&[2]),
                "if True:\nwait(1)\n",
                "if True:\n    wait(1)\n"
            ),
            dots(&[2])
        );
        // Removing everything removes every dot.
        assert_eq!(follow(&dots(&[1, 2]), old, ""), dots(&[]));
    }

    #[test]
    fn a_click_off_a_statement_places_the_dot_on_the_next_one() {
        let statements = dots(&[2, 5]);
        let mut placed = Dots::new();
        toggle(&mut placed, 1, Some(&statements));
        assert_eq!(placed, dots(&[2]));
        toggle(&mut placed, 3, Some(&statements));
        assert_eq!(placed, dots(&[2, 5]));
        toggle(&mut placed, 5, Some(&statements));
        assert_eq!(placed, dots(&[2]));
        // Nothing below the last statement takes a dot.
        toggle(&mut placed, 6, Some(&statements));
        assert_eq!(placed, dots(&[2]));
        // With errors the dot goes on the line asked for.
        toggle(&mut placed, 3, None);
        assert_eq!(placed, dots(&[2, 3]));
        assert_eq!(statement_at(3, &statements), Some(5));
        assert_eq!(statement_at(6, &statements), None);
    }
}
