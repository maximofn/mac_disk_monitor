pub mod routes;
pub mod sse;

use std::time::Instant;

use axum::Router;
use mac_disk_monitor_core::Snapshot;
use tokio::sync::watch;
use tower_http::trace::TraceLayer;

use crate::largest_files::RescanTrigger;

#[derive(Clone)]
pub struct AppState {
    pub started_at: Instant,
    pub snapshot_rx: watch::Receiver<Snapshot>,
    pub kernel: Option<String>,
    /// `None` when the scanner is disabled (`--no-largest-files`); rescan
    /// endpoints respond 503 in that case.
    pub rescan_tx: Option<RescanTrigger>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", axum::routing::get(routes::healthz))
        .route("/v1/info", axum::routing::get(routes::info))
        .route("/v1/snapshot", axum::routing::get(routes::snapshot))
        .route("/v1/mounts", axum::routing::get(routes::mounts))
        .route("/v1/mounts/*path", axum::routing::get(routes::mount))
        .route("/v1/rescan", axum::routing::post(routes::rescan_all))
        .route("/v1/rescan/*path", axum::routing::post(routes::rescan_one))
        .route("/v1/stream", axum::routing::get(sse::stream))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
