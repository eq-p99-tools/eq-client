//! Where the installation keeps each archive this crate reads, by its role.
//!
//! Every archive read today is an S3D (PFS) file named by a stem, such as
//! `qeynos`, `qeynos_obj` or `global_chr`. The stems are named here and the
//! file for a stem is found here, so a reader for another archive format
//! (the EQG files newer installations carry) changes this module and the
//! readers, not each place that opens an archive. Tests and probes that build
//! their own archives keep their literal file names.

use std::path::{Path, PathBuf};

/// The file in `directory` that holds the archive `stem`.
pub(crate) fn path(directory: &Path, stem: &str) -> PathBuf {
    directory.join(format!("{stem}.s3d"))
}

/// A zone's own archive: its terrain, collision and regions.
pub(crate) fn zone(short_name: &str) -> &str {
    short_name
}

/// A zone's object archive: the models its placements draw.
pub(crate) fn zone_objects(short_name: &str) -> String {
    format!("{short_name}_obj")
}

/// A zone's character archive: the models its own spawns draw first.
pub(crate) fn zone_characters(short_name: &str) -> String {
    format!("{short_name}_chr")
}

/// The global character archives, in the order a model is looked for in
/// them after a zone's own.
pub(crate) fn global_characters() -> impl Iterator<Item = String> {
    std::iter::once("global_chr".to_owned()).chain((2..=7).map(|i| format!("global{i}_chr")))
}

/// The classic item archives, searched in this order; each holds a WLD of
/// the same name. An installation may lack the later ones.
pub(crate) const ITEMS: [&str; 7] = [
    "gequip", "gequip2", "gequip3", "gequip4", "gequip5", "gequip6", "gequip8",
];

/// The archives that hold the worn equipment models of the newer player
/// models, in the order they are looked in.
pub(crate) const EQUIPMENT: [&str; 2] = ["lgequip", "lgequip2"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_role_names_the_s3d_archive_it_did_before() {
        let directory = Path::new("install");
        assert_eq!(
            path(directory, zone("qeynos")),
            directory.join("qeynos.s3d")
        );
        assert_eq!(
            path(directory, &zone_objects("qeynos")),
            directory.join("qeynos_obj.s3d")
        );
        assert_eq!(
            path(directory, &zone_characters("qeynos")),
            directory.join("qeynos_chr.s3d")
        );
        assert_eq!(
            global_characters().collect::<Vec<_>>(),
            [
                "global_chr",
                "global2_chr",
                "global3_chr",
                "global4_chr",
                "global5_chr",
                "global6_chr",
                "global7_chr"
            ]
        );
        assert_eq!(
            EQUIPMENT.map(|stem| path(directory, stem)),
            [
                directory.join("lgequip.s3d"),
                directory.join("lgequip2.s3d")
            ]
        );
    }
}
