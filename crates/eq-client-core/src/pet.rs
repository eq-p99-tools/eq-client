//! The player's pet: the words `/pet` takes for each command, as the
//! official client reads them.
use crate::pets::PetCommand;

/// The command a `/pet` line names, by its words in any case; None for words
/// that name none.
#[must_use]
pub fn command(words: &str) -> Option<PetCommand> {
    let words = words
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    Some(match words.as_str() {
        "attack" => PetCommand::Attack,
        "back" | "back off" | "backoff" => PetCommand::BackOff,
        "get lost" | "go away" => PetCommand::GetLost,
        "health" | "report health" | "health report" => PetCommand::Health,
        "guard" | "guard here" => PetCommand::GuardHere,
        "follow" | "follow me" => PetCommand::Follow,
        "guard me" => PetCommand::GuardMe,
        "sit" | "sit down" | "sit on" => PetCommand::Sit,
        "stand" | "stand up" | "sit off" => PetCommand::Stand,
        "taunt" => PetCommand::Taunt,
        "taunt on" => PetCommand::TauntOn,
        "taunt off" | "no taunt" => PetCommand::TauntOff,
        "hold" => PetCommand::Hold,
        "leader" => PetCommand::Leader,
        "feign" | "feign death" => PetCommand::Feign,
        "no cast" | "nocast" | "spellhold" => PetCommand::NoCast,
        "focus" => PetCommand::Focus,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pet_words_name_their_commands() {
        assert_eq!(command("attack"), Some(PetCommand::Attack));
        assert_eq!(command("Back  Off"), Some(PetCommand::BackOff));
        assert_eq!(command("get lost"), Some(PetCommand::GetLost));
        assert_eq!(command("sit down"), Some(PetCommand::Sit));
        assert_eq!(command("guard me"), Some(PetCommand::GuardMe));
        assert_eq!(command("health report"), Some(PetCommand::Health));
        assert_eq!(command("dance"), None);
        assert_eq!(command(""), None);
    }
}
