//! The installed Titanium client's database strings (`dbstr_us.txt`; each
//! generation's file is [`crate::ui::InstalledClient::db_string_file`]):
//! the words AA definitions name by number, each under a type. Read at run
//! time from the installation, never copied: the text goes into no log and
//! no repository.
//!
//! The layout is inferred until read R49-40d: one entry a line, `id^type^text`,
//! with any later `^` fields ignored, and the first of a repeated (type, id)
//! kept. So is which type an AA's string ids name (R49-40a): `EQEmu`'s
//! `db_str` convention, type 1 for the title and type 4 for the
//! description, is what [`DbStrings::aa_title`] and
//! [`DbStrings::aa_description`] use.
use std::{collections::HashMap, io, path::Path};

/// The most bytes read from the file; a larger one is refused rather than
/// read (ours: several times the size of a Titanium install's file).
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// The type an AA definition's `title_sid` names, by `EQEmu`'s convention
/// (inferred until read R49-40a).
pub const AA_TITLE: u32 = 1;
/// The type an AA definition's `desc_sid` names, by `EQEmu`'s convention
/// (inferred until read R49-40a).
pub const AA_DESCRIPTION: u32 = 4;

/// Database strings by type and id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DbStrings(HashMap<(u32, u32), String>);

impl DbStrings {
    /// Reads the installed `client`'s database strings, or none for a
    /// generation that has no such file.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read, or is larger than [`MAX_BYTES`].
    pub fn read(eq_directory: &Path, client: crate::ui::InstalledClient) -> io::Result<Self> {
        client.db_string_file().map_or_else(
            || Ok(Self::default()),
            |file| Self::read_file(&eq_directory.join(file)),
        )
    }

    fn read_file(path: &Path) -> io::Result<Self> {
        use std::io::Read;
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the database strings file is larger than its bound",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the database strings file grew past its bound",
            ));
        }
        Ok(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// Parses the inferred layout; a line that isn't `id^type^text` with
    /// numbers and some text is skipped.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut strings = HashMap::new();
        for line in text.lines() {
            let mut fields = line.split('^');
            let (Some(id), Some(kind), Some(words)) = (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let (Ok(id), Ok(kind)) = (id.trim().parse::<u32>(), kind.trim().parse::<u32>()) else {
                continue;
            };
            if words.trim().is_empty() {
                continue;
            }
            strings
                .entry((kind, id))
                .or_insert_with(|| words.to_owned());
        }
        Self(strings)
    }

    /// Whether there are no strings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The string of this type and id; None where the installation lacks
    /// it, never a placeholder.
    #[must_use]
    pub fn text(&self, kind: u32, id: u32) -> Option<&str> {
        self.0.get(&(kind, id)).map(String::as_str)
    }

    /// An AA's name, from its definition's `title_sid`.
    #[must_use]
    pub fn aa_title(&self, title_sid: u32) -> Option<&str> {
        self.text(AA_TITLE, title_sid)
    }

    /// An AA's description, from its definition's `desc_sid`.
    #[must_use]
    pub fn aa_description(&self, desc_sid: u32) -> Option<&str> {
        self.text(AA_DESCRIPTION, desc_sid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inferred_layout_keeps_the_first_of_each_type_and_id() {
        // Synthetic words only.
        let strings = DbStrings::parse(
            "7^1^Alpha Words^0\n\
             7^4^Alpha described\n\
             7^1^Second of a pair\n\
             8^4^\n\
             nine^1^Not a number\n\
             10^1\n\
             11^2^Other type^0^0\n",
        );
        assert_eq!(strings.aa_title(7), Some("Alpha Words"));
        assert_eq!(strings.aa_description(7), Some("Alpha described"));
        // Empty text, a word for a number and too few fields are skipped.
        assert_eq!(strings.aa_description(8), None);
        assert_eq!(strings.text(2, 11), Some("Other type"));
        assert_eq!(strings.aa_title(11), None);
        assert_eq!(strings.aa_title(10), None);
        assert!(DbStrings::parse("").is_empty());
    }

    #[test]
    fn only_titanium_reads_a_file_and_a_missing_or_oversized_one_gives_no_names() {
        use crate::ui::InstalledClient;
        let folder =
            std::env::temp_dir().join(format!("eq-client-db-strings-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        assert!(DbStrings::read(&folder, InstalledClient::Titanium).is_err());
        std::fs::write(folder.join("dbstr_us.txt"), "3^1^Synthetic\n").unwrap();
        let titanium = DbStrings::read(&folder, InstalledClient::Titanium).unwrap();
        assert_eq!(titanium.aa_title(3), Some("Synthetic"));
        // The Mac-era client keeps its AA names elsewhere (R49-41b).
        assert!(
            DbStrings::read(&folder, InstalledClient::EqMac)
                .unwrap()
                .is_empty()
        );
        let file = std::fs::File::create(folder.join("dbstr_us.txt")).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        assert!(DbStrings::read(&folder, InstalledClient::Titanium).is_err());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
