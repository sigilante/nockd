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
