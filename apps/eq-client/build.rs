//! Stamp only this application; libraries keep their normal rebuild behavior.
mod build_identity;

use std::{env, path::Path};

fn main() {
    println!("cargo:rerun-if-env-changed=EQ_CLIENT_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=EQ_CLIENT_BUILD_DIRTY");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_identity.rs");
    let manifest = env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets the manifest directory");
    let root = Path::new(&manifest).join("../..");
    let sha = env::var("EQ_CLIENT_BUILD_SHA").ok();
    let dirty = env::var("EQ_CLIENT_BUILD_DIRTY").ok();
    let identity = build_identity::identify(&root, sha.as_deref(), dirty.as_deref());
    if identity.invalid_override {
        println!(
            "cargo:warning=Invalid build provenance override; unavailable/unknown fields retained"
        );
    }
    if identity.source == "checkout" && identity.sha.is_some() {
        watch_git(&root);
    }
    let version = env::var("CARGO_PKG_VERSION").expect("Cargo sets the package version");
    println!(
        "cargo:rustc-env=EQ_CLIENT_BUILD_IDENTITY={}",
        identity.version(&version)
    );
}

fn watch(path: &str) {
    if !path.contains(['\r', '\n']) && !Path::new(path).is_absolute() {
        println!("cargo:rerun-if-changed=../../{path}");
    }
}

fn watch_git(root: &Path) {
    // Git worktrees have a .git file, per-worktree HEAD/index, and shared refs.
    if root.join(".git").is_file() {
        watch(".git");
    }
    let mut names = vec![
        "HEAD".to_owned(),
        "index".to_owned(),
        "packed-refs".to_owned(),
    ];
    if let Some(reference) = build_identity::git(root, &["symbolic-ref", "-q", "HEAD"]) {
        names.push(reference.trim().to_owned());
    }
    for name in names {
        if let Some(path) = build_identity::git(
            root,
            &["rev-parse", "--path-format=relative", "--git-path", &name],
        ) {
            // Missing optional packed-refs would otherwise trigger every build.
            let path = path.trim();
            if root.join(path).exists() {
                watch(path);
            }
        }
    }
    if let Some(files) = build_identity::git(root, &["ls-files", "--cached", "-z"]) {
        for file in files.split('\0').filter(|file| !file.is_empty()) {
            watch(file);
        }
    }
}
