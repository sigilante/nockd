//! The daemon: owns the registry, artifact store, and supervisor, and runs the reconcile
//! loop plus the HTTP control API + dashboard (DESIGN §5).

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tracing::info;

use crate::config::Paths;
use crate::registry::Registry;
use crate::store::Store;
use crate::supervisor::Supervisor;

pub struct Daemon {
    pub paths: Paths,
    pub registry: Registry,
    pub store: Store,
    pub supervisor: Supervisor,
}

impl Daemon {
    pub fn new(paths: Paths) -> Result<Arc<Self>> {
        let registry = Registry::open(&paths.db)?;
        let store = Store::new(paths.artifacts.clone());
        let supervisor = Supervisor::new(paths.clone());
        Ok(Arc::new(Daemon {
            paths,
            registry,
            store,
            supervisor,
        }))
    }

    /// Reconcile once (used after API mutations for snappy response).
    pub fn reconcile(&self) {
        if let Err(e) = self.supervisor.reconcile(&self.registry, &self.store) {
            tracing::warn!(error = %e, "reconcile failed");
        }
    }
}

pub async fn serve(daemon: Arc<Daemon>, host: IpAddr, port: u16) -> Result<()> {
    // Background reconcile loop (DESIGN §5.1).
    let bg = daemon.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            bg.reconcile();
        }
    });

    let app = crate::api::router(daemon.clone());
    let listener = TcpListener::bind((host, port))
        .await
        .with_context(|| format!("binding {host}:{port}"))?;
    info!("nockd listening on http://{host}:{port}  (dashboard at /)");
    axum::serve(listener, app).await.context("http server")?;
    Ok(())
}
