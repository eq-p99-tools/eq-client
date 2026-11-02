//! A bounded, asynchronous browser for `<installation>/Scripts` and its folders.
use super::{Replace, Tab, Workbench};
use crate::{
    background::Background,
    theme,
    windows::{
        self, Shown, WindowId,
        pointer::{TakesWheel, Wheel},
    },
};
use bevy::prelude::*;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

const MAX_ENTRIES: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    folder: bool,
}

impl Entry {
    /// Shorten only the display label; opening and saving retain the real path.
    fn label(&self) -> String {
        let name = if !self.folder
            && self
                .path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("eqscript"))
        {
            self.path.file_stem()
        } else {
            self.path.file_name()
        }
        .unwrap_or_default()
        .to_string_lossy();
        if self.folder {
            format!("[+] {name}/")
        } else {
            name.into_owned()
        }
    }
}

#[derive(Default)]
pub(super) struct Library {
    pub root: Option<PathBuf>,
    folder: PathBuf,
    entries: Vec<Entry>,
    job: Background<Result<Vec<Entry>, String>>,
    generation: u64,
    revision: u64,
    loaded: bool,
    status: String,
}

impl Library {
    pub fn invalidate(&mut self) {
        self.browse(self.folder.clone());
    }
    /// Display library files without the installation's absolute path.
    pub fn relative(&self, path: &Path) -> String {
        self.root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// Save names are relative to Scripts, including any nested directories.
    pub fn path(&self, name: &str) -> Result<PathBuf, String> {
        let root = self
            .root
            .as_ref()
            .ok_or("Choose an EQ installation to use its Scripts folder.")?;
        let relative = Path::new(name.trim());
        if relative.as_os_str().is_empty() || !normal(relative) {
            return Err("Enter a name relative to Scripts, such as tests/camera.eqscript.".into());
        }
        Ok(root.join(relative))
    }

    fn browse(&mut self, folder: PathBuf) {
        self.folder = folder;
        self.entries.clear();
        self.loaded = false;
        self.generation = self.generation.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
    }
}

fn normal(path: &Path) -> bool {
    path.components()
        .all(|part| matches!(part, Component::Normal(_)))
}

/// Validate each existing ancestor before creating children or reading a file.
/// Symlinks inside the library may not redirect these operations outside it.
pub(super) fn prepare(root: &Path, path: &Path, create_parents: bool) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Path is outside Scripts.")?;
    if !normal(relative) {
        return Err("Path must stay inside Scripts.".into());
    }
    if create_parents {
        fs::create_dir_all(root).map_err(|error| error.to_string())?;
    }
    let canonical = root
        .canonicalize()
        .map_err(|error| format!("Cannot open Scripts: {error}"))?;
    let mut current = root.to_owned();
    let parts: Vec<_> = relative.components().collect();
    for (index, part) in parts.iter().enumerate() {
        current.push(part.as_os_str());
        if create_parents && index + 1 < parts.len() && !current.exists() {
            fs::create_dir(&current).map_err(|error| error.to_string())?;
        }
        if current.exists()
            && !current
                .canonicalize()
                .map_err(|error| error.to_string())?
                .starts_with(&canonical)
        {
            return Err("A link points outside the Scripts folder.".into());
        }
    }
    Ok(())
}

fn listing(root: &Path, folder: &Path) -> Result<Vec<Entry>, String> {
    let directory = root.join(folder);
    if !root.try_exists().map_err(|error| error.to_string())? {
        return Ok(Vec::new());
    }
    prepare(root, &directory, false)?;
    let mut entries = Vec::new();
    for (index, entry) in fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .take(MAX_ENTRIES + 1)
        .enumerate()
    {
        if index == MAX_ENTRIES {
            return Err("Folder is too large; split scripts into subfolders.".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        let path = folder.join(entry.file_name());
        if kind.is_dir()
            || (kind.is_file()
                && path
                    .extension()
                    .and_then(|name| name.to_str())
                    .is_some_and(|extension| {
                        ["eqscript", "script", "txt"]
                            .iter()
                            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
                    }))
        {
            entries.push(Entry {
                path,
                folder: kind.is_dir(),
            });
        }
    }
    entries
        .sort_by_cached_key(|entry| (!entry.folder, entry.path.to_string_lossy().to_lowercase()));
    Ok(entries)
}

#[derive(Component)]
pub(super) enum Action {
    Up,
    Refresh,
    Choose(PathBuf, bool),
}
#[derive(Component)]
pub(super) struct List;
#[derive(Component)]
pub(super) struct Location;

/// Browsing never executes a script; file selection uses the normal dirty-buffer guard.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn actions(
    clicks: crate::clicks::Clicks<Action>,
    mut state: ResMut<Workbench>,
    script: Option<Res<crate::script::Script>>,
) {
    if script.is_some() || state.busy() {
        return;
    }
    for action in clicks.iter() {
        match action {
            Action::Refresh => {
                let folder = state.library.folder.clone();
                state.library.browse(folder);
            }
            Action::Up => {
                let parent = state
                    .library
                    .folder
                    .parent()
                    .unwrap_or(Path::new(""))
                    .to_owned();
                state.library.browse(parent);
            }
            Action::Choose(path, true) => state.library.browse(path.clone()),
            Action::Choose(path, false) => {
                if let Some(root) = &state.library.root {
                    let path = root.join(path);
                    state.replace(Replace::Open(path));
                }
            }
        }
    }
}

/// Read one directory in the worker pool, on opening or explicit refresh.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn poll(mut state: ResMut<Workbench>, shown: Res<Shown>) {
    let library = &mut state.library;
    if let Some(result) = library.job.poll(Some(library.generation)) {
        match result {
            Ok(entries) => {
                library.entries = entries;
                library.status = if library.entries.is_empty() {
                    "No scripts here. Save one or add files externally, then Refresh.".into()
                } else {
                    String::new()
                };
            }
            Err(error) => library.status = error,
        }
        library.loaded = true;
        library.revision = library.revision.wrapping_add(1);
    }
    if !shown.displayed(WindowId::Scripts)
        || library.loaded
        || library.job.pending(Some(library.generation))
    {
        return;
    }
    let Some(root) = library.root.clone() else {
        library.status = "Choose an EQ installation to browse Scripts.".into();
        library.loaded = true;
        library.revision = library.revision.wrapping_add(1);
        return;
    };
    let folder = library.folder.clone();
    library.status = "Reading folder…".into();
    library
        .job
        .start(library.generation, move || listing(&root, &folder));
}

fn button(commands: &mut Commands, title: &str, action: Action) -> Entity {
    commands
        .spawn((
            Button,
            action,
            theme::text(title, theme::Size::Small, theme::INK_BRIGHT),
            Node {
                padding: UiRect::all(px(6)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme::BUTTON),
        ))
        .id()
}

/// A scrollable folder/file list beside the editor.
pub(super) fn spawn(commands: &mut Commands) -> Entity {
    let location = commands
        .spawn((
            Location,
            theme::text("Scripts", theme::Size::Small, theme::INK_BRIGHT),
        ))
        .id();
    let up = button(commands, "Up", Action::Up);
    let refresh = button(commands, "Refresh", Action::Refresh);
    let controls = commands
        .spawn(Node {
            column_gap: px(5),
            ..default()
        })
        .add_children(&[up, refresh])
        .id();
    let list = commands
        .spawn((
            List,
            TakesWheel,
            windows::KeepsPress,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                height: px(332),
                overflow: Overflow::scroll_y(),
                row_gap: px(4),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((
            Node {
                width: px(170),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(8)),
                ..default()
            },
            BackgroundColor(theme::WELL),
        ))
        .add_children(&[location, controls, list])
        .id()
}

#[allow(clippy::needless_pass_by_value)]
/// Rebuild only when a directory snapshot or its status changes.
pub(super) fn draw(
    mut commands: Commands,
    state: Res<Workbench>,
    lists: Query<Entity, With<List>>,
    mut locations: Query<&mut Text, With<Location>>,
    mut previous: Local<Option<u64>>,
) {
    if *previous == Some(state.library.revision) {
        return;
    }
    let library = &state.library;
    for mut text in &mut locations {
        text.0 = format!("Scripts / {}", library.folder.display());
    }
    for list in &lists {
        commands.entity(list).despawn_children();
        for entry in &library.entries {
            let row = button(
                &mut commands,
                &entry.label(),
                Action::Choose(entry.path.clone(), entry.folder),
            );
            commands.entity(list).add_child(row);
        }
        if !library.status.is_empty() {
            let message = commands
                .spawn(theme::text(
                    &library.status,
                    theme::Size::Small,
                    theme::INK_DIM,
                ))
                .id();
            commands.entity(list).add_child(message);
        }
    }
    *previous = Some(library.revision);
}

#[allow(clippy::needless_pass_by_value)]
/// Scroll the file list independently of the document and game camera.
pub(super) fn scroll(
    wheel: Res<Wheel>,
    state: Res<Workbench>,
    mut lists: Query<(Entity, &ComputedNode, &mut ScrollPosition), With<List>>,
    mut revision: Local<u64>,
) {
    for (entity, node, mut scroll) in &mut lists {
        if *revision != state.library.revision {
            scroll.0 = Vec2::ZERO;
        }
        if wheel.surface == Some(entity) && state.tab == Tab::Editor {
            windows::scroll_by(&mut scroll, node, wheel.pixels);
        }
    }
    *revision = state.library.revision;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_hide_only_the_script_suffix() {
        for (path, folder, label) in [
            ("nested/example.eqscript", false, "example"),
            ("example.EQSCRIPT", false, "example"),
            ("notes.txt", false, "notes.txt"),
            ("folder.eqscript", true, "[+] folder.eqscript/"),
        ] {
            let entry = Entry {
                path: path.into(),
                folder,
            };
            assert_eq!(entry.label(), label);
            assert_eq!(entry.path, PathBuf::from(path));
        }
    }
    #[test]
    fn library_names_are_relative_and_nested() {
        let library = Library {
            root: Some(PathBuf::from("game/Scripts")),
            ..default()
        };
        assert_eq!(
            library.path("tests/camera.eqscript").unwrap(),
            PathBuf::from("game/Scripts/tests/camera.eqscript")
        );
        for invalid in ["", "../private.txt", "/outside.txt"] {
            assert!(library.path(invalid).is_err());
        }
        assert_eq!(
            library.relative(Path::new("game/Scripts/tests/camera.eqscript")),
            Path::new("tests/camera.eqscript").display().to_string()
        );
    }

    #[test]
    fn nested_folders_are_browsable_and_refresh_reads_external_changes() {
        let temporary = std::env::temp_dir();
        let root = temporary.join(format!(
            "eq-script-library-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = root.join("practice/nested/camera.eqscript");
        prepare(&root, &file, true).unwrap();
        fs::write(&file, "wait 1\n").unwrap();
        fs::write(root.join("ignore.png"), b"not a script").unwrap();
        assert_eq!(
            listing(&root, Path::new("")).unwrap(),
            vec![Entry {
                path: "practice".into(),
                folder: true
            }]
        );
        assert_eq!(
            listing(&root, Path::new("practice/nested")).unwrap()[0].path,
            PathBuf::from("practice/nested/camera.eqscript")
        );
        fs::write(root.join("external.eqscript"), "wait 2\n").unwrap();
        assert_eq!(listing(&root, Path::new("")).unwrap().len(), 2);
        assert!(listing(&root, Path::new("..")).is_err());
        assert!(root.starts_with(&temporary));
        fs::remove_dir_all(root).unwrap();
    }
}
