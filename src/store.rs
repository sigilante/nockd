//! Content-addressed artifact store (DESIGN §5.1, §3).
//!
//! An artifact is the Rust wrapper binary, optionally plus an `out.jam` kernel. Apps built
//! from a Nockup template read `out.jam` from cwd, so it ships separately; apps that
//! **embed their kernel** (e.g. `nockchain`, `nockchain-wallet` via `kernels_open_*`) are
//! binary-only. We compute (DESIGN OQ2, "strict both"):
//!   - `kernel_hash`   = BLAKE3(out.jam)  — reproducible semantic identity; empty when the
//!                                          kernel is embedded in the binary.
//!   - `artifact_hash` = BLAKE3([jam ‖] bin ‖ triple) — the full shipped bundle identity.
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

    /// Compute the content-addressed identity of an artifact (DESIGN OQ2): `kernel_hash` =
    /// BLAKE3(out.jam) (empty when the kernel is embedded), `artifact_hash` =
    /// BLAKE3([jam ‖] bin ‖ triple). Used by both the store and the client (to sign an
    /// attestation over the same hashes the daemon will compute).
    pub fn compute_hashes(jam: Option<&[u8]>, bin: &[u8], target_triple: &str) -> ArtifactRecord {
        let kernel_hash = jam
            .map(|j| blake3::hash(j).to_hex().to_string())
            .unwrap_or_default();
        let mut hasher = blake3::Hasher::new();
        if let Some(j) = jam {
            hasher.update(j);
        }
        hasher.update(bin);
        hasher.update(target_triple.as_bytes());
        ArtifactRecord {
            artifact_hash: hasher.finalize().to_hex().to_string(),
            kernel_hash,
            target_triple: target_triple.to_string(),
        }
    }

    /// Store an artifact, returning its record. `jam` is optional (binary-only artifacts
    /// embed their kernel). Idempotent: a hash already present is a no-op (dedup, DESIGN §3).
    pub fn put(
        &self,
        jam: Option<&[u8]>,
        bin: &[u8],
        target_triple: &str,
    ) -> Result<ArtifactRecord> {
        let record = Self::compute_hashes(jam, bin, target_triple);
        let artifact_hash = record.artifact_hash.clone();

        if self.has(&artifact_hash) {
            return Ok(record);
        }

        let adir = self.artifact_dir(&artifact_hash);
        std::fs::create_dir_all(&adir).with_context(|| format!("creating {}", adir.display()))?;
        if let Some(j) = jam {
            std::fs::write(self.jam_path(&artifact_hash), j).context("writing out.jam")?;
        }

        let bin_path = self.bin_path(&artifact_hash);
        std::fs::write(&bin_path, bin).context("writing binary")?;
        let mut perms = std::fs::metadata(&bin_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perms).context("chmod binary")?;

        Ok(record)
    }

    /// Copy the kernel into an app's state dir as `out.jam` (template apps read it from
    /// cwd). A no-op for binary-only artifacts that embed their kernel.
    pub fn stage_jam(&self, artifact_hash: &str, state_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(state_dir)
            .with_context(|| format!("creating state dir {}", state_dir.display()))?;
        let jam = self.jam_path(artifact_hash);
        if jam.exists() {
            std::fs::copy(jam, state_dir.join("out.jam"))
                .context("staging out.jam into state dir")?;
        }
        Ok(())
    }
}
