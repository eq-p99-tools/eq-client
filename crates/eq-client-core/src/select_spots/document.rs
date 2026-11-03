//! Retain the source document and its review provenance when proposing rows.
use super::{Spot, Spots};
use std::collections::HashMap;

/// Rewrites a spots document with proposals, preserving its comments and all
/// reviewed rows exactly. Untouched rows also retain their original bytes.
///
/// Proposals replace only unreviewed rows or append missing zones, always as
/// unreviewed. Header audit identities therefore still describe the same
/// reviewed rows; generating a position never extends an audit's authority.
/// Proposal order determines the order of appended zones. Existing line endings
/// and inline comments are retained; appended rows use the document's first
/// newline style, or LF for a document without one.
///
/// # Errors
/// Returns an error for an invalid source document, duplicate proposal zones
/// (case insensitive), invalid zone tokens or non-finite proposal values.
pub fn rewrite_proposals<'a>(
    text: &str,
    proposals: impl IntoIterator<Item = (&'a str, Spot)>,
) -> Result<String, String> {
    let listed = Spots::parse(text)?;
    let mut replacements = Vec::new();
    let mut indices = HashMap::new();
    for (zone, mut spot) in proposals {
        if zone.is_empty() || zone.chars().any(|c| c.is_whitespace() || c == '#') {
            return Err("a proposal zone must be a single nonempty token without #".into());
        }
        if !spot
            .loc
            .into_iter()
            .chain([spot.heading, spot.distance, spot.pitch])
            .all(f32::is_finite)
        {
            return Err(format!("{zone}: proposal values must be finite"));
        }
        let zone = zone.to_ascii_lowercase();
        if indices.insert(zone.clone(), replacements.len()).is_some() {
            return Err(format!("{zone}: proposed twice"));
        }
        spot.reviewed = false;
        replacements.push((zone, spot));
    }
    let newline = text.find('\n').map_or("\n", |index| {
        if text[..index].ends_with('\r') {
            "\r\n"
        } else {
            "\n"
        }
    });
    let mut consumed = vec![false; replacements.len()];
    let mut output = String::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let body = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(line);
        let data = body.split('#').next().unwrap_or_default();
        let Some(zone) = data.split_whitespace().next() else {
            output.push_str(line);
            continue;
        };
        let old = listed
            .get(zone)
            .ok_or_else(|| format!("line {}: invalid spot row", index + 1))?;
        let Some(&slot) = indices.get(&zone.to_ascii_lowercase()) else {
            output.push_str(line);
            continue;
        };
        consumed[slot] = true;
        if old.reviewed {
            output.push_str(line);
            continue;
        }
        output.push_str(&replacements[slot].1.line(zone));
        output.push_str(&body[data.trim_end().len()..]);
        output.push_str(&line[body.len()..]);
    }
    for ((zone, spot), consumed) in replacements.iter().zip(consumed) {
        if consumed {
            continue;
        }
        if !output.is_empty() && !output.ends_with('\n') {
            output.push_str(newline);
        }
        output.push_str(&spot.line(zone));
        output.push_str(newline);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(y: f32) -> Spot {
        Spot {
            loc: [y, 2.5, 3.75],
            heading: 128.0,
            distance: 25.0,
            pitch: 10.0,
            // The document writer must not trust a caller to approve a row.
            reviewed: true,
        }
    }

    #[test]
    fn repeated_rewrites_preserve_audit_identity_and_exact_reviewed_rows() {
        let header = "# Review audit: synthetic-four-cardinal-v1; reviewed rows only.\r\n# Unknown future metadata: retained verbatim.\r\n\r\n";
        let reviewed =
            "  Qeynos  1.123456 2.234567 3.345678 4 25 10 reviewed  # picked by hand\r\n";
        let untouched = "other 8 7 6 5 25 10 unreviewed # no new proposal\r\n";
        let source = format!(
            "{header}{reviewed}draft 0 0 0 0 25 10 unreviewed  # placement note\r\n{untouched}# end"
        );
        let proposals = [
            ("QEYnoS", candidate(99.0)),
            ("draft", candidate(10.0)),
            ("newzone", candidate(20.0)),
        ];
        let output = rewrite_proposals(&source, proposals).unwrap();
        assert!(output.starts_with(&format!("{header}{reviewed}")));
        assert!(output.contains(untouched));
        assert!(output.contains("  # placement note\r\n"));
        assert!(output.contains("# end\r\nnewzone"));
        assert!(!output.replace("\r\n", "").contains('\n'));
        let parsed = Spots::parse(&output).unwrap();
        assert_eq!(
            parsed.get("qeynos"),
            Spots::parse(&source).unwrap().get("qeynos")
        );
        assert_eq!(parsed.iter().filter(|(_, spot)| spot.reviewed).count(), 1);
        for zone in ["draft", "newzone"] {
            assert!(!parsed.get(zone).unwrap().reviewed);
        }
        let mut again = output.clone();
        for _ in 0..3 {
            again = rewrite_proposals(&again, proposals).unwrap();
            assert_eq!(again, output);
        }
    }

    #[test]
    fn no_proposals_round_trip_comments_line_endings_and_missing_final_newlines() {
        for source in [
            "",
            "# audit with no rows",
            "# mixed endings\r\n\nzone 1 2 3 4 25 10 reviewed\r\n# tail",
            "zone 1 2 3 4 25 10 unreviewed # note",
            super::super::BUILT_IN,
        ] {
            assert_eq!(rewrite_proposals(source, []).unwrap(), source);
        }
    }

    #[test]
    fn existing_unreviewed_row_keeps_comment_and_no_final_newline() {
        let source =
            "# audit applies only to reviewed rows\nzone 1 2 3 4 25 10 unreviewed \t# retain";
        let output = rewrite_proposals(source, [("zone", candidate(12.0))]).unwrap();
        assert_eq!(
            output,
            format!(
                "# audit applies only to reviewed rows\n{} \t# retain",
                Spot {
                    reviewed: false,
                    ..candidate(12.0)
                }
                .line("zone")
            )
        );
        assert_eq!(
            rewrite_proposals(&output, [("zone", candidate(12.0))]).unwrap(),
            output
        );
    }

    #[test]
    fn invalid_proposals_cannot_inject_rows_or_ambiguous_review_identity() {
        for zone in ["", "two zones", "zone\nother", "zone#comment"] {
            assert!(rewrite_proposals("# audit", [(zone, candidate(1.0))]).is_err());
        }
        assert!(
            rewrite_proposals("", [("zone", candidate(1.0)), ("ZoNe", candidate(2.0))]).is_err()
        );
        assert!(rewrite_proposals("", [("zone", candidate(f32::NAN))]).is_err());
        assert!(
            rewrite_proposals(
                "",
                [(
                    "zone",
                    Spot {
                        pitch: f32::INFINITY,
                        ..candidate(1.0)
                    }
                )]
            )
            .is_err()
        );
        assert!(rewrite_proposals("zone 1 2 3 4 25 10 maybe", []).is_err());
        assert!(rewrite_proposals("unparsed", []).is_err());
    }
}
