use super::*;
use eq_client_core::world::SpellImpactLine;

fn messages() -> Messages {
    Messages::parse("EQST0002\n0 0\n12203 %1 amount=%2 plural\n12204 %1 amount=%2 singular\n")
}

#[test]
fn spell_caster_and_bystander_get_the_target_description_without_an_amount() {
    let line = SpellImpactLine::Other {
        target: "a_rat000".into(),
        text: " is synthetically struck.".into(),
    };
    assert_eq!(
        spell_impact_text(&line, None).unwrap().words.text,
        "A rat is synthetically struck."
    );
}

#[test]
fn spell_target_uses_installed_singular_plural_or_plain_description() {
    let messages = messages();
    for (amount, expected) in [
        (None, "Synthetic target text"),
        (Some(1), "Synthetic target text amount=1 singular"),
        (Some(2), "Synthetic target text amount=2 plural"),
    ] {
        let line = SpellImpactLine::Own {
            text: "synthetic target text".into(),
            amount,
        };
        assert_eq!(
            spell_impact_text(&line, Some(&messages))
                .unwrap()
                .words
                .text,
            expected
        );
    }
}

#[test]
fn missing_and_empty_impact_text_do_not_invent_a_generic_damage_line() {
    for line in [
        SpellImpactLine::Other {
            target: "a rat".into(),
            text: String::new(),
        },
        SpellImpactLine::Own {
            text: String::new(),
            amount: None,
        },
    ] {
        assert!(spell_impact_text(&line, Some(&messages())).is_none());
    }
    let amount_only = SpellImpactLine::Own {
        text: String::new(),
        amount: Some(2),
    };
    assert!(spell_impact_text(&amount_only, None).is_none());
    assert!(spell_impact_text(&amount_only, Some(&Messages::default())).is_none());
    // Known empty target text still uses the native amount wrapper.
    assert_eq!(
        spell_impact_text(&amount_only, Some(&messages()))
            .unwrap()
            .words
            .text,
        " amount=2 plural"
    );
}
