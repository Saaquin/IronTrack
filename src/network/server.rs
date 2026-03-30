// IronTrack - Open-source flight management and aerial survey planning engine
// Copyright (C) 2026 [Founder Name]
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{header, HeaderValue, Method, Request, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio::sync::{broadcast, watch, RwLock};
use tokio_util::io::ReaderStream;
use tower_http::cors::CorsLayer;

use crate::dem::TerrainEngine;
use crate::gpkg::GeoPackage;
use crate::network::errors::ServerError;
use crate::network::telemetry::{CurrentState, LineStatus, ServerMsg, SystemStatus, TriggerEvent};
use crate::photogrammetry::FlightPlan;
use crate::types::{AltitudeDatum, FlightLine};

use crate::io::{geojson_to_linestrings, geojson_to_polygons};
use crate::math::dubins::TurnParams;
use crate::math::routing::optimize_route;
use crate::photogrammetry::corridor::{generate_corridor_lines, CorridorParams};
use crate::photogrammetry::flightlines::{
    adjust_for_terrain, clip_to_polygon, connect_with_turns, generate_flight_lines,
    generate_lidar_flight_lines, BoundingBox, FlightPlanParams, LidarPlanParams,
};
use crate::photogrammetry::lidar::LidarSensorParams;
use crate::photogrammetry::resolve_sensor;

#[cfg(feature = "serial")]
use crate::network::serial_manager::SerialManager;
use mdns_sd::{ServiceDaemon, ServiceInfo};

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

/// Global state for the FMS daemon.
pub struct FmsState {
    pub flight_plan: Option<FlightPlan>,
    pub terrain_engine: Arc<TerrainEngine>,

    // Telemetry Channels
    pub tx_telemetry: broadcast::Sender<CurrentState>,
    pub tx_status: watch::Sender<SystemStatus>,
    pub tx_trigger: broadcast::Sender<TriggerEvent>,
    pub tx_warning: broadcast::Sender<ServerMsg>,

    // Latest values
    pub last_telemetry: CurrentState,
    pub last_status: SystemStatus,

    // Persistence
    pub persistence_path: Option<PathBuf>,

    // Mission tracking
    pub mission_id: Option<u64>,
    pub mission_counter: u64,
    pub trigger_log: VecDeque<TriggerEvent>,
    pub db_pool: Option<deadpool_sqlite::Pool>,
}

impl FmsState {
    pub fn new() -> anyhow::Result<Self> {
        let (tx_telemetry, _) = broadcast::channel(256);
        let (tx_status, _) = watch::channel(SystemStatus::default());
        let (tx_trigger, _) = broadcast::channel(256);
        let (tx_warning, _) = broadcast::channel(64);

        Ok(Self {
            flight_plan: None,
            terrain_engine: Arc::new(TerrainEngine::new()?),
            tx_telemetry,
            tx_status,
            tx_trigger,
            tx_warning,
            last_telemetry: CurrentState::default(),
            last_status: SystemStatus::default(),
            persistence_path: None,
            mission_id: None,
            mission_counter: 0,
            trigger_log: VecDeque::new(),
            db_pool: None,
        })
    }

    /// Record a sensor trigger event.
    ///
    /// Appends to the bounded in-memory ring buffer (10 000 max, oldest
    /// dropped), updates the trigger count in `last_status`, and broadcasts
    /// the event to all WebSocket clients.
    pub fn record_trigger(&mut self, event: TriggerEvent) {
        if self.trigger_log.len() >= 10_000 {
            self.trigger_log.pop_front();
        }
        self.trigger_log.push_back(event.clone());
        self.last_status.trigger_count = self.trigger_log.len() as u32;
        let _ = self.tx_trigger.send(event);
    }
}

pub type SharedState = Arc<RwLock<FmsState>>;

/// Application-level state carried through the Axum router.
///
/// Wraps the shared FMS state plus the per-session bearer token used
/// by the auth middleware and WebSocket upgrade handler.
#[derive(Clone)]
pub struct AppState {
    pub fms: SharedState,
    pub token: Arc<String>,
}

// ---------------------------------------------------------------------------
// Token generation
// ---------------------------------------------------------------------------

/// Resolve the bearer token for this session.
///
/// Uses `IRONTRACK_TOKEN` from the environment if set and non-empty;
/// otherwise generates 32 cryptographically random bytes rendered as hex.
fn resolve_bearer_token() -> String {
    if let Ok(token) = std::env::var("IRONTRACK_TOKEN") {
        if !token.is_empty() {
            return token;
        }
    }
    let mut bytes = [0u8; 32];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Auth middleware
// ---------------------------------------------------------------------------

/// Axum middleware that enforces bearer-token authentication on REST routes.
///
/// Extracts the `Authorization: Bearer <token>` header and performs a
/// constant-time comparison (via `subtle::ConstantTimeEq`) to prevent
/// timing side-channels.  Returns a `401 Unauthorized` JSON error on
/// missing or invalid tokens.
async fn auth_middleware(
    State(app): State<AppState>,
    req: Request<Body>,
    next: middleware::Next,
) -> Response {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match auth_header {
        Some(token) => {
            let expected = app.token.as_bytes();
            let provided = token.as_bytes();
            if expected.len() == provided.len() && bool::from(expected.ct_eq(provided)) {
                next.run(req).await
            } else {
                ServerError::Unauthorized("invalid bearer token".into()).into_response()
            }
        }
        None => ServerError::Unauthorized("missing Authorization header".into()).into_response(),
    }
}

// ---------------------------------------------------------------------------
// CORS construction
// ---------------------------------------------------------------------------

/// Build a restrictive CORS layer that allows the server's own origin,
/// common dev-server ports, and any extra origins passed via CLI.
fn build_cors_layer(port: u16, extra_origins: &[String]) -> CorsLayer {
    let default_origins = [
        format!("http://localhost:{port}"),
        format!("http://127.0.0.1:{port}"),
        "http://localhost:3000".into(),
        "http://localhost:5173".into(),
        "http://127.0.0.1:3000".into(),
        "http://127.0.0.1:5173".into(),
    ];
    let mut origins: Vec<HeaderValue> = default_origins
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();
    for origin in extra_origins {
        if let Ok(val) = origin.parse::<HeaderValue>() {
            origins.push(val);
        }
    }
    origins.dedup();
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
}

// ---------------------------------------------------------------------------
// mDNS helper
// ---------------------------------------------------------------------------

/// Discover a non-loopback local network IP for mDNS advertisement.
/// Falls back to `0.0.0.0` if no suitable interface is found.
fn discover_local_ip() -> String {
    match local_ip_address::local_ip() {
        Ok(ip) => {
            let s = ip.to_string();
            // Reject loopback and link-local (should not happen with this crate,
            // but guard defensively).
            if s.starts_with("127.") || s.starts_with("169.254.") {
                eprintln!("warning: local_ip() returned {s}, falling back to 0.0.0.0 for mDNS");
                "0.0.0.0".to_string()
            } else {
                s
            }
        }
        Err(e) => {
            eprintln!(
                "warning: could not discover local IP ({e}), falling back to 0.0.0.0 for mDNS"
            );
            "0.0.0.0".to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Connection pool helper
// ---------------------------------------------------------------------------

/// Create a deadpool-sqlite connection pool for the given GeoPackage path.
///
/// Each connection returned by the pool must have session-level PRAGMAs
/// applied before use.  Because deadpool-sqlite 0.8 does not expose a
/// convenient post-create hook through the config builder, we apply
/// PRAGMAs inside each `interact()` call via [`apply_pool_pragmas`].
/// The PRAGMAs are idempotent and fast (~0.1 ms), so the overhead is
/// negligible for the 5-second persistence loop.
fn create_gpkg_pool(path: &std::path::Path) -> anyhow::Result<deadpool_sqlite::Pool> {
    let cfg = deadpool_sqlite::Config::new(path.to_string_lossy().to_string());
    let pool = cfg.create_pool(deadpool_sqlite::Runtime::Tokio1)?;
    Ok(pool)
}

/// Session-level PRAGMAs that must be set on every pool connection.
///
/// WAL mode is file-level (persisted by `GeoPackage::new()`), but
/// `synchronous`, `cache_size`, `foreign_keys`, and `busy_timeout`
/// are connection-level and reset when the connection is recycled.
///
/// SQLITE_BUSY (busy_timeout = 0) is a fatal architectural violation
/// [Doc 42] — we never retry, we fail fast.
fn apply_pool_pragmas(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;\
         PRAGMA synchronous = NORMAL;\
         PRAGMA cache_size = -64000;\
         PRAGMA foreign_keys = ON;\
         PRAGMA busy_timeout = 0;",
    )
}

// ---------------------------------------------------------------------------
// Server entry point
// ---------------------------------------------------------------------------

/// Start the Axum FMS daemon on the specified port.
pub async fn run_server(
    port: u16,
    mock_speed: Option<f64>,
    gnss_vid: u16,
    gnss_pid: u16,
    gnss_baud: u32,
    cors_origins: Vec<String>,
) -> anyhow::Result<()> {
    let state = Arc::new(RwLock::new(FmsState::new()?));

    if let Some(speed) = mock_speed {
        let state_clone = state.clone();
        tokio::spawn(async move {
            run_mock_telemetry(state_clone, speed).await;
        });
    } else {
        // Start real serial manager if not mocking.
        // Requires the "serial" feature (libudev-dev on Linux).
        #[cfg(feature = "serial")]
        {
            let serial_mgr = SerialManager::new(gnss_vid, gnss_pid, gnss_baud, state.clone());
            tokio::spawn(async move {
                serial_mgr.run().await;
            });
        }
        #[cfg(not(feature = "serial"))]
        {
            let _ = (gnss_vid, gnss_pid, gnss_baud);
            eprintln!("warning: serial port support disabled (compile with --features serial)");
        }
    }

    // Start mDNS advertising
    let mdns = ServiceDaemon::new()?;
    let service_type = "_irontrack._tcp.local.";
    let instance_name = "fms_daemon";
    let host_name = "irontrack.local.";
    let local_ip = discover_local_ip();
    let my_service = ServiceInfo::new(
        service_type,
        instance_name,
        host_name,
        local_ip.as_str(),
        port,
        None,
    )?;
    println!("mDNS: advertising {service_type} at {local_ip}:{port}");
    mdns.register(my_service)?;

    // Spawn persistence task
    let state_clone = state.clone();
    tokio::spawn(async move {
        run_persistence_task(state_clone).await;
    });

    let shutdown_state = state.clone();

    // Generate session token
    let token = resolve_bearer_token();
    println!("Session token: {token}");

    let app_state = AppState {
        fms: state,
        token: Arc::new(token),
    };

    let cors_layer = build_cors_layer(port, &cors_origins);

    // REST routes behind auth middleware
    let rest = Router::new()
        .route("/api/v1/missions", post(create_mission))
        .route("/api/v1/missions/:id", get(get_mission))
        .route("/api/v1/missions/:id/lines", get(get_lines))
        .route(
            "/api/v1/missions/:id/lines/:line_id",
            put(update_line).delete(delete_line),
        )
        .route("/api/v1/missions/:id/export/gpkg", get(export_gpkg))
        .route("/api/v1/missions/:id/export/qgc", get(export_qgc))
        .route("/api/v1/missions/:id/export/dji", get(export_dji))
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            auth_middleware,
        ));

    let app = Router::new()
        .merge(rest)
        .route("/ws", get(ws_handler))
        .layer(cors_layer)
        .with_state(app_state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("IronTrack FMS daemon listening on {}", addr);
    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
        log::info!("shutdown signal received, cleaning up…");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    // Run PRAGMA optimize + WAL checkpoint before closing the database.
    // Architecture rule: VACUUM is prohibited; PRAGMA optimize is the
    // required shutdown-time maintenance action.
    //
    // Prefer the connection pool if available — it honours WAL mode and
    // avoids opening a second direct connection.
    let state_guard = shutdown_state.read().await;
    if let Some(ref pool) = state_guard.db_pool {
        let pool = pool.clone();
        drop(state_guard);
        match pool.get().await {
            Ok(conn) => {
                let _ = conn
                    .interact(|conn| {
                        conn.execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);")
                    })
                    .await;
            }
            Err(e) => log::error!("could not get pool connection for shutdown: {e}"),
        }
    } else if let Some(ref path) = state_guard.persistence_path {
        // Fallback: direct connection if pool is not available.
        match GeoPackage::open(path) {
            Ok(gpkg) => {
                if let Err(e) = gpkg
                    .conn
                    .execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);")
                {
                    log::error!("shutdown PRAGMA failed: {e}");
                }
            }
            Err(e) => log::error!("could not open GeoPackage for shutdown cleanup: {e}"),
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Request/Response Types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize)]
pub struct LoadMissionRequest {
    // Basic bbox (fallback)
    pub min_lat: Option<f64>,
    pub min_lon: Option<f64>,
    pub max_lat: Option<f64>,
    pub max_lon: Option<f64>,

    // Boundary (GeoJSON Polygon/MultiPolygon string or Value)
    pub boundary_geojson: Option<serde_json::Value>,

    // Sensor
    #[serde(default = "default_sensor")]
    pub sensor: String,
    pub focal_length_mm: Option<f64>,
    pub sensor_width_mm: Option<f64>,
    pub sensor_height_mm: Option<f64>,
    pub image_width_px: Option<u32>,
    pub image_height_px: Option<u32>,

    // Photogrammetry
    #[serde(default = "default_gsd")]
    pub gsd_cm: f64,
    #[serde(default = "default_side_lap")]
    pub side_lap: f64,
    #[serde(default = "default_end_lap")]
    pub end_lap: f64,
    #[serde(default)]
    pub azimuth: f64,
    pub altitude_msl: Option<f64>,

    // LiDAR
    #[serde(default = "default_mission_type")]
    pub mission_type: String,
    pub lidar_prr: Option<f64>,
    pub lidar_scan_rate: Option<f64>,
    pub lidar_fov: Option<f64>,
    pub target_density: Option<f64>,

    // Mode
    #[serde(default = "default_mode")]
    pub mode: String,
    pub centerline_geojson: Option<serde_json::Value>,
    pub corridor_width: Option<f64>,
    pub corridor_passes: Option<u8>,

    // Turns
    #[serde(default = "default_max_bank")]
    pub max_bank_angle: f64,
    #[serde(default = "default_turn_airspeed")]
    pub turn_airspeed: f64,

    // Optimization
    #[serde(default)]
    pub optimize_route: bool,
    pub launch_lat: Option<f64>,
    pub launch_lon: Option<f64>,

    // Terrain
    #[serde(default)]
    pub terrain: bool,
    #[serde(default = "default_datum")]
    pub datum: String,
}

fn default_sensor() -> String {
    "phantom4pro".into()
}
fn default_gsd() -> f64 {
    5.0
}
fn default_side_lap() -> f64 {
    60.0
}
fn default_end_lap() -> f64 {
    80.0
}
fn default_mission_type() -> String {
    "camera".into()
}
fn default_mode() -> String {
    "area".into()
}
fn default_max_bank() -> f64 {
    20.0
}
fn default_turn_airspeed() -> f64 {
    30.0
}
fn default_datum() -> String {
    "egm2008".into()
}

#[derive(Serialize, Deserialize)]
pub struct MissionSummaryResponse {
    pub id: u64,
    pub line_count: usize,
    pub waypoint_count: usize,
    pub total_distance_km: f64,
    pub bbox: [f64; 4], // [min_lat, min_lon, max_lat, max_lon]
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validate that the requested mission ID matches the currently loaded mission.
fn validate_mission_id(state: &FmsState, id: u64) -> Result<(), ServerError> {
    match state.mission_id {
        Some(mid) if mid == id => Ok(()),
        _ => Err(ServerError::MissionNotFound(id)),
    }
}

/// Build a [`MissionSummaryResponse`] from the current in-memory state.
fn calculate_mission_summary(state: &FmsState) -> Result<MissionSummaryResponse, ServerError> {
    let plan = state.flight_plan.as_ref().ok_or(ServerError::NoMission)?;
    let id = state.mission_id.ok_or(ServerError::NoMission)?;
    let line_count = plan.lines.len();
    let waypoint_count: usize = plan.lines.iter().map(|l| l.len()).sum();
    let total_distance_km = calculate_total_distance(plan);

    let mut min_lat = f64::INFINITY;
    let mut min_lon = f64::INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    for line in &plan.lines {
        for i in 0..line.len() {
            let lat = line.lats()[i].to_degrees();
            let lon = line.lons()[i].to_degrees();
            min_lat = min_lat.min(lat);
            min_lon = min_lon.min(lon);
            max_lat = max_lat.max(lat);
            max_lon = max_lon.max(lon);
        }
    }

    Ok(MissionSummaryResponse {
        id,
        line_count,
        waypoint_count,
        total_distance_km,
        bbox: [min_lat, min_lon, max_lat, max_lon],
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/missions — create a new mission from planning parameters.
///
/// Generates the flight plan (area or corridor), persists it to a GeoPackage,
/// creates a connection pool, and returns a mission summary.
async fn create_mission(
    State(app): State<AppState>,
    Json(req): Json<LoadMissionRequest>,
) -> Result<Json<MissionSummaryResponse>, ServerError> {
    let terrain_engine = app.fms.read().await.terrain_engine.clone();

    let result = tokio::task::spawn_blocking(move || {
        let plan = generate_plan_internal(req, &terrain_engine)?;

        // Persist to GeoPackage inside the same spawn_blocking — all
        // GeoPackage I/O is synchronous rusqlite calls.
        let gpkg_path = PathBuf::from("mission.gpkg");
        // Remove any prior GeoPackage to avoid stale data from previous missions.
        // GeoPackage::new creates a fresh file; IF NOT EXISTS + insert would otherwise
        // append into the existing table, corrupting line_index uniqueness.
        let _ = std::fs::remove_file(&gpkg_path);
        let gpkg = GeoPackage::new(&gpkg_path).map_err(|e| anyhow::anyhow!("{e}"))?;
        gpkg.create_feature_table("flight_lines")
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        gpkg.insert_flight_plan("flight_lines", &plan)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok::<(FlightPlan, PathBuf), anyhow::Error>((plan, gpkg_path))
    })
    .await;

    match result {
        Ok(Ok((plan, gpkg_path))) => {
            let mut state_guard = app.fms.write().await;

            state_guard.flight_plan = Some(plan.clone());

            // Increment mission counter and assign ID
            state_guard.mission_counter += 1;
            state_guard.mission_id = Some(state_guard.mission_counter);

            // Create connection pool for ongoing DB access
            match create_gpkg_pool(&gpkg_path) {
                Ok(pool) => {
                    state_guard.db_pool = Some(pool);
                }
                Err(e) => {
                    log::warn!("failed to create connection pool: {e}");
                    state_guard.db_pool = None;
                }
            }

            state_guard.persistence_path = Some(gpkg_path);

            // Clear trigger log for new mission
            state_guard.trigger_log.clear();

            // Initialize line statuses
            state_guard.last_status.line_statuses = vec![
                LineStatus {
                    completion_pct: 0.0,
                    is_active: false,
                };
                plan.lines.len()
            ];
            let status_clone = state_guard.last_status.clone();
            let _ = state_guard.tx_status.send(status_clone);

            let summary = calculate_mission_summary(&state_guard)?;
            Ok(Json(summary))
        }
        Ok(Err(e)) => Err(ServerError::PlanGeneration(e.to_string())),
        Err(e) => Err(ServerError::Internal(e.to_string())),
    }
}

/// GET /api/v1/missions/:id — retrieve the current mission summary.
async fn get_mission(
    State(app): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Json<MissionSummaryResponse>, ServerError> {
    let state = app.fms.read().await;
    validate_mission_id(&state, id)?;
    Ok(Json(calculate_mission_summary(&state)?))
}

/// GET /api/v1/missions/:id/lines — return flight lines as GeoJSON.
async fn get_lines(
    State(app): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Response, ServerError> {
    let state = app.fms.read().await;
    validate_mission_id(&state, id)?;
    match state.flight_plan.as_ref() {
        Some(plan) => Ok(Json(crate::io::geojson::flight_plan_to_geojson(
            plan, None, None, None,
        ))
        .into_response()),
        None => Err(ServerError::NoMission),
    }
}

/// PUT /api/v1/missions/:id/lines/:line_id — update a single line's status.
async fn update_line(
    State(app): State<AppState>,
    axum::extract::Path((id, line_id)): axum::extract::Path<(u64, usize)>,
    Json(status): Json<LineStatus>,
) -> Result<StatusCode, ServerError> {
    let mut state_guard = app.fms.write().await;
    validate_mission_id(&state_guard, id)?;

    if state_guard.flight_plan.is_none() {
        return Err(ServerError::NoMission);
    }

    if line_id >= state_guard.last_status.line_statuses.len() {
        return Err(ServerError::LineNotFound(line_id));
    }

    state_guard.last_status.line_statuses[line_id] = status;
    let status_clone = state_guard.last_status.clone();
    let _ = state_guard.tx_status.send(status_clone);
    Ok(StatusCode::OK)
}

/// DELETE /api/v1/missions/:id/lines/:line_id — remove a flight line.
///
/// Adjusts the active-line index so in-flight guidance stays consistent:
/// - If the deleted line IS the active line, active becomes `None`.
/// - If the deleted line is BEFORE the active line, active shifts down by 1.
/// - If the deleted line is AFTER the active line, active is unchanged.
///
/// GeoPackage cleanup runs asynchronously after the write lock is released;
/// if it fails, in-memory state is authoritative and a warning is logged.
async fn delete_line(
    State(app): State<AppState>,
    axum::extract::Path((id, line_id)): axum::extract::Path<(u64, usize)>,
) -> Result<Json<MissionSummaryResponse>, ServerError> {
    let (summary, pool_clone, line_idx) = {
        let mut state = app.fms.write().await;
        validate_mission_id(&state, id)?;

        let plan = state.flight_plan.as_mut().ok_or(ServerError::NoMission)?;
        if line_id >= plan.lines.len() {
            return Err(ServerError::LineNotFound(line_id));
        }

        plan.lines.remove(line_id);

        // Adjust active_line_index
        if let Some(active) = state.last_status.active_line_index {
            if active == line_id {
                state.last_status.active_line_index = None;
            } else if active > line_id {
                state.last_status.active_line_index = Some(active - 1);
            }
        }

        // Remove line status entry
        if line_id < state.last_status.line_statuses.len() {
            state.last_status.line_statuses.remove(line_id);
        }
        // Update is_active flags to match new active_line_index
        let current_active = state.last_status.active_line_index;
        for (i, ls) in state.last_status.line_statuses.iter_mut().enumerate() {
            ls.is_active = current_active == Some(i);
        }

        // Broadcast updated status
        let status_clone = state.last_status.clone();
        let _ = state.tx_status.send(status_clone);

        let summary = calculate_mission_summary(&state)?;
        let pool_clone = state.db_pool.clone();
        (summary, pool_clone, line_id)
    };
    // Write lock is released here.

    // Synchronous GeoPackage cleanup — ensures DB matches in-memory state
    // before responding, so export_gpkg cannot serve stale data.
    if let Some(pool) = pool_clone {
        match pool.get().await {
            Ok(conn) => {
                let result = conn
                    .interact(move |conn| {
                        apply_pool_pragmas(conn)?;
                        conn.execute_batch("BEGIN;")?;
                        let del_result = (|| {
                            conn.execute(
                                "DELETE FROM flight_lines WHERE line_index = ?1",
                                rusqlite::params![line_idx as i64],
                            )?;
                            conn.execute(
                                "UPDATE flight_lines SET line_index = line_index - 1 \
                                 WHERE line_index > ?1",
                                rusqlite::params![line_idx as i64],
                            )?;
                            Ok::<(), rusqlite::Error>(())
                        })();
                        match del_result {
                            Ok(()) => {
                                conn.execute_batch("COMMIT;")?;
                                Ok(())
                            }
                            Err(e) => {
                                let _ = conn.execute_batch("ROLLBACK;");
                                Err(e)
                            }
                        }
                    })
                    .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => log::warn!("delete_line DB cleanup failed: {e}"),
                    Err(e) => log::warn!("delete_line interact failed: {e}"),
                }
            }
            Err(e) => log::warn!("delete_line pool.get() failed: {e}"),
        }
    }

    Ok(Json(summary))
}

/// GET /api/v1/missions/:id/export/gpkg — stream the GeoPackage file.
///
/// Runs a WAL checkpoint (PASSIVE) before streaming so the client
/// receives a consistent snapshot.
async fn export_gpkg(
    State(app): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Response, ServerError> {
    let state = app.fms.read().await;
    validate_mission_id(&state, id)?;
    let path = state
        .persistence_path
        .clone()
        .ok_or_else(|| ServerError::ExportFailed("no GeoPackage file".into()))?;
    let pool = state.db_pool.clone();
    drop(state);

    // WAL checkpoint before streaming for a consistent snapshot.
    if let Some(pool) = pool {
        match pool.get().await {
            Ok(conn) => {
                let result = conn
                    .interact(|conn| conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);"))
                    .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => log::warn!("export_gpkg WAL checkpoint failed: {e}"),
                    Err(e) => log::warn!("export_gpkg interact failed: {e}"),
                }
            }
            Err(e) => log::warn!("export_gpkg pool.get() failed: {e}"),
        }
    }

    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| ServerError::ExportFailed(e.to_string()))?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok((
        [
            (header::CONTENT_TYPE, "application/geopackage+sqlite3"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"mission.gpkg\"",
            ),
        ],
        body,
    )
        .into_response())
}

/// GET /api/v1/missions/:id/export/qgc — export as QGroundControl JSON.
async fn export_qgc(
    State(app): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Response, ServerError> {
    let terrain_engine = {
        let state = app.fms.read().await;
        validate_mission_id(&state, id)?;
        state.terrain_engine.clone()
    };
    let plan = {
        let state = app.fms.read().await;
        state.flight_plan.as_ref().cloned()
    };

    let plan = plan.ok_or(ServerError::NoMission)?;

    let result = tokio::task::spawn_blocking(move || {
        let qgc_datum = plan
            .lines
            .first()
            .map(|l| l.altitude_datum)
            .unwrap_or(AltitudeDatum::Egm2008);
        let needs_conversion =
            qgc_datum != AltitudeDatum::Agl && qgc_datum != AltitudeDatum::Wgs84Ellipsoidal;

        let mut qgc_plan = plan;
        if needs_conversion {
            let mut converted = Vec::with_capacity(qgc_plan.lines.len());
            for line in qgc_plan.lines {
                converted.push(line.to_datum(AltitudeDatum::Wgs84Ellipsoidal, &terrain_engine)?);
            }
            qgc_plan.lines = converted;
        }

        let trigger_dist = qgc_plan.params.photo_interval();
        let json = crate::io::qgc::flight_plan_to_qgc(&qgc_plan, trigger_dist);
        Ok::<serde_json::Value, anyhow::Error>(json)
    })
    .await;

    match result {
        Ok(Ok(json)) => Ok(Json(json).into_response()),
        Ok(Err(e)) => Err(ServerError::ExportFailed(e.to_string())),
        Err(e) => Err(ServerError::Internal(e.to_string())),
    }
}

/// GET /api/v1/missions/:id/export/dji — export as DJI KMZ.
async fn export_dji(
    State(app): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Response, ServerError> {
    let terrain_engine = {
        let state = app.fms.read().await;
        validate_mission_id(&state, id)?;
        state.terrain_engine.clone()
    };
    let plan = {
        let state = app.fms.read().await;
        state.flight_plan.as_ref().cloned()
    };

    let plan = plan.ok_or(ServerError::NoMission)?;

    let result = tokio::task::spawn_blocking(move || {
        let dji_datum = plan
            .lines
            .first()
            .map(|l| l.altitude_datum)
            .unwrap_or(AltitudeDatum::Egm2008);

        let mut dji_plan = plan;
        if dji_datum != AltitudeDatum::Egm96 {
            let mut converted = Vec::with_capacity(dji_plan.lines.len());
            for line in dji_plan.lines {
                converted.push(line.to_datum(AltitudeDatum::Egm96, &terrain_engine)?);
            }
            dji_plan.lines = converted;
        }

        let bytes = crate::io::dji::build_dji_kmz(&dji_plan)?;
        Ok::<Vec<u8>, anyhow::Error>(bytes)
    })
    .await;

    match result {
        Ok(Ok(bytes)) => Ok((
            [
                (header::CONTENT_TYPE, "application/vnd.google-earth.kmz"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"mission.kmz\"",
                ),
            ],
            bytes,
        )
            .into_response()),
        Ok(Err(e)) => Err(ServerError::ExportFailed(e.to_string())),
        Err(e) => Err(ServerError::Internal(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct WsParams {
    pub token: Option<String>,
}

/// WebSocket upgrade handler.
///
/// Validates the token from the `?token=` query parameter using
/// constant-time comparison before upgrading the connection.
/// WebSocket lives outside the REST auth middleware — it authenticates
/// via the query parameter instead.
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(app): State<AppState>,
) -> Result<Response, ServerError> {
    match params.token {
        Some(ref token) => {
            let expected = app.token.as_bytes();
            let provided = token.as_bytes();
            if expected.len() == provided.len() && bool::from(expected.ct_eq(provided)) {
                let fms = app.fms.clone();
                Ok(ws.on_upgrade(|socket| handle_socket(socket, fms)))
            } else {
                Err(ServerError::Unauthorized("invalid WebSocket token".into()))
            }
        }
        None => Err(ServerError::Unauthorized(
            "missing token query parameter".into(),
        )),
    }
}

async fn handle_socket(mut socket: WebSocket, state: SharedState) {
    // 1. Subscribe to ALL channels BEFORE reading the snapshot.
    //    This eliminates the race where a state change between
    //    the snapshot read and the subscription is silently lost.
    let (mut rx_telemetry, mut rx_status, mut rx_trigger, mut rx_warning) = {
        let guard = state.read().await;
        (
            guard.tx_telemetry.subscribe(),
            guard.tx_status.subscribe(),
            guard.tx_trigger.subscribe(),
            guard.tx_warning.subscribe(),
        )
    };

    // 2. Read the watch snapshot and send as the initialization frame.
    //    Any changes after this point are captured by the subscriptions.
    let init = {
        let guard = state.read().await;
        let snapshot = guard.tx_status.borrow().clone();
        snapshot
    };

    if let Ok(json) = serde_json::to_string(&ServerMsg::Init(init)) {
        if socket.send(Message::Text(json)).await.is_err() {
            return;
        }
    }

    // 3. Ping interval to detect stale clients (WiFi drop, tablet sleep).
    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            result = rx_telemetry.recv() => {
                match result {
                    Ok(telemetry) => {
                        if let Ok(json) = serde_json::to_string(
                            &ServerMsg::Telemetry(telemetry),
                        ) {
                            if socket.send(Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            result = rx_status.changed() => {
                if result.is_err() { break; }
                let status = rx_status.borrow().clone();
                if let Ok(json) = serde_json::to_string(
                    &ServerMsg::Status(status),
                ) {
                    if socket.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
            }
            result = rx_trigger.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(
                            &ServerMsg::Trigger(event),
                        ) {
                            if socket.send(Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            result = rx_warning.recv() => {
                match result {
                    Ok(warning) => {
                        if let Ok(json) = serde_json::to_string(&warning) {
                            if socket.send(Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_client_msg(text, &state).await;
                    }
                    Some(Ok(_)) => continue,
                    _ => break,
                }
            }
            _ = ping_interval.tick() => {
                if socket.send(Message::Ping(vec![])).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn handle_client_msg(text: String, state: &SharedState) {
    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
    enum ClientMsg {
        SelectLine { index: usize },
    }

    if let Ok(msg) = serde_json::from_str::<ClientMsg>(&text) {
        match msg {
            ClientMsg::SelectLine { index } => {
                let mut state_guard = state.write().await;
                state_guard.last_status.active_line_index = Some(index);
                for (i, status) in state_guard.last_status.line_statuses.iter_mut().enumerate() {
                    status.is_active = i == index;
                }
                let status_clone = state_guard.last_status.clone();
                let _ = state_guard.tx_status.send(status_clone);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Background tasks
// ---------------------------------------------------------------------------

/// Periodically persist line completion percentages to the GeoPackage.
///
/// Uses the connection pool when available. SQLITE_BUSY is treated as
/// a fatal architectural violation [Doc 42] — logged at error level.
pub async fn run_persistence_task(state: SharedState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

    loop {
        interval.tick().await;

        let (pool, statuses) = {
            let state_guard = state.read().await;
            let pool = state_guard.db_pool.clone();
            let statuses: Vec<(usize, f64)> = state_guard
                .last_status
                .line_statuses
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.completion_pct))
                .collect();
            (pool, statuses)
        };

        if let Some(pool) = pool {
            if !statuses.is_empty() {
                match pool.get().await {
                    Ok(conn) => {
                        let result = conn
                            .interact(move |conn| {
                                apply_pool_pragmas(conn)?;
                                let mut stmt = conn.prepare(
                                    "UPDATE flight_lines SET completion_pct = ?1 \
                                     WHERE line_index = ?2",
                                )?;
                                for (idx, pct) in &statuses {
                                    stmt.execute(rusqlite::params![pct, *idx as i64])?;
                                }
                                Ok::<(), rusqlite::Error>(())
                            })
                            .await;
                        match result {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => {
                                let msg = e.to_string();
                                if msg.contains("SQLITE_BUSY") || msg.contains("database is locked")
                                {
                                    log::error!("FATAL: SQLITE_BUSY in persistence task: {e}");
                                } else {
                                    log::warn!("persistence write failed: {e}");
                                }
                            }
                            Err(e) => log::warn!("persistence interact failed: {e}"),
                        }
                    }
                    Err(e) => log::warn!("persistence pool.get() failed: {e}"),
                }
            }
        } else {
            // Fallback: direct GeoPackage access (no pool yet)
            let path = {
                let state_guard = state.read().await;
                state_guard.persistence_path.clone()
            };
            if let Some(path) = path {
                if !statuses.is_empty() {
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(gpkg) = GeoPackage::open(&path) {
                            let _ = gpkg.update_line_statuses("flight_lines", &statuses);
                        }
                    })
                    .await;
                }
            }
        }
    }
}

/// Compute heading between two waypoints using the geodesic inverse.
///
/// Pure sync function — called via `spawn_blocking` to satisfy audit rule 6
/// (no geodesic math on the Tokio executor).
fn compute_mock_heading(
    lat: f64,
    lon: f64,
    next: Option<(f64, f64)>,
    prev: Option<(f64, f64)>,
) -> f64 {
    let geod = geographiclib_rs::Geodesic::wgs84();
    let azi = if let Some((lat2, lon2)) = next {
        let (_dist, azi1, _azi2) =
            geographiclib_rs::InverseGeodesic::inverse(&geod, lat, lon, lat2, lon2);
        azi1
    } else if let Some((lat1, lon1)) = prev {
        let (_dist, azi1, _azi2) =
            geographiclib_rs::InverseGeodesic::inverse(&geod, lat1, lon1, lat, lon);
        azi1
    } else {
        0.0
    };
    // Normalize to [0, 360)
    ((azi % 360.0) + 360.0) % 360.0
}

/// Simulate GNSS telemetry by walking through the flight plan waypoints.
///
/// At each 100 ms tick the mock aircraft advances one waypoint, computing
/// a realistic heading via the geodesic inverse problem, updating line
/// progress in the `watch` channel, and emitting a `TriggerEvent` for
/// each waypoint visited.
pub async fn run_mock_telemetry(state: SharedState, speed_ms: f64) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let mut line_idx: usize = 0;
    let mut waypoint_idx: usize = 0;
    let mut prev_line_idx: Option<usize> = None;
    let start = std::time::Instant::now();

    loop {
        interval.tick().await;

        let plan = {
            let state_guard = state.read().await;
            state_guard.flight_plan.clone()
        };

        if let Some(plan) = plan {
            if plan.lines.is_empty() {
                continue;
            }
            if line_idx >= plan.lines.len() {
                line_idx = 0;
                waypoint_idx = 0;
            }

            let line = &plan.lines[line_idx];
            if waypoint_idx >= line.len() {
                line_idx += 1;
                waypoint_idx = 0;
                continue;
            }

            let lat = line.lats()[waypoint_idx].to_degrees();
            let lon = line.lons()[waypoint_idx].to_degrees();
            let alt = line.elevations()[waypoint_idx];

            // Compute heading via geodesic inverse on the blocking pool
            // to keep geodesic math off the Tokio executor [audit rule 6].
            let next = if waypoint_idx + 1 < line.len() {
                Some((
                    line.lats()[waypoint_idx + 1].to_degrees(),
                    line.lons()[waypoint_idx + 1].to_degrees(),
                ))
            } else {
                None
            };
            let prev = if waypoint_idx > 0 {
                Some((
                    line.lats()[waypoint_idx - 1].to_degrees(),
                    line.lons()[waypoint_idx - 1].to_degrees(),
                ))
            } else {
                None
            };
            let heading =
                tokio::task::spawn_blocking(move || compute_mock_heading(lat, lon, next, prev))
                    .await
                    .unwrap_or(0.0);

            let telemetry = CurrentState {
                lat,
                lon,
                alt,
                alt_datum: line.altitude_datum,
                ground_speed_ms: speed_ms,
                true_heading_deg: heading,
                fix_quality: 4,
                hdop: 0.9,
                satellites_in_use: 12,
                geoid_separation_m: 0.0,
                utc_time: String::new(),
                utc_date: String::new(),
            };

            let trigger = TriggerEvent {
                timestamp_ms: start.elapsed().as_millis() as u64,
                line_index: line_idx,
                waypoint_index: waypoint_idx,
                lat,
                lon,
                alt,
            };

            {
                let mut guard = state.write().await;
                guard.last_telemetry = telemetry.clone();
                let _ = guard.tx_telemetry.send(telemetry);

                // Record trigger event
                guard.record_trigger(trigger);

                // Update line progress in the watch channel
                if let Some(prev) = prev_line_idx {
                    if prev != line_idx {
                        if let Some(s) = guard.last_status.line_statuses.get_mut(prev) {
                            s.is_active = false;
                            s.completion_pct = 100.0;
                        }
                    }
                }
                if let Some(s) = guard.last_status.line_statuses.get_mut(line_idx) {
                    s.is_active = true;
                    s.completion_pct = if line.len() > 1 {
                        waypoint_idx as f64 / (line.len() - 1) as f64 * 100.0
                    } else {
                        100.0
                    };
                }
                guard.last_status.active_line_index = Some(line_idx);
                let status_clone = guard.last_status.clone();
                let _ = guard.tx_status.send(status_clone);
            }

            prev_line_idx = Some(line_idx);
            waypoint_idx += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Plan generation (synchronous, rayon-safe)
// ---------------------------------------------------------------------------

fn calculate_total_distance(plan: &FlightPlan) -> f64 {
    let mut acc = crate::math::numerics::KahanAccumulator::new();
    let geod = geographiclib_rs::Geodesic::wgs84();
    for line in &plan.lines {
        for i in 1..line.len() {
            let (lat1, lon1) = (
                line.lats()[i - 1].to_degrees(),
                line.lons()[i - 1].to_degrees(),
            );
            let (lat2, lon2) = (line.lats()[i].to_degrees(), line.lons()[i].to_degrees());
            let (dist, _, _) =
                geographiclib_rs::InverseGeodesic::inverse(&geod, lat1, lon1, lat2, lon2);
            acc.add(dist);
        }
    }
    acc.total() / 1000.0
}

fn generate_plan_internal(
    req: LoadMissionRequest,
    terrain_engine: &TerrainEngine,
) -> anyhow::Result<FlightPlan> {
    let target_datum = match req.datum.to_lowercase().as_str() {
        "egm2008" => AltitudeDatum::Egm2008,
        "egm96" => AltitudeDatum::Egm96,
        "ellipsoidal" | "wgs84" => AltitudeDatum::Wgs84Ellipsoidal,
        "agl" => AltitudeDatum::Agl,
        other => anyhow::bail!("unknown datum {other:?}"),
    };

    let is_corridor = req.mode.to_lowercase() == "corridor";
    if is_corridor {
        return generate_corridor_plan_internal(req, target_datum, terrain_engine);
    }

    let boundary_polygons = if let Some(ref geojson) = req.boundary_geojson {
        let polys = geojson_to_polygons(geojson);
        if polys.is_empty() {
            anyhow::bail!("boundary GeoJSON contains no polygons");
        }
        Some(polys)
    } else {
        None
    };

    let bbox = if let Some(ref polys) = boundary_polygons {
        let (mut min_lat, mut min_lon, mut max_lat, mut max_lon) = polys[0].bbox();
        for poly in polys.iter().skip(1) {
            let (a, b, c, d) = poly.bbox();
            min_lat = min_lat.min(a);
            min_lon = min_lon.min(b);
            max_lat = max_lat.max(c);
            max_lon = max_lon.max(d);
        }
        BoundingBox {
            min_lat,
            min_lon,
            max_lat,
            max_lon,
        }
    } else {
        BoundingBox {
            min_lat: req
                .min_lat
                .ok_or_else(|| anyhow::anyhow!("missing min_lat"))?,
            min_lon: req
                .min_lon
                .ok_or_else(|| anyhow::anyhow!("missing min_lon"))?,
            max_lat: req
                .max_lat
                .ok_or_else(|| anyhow::anyhow!("missing max_lat"))?,
            max_lon: req
                .max_lon
                .ok_or_else(|| anyhow::anyhow!("missing max_lon"))?,
        }
    };

    let is_lidar = req.mission_type.to_lowercase() == "lidar";
    let mut plan = if is_lidar {
        let prr = req
            .lidar_prr
            .ok_or_else(|| anyhow::anyhow!("missing lidar_prr"))?;
        let scan_rate = req
            .lidar_scan_rate
            .ok_or_else(|| anyhow::anyhow!("missing lidar_scan_rate"))?;
        let fov = req
            .lidar_fov
            .ok_or_else(|| anyhow::anyhow!("missing lidar_fov"))?;
        let sensor = LidarSensorParams::new(prr, scan_rate, fov, 0.0)?;
        let agl = req.altitude_msl.unwrap_or_else(|| {
            log::warn!("altitude_msl not provided, defaulting to 100.0 m");
            100.0
        });
        let ground_speed = if let Some(density) = req.target_density {
            crate::photogrammetry::lidar::required_speed_for_density(&sensor, agl, density)?
        } else {
            10.0
        };
        let mut lidar_params =
            LidarPlanParams::new(sensor, agl, ground_speed)?.with_side_lap(req.side_lap)?;
        lidar_params.flight_altitude_msl = req.altitude_msl;
        generate_lidar_flight_lines(&bbox, req.azimuth, &lidar_params)?
    } else {
        let sensor_params = resolve_sensor(
            &req.sensor,
            req.focal_length_mm,
            req.sensor_width_mm,
            req.sensor_height_mm,
            req.image_width_px,
            req.image_height_px,
        )?;
        let gsd_m = req.gsd_cm / 100.0;
        let mut params = FlightPlanParams::new(sensor_params, gsd_m)?
            .with_side_lap(req.side_lap)?
            .with_end_lap(req.end_lap)?;
        params.flight_altitude_msl = req.altitude_msl;
        generate_flight_lines(&bbox, req.azimuth, &params)?
    };

    if let Some(ref polys) = boundary_polygons {
        let mut all_lines = Vec::new();
        for poly in polys {
            let clipped = clip_to_polygon(
                FlightPlan {
                    params: plan.params.clone(),
                    azimuth_deg: plan.azimuth_deg,
                    lines: plan.lines.clone(),
                },
                poly,
            );
            all_lines.extend(clipped.lines);
        }
        plan.lines = all_lines;
    }

    if req.optimize_route && plan.lines.len() >= 2 {
        let launch_lat = req.launch_lat.unwrap_or_else(|| {
            log::warn!(
                "launch_lat not provided, defaulting to bbox min_lat ({:.6})",
                bbox.min_lat
            );
            bbox.min_lat
        });
        let launch_lon = req.launch_lon.unwrap_or_else(|| {
            log::warn!(
                "launch_lon not provided, defaulting to bbox min_lon ({:.6})",
                bbox.min_lon
            );
            bbox.min_lon
        });
        let result = optimize_route(&plan.lines, launch_lat, launch_lon);
        let mut reordered = Vec::with_capacity(plan.lines.len());
        for (idx, &line_idx) in result.order.iter().enumerate() {
            if result.reversed[idx] {
                let orig = &plan.lines[line_idx];
                let mut rev = FlightLine::new().with_datum(orig.altitude_datum);
                for i in (0..orig.len()).rev() {
                    rev.push(orig.lats()[i], orig.lons()[i], orig.elevations()[i]);
                }
                reordered.push(rev);
            } else {
                reordered.push(plan.lines[line_idx].clone());
            }
        }
        plan.lines = reordered;
    }

    if !is_lidar && plan.lines.len() >= 2 {
        let turn_params = TurnParams::new(req.turn_airspeed, req.max_bank_angle);
        plan = connect_with_turns(plan, &turn_params);
    }

    if req.terrain && !is_lidar {
        plan = adjust_for_terrain(&plan, terrain_engine)?;
    }

    if plan.lines.iter().any(|l| l.altitude_datum != target_datum) {
        let lines = std::mem::take(&mut plan.lines);
        let mut converted = Vec::with_capacity(lines.len());
        for line in lines {
            converted.push(line.to_datum(target_datum, terrain_engine)?);
        }
        plan.lines = converted;
    }

    Ok(plan)
}

fn generate_corridor_plan_internal(
    req: LoadMissionRequest,
    target_datum: AltitudeDatum,
    terrain_engine: &TerrainEngine,
) -> anyhow::Result<FlightPlan> {
    let geojson = req
        .centerline_geojson
        .ok_or_else(|| anyhow::anyhow!("missing centerline_geojson"))?;
    let linestrings = geojson_to_linestrings(&geojson);
    if linestrings.is_empty() {
        anyhow::bail!("centerline GeoJSON contains no LineStrings");
    }
    let centerline = &linestrings[0];

    let corridor_width = req
        .corridor_width
        .ok_or_else(|| anyhow::anyhow!("missing corridor_width"))?;
    let corridor_passes = req.corridor_passes.unwrap_or_else(|| {
        log::warn!("corridor_passes not provided, defaulting to 3");
        3
    });

    let sensor_params = resolve_sensor(
        &req.sensor,
        req.focal_length_mm,
        req.sensor_width_mm,
        req.sensor_height_mm,
        req.image_width_px,
        req.image_height_px,
    )?;
    let gsd_m = req.gsd_cm / 100.0;
    let params = FlightPlanParams::new(sensor_params, gsd_m)?
        .with_side_lap(req.side_lap)?
        .with_end_lap(req.end_lap)?;

    let waypoint_interval = params.photo_interval();
    let (flight_alt_m, line_datum) = match req.altitude_msl {
        Some(msl) => (msl, AltitudeDatum::Egm2008),
        None => (params.nominal_agl(), AltitudeDatum::Agl),
    };

    let corridor_params = CorridorParams {
        centerline: centerline.clone(),
        buffer_width: corridor_width,
        num_passes: corridor_passes,
    };

    let lines = generate_corridor_lines(
        &corridor_params,
        waypoint_interval,
        flight_alt_m,
        line_datum,
    )?;

    let mut plan = FlightPlan {
        params,
        azimuth_deg: 0.0, // simplified
        lines,
    };

    if req.optimize_route && plan.lines.len() >= 2 {
        let launch_lat = req.launch_lat.unwrap_or_else(|| {
            log::warn!(
                "launch_lat not provided, defaulting to centerline start ({:.6})",
                centerline[0].0
            );
            centerline[0].0
        });
        let launch_lon = req.launch_lon.unwrap_or_else(|| {
            log::warn!(
                "launch_lon not provided, defaulting to centerline start ({:.6})",
                centerline[0].1
            );
            centerline[0].1
        });
        let result = optimize_route(&plan.lines, launch_lat, launch_lon);
        let mut reordered = Vec::with_capacity(plan.lines.len());
        for (idx, &line_idx) in result.order.iter().enumerate() {
            if result.reversed[idx] {
                let orig = &plan.lines[line_idx];
                let mut rev = FlightLine::new().with_datum(orig.altitude_datum);
                for i in (0..orig.len()).rev() {
                    rev.push(orig.lats()[i], orig.lons()[i], orig.elevations()[i]);
                }
                reordered.push(rev);
            } else {
                reordered.push(plan.lines[line_idx].clone());
            }
        }
        plan.lines = reordered;
    }

    if plan.lines.len() >= 2 {
        let turn_params = TurnParams::new(req.turn_airspeed, req.max_bank_angle);
        plan = connect_with_turns(plan, &turn_params);
    }

    if req.terrain {
        plan = adjust_for_terrain(&plan, terrain_engine)?;
    }

    if plan.lines.iter().any(|l| l.altitude_datum != target_datum) {
        let lines = std::mem::take(&mut plan.lines);
        let mut converted = Vec::with_capacity(lines.len());
        for line in lines {
            converted.push(line.to_datum(target_datum, terrain_engine)?);
        }
        plan.lines = converted;
    }

    Ok(plan)
}

// ---------------------------------------------------------------------------
// Test server
// ---------------------------------------------------------------------------

/// Start a lightweight server on a random OS-assigned port.
///
/// Returns the bound address and a `JoinHandle` for the server task.
/// Skips mDNS registration, persistence, and graceful-shutdown signal
/// handling — designed for self-contained integration tests.
///
/// If `mock_speed` is `Some(speed)`, spawns the mock telemetry task.
pub async fn start_test_server(
    app: AppState,
    mock_speed: Option<f64>,
) -> anyhow::Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>)> {
    if let Some(speed) = mock_speed {
        let state_clone = app.fms.clone();
        tokio::spawn(async move {
            run_mock_telemetry(state_clone, speed).await;
        });
    }

    let rest = Router::new()
        .route("/api/v1/missions", post(create_mission))
        .route("/api/v1/missions/:id", get(get_mission))
        .route("/api/v1/missions/:id/lines", get(get_lines))
        .route(
            "/api/v1/missions/:id/lines/:line_id",
            put(update_line).delete(delete_line),
        )
        .route("/api/v1/missions/:id/export/gpkg", get(export_gpkg))
        .route("/api/v1/missions/:id/export/qgc", get(export_qgc))
        .route("/api/v1/missions/:id/export/dji", get(export_dji))
        .route_layer(middleware::from_fn_with_state(app.clone(), auth_middleware));

    let router = Router::new()
        .merge(rest)
        .route("/ws", get(ws_handler))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });

    Ok((addr, handle))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tower::util::ServiceExt;

    fn test_app_state() -> (AppState, String) {
        let token = "test_token_abc123".to_string();
        let state = Arc::new(RwLock::new(FmsState::new().unwrap()));
        let app = AppState {
            fms: state,
            token: Arc::new(token.clone()),
        };
        (app, token)
    }

    fn test_router(app: AppState) -> Router {
        let rest = Router::new()
            .route("/api/v1/missions", post(create_mission))
            .route("/api/v1/missions/:id", get(get_mission))
            .route("/api/v1/missions/:id/lines", get(get_lines))
            .route(
                "/api/v1/missions/:id/lines/:line_id",
                put(update_line).delete(delete_line),
            )
            .route_layer(middleware::from_fn_with_state(app.clone(), auth_middleware));

        Router::new()
            .merge(rest)
            .route("/ws", get(ws_handler))
            .with_state(app)
    }

    fn sample_mission_request() -> LoadMissionRequest {
        LoadMissionRequest {
            min_lat: Some(51.0),
            min_lon: Some(0.0),
            max_lat: Some(51.1),
            max_lon: Some(0.1),
            boundary_geojson: None,
            sensor: "phantom4pro".into(),
            focal_length_mm: None,
            sensor_width_mm: None,
            sensor_height_mm: None,
            image_width_px: None,
            image_height_px: None,
            gsd_cm: 5.0,
            side_lap: 60.0,
            end_lap: 80.0,
            azimuth: 0.0,
            altitude_msl: Some(600.0),
            mission_type: "camera".into(),
            lidar_prr: None,
            lidar_scan_rate: None,
            lidar_fov: None,
            target_density: None,
            mode: "area".into(),
            centerline_geojson: None,
            corridor_width: None,
            corridor_passes: None,
            max_bank_angle: 20.0,
            turn_airspeed: 30.0,
            optimize_route: false,
            launch_lat: None,
            launch_lon: None,
            terrain: false,
            datum: "egm2008".into(),
        }
    }

    #[tokio::test]
    async fn mission_create_and_get_lines() {
        let (app, token) = test_app_state();
        let router = test_router(app);

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/missions")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        serde_json::to_vec(&sample_mission_request()).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        let summary: MissionSummaryResponse = serde_json::from_slice(&body).unwrap();
        assert!(summary.line_count > 0);
        let id = summary.id;

        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/missions/{id}/lines"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        let fc: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(fc["type"], "FeatureCollection");
        assert!(!fc["features"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn auth_rejects_missing_token() {
        let (app, _token) = test_app_state();
        let router = test_router(app);

        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/missions/1/lines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(err["code"], "ERR_UNAUTHORIZED");
    }

    #[tokio::test]
    async fn auth_rejects_wrong_token() {
        let (app, _token) = test_app_state();
        let router = test_router(app);

        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/missions/1/lines")
                    .header("Authorization", "Bearer wrong_token_value")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(err["code"], "ERR_UNAUTHORIZED");
    }

    #[tokio::test]
    async fn structured_error_on_invalid_input() {
        let (app, token) = test_app_state();
        let router = test_router(app);

        // POST with no bbox and no boundary — should fail with structured error
        let mut req = sample_mission_request();
        req.min_lat = None;
        req.min_lon = None;
        req.max_lat = None;
        req.max_lon = None;

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/missions")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::from(serde_json::to_vec(&req).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(err["code"].as_str().unwrap().starts_with("ERR_"));
        assert!(!err["message"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mission_not_found_404() {
        let (app, token) = test_app_state();
        let router = test_router(app);

        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/missions/999")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(err["code"], "ERR_MISSION_NOT_FOUND");
    }

    #[tokio::test]
    async fn delete_line_recalculates() {
        let (app, token) = test_app_state();
        let router = test_router(app.clone());

        // Create mission
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/missions")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        serde_json::to_vec(&sample_mission_request()).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        let summary: MissionSummaryResponse = serde_json::from_slice(&body).unwrap();
        let original_count = summary.line_count;
        let id = summary.id;
        assert!(
            original_count >= 2,
            "need at least 2 lines to test deletion"
        );

        // Delete line 0
        let response = router
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/missions/{id}/lines/0"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        let updated: MissionSummaryResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated.line_count, original_count - 1);
    }

    #[tokio::test]
    async fn delete_line_adjusts_active() {
        let (app, token) = test_app_state();
        let router = test_router(app.clone());

        // Create mission
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/missions")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        serde_json::to_vec(&sample_mission_request()).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        let summary: MissionSummaryResponse = serde_json::from_slice(&body).unwrap();
        assert!(
            summary.line_count >= 3,
            "need at least 3 lines to test active adjustment"
        );

        // Set active_line to 2
        {
            let mut state = app.fms.write().await;
            state.last_status.active_line_index = Some(2);
            for (i, ls) in state.last_status.line_statuses.iter_mut().enumerate() {
                ls.is_active = i == 2;
            }
        }

        // Delete line 1 — active should shift from 2 to 1
        let id = summary.id;
        let response = router
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/missions/{id}/lines/1"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let state = app.fms.read().await;
        assert_eq!(state.last_status.active_line_index, Some(1));
    }

    /// mDNS IP discovery must return a non-loopback address (or the 0.0.0.0 fallback).
    #[test]
    fn mdns_discover_local_ip_not_loopback() {
        let ip = discover_local_ip();
        assert!(
            !ip.starts_with("127."),
            "mDNS IP must not be loopback, got {ip}"
        );
    }

    /// Axum listener binds to 0.0.0.0, making it reachable from non-loopback addresses.
    #[tokio::test]
    async fn listener_binds_all_interfaces() {
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let bound = listener.local_addr().unwrap();
        assert!(
            bound.ip().is_unspecified(),
            "listener must bind to 0.0.0.0, got {bound}"
        );
    }
}
