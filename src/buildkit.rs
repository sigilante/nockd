//! Client-side build orchestration (DESIGN principle 7, §5.2). `nockd deploy --project`
//! shells out to the upstream `nockup` toolchain, locates the built artifact, and captures
//! provenance. The daemon never compiles; this runs on the dev/CI side only.
//!
//! Provenance today captures the project manifest, git commit, and `nockup` version. The
//! resolved typhoon dependency graph (DESIGN §7.3, OQ2) is the missing piece — there is no
//! upstream `nockapp.lock` yet — so we record what we can and flag the gap.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::config::now_secs;

pub struct BuiltArtifact {
    pub name: String,
    pub bin: PathBuf,
    pub jam: PathBuf,
    pub provenance: String,
}

#[derive(Serialize)]
struct Provenance {
    manifest_file: String,
    manifest: String,
    git_commit: Option<String>,
    nockup_version: Option<String>,
    built_at_unix: i64,
    note: String,
}

/// Build a NockApp project via `nockup`, returning the located artifact + provenance.
pub fn build_project(project_dir: &Path) -> Result<BuiltArtifact> {
    let project_dir = project_dir
        .canonicalize()
        .with_context(|| format!("project dir {} not found", project_dir.display()))?;

    let (manifest_file, manifest_text) = read_manifest(&project_dir)?;
    let name = parse_project_name(&manifest_text).unwrap_or_else(|| {
        project_dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "app".into())
    });

    run_nockup_build(&project_dir)?;

    let bin = locate_bin(&project_dir, &name)?;
    let jam = locate_jam(&project_dir)?;

    let provenance = Provenance {
        manifest_file,
        manifest: manifest_text,
        git_commit: git_commit(&project_dir),
        nockup_version: nockup_version(),
        built_at_unix: now_secs(),
        note: "resolved typhoon dependency graph not captured: no upstream nockapp.lock yet \
               (DESIGN OQ2/§7.3)"
            .into(),
    };
    let provenance = serde_json::to_string_pretty(&provenance)?;

    Ok(BuiltArtifact {
        name,
        bin,
        jam,
        provenance,
    })
}

fn read_manifest(dir: &Path) -> Result<(String, String)> {
    for candidate in ["nockapp.toml", "manifest.toml"] {
        let path = dir.join(candidate);
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            return Ok((candidate.to_string(), text));
        }
    }
    bail!(
        "no nockapp.toml (or legacy manifest.toml) found in {}",
        dir.display()
    );
}

fn parse_project_name(manifest_text: &str) -> Option<String> {
    let value: toml::Value = manifest_text.parse().ok()?;
    let pkg = value.get("package").or_else(|| value.get("project"))?;
    // upstream nockapp.toml uses [package].name; the legacy manifest used project_name.
    pkg.get("project_name")
        .or_else(|| pkg.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn run_nockup_build(project_dir: &Path) -> Result<()> {
    // `nockup project build` with no arg, run inside the project dir, interprets the package
    // name as a subdirectory (looks for <dir>/<name>) and fails. Pass the absolute project
    // path explicitly, from the parent, so nockup resolves it unambiguously. (project_dir is
    // canonicalized by build_project.)
    let parent = project_dir.parent().unwrap_or(project_dir);
    let status = Command::new("nockup")
        .args(["project", "build"])
        .arg(project_dir)
        .current_dir(parent)
        .status()
        .context(
            "failed to run `nockup` — is the toolchain installed and on PATH? \
             (build happens client-side; the daemon never compiles)",
        )?;
    if !status.success() {
        bail!("`nockup project build` failed with status {status}");
    }
    Ok(())
}

fn locate_bin(project_dir: &Path, name: &str) -> Result<PathBuf> {
    let candidate = project_dir.join("target/release").join(name);
    if candidate.exists() {
        return Ok(candidate);
    }
    bail!(
        "could not find built binary at {} — check the project name matches the [[bin]] target",
        candidate.display()
    );
}

fn locate_jam(project_dir: &Path) -> Result<PathBuf> {
    for rel in ["out.jam", "target/release/out.jam"] {
        let path = project_dir.join(rel);
        if path.exists() {
            return Ok(path);
        }
    }
    bail!("could not find out.jam under {}", project_dir.display());
}

fn git_commit(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn nockup_version() -> Option<String> {
    let out = Command::new("nockup").arg("--version").output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}
