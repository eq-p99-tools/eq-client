//! Compile-time provenance without timestamps, machine paths or Git output logs.
use std::{path::Path, process::Command};

#[derive(Debug, PartialEq, Eq)]
pub enum Dirty {
    Clean,
    Dirty,
    Unknown,
}

impl Dirty {
    pub const fn text(&self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug)]
pub struct Identity {
    pub sha: Option<String>,
    pub dirty: Dirty,
    pub source: &'static str,
    pub invalid_override: bool,
}

impl Identity {
    pub fn version(&self, package: &str) -> String {
        format!(
            "{package} (git {}; tracked tree {}; {})",
            self.sha.as_deref().unwrap_or("unavailable"),
            self.dirty.text(),
            self.source
        )
    }
}

fn sha(value: &str) -> Option<String> {
    matches!(value.len(), 40 | 64)
        .then_some(value)
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

pub fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

pub fn identify(root: &Path, sha_override: Option<&str>, dirty_override: Option<&str>) -> Identity {
    // Any override selects a single provenance source. Never attach the local
    // checkout's clean flag to a different revision supplied by a packager.
    if sha_override.is_some() || dirty_override.is_some() {
        let sha = sha_override.and_then(sha);
        let dirty = match dirty_override {
            Some("clean") => Dirty::Clean,
            Some("dirty") => Dirty::Dirty,
            _ => Dirty::Unknown,
        };
        return Identity {
            invalid_override: (sha_override.is_some() && sha.is_none())
                || dirty_override
                    .is_some_and(|value| !matches!(value, "clean" | "dirty" | "unknown")),
            sha,
            dirty,
            source: "override",
        };
    }
    // An archive nested in somebody else's checkout is still an archive.
    let checkout = root.join(".git").exists();
    let sha = if checkout {
        git(root, &["rev-parse", "--verify", "HEAD"]).and_then(|value| sha(value.trim()))
    } else {
        None
    };
    let dirty = if sha.is_some() {
        Command::new("git")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .arg("-C")
            .arg(root)
            .args([
                "diff",
                "--no-ext-diff",
                "--quiet",
                "--ignore-submodules=none",
                "HEAD",
                "--",
            ])
            .output()
            .ok()
            .map_or(Dirty::Unknown, |output| match output.status.code() {
                Some(0) => Dirty::Clean,
                Some(1) => Dirty::Dirty,
                _ => Dirty::Unknown,
            })
    } else {
        Dirty::Unknown
    };
    Identity {
        sha,
        dirty,
        source: if checkout { "checkout" } else { "archive" },
        invalid_override: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_are_bounded_and_never_mix_with_checkout_state() {
        let root = Path::new("synthetic-archive");
        let id = identify(root, Some(&"a".repeat(40)), None);
        assert_eq!(id.dirty, Dirty::Unknown);
        assert_eq!(id.source, "override");
        assert!(!id.invalid_override);
        for bad in ["1234567", "bad\nvalue", &"z".repeat(40)] {
            let id = identify(root, Some(bad), Some("clean"));
            assert!(id.sha.is_none());
            assert!(id.invalid_override);
            assert!(!id.version("1.0").contains(bad));
        }
        assert!(identify(root, None, Some("false")).invalid_override);
        assert_eq!(identify(root, None, Some("dirty")).dirty, Dirty::Dirty);
    }

    #[test]
    fn source_archives_are_explicitly_unavailable_and_strings_are_deterministic() {
        let id = identify(Path::new("synthetic-archive"), None, None);
        assert!(id.sha.is_none());
        assert_eq!(
            id.version("1.0"),
            "1.0 (git unavailable; tracked tree unknown; archive)"
        );
        assert_eq!(id.version("1.0"), id.version("1.0"));
    }

    #[test]
    fn full_hashes_support_both_git_object_formats() {
        assert_eq!(sha(&"A".repeat(40)), Some("a".repeat(40)));
        assert_eq!(sha(&"B".repeat(64)), Some("b".repeat(64)));
        assert!(sha(&"a".repeat(41)).is_none());
    }

    #[test]
    fn git_tracks_dirty_edits_commits_linked_worktrees_and_nested_archives() {
        // A disposable local source fixture only; no user checkout or config.
        let root = std::env::temp_dir().join(format!(
            "eq-build-identity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "synthetic git command failed");
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("tracked"), "one\n").unwrap();
        run(&["add", "tracked"]);
        run(&[
            "-c",
            "user.name=Build Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=empty-hooks",
            "commit",
            "-q",
            "--no-gpg-sign",
            "-m",
            "fixture",
        ]);
        let clean = identify(&root, None, None);
        assert_eq!(clean.dirty, Dirty::Clean);
        std::fs::write(root.join("untracked"), "outside tracked flag\n").unwrap();
        assert_eq!(identify(&root, None, None).dirty, Dirty::Clean);
        std::fs::write(root.join("tracked"), "two\n").unwrap();
        let dirty = identify(&root, None, None);
        assert_eq!(dirty.sha, clean.sha);
        assert_eq!(dirty.dirty, Dirty::Dirty);
        run(&["add", "tracked"]);
        assert_eq!(identify(&root, None, None).dirty, Dirty::Dirty);
        run(&[
            "-c",
            "user.name=Build Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=empty-hooks",
            "commit",
            "-q",
            "--no-gpg-sign",
            "-m",
            "second",
        ]);
        let changed = identify(&root, None, None);
        assert_ne!(changed.sha, clean.sha);
        assert_eq!(changed.dirty, Dirty::Clean);
        run(&["worktree", "add", "-q", "--detach", "linked", "HEAD"]);
        assert_eq!(identify(&root.join("linked"), None, None).sha, changed.sha);
        std::fs::create_dir(root.join("archive")).unwrap();
        assert!(identify(&root.join("archive"), None, None).sha.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
