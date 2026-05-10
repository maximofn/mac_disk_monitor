use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use mac_disk_monitor_core::{Mount, Snapshot};
use serde::Serialize;

use super::AppState;
use crate::largest_files::RescanRequest;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_s: u64,
}

pub async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        uptime_s: state.started_at.elapsed().as_secs(),
    })
}

#[derive(Serialize)]
pub struct InfoResponse {
    pub backend_version: &'static str,
    pub api_version: &'static str,
    pub host: String,
    pub kernel: Option<String>,
    pub mount_count: usize,
}

pub async fn info(State(state): State<AppState>) -> Json<InfoResponse> {
    let snap = state.snapshot_rx.borrow();
    Json(InfoResponse {
        backend_version: env!("CARGO_PKG_VERSION"),
        api_version: mac_disk_monitor_core::API_VERSION,
        host: snap.host.clone(),
        kernel: state.kernel.clone(),
        mount_count: snap.mounts.len(),
    })
}

pub async fn snapshot(State(state): State<AppState>) -> Json<Snapshot> {
    Json(state.snapshot_rx.borrow().clone())
}

#[derive(Serialize)]
pub struct MountSummary {
    pub mount_point: String,
    pub device: String,
    pub fs_type: String,
    pub total_bytes: u64,
}

pub async fn mounts(State(state): State<AppState>) -> Json<Vec<MountSummary>> {
    let snap = state.snapshot_rx.borrow();
    Json(
        snap.mounts
            .iter()
            .map(|m| MountSummary {
                mount_point: m.mount_point.clone(),
                device: m.device.clone(),
                fs_type: m.fs_type.clone(),
                total_bytes: m.usage.total_bytes,
            })
            .collect(),
    )
}

/// `path` is the URL-encoded mount point with the leading slash already
/// stripped by axum (e.g. `/v1/mounts/Volumes/External` matches `/Volumes/External`).
pub async fn mount(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Json<Mount>, StatusCode> {
    let needle = format!("/{}", path.trim_start_matches('/'));
    let snap = state.snapshot_rx.borrow();
    snap.mounts
        .iter()
        .find(|m| m.mount_point == needle || m.mount_point.trim_start_matches('/') == path)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Serialize)]
pub struct RescanResponse {
    pub queued: bool,
    pub target: String,
}

pub async fn rescan_all(State(state): State<AppState>) -> Result<Json<RescanResponse>, StatusCode> {
    let tx = state.rescan_tx.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    tx.send(RescanRequest::All)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(RescanResponse {
        queued: true,
        target: "*".into(),
    }))
}

pub async fn rescan_one(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Json<RescanResponse>, StatusCode> {
    let tx = state.rescan_tx.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let needle = format!("/{}", path.trim_start_matches('/'));
    let known = state
        .snapshot_rx
        .borrow()
        .mounts
        .iter()
        .any(|m| m.mount_point == needle);
    if !known {
        return Err(StatusCode::NOT_FOUND);
    }
    tx.send(RescanRequest::One(needle.clone()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(RescanResponse {
        queued: true,
        target: needle,
    }))
}
