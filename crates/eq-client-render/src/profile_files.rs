//! The files this client keeps in the user's settings directory. As the
//! official client keeps its settings per character, each character on each
//! world has its own file; a shared one covers the screens before a
//! character enters and seeds characters that have none yet.
use std::path::Path;

/// The shared file's name, or `<prefix>-<world>-<character>.txt` with
/// anything but letters, digits, `-` and `_` replaced.
pub(crate) fn name(prefix: &str, shared: &str, profile: Option<&(String, String)>) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    profile.map_or_else(
        || shared.to_owned(),
        |(world, character)| format!("{prefix}-{}-{}.txt", clean(world), clean(character)),
    )
}

/// Replaces the file in one step, so a crash never leaves half of it.
pub(crate) fn write(directory: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let partial = path.with_extension("tmp");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_character_on_each_world_has_a_file_of_its_own() {
        assert_eq!(name("windows", "windows.txt", None), "windows.txt");
        let profile = ("P1999Green".to_owned(), "Example".to_owned());
        assert_eq!(
            name("options", "options.txt", Some(&profile)),
            "options-P1999Green-Example.txt"
        );
        let odd = ("a/b".to_owned(), "..".to_owned());
        assert_eq!(
            name("options", "options.txt", Some(&odd)),
            "options-a_b-__.txt"
        );
    }
}
