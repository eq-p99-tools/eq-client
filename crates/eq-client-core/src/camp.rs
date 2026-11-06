//! Where a camp ends: the character list, or, with `/camp desktop`, out of
//! the program. The official client camps the same way either way and,
//! camped to the desktop, closes instead of showing the character list
//! (the project owner's report of the official client, 11-01). Which other
//! words its `/camp` takes, and what it does with a word it doesn't know,
//! are unread (checklist 6.323); until then any other word is refused
//! (inferred, 6.323).

/// Where the player's camp ends.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CampTo {
    /// Back to the character list, as a plain `/camp` and the Actions
    /// window's Camp do.
    #[default]
    CharacterSelect,
    /// Out of the program, as `/camp desktop` does.
    Desktop,
}

/// The evidence that ended the local camp lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CampOutcome {
    /// The zone's logout reply was received.
    LogoutReply,
    /// The client's documented close deadline elapsed without that reply.
    /// This does not confirm server logout or cleanup.
    LocalDeadline,
}

/// The last completed camp, scoped to its departing admission when known.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    /// The admission that camped; absent when the host never knew one.
    pub admission_id: Option<u64>,
    /// The requested local destination.
    pub destination: CampTo,
    /// Received reply or local deadline, without inferring server cleanup.
    pub outcome: CampOutcome,
}

/// Where the words after `/camp` say the camp ends: none, the character
/// list; `desktop`, in any case, out of the program.
///
/// # Errors
///
/// Any other words, in the client's own words (inferred, 6.323).
pub fn destination(words: &str) -> Result<CampTo, String> {
    let words = words.trim();
    if words.is_empty() {
        Ok(CampTo::CharacterSelect)
    } else if words.eq_ignore_ascii_case("desktop") {
        Ok(CampTo::Desktop)
    } else {
        Err(format!(
            "/camp takes no words but desktop, not \"{words}\": use /camp or /camp desktop"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_words_camp_to_the_list_and_desktop_out_of_the_program() {
        assert_eq!(destination(""), Ok(CampTo::CharacterSelect));
        assert_eq!(destination("  "), Ok(CampTo::CharacterSelect));
        assert_eq!(destination("desktop"), Ok(CampTo::Desktop));
        assert_eq!(destination(" Desktop "), Ok(CampTo::Desktop));
        assert_eq!(destination("DESKTOP"), Ok(CampTo::Desktop));
    }

    #[test]
    fn other_words_are_refused_naming_the_one_word_taken() {
        for words in ["server", "desk", "desktop now"] {
            let refusal = destination(words).unwrap_err();
            assert!(refusal.contains("/camp desktop"), "{refusal}");
            assert!(refusal.contains(words), "{refusal}");
        }
    }
}
