use crate::ui::InstalledClient;

/// One class's unmodified numeric constructor data for a skill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassRule {
    /// The constructor's training level, including its unused defaults.
    pub train_level: u8,
    /// Titanium's class-usable value or the Mac-era class cap. Nonzero is
    /// usable; the magnitude is retained and is not a training-level test.
    pub class_value: u16,
}

impl ClassRule {
    /// Whether the program's class-usable field is nonzero.
    #[must_use]
    pub const fn usable(self) -> bool {
        self.class_value != 0
    }
}

#[derive(Clone, Debug, Default)]
struct Skill {
    name_string: u32,
    classes: [ClassRule; 17],
}

#[derive(Clone, Debug)]
pub(super) struct SkillTable([Skill; 100]);

/// A numeric constructor table is malformed or incomplete.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SkillTableError {
    /// The columns do not identify this generation's data layout.
    #[error("unexpected skill table columns")]
    Columns,
    /// A record has invalid numbers, indices or field count.
    #[error("invalid skill table record at line {0}")]
    Record(usize),
    /// More than one record defines the same skill/class cell.
    #[error("duplicate skill {skill}, class {class}")]
    Duplicate {
        /// Skill index.
        skill: usize,
        /// Class index.
        class: usize,
    },
    /// A skill's name ID differs between class records.
    #[error("inconsistent name ID for skill {0}")]
    Name(usize),
    /// A required constructor cell is absent.
    #[error("missing skill {skill}, class {class}")]
    Missing {
        /// Skill index.
        skill: usize,
        /// Class index.
        class: usize,
    },
}

impl SkillTable {
    pub(super) fn parse(client: InstalledClient, text: &str) -> Result<Self, SkillTableError> {
        let mut lines = text
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.starts_with('#') && !line.trim().is_empty());
        let header = match client {
            InstalledClient::Titanium => "skill,class,train_level,class_usable,name_eqstr",
            InstalledClient::EqMac => "skill,class,train_level,class_cap,name_eqstr",
        };
        if lines.next().map(|(_, line)| line) != Some(header) {
            return Err(SkillTableError::Columns);
        }
        let mut skills: [Skill; 100] = std::array::from_fn(|_| Skill::default());
        let mut seen = [[false; 17]; 100];
        for (line, record) in lines {
            let invalid = || SkillTableError::Record(line + 1);
            let values: Vec<u32> = record
                .split(',')
                .map(str::parse)
                .collect::<Result<_, _>>()
                .map_err(|_| invalid())?;
            let [skill, class, level, class_value, name] = values.as_slice() else {
                return Err(invalid());
            };
            let skill = usize::try_from(*skill).map_err(|_| invalid())?;
            let class = usize::try_from(*class).map_err(|_| invalid())?;
            let cell = seen
                .get_mut(skill)
                .and_then(|classes| classes.get_mut(class))
                .ok_or_else(invalid)?;
            if *cell {
                return Err(SkillTableError::Duplicate { skill, class });
            }
            let definition = &mut skills[skill];
            if seen[skill].iter().any(|seen| *seen) && definition.name_string != *name {
                return Err(SkillTableError::Name(skill));
            }
            definition.name_string = *name;
            definition.classes[class] = ClassRule {
                train_level: u8::try_from(*level).map_err(|_| invalid())?,
                class_value: u16::try_from(*class_value).map_err(|_| invalid())?,
            };
            seen[skill][class] = true;
        }
        for (skill, classes) in seen.iter().enumerate() {
            for (class, seen) in classes.iter().enumerate() {
                if !seen {
                    return Err(SkillTableError::Missing { skill, class });
                }
            }
        }
        Ok(Self(skills))
    }

    pub(super) fn name_string(&self, skill: u32) -> Option<u32> {
        Some(self.0.get(usize::try_from(skill).ok()?)?.name_string)
    }

    pub(super) fn class_rule(&self, skill: u32, class: u32) -> Option<ClassRule> {
        self.0
            .get(usize::try_from(skill).ok()?)?
            .classes
            .get(usize::try_from(class).ok()?)
            .copied()
    }
}
