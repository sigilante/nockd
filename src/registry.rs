//! SQLite registry: desired state + artifacts + event log (DESIGN §6).
//!
//! Phase 0 subset of the schema in DESIGN §6. Observed state (live PIDs, health) is NOT
//! here — it lives in the supervisor and is reconstructed on restart.

use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

use crate::config::now_secs;
use crate::store::ArtifactRecord;

pub struct Registry {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppRow {
    pub name: String,
    pub artifact_hash: String,
    pub kernel_hash: String,
    pub endpoint: Option<String>,
    pub restart_policy: String,
    pub args: Vec<String>,
    pub state_path: String,
    pub desired_status: String, // "running" | "stopped"
    /// App's private/admin gRPC address for health probing (DESIGN §5.3).
    pub admin_addr: Option<String>,
    /// Optional shell command nockd runs periodically; its first stdout line becomes the
    /// app's custom status line (e.g. block height for a nockchain observer).
    pub status_cmd: Option<String>,
    pub status_label: Option<String>,
    /// The port an HTTP NockApp serves on. nockd exports it as `NOCKD_APP_PORT` and substitutes
    /// `{port}` in args (so the app binds the port nockd declares); the dashboard derives an
    /// "open app" relay link to `localhost:<port>`.
    pub port: Option<u16>,
    /// Absolute path to the `nockd.toml` this app was deployed from, if any. Lets the daemon
    /// re-read the manifest and re-apply its declarative config (the dashboard "Reload"
    /// button) without a client-side rebuild. None for flag-based deploys.
    pub manifest_path: Option<String>,
    /// Verification status of the current artifact (verified | unverified | drift).
    pub verified_status: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub ts: i64,
    pub app_name: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointRow {
    pub name: String,
    pub url: String,
    pub kind: String,
    pub created_at: i64,
}

impl Registry {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path).context("opening sqlite registry")?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS artifact (
                hash            TEXT PRIMARY KEY,
                kernel_hash     TEXT NOT NULL,
                target_triple   TEXT NOT NULL,
                created_at      INTEGER NOT NULL,
                provenance      TEXT,
                verified_status TEXT,   -- verified | unverified | drift
                builder         TEXT    -- attesting builder pubkey, if any
            );
            CREATE TABLE IF NOT EXISTS trusted_builder (
                pubkey   TEXT PRIMARY KEY,
                added_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS app (
                name           TEXT PRIMARY KEY,
                artifact_hash  TEXT NOT NULL,
                endpoint       TEXT,
                restart_policy TEXT NOT NULL,
                args           TEXT NOT NULL,          -- JSON array
                state_path     TEXT NOT NULL,
                desired_status TEXT NOT NULL,
                admin_addr     TEXT,
                status_cmd     TEXT,
                status_label   TEXT,
                port           INTEGER,
                manifest_path  TEXT,
                created_at     INTEGER NOT NULL,
                updated_at     INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS event (
                id       INTEGER PRIMARY KEY AUTOINCREMENT,
                ts       INTEGER NOT NULL,
                app_name TEXT NOT NULL,
                kind     TEXT NOT NULL,
                detail   TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS endpoint (
                name       TEXT PRIMARY KEY,
                url        TEXT NOT NULL,
                kind       TEXT NOT NULL,          -- remote | local-socket
                created_at INTEGER NOT NULL
            );
            "#,
        )
        .context("initializing schema")?;
        // Tolerate older DBs created before these columns existed.
        for ddl in [
            "ALTER TABLE app ADD COLUMN admin_addr TEXT",
            "ALTER TABLE app ADD COLUMN status_cmd TEXT",
            "ALTER TABLE app ADD COLUMN status_label TEXT",
            "ALTER TABLE app ADD COLUMN port INTEGER",
            "ALTER TABLE app ADD COLUMN manifest_path TEXT",
            "ALTER TABLE artifact ADD COLUMN verified_status TEXT",
            "ALTER TABLE artifact ADD COLUMN builder TEXT",
        ] {
            let _ = conn.execute(ddl, []);
        }
        Ok(Registry { conn: Mutex::new(conn) })
    }

    pub fn put_artifact(&self, rec: &ArtifactRecord, provenance: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO artifact (hash, kernel_hash, target_triple, created_at, provenance)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                rec.artifact_hash,
                rec.kernel_hash,
                rec.target_triple,
                now_secs(),
                provenance,
            ],
        )?;
        Ok(())
    }

    /// Insert or update an app, setting its desired status to "running".
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_app(
        &self,
        name: &str,
        artifact_hash: &str,
        endpoint: Option<&str>,
        restart_policy: &str,
        args: &[String],
        state_path: &str,
        admin_addr: Option<&str>,
        status_cmd: Option<&str>,
        status_label: Option<&str>,
        port: Option<u16>,
        manifest_path: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = now_secs();
        let args_json = serde_json::to_string(args)?;
        conn.execute(
            "INSERT INTO app (name, artifact_hash, endpoint, restart_policy, args, state_path, desired_status, admin_addr, status_cmd, status_label, port, manifest_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7, ?8, ?9, ?11, ?12, ?10, ?10)
             ON CONFLICT(name) DO UPDATE SET
                artifact_hash=?2, endpoint=?3, restart_policy=?4, args=?5,
                state_path=?6, desired_status='running', admin_addr=?7,
                status_cmd=?8, status_label=?9, port=?11, manifest_path=?12, updated_at=?10",
            rusqlite::params![name, artifact_hash, endpoint, restart_policy, args_json, state_path, admin_addr, status_cmd, status_label, now, port, manifest_path],
        )?;
        Ok(())
    }

    /// Re-apply declarative config to an existing app WITHOUT touching its artifact (the
    /// "Reload" path: re-read nockd.toml and update runtime config in place). Leaves
    /// artifact_hash, state_path, and desired_status untouched. Returns false if no such app.
    #[allow(clippy::too_many_arguments)]
    pub fn update_app_config(
        &self,
        name: &str,
        endpoint: Option<&str>,
        restart_policy: &str,
        args: &[String],
        admin_addr: Option<&str>,
        status_cmd: Option<&str>,
        status_label: Option<&str>,
        port: Option<u16>,
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let args_json = serde_json::to_string(args)?;
        let n = conn.execute(
            "UPDATE app SET endpoint=?2, restart_policy=?3, args=?4, admin_addr=?5,
                status_cmd=?6, status_label=?7, port=?8, updated_at=?9 WHERE name=?1",
            rusqlite::params![name, endpoint, restart_policy, args_json, admin_addr, status_cmd, status_label, port, now_secs()],
        )?;
        Ok(n > 0)
    }

    pub fn set_desired(&self, name: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE app SET desired_status=?2, updated_at=?3 WHERE name=?1",
            rusqlite::params![name, status, now_secs()],
        )?;
        Ok(())
    }

    pub fn get_app(&self, name: &str) -> Result<Option<AppRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT a.name, a.artifact_hash, ar.kernel_hash, a.endpoint, a.restart_policy,
                    a.args, a.state_path, a.desired_status, a.created_at, a.updated_at, a.admin_addr, a.status_cmd, a.status_label, ar.verified_status, a.port, a.manifest_path
             FROM app a LEFT JOIN artifact ar ON ar.hash = a.artifact_hash
             WHERE a.name = ?1",
        )?;
        let row = stmt
            .query_row([name], Self::map_app_row)
            .ok();
        Ok(row)
    }

    pub fn list_apps(&self) -> Result<Vec<AppRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT a.name, a.artifact_hash, ar.kernel_hash, a.endpoint, a.restart_policy,
                    a.args, a.state_path, a.desired_status, a.created_at, a.updated_at, a.admin_addr, a.status_cmd, a.status_label, ar.verified_status, a.port, a.manifest_path
             FROM app a LEFT JOIN artifact ar ON ar.hash = a.artifact_hash
             ORDER BY a.name",
        )?;
        let rows = stmt
            .query_map([], Self::map_app_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn map_app_row(row: &rusqlite::Row) -> rusqlite::Result<AppRow> {
        let args_json: String = row.get(5)?;
        let args: Vec<String> = serde_json::from_str(&args_json).unwrap_or_default();
        Ok(AppRow {
            name: row.get(0)?,
            artifact_hash: row.get(1)?,
            kernel_hash: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            endpoint: row.get(3)?,
            restart_policy: row.get(4)?,
            args,
            state_path: row.get(6)?,
            desired_status: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            admin_addr: row.get(10)?,
            status_cmd: row.get(11)?,
            status_label: row.get(12)?,
            verified_status: row.get(13)?,
            port: row.get(14)?,
            manifest_path: row.get(15)?,
        })
    }

    /// Record the verification result for an artifact (set at deploy time). Verified is
    /// sticky: a content-addressed artifact that already has a valid attestation stays
    /// verified even if a later deploy of the same bytes attaches no/another attestation.
    pub fn set_artifact_verification(
        &self,
        hash: &str,
        status: &str,
        builder: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE artifact SET verified_status=?2, builder=?3
             WHERE hash=?1 AND NOT (COALESCE(verified_status,'')='verified' AND ?2 != 'verified')",
            rusqlite::params![hash, status, builder],
        )?;
        Ok(())
    }

    pub fn add_trusted_builder(&self, pubkey: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO trusted_builder (pubkey, added_at) VALUES (?1, ?2)",
            rusqlite::params![pubkey, now_secs()],
        )?;
        Ok(())
    }

    pub fn remove_trusted_builder(&self, pubkey: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM trusted_builder WHERE pubkey=?1", [pubkey])?)
    }

    pub fn list_trusted_builders(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT pubkey FROM trusted_builder ORDER BY added_at")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn is_trusted_builder(&self, pubkey: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT 1 FROM trusted_builder WHERE pubkey=?1",
            [pubkey],
            |_| Ok(()),
        )
        .is_ok()
    }

    pub fn add_event(&self, app_name: &str, kind: &str, detail: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO event (ts, app_name, kind, detail) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![now_secs(), app_name, kind, detail],
        )?;
        Ok(())
    }

    pub fn list_events(&self, limit: i64) -> Result<Vec<EventRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, app_name, kind, detail FROM event ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], Self::map_event_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Events for one app, newest first.
    pub fn list_app_events(&self, app_name: &str, limit: i64) -> Result<Vec<EventRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, app_name, kind, detail FROM event
             WHERE app_name = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![app_name, limit], Self::map_event_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Events with id greater than `after`, oldest first (for SSE tailing).
    pub fn events_since(&self, after: i64, limit: i64) -> Result<Vec<EventRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, app_name, kind, detail FROM event
             WHERE id > ?1 ORDER BY id ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![after, limit], Self::map_event_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    fn map_event_row(row: &rusqlite::Row) -> rusqlite::Result<EventRow> {
        Ok(EventRow {
            id: row.get(0)?,
            ts: row.get(1)?,
            app_name: row.get(2)?,
            kind: row.get(3)?,
            detail: row.get(4)?,
        })
    }

    pub fn add_endpoint(&self, name: &str, url: &str, kind: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO endpoint (name, url, kind, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name) DO UPDATE SET url=?2, kind=?3",
            rusqlite::params![name, url, kind, now_secs()],
        )?;
        Ok(())
    }

    pub fn remove_endpoint(&self, name: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM endpoint WHERE name=?1", [name])?)
    }

    pub fn get_endpoint(&self, name: &str) -> Result<Option<EndpointRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT name, url, kind, created_at FROM endpoint WHERE name = ?1")?;
        let row = stmt
            .query_row([name], |row| {
                Ok(EndpointRow {
                    name: row.get(0)?,
                    url: row.get(1)?,
                    kind: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .ok();
        Ok(row)
    }

    pub fn list_endpoints(&self) -> Result<Vec<EndpointRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT name, url, kind, created_at FROM endpoint ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            Ok(EndpointRow {
                name: row.get(0)?,
                url: row.get(1)?,
                kind: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
}
