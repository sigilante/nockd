//! Content-addressed artifact store (DESIGN §5.1, §3).
//!
//! An artifact = the Rust wrapper binary + the `out.jam` kernel. We compute two hashes
//! (DESIGN OQ2, "strict both"):
//!   - `kernel_hash`   = BLAKE3(out.jam)             — the reproducible, semantic identity
//!                                                     (matches the runtime's `ker_hash`).
//!   - `artifact_hash` = BLAKE3(jam ‖ bin ‖ triple)  — the full shipped bundle identity.
//!
//! Provenance (build host/time, resolved typhoon graph, attestation) lives in a sidecar
//! and is intentionally excluded from the hashed bytes.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ArtifactRecord {
    pub artifact_hash: String,
    pub kernel_hash: String,
    pub target_triple: String,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Store { dir }
    }

    fn artifact_dir(&self, artifact_hash: &str) -> PathBuf {
        self.dir.join(artifact_hash)
    }

    pub fn bin_path(&self, artifact_hash: &str) -> PathBuf {
        self.artifact_dir(artifact_hash).join("bin")
    }

    pub fn jam_path(&self, artifact_hash: &str) -> PathBuf {
        self.artifact_dir(artifact_hash).join("out.jam")
    }

    pub fn has(&self, artifact_hash: &str) -> bool {
        self.bin_path(artifact_hash).exists() && self.jam_path(artifact_hash).exists()
    }

    /// Store an artifact, returning its record. Idempotent: a hash already present is a
    /// no-op (content-addressed dedup, DESIGN §3).
    pub fn put(&self, jam: &[u8], bin: &[u8], target_triple: &str) -> Result<ArtifactRecord> {
        let kernel_hash = blake3::hash(jam).to_hex().to_string();

        let mut hasher = blake3::Hasher::new();
        hasher.update(jam);
        hasher.update(bin);
        hasher.update(target_triple.as_bytes());
        let artifact_hash = hasher.finalize().to_hex().to_string();

        let record = ArtifactRecord {
            artifact_hash: artifact_hash.clone(),
            kernel_hash,
            target_triple: target_triple.to_string(),
        };

        if self.has(&artifact_hash) {
            return Ok(record);
        }

        let adir = self.artifact_dir(&artifact_hash);
        std::fs::create_dir_all(&adir).with_context(|| format!("creating {}", adir.display()))?;
        std::fs::write(self.jam_path(&artifact_hash), jam).context("writing out.jam")?;

        let bin_path = self.bin_path(&artifact_hash);
        std::fs::write(&bin_path, bin).context("writing binary")?;
        let mut perms = std::fs::metadata(&bin_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perms).context("chmod binary")?;

        Ok(record)
    }

    /// Copy the kernel into an app's state dir as `out.jam` (the wrapper reads it from cwd).
    pub fn stage_jam(&self, artifact_hash: &str, state_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(state_dir)
            .with_context(|| format!("creating state dir {}", state_dir.display()))?;
        std::fs::copy(self.jam_path(artifact_hash), state_dir.join("out.jam"))
            .context("staging out.jam into state dir")?;
        Ok(())
    }
}
