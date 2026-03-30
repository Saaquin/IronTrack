# Daemon & API

IronTrack's network daemon provides a REST API and WebSocket telemetry stream for real-time flight management. It is the foundation of the v0.4 release and the interface layer between the core engine and all client applications (Glass Cockpit, Web Planning UI, third-party integrations).

## Overview

The daemon is started with:

```bash
irontrack daemon --port 8080 --mock-telemetry
```

It provides three communication channels:

1. **REST API** (`/api/v1/`) — Mission CRUD, terrain queries, export generation.
2. **WebSocket** (`/ws`) — 10 Hz telemetry stream plus mission status updates.
3. **mDNS** (`_irontrack._tcp.local.`) — Zero-configuration service discovery on the LAN.

The daemon is built on Axum (Tokio runtime) and uses `Arc<RwLock<FmsState>>` for thread-safe shared state.

## REST API

All endpoints are under `/api/v1/`. Request and response bodies are JSON unless noted otherwise.

### POST `/mission/load`

Generate and load a flight plan. Accepts a comprehensive parameter set covering area surveys, corridor surveys, and LiDAR missions.

**Key request fields:**

- **Boundary:** `min_lat`, `min_lon`, `max_lat`, `max_lon` for bounding box, or `boundary_geojson` for Polygon/MultiPolygon.
- **Sensor:** `sensor` (preset name: `phantom4pro`, `mavic3`, `ixm100`) or custom parameters (`focal_length_mm`, `sensor_width_mm`, `sensor_height_mm`, `image_width_px`, `image_height_px`).
- **Photogrammetry:** `gsd_cm`, `side_lap`, `end_lap`, `azimuth`, `altitude_msl`.
- **LiDAR:** `mission_type: "lidar"`, `lidar_prr`, `lidar_scan_rate`, `lidar_fov`, `target_density`.
- **Corridor:** `mode: "corridor"` with `centerline_geojson`, `corridor_width`, `corridor_passes`.
- **Turns:** `max_bank_angle`, `turn_airspeed` (enables Dubins procedure turns).
- **Optimization:** `optimize_route: true`, `launch_lat`, `launch_lon`.
- **Terrain:** `terrain: true`, `datum` (`egm2008`, `egm96`, `ellipsoidal`, `agl`).

**Response:** `MissionSummaryResponse` with `line_count`, `waypoint_count`, `total_distance_km`, and `bbox`.

### GET `/mission/plan`

Returns the currently loaded flight plan as a GeoJSON FeatureCollection.

### PUT `/mission/lines/:index`

Update the progress or active state of a specific flight line.

**Request body:**

```json
{
  "completion_pct": 75.0,
  "is_active": true
}
```

Triggers a `SystemStatus` update broadcast to all WebSocket clients.

### GET `/mission/export/qgc`

Export the current mission as a QGroundControl `.plan` JSON file.

### GET `/mission/export/dji`

Export the current mission as a DJI WPML `.kmz` binary file.

## WebSocket Protocol

Connect at `ws://host:port/ws?token=<token>`.

### Authentication

A token query parameter is required. Missing or invalid tokens receive a 401 response. Tokens are ephemeral (generated at daemon startup). Full cryptographic authentication is planned for v1.0.

### Message Flow

**On connection:** The server immediately sends the current `SystemStatus` as JSON:

```json
{
  "active_line_index": null,
  "line_statuses": [
    {"completion_pct": 0.0, "is_active": false},
    {"completion_pct": 0.0, "is_active": false}
  ],
  "trigger_count": 0
}
```

**Telemetry stream (~10 Hz):** `CurrentState` frames broadcast via a `tokio::sync::broadcast` channel (256-message buffer):

```json
{
  "lat": 49.2827,
  "lon": -123.1207,
  "alt": 150.5,
  "alt_datum": "Egm2008",
  "ground_speed_ms": 25.3,
  "true_heading_deg": 270.0,
  "fix_quality": 4
}
```

**Fix quality values:** 0 = No fix, 1 = Autonomous, 2 = DGPS, 4 = RTK Fixed, 5 = RTK Float.

**Status updates:** Sent whenever mission state changes (e.g., line completion updated via PUT endpoint). Uses a `tokio::sync::watch` channel so newly connected clients always receive the latest status immediately.

### Client Messages

Clients can send JSON messages to the server:

```json
{"type": "SELECT_LINE", "index": 3}
```

This updates the `active_line_index` in `SystemStatus` and toggles `is_active` flags for the corresponding line.

### Multiplexing

The WebSocket handler uses `tokio::select!` to multiplex three event sources concurrently: the telemetry broadcast receiver, the status watch receiver, and incoming client messages. Lagged telemetry messages (when a slow client falls behind the 256-message buffer) are silently dropped.

## NMEA Parser

The daemon parses three NMEA sentence types from GNSS receivers:

| Sentence | Source | Fields Parsed |
|----------|--------|---------------|
| **GGA** | Standard GNSS | Lat/lon, fix quality, orthometric height (EGM96) |
| **RMC** | Standard GNSS | Lat/lon, ground speed (knots → m/s), course over ground |
| **$PTNL,GGK** | Trimble RTK | Lat/lon, RTK quality, ellipsoidal height (WGS84) |

**Coordinate format:** Latitude is parsed as 2-digit degrees + decimal minutes (ddmm.mmmm); longitude as 3-digit degrees + decimal minutes (dddmm.mmmm). S/W hemispheres are negated.

**Checksum validation:** Every sentence is verified via XOR checksum (`$BODY*XX`). Invalid checksums are rejected.

**Altitude datums from NMEA:** GGA reports orthometric height above EGM96. Trimble GGK reports WGS84 ellipsoidal height. The parser tags each reading with the correct `AltitudeDatum` so the downstream pipeline can handle both.

## Serial Manager

The serial manager handles USB GNSS receiver connections with automatic device detection and hot-plug recovery.

**Device detection:** Scans USB ports by VID/PID. Supported chipsets: u-blox, CP2102, FTDI. The CH340 chipset is rejected by project policy (unreliable timing).

**Connection parameters:** 115,200 baud, line-based protocol (one NMEA sentence per line).

**Hot-plug handling:** If the serial connection drops (device unplugged or EOF), the manager enters a 500 ms retry loop, re-scanning available ports until the device reappears.

**RTK degradation detection:** The manager monitors `fix_quality` transitions and logs a warning if quality drops from RTK Fixed (4) to a lower state. Broadcast event notification for this transition is planned for v0.4 completion.

**Feature gating:** The serial manager is behind the `serial` Cargo feature flag. Building without it omits all serial port dependencies.

## mDNS Service Discovery

The daemon advertises itself on the local network as `_irontrack._tcp.local.` using the `mdns-sd` crate. Clients (Glass Cockpit, Web UI) can discover the daemon without manual IP configuration.

**Known limitation:** mDNS can be unreliable on aircraft WiFi networks with client isolation enabled. The fallback chain is: mDNS → cached IP → manual entry.

## State Management

### FmsState

The central state container holds:

- `flight_plan: Option<FlightPlan>` — The currently loaded mission.
- `terrain_engine: Arc<TerrainEngine>` — Shared DEM query engine.
- `tx_telemetry: broadcast::Sender<CurrentState>` — 256-message broadcast channel.
- `tx_status: watch::Sender<SystemStatus>` — Latest-value status channel.
- `last_telemetry`, `last_status` — Cached for new WebSocket client initialisation.
- `persistence_path: Option<PathBuf>` — GeoPackage database for mission persistence.

### Concurrency Model

- **RwLock** for `FmsState` — Optimised for read-heavy access (telemetry queries far outnumber mission loads).
- **Broadcast channel** for telemetry fanout — Multiple WebSocket clients receive the same frames without contention.
- **Watch channel** for status — Newly connected clients immediately receive the latest status without needing to replay history.
- No locks are held across `.await` points.

### Persistence

A background task runs every 5 seconds, writing line completion percentages to the GeoPackage database. On graceful shutdown (SIGINT), the daemon runs `PRAGMA optimize` and `PRAGMA wal_checkpoint(TRUNCATE)`. VACUUM is explicitly prohibited per project policy.

## Mock Telemetry

When started with `--mock-telemetry`, the daemon generates synthetic GPS/attitude data by iterating through the loaded flight plan's waypoints at 10 Hz. Fix quality is hardcoded to 4 (RTK Fixed). This allows full REST/WebSocket integration testing without hardware.

---

## Deep Dive: Implementation Notes

### CORS

The daemon currently uses `CorsLayer::permissive()`, which allows requests from any origin. This is appropriate for localhost development but must be restricted before network deployment.

### Graceful Shutdown

On SIGINT (Ctrl+C), the daemon:

1. Stops accepting new connections.
2. Runs `PRAGMA optimize` on the GeoPackage database.
3. Checkpoints the WAL with `PRAGMA wal_checkpoint(TRUNCATE)`.
4. If the database is busy during checkpoint, the WAL file remains but the database is consistent (WAL mode guarantees this).

VACUUM is never called — it is prohibited in the project's architectural rules because it can block for seconds on large databases and is unsafe during flight operations.

### Security Roadmap

The current token system is ephemeral and not cryptographically secure. The security roadmap is:

- **v0.4:** Ephemeral bearer tokens (current state). Plaintext HTTP over WPA2/WPA3 LAN.
- **v1.0:** TLS with certificate pinning. Proper token rotation.

The design assumes a trusted LAN environment (aircraft WiFi) for the v0.4 release.

### Integration Testing

The `daemon_integration.rs` test suite exercises the full daemon lifecycle:

1. Start the daemon on a test port with mock telemetry.
2. POST a mission load request.
3. Verify the response contains valid line/waypoint counts.
4. Connect a WebSocket client.
5. Receive initial SystemStatus and telemetry frames.
6. PUT a line completion update and verify the WebSocket receives the status change.

### Reference Documents

- `28_irontrack_daemon_design.md` — Primary v0.4 reference: REST design, WebSocket protocol, state concurrency, mDNS, security.
- `29_avionics_serial_communication.md` — UART, CRC-32, RTS/CTS hardware flow control.
- `47_daemon_field_hardware.md` — Deployment targets: Raspberry Pi 4/5, Intel NUC, tablets.
- `42_geopackage_extensions.md` — WAL mode, TGCE, persistence details.
