//! Special-cased support for Nockchain (the premier NockApp): a minimal hand-written gRPC
//! client for the public `NockchainMetricsService`, so endpoint tiles can show real chain
//! height. We avoid vendoring the full proto tree (and the protoc/tonic-build toolchain) by
//! declaring only the messages we read as prost structs — prost skips unknown fields on
//! decode, and a oneof arm on the wire is just a plain optional message field by its tag.

use prost::Message;
use tonic::transport::Channel;

#[derive(Clone, PartialEq, Message)]
pub struct GetExplorerMetricsRequest {}

#[derive(Clone, PartialEq, Message)]
pub struct ExplorerMetrics {
    #[prost(uint64, tag = "1")]
    pub cache_height: u64,
    #[prost(uint64, tag = "2")]
    pub heaviest_height: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetExplorerMetricsResponse {
    // The real response is `oneof result { ExplorerMetrics metrics = 1; ErrorStatus error = 2 }`;
    // a oneof message arm is wire-identical to a plain optional message at the same tag, so we
    // decode field 1 directly and simply ignore the error arm (field 2).
    #[prost(message, optional, tag = "1")]
    pub metrics: Option<ExplorerMetrics>,
}

/// Query `nockchain.public.v2.NockchainMetricsService/GetExplorerMetrics` on an established
/// gRPC channel. Returns `Some(heaviest_height)` if the call succeeded (which also proves the
/// endpoint is a reachable Nockchain v2 gRPC server) — the height may be 0 if the explorer
/// cache is still cold. Returns `None` only if the call itself failed.
pub async fn explorer_height(channel: Channel) -> Option<u64> {
    let mut grpc = tonic::client::Grpc::new(channel);
    grpc.ready().await.ok()?;
    let codec: tonic::codec::ProstCodec<GetExplorerMetricsRequest, GetExplorerMetricsResponse> =
        tonic::codec::ProstCodec::default();
    let path = tonic::codegen::http::uri::PathAndQuery::from_static(
        "/nockchain.public.v2.NockchainMetricsService/GetExplorerMetrics",
    );
    let resp = grpc
        .unary(
            tonic::Request::new(GetExplorerMetricsRequest {}),
            path,
            codec,
        )
        .await
        .ok()?;
    Some(resp.into_inner().metrics?.heaviest_height)
}
