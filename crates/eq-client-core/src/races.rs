//! Which classic model draws a race: the three-letter code its skeleton and
//! meshes are named by in the client's character archives, such as `QCM` for
//! a male Qeynos citizen (`QCM_HS_DEF`). A zone's own archive
//! (`qeytoqrg_chr.s3d`) or the global ones hold each model; the race numbers
//! are the servers' (`EQEmu` common/races.h).

/// Per race: its male, female and neuter models, empty where it has none.
/// Covers the playable races and the creatures of the classic, Kunark and
/// Velious zones.
const MODELS: [(u32, [&str; 3]); 179] = [
    (1, ["HUM", "HUF", ""]),
    (2, ["BAM", "BAF", ""]),
    (3, ["ERM", "ERF", ""]),
    (4, ["ELM", "ELF", ""]),
    (5, ["HIM", "HIF", ""]),
    (6, ["DAM", "DAF", ""]),
    (7, ["HAM", "HAF", ""]),
    (8, ["DWM", "DWF", ""]),
    (9, ["TRM", "TRF", ""]),
    (10, ["OGM", "OGF", ""]),
    (11, ["HOM", "HOF", ""]),
    (12, ["GNM", "GNF", ""]),
    (13, ["", "", "AVI"]),        // Aviak
    (14, ["", "", "WER"]),        // Werewolf
    (15, ["BRM", "BRF", ""]),     // Brownie
    (16, ["", "", "CEN"]),        // Centaur
    (17, ["GOM", "", "GOL"]),     // Golem
    (18, ["", "", "GIA"]),        // Giant
    (19, ["", "", "TRK"]),        // Trakanon
    (20, ["", "", "VST"]),        // Doppleganger
    (21, ["", "", "BEH"]),        // Evil eye
    (22, ["", "", "BET"]),        // Beetle
    (23, ["CPM", "CPF", ""]),     // Kerran
    (24, ["", "", "FIS"]),        // Fish
    (25, ["", "FAF", ""]),        // Fairy
    (26, ["", "", "FRO"]),        // Froglok
    (27, ["", "", "FRG"]),        // Froglok ghoul
    (28, ["", "", "FUN"]),        // Fungusman
    (29, ["GAM", "", "GAR"]),     // Gargoyle
    (31, ["", "", "CUB"]),        // Gelatinous cube
    (33, ["", "", "GHU"]),        // Ghoul
    (34, ["", "", "BAT"]),        // Giant bat
    (35, ["", "", "EEL"]),        // Giant eel
    (36, ["", "", "RAT"]),        // Giant rat
    (37, ["", "", "SNA"]),        // Giant snake
    (38, ["", "", "SPI"]),        // Giant spider
    (39, ["", "", "GNN"]),        // Gnoll
    (40, ["", "", "GOB"]),        // Goblin
    (41, ["", "", "GOR"]),        // Gorilla
    (42, ["", "", "WOL"]),        // Wolf
    (43, ["", "", "BEA"]),        // Bear
    (44, ["FPM", "", ""]),        // Freeport guard
    (45, ["", "", "DML"]),        // Demi lich
    (46, ["", "", "IMP"]),        // Imp
    (47, ["", "", "GRI"]),        // Griffin
    (48, ["", "", "KOB"]),        // Kobold
    (49, ["", "", "DRA"]),        // Lava dragon
    (50, ["LIM", "LIF", ""]),     // Lion
    (51, ["", "", "LIZ"]),        // Lizard man
    (52, ["", "", "MIM"]),        // Mimic
    (53, ["", "", "MIN"]),        // Minotaur
    (54, ["", "", "ORC"]),        // Orc
    (55, ["BGM", "", ""]),        // Human beggar
    (56, ["", "PIF", ""]),        // Pixie
    (57, ["DRM", "DRF", ""]),     // Drachnid
    (58, ["", "", "SOL"]),        // Solusek Ro
    (59, ["", "", "BGG"]),        // Bloodgill goblin
    (60, ["", "", "SKE"]),        // Skeleton
    (61, ["", "", "SHA"]),        // Shark
    (62, ["", "", "TUN"]),        // Tunare
    (63, ["", "", "TIG"]),        // Tiger
    (64, ["", "", "TRE"]),        // Treant
    (65, ["DVM", "", ""]),        // Vampire
    (66, ["", "", "RAL"]),        // Rallos Zek
    (67, ["HHM", "", ""]),        // Highpass citizen
    (68, ["", "", "TEN"]),        // Tentacle
    (69, ["", "", "WIL"]),        // Will-o'-wisp
    (70, ["ZOM", "ZOF", ""]),     // Zombie
    (71, ["QCM", "QCF", ""]),     // Qeynos citizen
    (74, ["", "", "PIR"]),        // Piranha
    (75, ["", "", "ELE"]),        // Elemental
    (76, ["", "", "PUM"]),        // Puma
    (77, ["NGM", "", ""]),        // Neriak citizen
    (78, ["EGM", "", ""]),        // Erudite citizen
    (79, ["", "", "BIX"]),        // Bixie
    (80, ["", "", "REA"]),        // Reanimated hand
    (81, ["RIM", "RIF", ""]),     // Rivervale citizen
    (82, ["", "", "SCA"]),        // Scarecrow
    (83, ["", "", "SKU"]),        // Skunk
    (85, ["", "", "SPE"]),        // Spectre
    (86, ["", "", "SPH"]),        // Sphinx
    (87, ["", "", "ARM"]),        // Armadillo
    (88, ["CLM", "CLF", ""]),     // Clockwork gnome
    (89, ["", "", "DRK"]),        // Drake
    (90, ["HLM", "HLF", ""]),     // Halas citizen
    (91, ["", "", "ALL"]),        // Alligator
    (92, ["GRM", "GRF", ""]),     // Grobb citizen
    (93, ["OKM", "OKF", ""]),     // Oggok citizen
    (94, ["KAM", "KAF", ""]),     // Kaladim citizen
    (95, ["", "", "CAZ"]),        // Cazic Thule
    (96, ["", "", "COC"]),        // Cockatrice
    (98, ["VSM", "VSF", ""]),     // Elf vampire
    (99, ["", "", "DEN"]),        // Denizen
    (100, ["", "", "DER"]),       // Dervish
    (101, ["", "", "EFR"]),       // Efreeti
    (102, ["", "", "FRT"]),       // Froglok tadpole
    (103, ["", "", "KED"]),       // Kedge
    (104, ["", "", "LEE"]),       // Leech
    (105, ["", "", "SWO"]),       // Swordfish
    (106, ["FEM", "", ""]),       // Felguard
    (107, ["", "", "MAM"]),       // Mammoth
    (108, ["", "", "EYE"]),       // Eye of Zomm
    (109, ["", "", "WAS"]),       // Wasp
    (110, ["", "", "MER"]),       // Mermaid
    (111, ["", "", "HAR"]),       // Harpy
    (112, ["GFM", "GFF", ""]),    // Fayguard
    (113, ["", "", "DRI"]),       // Drixie
    (116, ["", "", "SEA"]),       // Sea horse
    (117, ["GDM", "", ""]),       // Ghost dwarf
    (118, ["GEM", "GEF", ""]),    // Erudite ghost
    (119, ["", "", "STC"]),       // Sabertooth cat
    (120, ["", "", "WOE"]),       // Wolf elemental
    (121, ["", "", "GRG"]),       // Gorgon
    (122, ["", "", "DRU"]),       // Dragon skeleton
    (123, ["", "", "INN"]),       // Innoruuk
    (124, ["", "", "UNI"]),       // Unicorn
    (125, ["", "", "PEG"]),       // Pegasus
    (126, ["", "", "DJI"]),       // Djinn
    (128, ["IKM", "IKF", ""]),    // Iksar
    (129, ["", "", "SCR"]),       // Scorpion
    (131, ["", "", "SRW"]),       // Sarnak
    (133, ["", "", "LYC"]),       // Lycanthrope
    (134, ["", "", "MOS"]),       // Mosquito
    (135, ["", "", "RHI"]),       // Rhino
    (136, ["", "", "XAL"]),       // Xalgoz
    (137, ["", "", "KGO"]),       // Kunark goblin
    (138, ["", "", "YET"]),       // Yeti
    (139, ["ICM", "ICF", ""]),    // Iksar citizen
    (140, ["", "", "FGI"]),       // Forest giant
    (144, ["", "", "BRN"]),       // Burynai
    (145, ["", "", "GOO"]),       // Goo
    (146, ["", "", "SSN"]),       // Spectral sarnak
    (147, ["SIM", "", ""]),       // Spectral iksar
    (148, ["", "", "BAC"]),       // Kunark fish
    (149, ["", "", "ISC"]),       // Iksar scorpion
    (150, ["", "", "ERO"]),       // Erollisi
    (151, ["", "", "TRI"]),       // Tribunal
    (153, ["", "", "BRI"]),       // Bristlebane
    (154, ["", "", "FDR"]),       // Fay drake
    (155, ["", "", "SSK"]),       // Sarnak skeleton
    (156, ["", "", "VRM"]),       // Ratman
    (157, ["", "", "WYV"]),       // Wyvern
    (158, ["", "", "WUR"]),       // Wurm
    (159, ["", "", "DEV"]),       // Devourer
    (160, ["", "", "IKG"]),       // Iksar golem
    (161, ["", "", "IKS"]),       // Iksar skeleton
    (162, ["", "", "MEP"]),       // Man-eating plant
    (163, ["", "", "RAP"]),       // Raptor
    (164, ["", "", "SGO"]),       // Sarnak golem
    (165, ["", "", "SED"]),       // Water dragon
    (166, ["", "", "IKH"]),       // Iksar hand
    (167, ["", "", "SUC"]),       // Succulent
    (168, ["", "", "FMO"]),       // Flying monkey
    (169, ["", "", "BTM"]),       // Brontotherium
    (170, ["", "", "SDE"]),       // Snow dervish
    (171, ["", "", "DIW"]),       // Dire wolf
    (172, ["", "", "MTC"]),       // Manticore
    (173, ["", "", "TOT"]),       // Totem
    (174, ["", "", "SPC"]),       // Cold spectre
    (175, ["", "", "ENA"]),       // Enchanted armor
    (176, ["", "", "SBU"]),       // Snow bunny
    (177, ["", "", "WAL"]),       // Walrus
    (178, ["", "", "RGM"]),       // Rock-gem man
    (181, ["", "", "YAK"]),       // Yak man
    (183, ["COM", "COF", "COK"]), // Coldain
    (184, ["", "", "DR2"]),       // Velious dragon
    (185, ["", "", "HAG"]),       // Hag
    (187, ["", "", "SIR"]),       // Siren
    (188, ["", "", "FSG"]),       // Frost giant
    (189, ["STM", "", "STG"]),    // Storm giant
    (190, ["", "", "OTM"]),       // Othmir
    (191, ["", "", "WLM"]),       // Walrus man
    (192, ["", "", "CCD"]),       // Clockwork dragon
    (193, ["", "", "ABH"]),       // Abhorrent
    (194, ["", "", "STU"]),       // Sea turtle
    (195, ["", "", "BWD"]),       // Black and white dragon
    (196, ["", "", "GDR"]),       // Ghost dragon
    (198, ["", "", "PRI"]),       // Prismatic dragon
    // A later skeleton, which classic clients draw as the original.
    (367, ["", "", "SKE"]),
];

/// Whether a race is drawn at all. Servers place spawn points and teleport
/// pads as unseen markers of the invisible man (127) and the teleport man
/// (240), which the client draws as nothing.
#[must_use]
pub const fn drawn(race: u32) -> bool {
    !matches!(race, 127 | 240)
}

/// The model a spawn of this race and gender (0 male, 1 female, 2 neuter)
/// draws with. A gender the race has no model for takes its neuter model,
/// then its male one, then its female one; None for a race without a model.
#[must_use]
pub fn model(race: u32, gender: u32) -> Option<&'static str> {
    let (_, models) = MODELS.iter().find(|(id, _)| *id == race)?;
    let own = usize::try_from(gender)
        .ok()
        .and_then(|gender| models.get(gender));
    own.into_iter()
        .chain([&models[2], &models[0], &models[1]])
        .copied()
        .find(|code| !code.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_gender_draws_its_own_model_or_the_nearest_one_the_race_has() {
        assert_eq!(model(71, 0), Some("QCM"));
        assert_eq!(model(71, 1), Some("QCF"));
        assert_eq!(model(71, 2), Some("QCM"));
        // Creatures with one model use it for every gender.
        assert_eq!(model(39, 0), Some("GNN"));
        assert_eq!(model(42, 1), Some("WOL"));
        // A female freeport guard is drawn as the male one.
        assert_eq!(model(44, 1), Some("FPM"));
        assert_eq!(model(183, 2), Some("COK"));
        assert_eq!(model(183, 9), Some("COK"));
        assert_eq!(model(127, 0), None);
        assert_eq!(model(9999, 0), None);
        assert!(drawn(71) && drawn(9999) && !drawn(127) && !drawn(240));
    }

    #[test]
    fn races_are_listed_once_in_order() {
        assert!(MODELS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(
            MODELS
                .iter()
                .all(|(_, codes)| codes.iter().any(|code| code.len() == 3))
        );
    }
}
