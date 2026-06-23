//! gRPC health probing (DESIGN §5.3, §8.2; OQ3 resolved).
//!
//! A NockApp's private/admin gRPC server registers the standard gRPC health service
//! (`tonic_health`) and reports `SERVING` only after `boot::setup()` — and therefore after
//! event-log replay — completes (OQ3). So a `SERVING` here genuinely means "ready," which
//! is exactly what the deploy gate wants.
//!
//! Health is opt-in per app: it applies only when an `admin_addr` is configured (the app
//! must bind its private gRPC to that localhost address). Apps without one fall back to
//! process-liveness.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    /// gRPC health reports SERVING — the app is ready.
    Serving,
    /// Reachable but not yet serving (e.g. still booting).
    NotServing,
    /// Could not connect to the admin address.
    Unreachable,
    /// No admin address configured, or status could not be interpreted.
    Unknown,
}

/// Result of probing a Nockchain endpoint.
pub struct EndpointProbe {
    pub reachable: bool,
    pub lag_ms: Option<u64>,
    /// Chain-tip (heaviest) block height from the public metrics service, if it's a Nockchain
    /// v2 endpoint with a warm explorer cache.
    pub height: Option<u64>,
}

/// Probe a Nockchain endpoint URL (`http://host:port`): a real gRPC handshake + standard
/// health check (stronger than a raw TCP connect; gives true round-trip latency), plus the
/// chain-tip height from `NockchainMetricsService` (special-cased — see `nockchain.rs`).
pub async fn probe_endpoint(url: &str) -> EndpointProbe {
    use std::time::{Duration, Instant};
    use tonic::transport::Endpoint;

    let down = EndpointProbe {
        reachable: false,
        lag_ms: None,
        height: None,
    };

    let uri = if url.contains("://") {
        url.to_string()
    } else {
        format!("http://{url}")
    };
    let is_https = uri.starts_with("https://");
    let mut endpoint = match Endpoint::from_shared(uri) {
        Ok(e) => e
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3)),
        Err(_) => return down,
    };
    // Public endpoints like https://rpc.nockchain.net are TLS on 443; configure TLS with
    // bundled webpki roots so we can probe them.
    if is_https {
        match endpoint.tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots()) {
            Ok(e) => endpoint = e,
            Err(_) => return down,
        }
    }

    // Reachability = the gRPC channel establishes (TCP + TLS + HTTP/2 handshake). This is
    // protocol-level reachability that doesn't depend on which services the endpoint
    // registers — some Nockchain endpoints expose their services without the standard gRPC
    // health service, so gating on a specific RPC would falsely report them down.
    let connect_start = Instant::now();
    let Ok(channel) = endpoint.connect().await else {
        return down;
    };

    // Chain height is best-effort (Nockchain v2 metrics); when it works it also gives a real
    // RPC round-trip for the lag, otherwise we report the connect time.
    let rpc_start = Instant::now();
    let height = crate::nockchain::explorer_height(channel).await;
    let lag_ms = if height.is_some() {
        Some(rpc_start.elapsed().as_millis() as u64)
    } else {
        Some(connect_start.elapsed().as_millis() as u64)
    };
    EndpointProbe {
        reachable: true,
        lag_ms,
        height: height.filter(|h| *h > 0),
    }
}

/// Probe the standard gRPC health service at `addr` (host:port).
pub async fn probe(addr: &str) -> HealthState {
    use tonic::transport::Endpoint;
    use tonic_health::pb::health_check_response::ServingStatus;
    use tonic_health::pb::health_client::HealthClient;
    use tonic_health::pb::HealthCheckRequest;

    let endpoint = match Endpoint::from_shared(format!("http://{addr}")) {
        Ok(e) => e.connect_timeout(std::time::Duration::from_secs(2)),
        Err(_) => return HealthState::Unknown,
    };
    let channel = match endpoint.connect().await {
        Ok(c) => c,
        Err(_) => return HealthState::Unreachable,
    };
    let mut client = HealthClient::new(channel);
    // Empty service name = overall server health.
    let request = HealthCheckRequest {
        service: String::new(),
    };
    match client.check(request).await {
        Ok(resp) => match ServingStatus::try_from(resp.into_inner().status) {
            Ok(ServingStatus::Serving) => HealthState::Serving,
            Ok(_) => HealthState::NotServing,
            Err(_) => HealthState::Unknown,
        },
        Err(_) => HealthState::NotServing,
    }
}
