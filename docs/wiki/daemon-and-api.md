# Daemon & API

IronTrack's network daemon provides a REST API and WebSocket telemetry stream for real-time flight management. It is the primary deliverable of v0.4 and the interface layer between the core engine and all client applications (Glass Cockpit, Web Planning UI, third-party integrations).

## Overview

The daemon is started with:

```bash
irontrack daemon --port 8080 --mock-telemetry
```

Full startup options:

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | 8080 | HTTP/WS listen port |
| `--mock-telemetry` | off | Synthetic NMEA along loaded flight plan |
| `--mock-speed` | 15.0 | Mock aircraft ground speed (m/s) |
| `--gnss-vid` | 0x1546 | USB Vendor ID for GNSS receiver |
| `--gnss-pid` | 0x01A9 | USB Product ID for GNSS receiver |
| `--gnss-baud` | 115200 | Serial baud rate |
| `--cors-origin` | (none) | Allowed CORS origins (repeatable) |

It provides three communication channels:

1. **REST API** (`/api/v1/`) — Mission CRUD, terrain queries, export generation.
2. **WebSocket** (`/ws`) — 10 Hz telemetry stream plus mission status updates.
3. **mDNS** (`_irontrack._tcp.local.`) — Zero-configuration service discovery on the LAN.

The daemon is built on Axum (Tokio runtime) with `deadpool-sqlite` connection pooling and `Arc<RwLock<FmsState>>` for thread-safe shared state.

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

A token query parameter is required (browser WebSocket APIs do not support custom headers during HTTP upgrade). Missing or invalid tokens receive a 401 response. Tokens are ephemeral: a 32-byte cryptographically random hex string generated at daemon startup and printed to stdout. Verification uses constant-time comparison (`subtle::ConstantTimeEq`) to prevent timing side-channel attacks. Full TLS with certificate pinning is planned for v1.0.

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

**Telemetry stream (~10 Hz):** All server messages are wrapped in a discriminated `ServerMsg` envelope with a `type` field. Telemetry frames use the `TELEMETRY` type:

```json
{
  "type": "TELEMETRY",
  "payload": {
    "lat": 49.2827,
    "lon": -123.1207,
    "alt": 150.5,
    "alt_datum": "Egm2008",
    "ground_speed_ms": 25.3,
    "true_heading_deg": 270.0,
    "fix_quality": 4,
    "hdop": 1.2,
    "satellites_in_use": 14,
    "geoid_separation_m": -17.4,
    "utc_time": "143025.00",
    "utc_date": "290326"
  }
}
```

**Fix quality values:** 0 = No fix, 1 = Autonomous, 2 = DGPS, 4 = RTK Fixed, 5 = RTK Float.

**Message types:**

| Type | Direction | Channel | Description |
|------|-----------|---------|-------------|
| `INIT` | Server → Client | On connect | Full `SystemStatus` snapshot |
| `TELEMETRY` | Server → Client | broadcast (10 Hz) | GPS/attitude `CurrentState` |
| `STATUS` | Server → Client | watch | Line completion, active line changes |
| `TRIGGER` | Server → Client | broadcast | Sensor trigger event (lat/lon/timestamp) |
| `WARNING` | Server → Client | broadcast | RTK degradation, GNSS loss, system alerts |

**Status updates:** Sent whenever mission state changes (e.g., line completion updated via PUT endpoint). Uses a `tokio::sync::watch` channel so newly connected clients always receive the latest status immediately.

### Client Messages

Clients can send JSON messages to the server:

```json
{"type": "SELECT_LINE", "index": 3}
```

This updates the `active_line_index` in `SystemStatus` and toggles `is_active` flags for the corresponding line.

### Multiplexing

The WebSocket handler uses `tokio::select!` to multiplex five event sources concurrently: the telemetry broadcast receiver, the status watch receiver, the trigger broadcast receiver, the warning broadcast receiver, and incoming client messages. Lagged telemetry messages (when a slow client falls behind the 256-message buffer) are silently dropped — this is intentional. At 80 m/s, stale position data is dangerous; rendering a 5-second-old frame is worse than skipping it.

**Connection lifecycle:** On upgrade, the handler subscribes to all channels *before* reading the watch snapshot. This prevents a race condition where a status change between subscribe and read would be lost. A 30-second ping interval detects stale clients (WiFi dropout, tablet sleep).

## NMEA Parser

The daemon parses three NMEA sentence types from GNSS receivers:

| Sentence | Source | Fields Parsed |
|----------|--------|---------------|
| **GGA** | Standard GNSS | Lat/lon, fix quality, orthometric height (EGM96) |
| **RMC** | Standard GNSS | Lat/lon, ground speed (knots → m/s), course over ground |
| **$PTNL,GGK** | Trimble RTK | Lat/lon, RTK quality, ellipsoidal height (WGS84) |

**Coordinate format:** Latitude is parsed as 2-digit degrees + decimal minutes (ddmm.mmmm); longitude as 3-digit degrees + decimal minutes (dddmm.mmmm). S/W hemispheres are negated.

**Checksum validation:** Every sentence is verified via XOR checksum (`$BODY*XX`). Invalid checksums are rejected.

**Epoch pairing:** The parser buffers a GGA sentence until a matching RMC arrives with the same UTC timestamp. Only a complete GGA+RMC pair produces a `CurrentState` update. Stale GGA epochs (UTC mismatch) are silently consumed. This ensures position and kinematic data are always from the same fix.

**Altitude datums from NMEA:** GGA reports orthometric height above EGM96. Trimble GGK reports WGS84 ellipsoidal height. The parser tags each reading with the correct `AltitudeDatum` so the downstream pipeline can handle both.

**Error handling:** The parser never defaults to silent 0.0 values on malformed fields. If the altitude or geoid separation field fails to parse, the entire epoch is rejected and the raw sentence is logged at `warn` level. This prevents a sea-level phantom reading from reaching the cockpit display.

## Serial Manager

The serial manager handles USB GNSS receiver connections with automatic device detection and hot-plug recovery.

**Device detection:** Scans USB ports by VID/PID at startup. Default: u-blox (0x1546:0x01A9). Override with `--gnss-vid` / `--gnss-pid`. Supported chipsets: u-blox, CP2102, FTDI. The CH340 chipset is rejected by project policy (macOS kernel panics on Apple Silicon, unreliable timing under EMI). Device paths are never hardcoded (`/dev/ttyUSB0`, `COM3`); the manager always resolves from VID/PID.

**Connection parameters:** Configurable baud rate (default 115,200, override with `--gnss-baud`). Line-based protocol (one NMEA sentence per line).

**Hot-plug handling:** Aircraft vibration and EMI cause USB brown-outs. When the serial connection drops (ConnectionReset, BrokenPipe, or EOF), the manager follows a 5-step recovery: (1) drop the file descriptor, (2) async backoff (500 ms), (3) re-enumerate available ports and re-match VID/PID (device path may change, e.g., ttyUSB0 to ttyUSB1), (4) open a fresh async serial stream, (5) resume the read loop.

**RTK degradation detection:** The manager monitors `fix_quality` transitions with 20-epoch debouncing (2 seconds at 10 Hz) to reject transient spikes. When quality drops from RTK Fixed (4) to Float (5) or lower, a `ServerMsg::Warning` is broadcast to all connected WebSocket clients with the degradation event, timestamp, and new quality level. This appears as an amber alert on cockpit displays.

**Heartbeat watchdog:** A 3-second timeout detects GNSS silence (disconnected receiver, dead battery). On timeout, the manager broadcasts a loss-of-signal warning.

**Feature gating:** The serial manager is behind the `serial` Cargo feature flag. Building without it omits all serial port dependencies (`serialport`, `tokio-serial`).

## mDNS Service Discovery

The daemon advertises itself on the local network as `_irontrack._tcp.local.` using the `mdns-sd` crate. Clients (Glass Cockpit, Web UI) can discover the daemon without manual IP configuration.

**Known limitation:** mDNS can be unreliable on aircraft WiFi networks with client isolation enabled. The fallback chain is: mDNS → cached IP → manual entry.

## State Management

### FmsState

The central state container holds:

- `flight_plan: Option<FlightPlan>` — The currently loaded mission.
- `terrain_engine: Arc<TerrainEngine>` — Shared DEM query engine.
- `tx_telemetry: broadcast::Sender<CurrentState>` — 256-message telemetry fanout.
- `tx_status: watch::Sender<SystemStatus>` — Latest-value status channel.
- `tx_trigger: broadcast::Sender<TriggerEvent>` — Sensor trigger event relay.
- `tx_warning: broadcast::Sender<ServerMsg>` — RTK/GNSS/system warnings.
- `trigger_log: VecDeque<TriggerEvent>` — Ring buffer (max 10,000 events) for post-flight review.
- `persistence_path: Option<PathBuf>` — GeoPackage database for mission persistence.

The design deliberately uses a centralized `Arc<RwLock<FmsState>>` rather than distributed state (CRDTs). CRDT-based collaboration was evaluated and rejected as a locked architectural decision (Manifest v3.5, Doc 28): semantic merge conflicts in flight plan geometry can produce physically infeasible trajectories that are silently accepted by convergence.

### Concurrency Model

- **`tokio::sync::RwLock`** (not `std::sync::RwLock`) for `FmsState` — Optimised for read-heavy access (many WebSocket readers vs. one NMEA writer). `tokio::sync::RwLock` uses a FIFO queue that prevents writer starvation across `.await` points.
- **Broadcast channels** for telemetry, trigger, and warning fanout — Multiple WebSocket clients receive the same frames without contention.
- **Watch channel** for status — Newly connected clients immediately receive the latest status without needing to replay history.
- Heavy math (UTM projection, R-tree queries, Karney distance) runs on rayon threads via `tokio::task::spawn_blocking`, preventing 500 ms computations from starving NMEA ingestion.
- No locks are held across `.await` points.

### Connection Pooling (`deadpool-sqlite`)

The daemon uses `deadpool-sqlite` for SQLite connection management. Every connection from the pool has session-level PRAGMAs applied:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -64000;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 0;
```

`busy_timeout = 0` means the daemon demands immediate failure, not silent retry. If a second process has the database locked, the connection fails instantly rather than waiting — this is a safety mechanism, not a performance compromise (see SQLITE_BUSY below).

**WAL mode constraint:** WAL fails catastrophically on network filesystems (NFS, SMB, CIFS). The GeoPackage must reside on local storage. This is enforced by the architecture but not currently validated at startup.

### SQLITE_BUSY Handling

SQLITE_BUSY is **not** a transient retry condition. It is a fatal architectural violation (Manifest v3.5, Doc 42). The WAL-reset bug can cause permanent, unrecoverable database corruption under blind retry.

Detection uses the structured `rusqlite::ffi::ErrorCode::DatabaseBusy` match (not string matching) via a dedicated `is_sqlite_busy()` helper. On detection:

1. The error is logged at `error` level with full context.
2. A `ServerMsg::Warning` with code `ERR_DB_LOCKED` is broadcast to all WebSocket clients.
3. The offending task halts.

### Persistence

A background task runs every 5 seconds, writing line completion percentages to the GeoPackage database via the connection pool. Every 12th tick (~60 seconds), the task runs `PRAGMA wal_checkpoint(PASSIVE)` to bound WAL growth during long flights. PASSIVE is non-blocking: it checkpoints only pages not currently read-locked, which is safe during active cockpit display queries.

On graceful shutdown (SIGINT), the daemon runs `PRAGMA optimize` followed by `PRAGMA wal_checkpoint(TRUNCATE)`. VACUUM is explicitly prohibited per project policy — it acquires an exclusive lock that would freeze cockpit displays for minutes on large databases and can double storage requirements.

## Mock Telemetry

When started with `--mock-telemetry`, the daemon generates synthetic GPS/attitude data by iterating through the loaded flight plan's waypoints at 10 Hz. Fix quality is hardcoded to 4 (RTK Fixed). This allows full REST/WebSocket integration testing without hardware.

---

## Deep Dive: Implementation Notes

### CORS

The daemon accepts configurable CORS origins via `--cors-origin` (repeatable flag). Without explicit origins, CORS is restricted to localhost. For LAN deployment (e.g., tablet connecting to a Pi), pass the client's origin: `--cors-origin http://192.168.1.50:3000`.

### Graceful Shutdown

On SIGINT (Ctrl+C), the daemon:

1. Stops accepting new connections.
2. Runs `PRAGMA optimize` on the GeoPackage via the connection pool (or a direct connection as fallback).
3. Checkpoints the WAL with `PRAGMA wal_checkpoint(TRUNCATE)`.
4. If the database is busy during checkpoint, the WAL file remains but the database is consistent (WAL mode guarantees this).

VACUUM is never called — it is prohibited in the project's architectural rules because it acquires an exclusive lock that can block for minutes on large databases and is unsafe during flight operations.

### Security Roadmap

- **v0.4 (current):** Ephemeral bearer tokens (32-byte hex, constant-time comparison via `subtle`). REST endpoints require `Authorization: Bearer <token>` header. WebSocket passes token via query parameter (`?token=<token>`). Plaintext HTTP over WPA2/WPA3 LAN. Token is printed to stdout at daemon startup.
- **v1.0 (planned):** Local Root CA generated during installation. Distribute CA cert to operator devices. TLS for zero-trust edge security. Token rotation.

The design assumes a trusted LAN environment (aircraft WiFi) for the v0.4 release. The bearer token is transmitted in plaintext and is vulnerable to sniffing on open WiFi.

### Integration Testing

The `daemon_integration.rs` test suite (15 tests) exercises the full daemon lifecycle:

1. Start the daemon on a test port with mock telemetry.
2. POST a mission load request and verify the response contains valid line/waypoint counts.
3. Connect a WebSocket client and verify the `INIT` frame contains a full `SystemStatus`.
4. Verify 10 Hz telemetry frames arrive with `TELEMETRY` envelope.
5. PUT a line completion update and verify all connected WebSocket clients receive the `STATUS` change.
6. Simulate RTK degradation in mock telemetry and verify the `WARNING` message appears.
7. Kill and restart the daemon, verify WAL recovery restores line completion status.
8. Verify the `/mission/export/qgc` and `/mission/export/dji` endpoints stream valid files.
9. Verify 401 rejection on missing, invalid, and malformed tokens.
10. Verify structured error JSON responses on invalid input (`ERR_GEOID_MISSING`, `ERR_NO_MISSION`, etc.).

### Reference Documents

- `28_irontrack_daemon_design.md` — Primary v0.4 reference: REST design, WebSocket protocol, state concurrency, mDNS, CRDT rejection, security.
- `29_avionics_serial_communication.md` — UART, VID/PID detection, CH340 rejection, hot-plug, CRC-32, RTS/CTS hardware flow control.
- `42_geopackage_extensions.md` — WAL mode, SQLITE_BUSY fatality, VACUUM prohibition, TGCE, persistence details.
- `47_daemon_field_hardware.md` — Deployment targets: Raspberry Pi 4/5, Intel NUC, ruggedized tablets.
