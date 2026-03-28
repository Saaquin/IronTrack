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
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, watch, RwLock};
use tower_http::cors::CorsLayer;

use crate::dem::TerrainEngine;
use crate::gpkg::GeoPackage;
use crate::network::telemetry::{CurrentState, LineStatus, SystemStatus};
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

/// Global state for the FMS daemon.
pub struct FmsState {
    pub flight_plan: Option<FlightPlan>,
    pub terrain_engine: Arc<TerrainEngine>,

    // Telemetry Channels
    pub tx_telemetry: broadcast::Sender<CurrentState>,
    pub tx_status: watch::Sender<SystemStatus>,

    // Latest values
    pub last_telemetry: CurrentState,
    pub last_status: SystemStatus,

    // Persistence
    pub persistence_path: Option<PathBuf>,
}

impl FmsState {
    pub fn new() -> anyhow::Result<Self> {
        let (tx_telemetry, _) = broadcast::channel(256);
        let (tx_status, _) = watch::channel(SystemStatus::default());

        Ok(Self {
            flight_plan: None,
            terrain_engine: Arc::new(TerrainEngine::new()?),
            tx_telemetry,
            tx_status,
            last_telemetry: CurrentState::default(),
            last_status: SystemStatus::default(),
            persistence_path: None,
        })
    }
}

pub type SharedState = Arc<RwLock<FmsState>>;

#[cfg(feature = "serial")]
use crate::network::serial_manager::SerialManager;
use mdns_sd::{ServiceDaemon, ServiceInfo};

/// Start the Axum FMS daemon on the specified port.
pub async fn run_server(
    port: u16,
    mock_speed: Option<f64>,
    gnss_vid: u16,
    gnss_pid: u16,
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
            let serial_mgr = SerialManager::new(gnss_vid, gnss_pid, state.clone());
            tokio::spawn(async move {
                serial_mgr.run().await;
            });
        }
        #[cfg(not(feature = "serial"))]
        {
            let _ = (gnss_vid, gnss_pid);
            eprintln!("warning: serial port support disabled (compile with --features serial)");
        }
    }

    // Start mDNS advertising
    let mdns = ServiceDaemon::new()?;
    let service_type = "_irontrack._tcp.local.";
    let instance_name = "fms_daemon";
    let host_name = "irontrack.local.";
    let my_service = ServiceInfo::new(
        service_type,
        instance_name,
        host_name,
        "127.0.0.1", // TODO: Discover local IP
        port,
        None,
    )?;
    mdns.register(my_service)?;

    // Spawn persistence task
    let state_clone = state.clone();
    tokio::spawn(async move {
        run_persistence_task(state_clone).await;
    });

    let app = Router::new()
        .route("/api/v1/mission/load", post(load_mission))
        .route("/api/v1/mission/plan", get(get_plan))
        .route("/api/v1/mission/lines/:index", put(update_line))
        .route("/api/v1/mission/export/qgc", get(export_qgc))
        .route("/api/v1/mission/export/dji", get(export_dji))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("IronTrack FMS daemon listening on {}", addr);
    axum::serve(listener, app).await?;

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

#[derive(Serialize)]
pub struct MissionSummaryResponse {
    pub line_count: usize,
    pub waypoint_count: usize,
    pub total_distance_km: f64,
    pub bbox: [f64; 4], // [min_lat, min_lon, max_lat, max_lon]
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn load_mission(
    State(state): State<SharedState>,
    Json(req): Json<LoadMissionRequest>,
) -> impl IntoResponse {
    let terrain_engine = state.read().await.terrain_engine.clone();

    let result =
        tokio::task::spawn_blocking(move || generate_plan_internal(req, &terrain_engine)).await;

    match result {
        Ok(Ok(plan)) => {
            let mut state_guard = state.write().await;

            let line_count = plan.lines.len();
            let waypoint_count: usize = plan.lines.iter().map(|l| l.len()).sum();

            let total_distance_km = calculate_total_distance(&plan);

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

            state_guard.flight_plan = Some(plan.clone());

            // Initialize persistence
            let gpkg_path = PathBuf::from("mission.gpkg");
            if let Ok(gpkg) = GeoPackage::new(&gpkg_path) {
                if let Ok(()) = gpkg.create_feature_table("flight_lines") {
                    let _ = gpkg.insert_flight_plan("flight_lines", &plan);
                }
            }
            state_guard.persistence_path = Some(gpkg_path);

            // Initialize line statuses
            state_guard.last_status.line_statuses = vec![
                LineStatus {
                    completion_pct: 0.0,
                    is_active: false
                };
                plan.lines.len()
            ];
            let status_clone = state_guard.last_status.clone();
            let _ = state_guard.tx_status.send(status_clone);

            Json(MissionSummaryResponse {
                line_count,
                waypoint_count,
                total_distance_km,
                bbox: [min_lat, min_lon, max_lat, max_lon],
            })
            .into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, format!("Error: {e}")).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Internal Error: {e}"),
        )
            .into_response(),
    }
}

fn calculate_total_distance(plan: &FlightPlan) -> f64 {
    let mut total_m = 0.0;
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
            total_m += dist;
        }
    }
    total_m / 1000.0
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
        let agl = req.altitude_msl.unwrap_or(100.0);
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
        let launch_lat = req.launch_lat.unwrap_or(bbox.min_lat);
        let launch_lon = req.launch_lon.unwrap_or(bbox.min_lon);
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
        let mut converted_lines = Vec::with_capacity(plan.lines.len());
        for line in &plan.lines {
            converted_lines.push(line.to_datum(target_datum, terrain_engine)?);
        }
        plan.lines = converted_lines;
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
    let corridor_passes = req.corridor_passes.unwrap_or(3);

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
        let launch_lat = req.launch_lat.unwrap_or(centerline[0].0);
        let launch_lon = req.launch_lon.unwrap_or(centerline[0].1);
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
        let mut converted_lines = Vec::with_capacity(plan.lines.len());
        for line in &plan.lines {
            converted_lines.push(line.to_datum(target_datum, terrain_engine)?);
        }
        plan.lines = converted_lines;
    }

    Ok(plan)
}

async fn get_plan(State(state): State<SharedState>) -> impl IntoResponse {
    let state_guard = state.read().await;
    if let Some(ref plan) = state_guard.flight_plan {
        let geojson = crate::io::geojson::flight_plan_to_geojson(plan, None, None, None);
        Json(geojson).into_response()
    } else {
        (StatusCode::NOT_FOUND, "No mission loaded").into_response()
    }
}

async fn export_qgc(State(state): State<SharedState>) -> impl IntoResponse {
    let terrain_engine = state.read().await.terrain_engine.clone();
    let plan = state.read().await.flight_plan.as_ref().cloned();

    let plan = match plan {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "No mission loaded").into_response(),
    };

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
        Ok(Ok(json)) => Json(json).into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Export failed: {e}"),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Internal Error: {e}"),
        )
            .into_response(),
    }
}

async fn export_dji(State(state): State<SharedState>) -> impl IntoResponse {
    let terrain_engine = state.read().await.terrain_engine.clone();
    let plan = state.read().await.flight_plan.as_ref().cloned();

    let plan = match plan {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "No mission loaded").into_response(),
    };

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
        Ok(Ok(bytes)) => (
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "application/vnd.google-earth.kmz",
                ),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    "attachment; filename=\"mission.kmz\"",
                ),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Export failed: {e}"),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Internal Error: {e}"),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct WsParams {
    pub token: Option<String>,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    if let Some(_token) = params.token {
        ws.on_upgrade(|socket| handle_socket(socket, state))
    } else {
        (StatusCode::UNAUTHORIZED, "Missing token").into_response()
    }
}

async fn handle_socket(mut socket: WebSocket, state: SharedState) {
    let initial_status = {
        let state_guard = state.read().await;
        let status = state_guard.tx_status.borrow().clone();
        status
    };

    if let Ok(msg) = serde_json::to_string(&initial_status) {
        if socket.send(Message::Text(msg)).await.is_err() {
            return;
        }
    }

    let (mut rx_telemetry, mut rx_status) = {
        let state_guard = state.read().await;
        (
            state_guard.tx_telemetry.subscribe(),
            state_guard.tx_status.subscribe(),
        )
    };

    loop {
        tokio::select! {
            result = rx_telemetry.recv() => {
                match result {
                    Ok(telemetry) => {
                        if let Ok(msg) = serde_json::to_string(&telemetry) {
                            if socket.send(Message::Text(msg)).await.is_err() {
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
                if let Ok(msg) = serde_json::to_string(&status) {
                    if socket.send(Message::Text(msg)).await.is_err() {
                        break;
                    }
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

pub async fn run_persistence_task(state: SharedState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

    loop {
        interval.tick().await;

        let (path, statuses) = {
            let state_guard = state.read().await;
            let path = state_guard.persistence_path.clone();
            let statuses: Vec<(usize, f64)> = state_guard
                .last_status
                .line_statuses
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.completion_pct))
                .collect();
            (path, statuses)
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

pub async fn run_mock_telemetry(state: SharedState, speed_ms: f64) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let mut line_idx = 0;
    let mut waypoint_idx = 0;

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

            let telemetry = CurrentState {
                lat,
                lon,
                alt,
                alt_datum: line.altitude_datum,
                ground_speed_ms: speed_ms,
                true_heading_deg: 0.0,
                fix_quality: 4,
            };

            {
                let mut state_guard = state.write().await;
                state_guard.last_telemetry = telemetry.clone();
                let _ = state_guard.tx_telemetry.send(telemetry);
            }

            waypoint_idx += 1;
        }
    }
}

async fn update_line(
    State(state): State<SharedState>,
    axum::extract::Path(index): axum::extract::Path<usize>,
    Json(status): Json<LineStatus>,
) -> impl IntoResponse {
    let mut state_guard = state.write().await;
    if let Some(ref mut _plan) = state_guard.flight_plan {
        if index < state_guard.last_status.line_statuses.len() {
            state_guard.last_status.line_statuses[index] = status;
            let status_clone = state_guard.last_status.clone();
            let _ = state_guard.tx_status.send(status_clone);
            StatusCode::OK.into_response()
        } else {
            (StatusCode::NOT_FOUND, "Line index out of bounds").into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, "No mission loaded").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn fms_state_roundtrip() {
        let state = Arc::new(RwLock::new(FmsState::new().unwrap()));
        let req = LoadMissionRequest {
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
        };

        let app = Router::new()
            .route("/api/v1/mission/load", post(load_mission))
            .route("/api/v1/mission/plan", get(get_plan))
            .with_state(state.clone());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/mission/load")
                    .header("Content-Type", "application/json")
                    .body(axum::body::Body::from(serde_json::to_vec(&req).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/mission/plan")
                    .body(axum::body::Body::empty())
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
        assert!(fc["features"].as_array().unwrap().len() > 0);
    }
}
