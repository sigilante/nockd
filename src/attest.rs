//! Build attestations (DESIGN G5/OQ2) — the "verifiable deploys" trust layer.
//!
//! A signed statement that a specific artifact was built from specific pinned source: the
//! content-addressed hashes, the target/toolchain, and the resolved provenance (including the
//! typhoon dependency closure — the Hoon "pinned source", see DESIGN §7.3). Signed by a
//! builder key (ed25519), which is the seed of the PKI (OQ11) the artifact registry will use.
//!
//! Three verification levels (DESIGN OQ2):
//!   1. signed     — signature valid for the builder key (this module).
//!   2. hash-bound — hashes match the deployed artifact (checked at deploy).
//!   3. reproducible — re-resolve typhoon + recompile + compare kernel_hash (builder/CI-side,
//!                     needs the toolchain — principle 7; a later step).

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::config::now_secs;

pub const SCHEMA: &str = "nockd-attestation/v1";

/// The signed body of an attestation. Serialized canonically (struct field order, via
/// serde_json) to produce the exact bytes that are signed and verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationPayload {
    pub schema: String,
    pub artifact_hash: String,
    pub kernel_hash: String,
    pub target_triple: String,
    /// Free-form provenance (nockapp.toml, git commit, toolchain, resolved typhoon closure).
    /// Captured by the build side; opaque to signing — it's covered by the signature.
    pub provenance: serde_json::Value,
    /// Builder identity = ed25519 public key, hex.
    pub builder: String,
    pub signed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attestation {
    pub payload: AttestationPayload,
    /// ed25519 signature over the canonical bytes of `payload`, hex.
    pub signature: String,
}

/// Canonical bytes of a payload (deterministic — serde_json honors struct field order).
fn canonical(payload: &AttestationPayload) -> Result<Vec<u8>> {
    serde_json::to_vec(payload).context("serializing attestation payload")
}

/// Create and sign an attestation for an artifact.
pub fn create(
    key: &SigningKey,
    artifact_hash: &str,
    kernel_hash: &str,
    target_triple: &str,
    provenance: serde_json::Value,
) -> Result<Attestation> {
    let payload = AttestationPayload {
        schema: SCHEMA.to_string(),
        artifact_hash: artifact_hash.to_string(),
        kernel_hash: kernel_hash.to_string(),
        target_triple: target_triple.to_string(),
        provenance,
        builder: hex::encode(key.verifying_key().to_bytes()),
        signed_at: now_secs(),
    };
    let sig = key.sign(&canonical(&payload)?);
    Ok(Attestation {
        payload,
        signature: hex::encode(sig.to_bytes()),
    })
}

/// Verify an attestation's signature against the builder key embedded in it. Returns the
/// builder pubkey (hex) on success. (Whether to *trust* that builder is a policy decision
/// made by the caller — e.g. a known-builders allowlist.)
pub fn verify_signature(att: &Attestation) -> Result<String> {
    let pk_bytes: [u8; 32] = hex::decode(&att.payload.builder)
        .ok()
        .and_then(|b| b.try_into().ok())
        .context("invalid builder pubkey")?;
    let vk = VerifyingKey::from_bytes(&pk_bytes).context("bad builder pubkey")?;
    let sig_bytes: [u8; 64] = hex::decode(&att.signature)
        .ok()
        .and_then(|b| b.try_into().ok())
        .context("invalid signature encoding")?;
    let sig = Signature::from_bytes(&sig_bytes);
    vk.verify(&canonical(&att.payload)?, &sig)
        .context("signature verification failed")?;
    Ok(att.payload.builder.clone())
}

/// Does this attestation describe the given artifact (hash-bound, level 2)?
pub fn binds(att: &Attestation, artifact_hash: &str, kernel_hash: &str) -> bool {
    att.payload.artifact_hash == artifact_hash && att.payload.kernel_hash == kernel_hash
}

/// Assess an attestation against a deployed artifact, returning (status, builder):
///   - "verified"   — signature valid, hashes bound, builder trusted.
///   - "unverified" — signature valid + bound, but the builder isn't trusted (record it).
///   - "drift"      — signature invalid (tamper), or the attestation is for a different
///                    artifact (hashes don't bind).
pub fn assess(
    att: &Attestation,
    artifact_hash: &str,
    kernel_hash: &str,
    is_trusted: impl Fn(&str) -> bool,
) -> (String, Option<String>) {
    match verify_signature(att) {
        Ok(builder) => {
            if !binds(att, artifact_hash, kernel_hash) {
                ("drift".to_string(), Some(builder))
            } else if is_trusted(&builder) {
                ("verified".to_string(), Some(builder))
            } else {
                ("unverified".to_string(), Some(builder))
            }
        }
        Err(_) => ("drift".to_string(), None),
    }
}

// ---- Builder key management ----

/// Generate a fresh builder signing key.
pub fn generate_key() -> SigningKey {
    use rand::RngCore;
    let mut seed = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed);
    SigningKey::from_bytes(&seed)
}

/// Load a signing key from a 32-byte seed file.
pub fn load_key(path: &std::path::Path) -> Result<SigningKey> {
    let bytes = std::fs::read(path).with_context(|| format!("reading key {}", path.display()))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("key file is not a 32-byte ed25519 seed"))?;
    Ok(SigningKey::from_bytes(&seed))
}

/// Write a signing key's 32-byte seed to a 0600 file.
pub fn save_key(key: &SigningKey, path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        bail!("key already exists at {} — refusing to overwrite", path.display());
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating key {}", path.display()))?;
    use std::io::Write;
    f.write_all(&key.to_bytes())?;
    Ok(())
}

pub fn pubkey_hex(key: &SigningKey) -> String {
    hex::encode(key.verifying_key().to_bytes())
}
