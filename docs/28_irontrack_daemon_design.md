# IronTrack Network Daemon: Architecture and Engineering Blueprint

> **Source document:** 28_IronTrack-Daemon-Design.docx
>
> **Scope:** Complete extraction — all content is unique to the project. Covers REST API design, WebSocket telemetry, state management concurrency, mDNS service discovery, network resilience, and LAN security.

---

## 1. REST API Design

### 1.1 Resource Model and CRUD Operations

| HTTP Method | Endpoint | Description |
|---|---|---|
| `POST` | `/api/v1/missions` | Create: ingest boundary polygon (KML/KMZ/GeoJSON), apply sensor config, auto-generate flight lines |
| `GET` | `/api/v1/missions/{id}` | Read: mission metadata (bounding box, sensor config, completion stats) |
| `GET` | `/api/v1/missions/{id}/lines` | Read: flight line array (SoA lat/lon/elevation per waypoint) |
| `PUT` | `/api/v1/missions/{id}/lines/{line_id}` | Update: reposition line, adjust altitude, compensate for environment |
| `DELETE` | `/api/v1/missions/{id}/lines/{line_id}` | Delete: remove line, recalculate block stats and adjacent spacing |
| `GET` | `/api/v1/missions/{id}/export` | Export: finalize plan, serve GeoPackage as downloadable binary |

Resource-oriented routing (not RPC). Axum's routing macro system provides compile-time verification of path parameters.

### 1.2 API Versioning: Path-Based `/api/v1/`

| Strategy | Mechanism | IronTrack Suitability |
|---|---|---|
| **Path versioning** | `/api/v1/missions` | **Selected.** Visible in logs, simple Axum nested routing, cache-friendly, debuggable in field via curl. |
| Header versioning | `Accept: application/v1+json` | Poor discoverability, hard to debug without packet inspection. |
| Hostname versioning | `v1.api.irontrack.local` | Requires complex local DNS, infeasible in ad-hoc aircraft networks. |

Semantic versioning: minor backward-compatible features absorbed into v1. v2 path only for schema-breaking changes.

### 1.3 Dual Response Formats: JSON + Binary Streaming

**JSON (standard CRUD):** Axum `Json` extractor with `serde::Serialize`/`Deserialize`. Automatic 400 Bad Request on malformed or missing fields.

**Binary GeoPackage export:** The GeoPackage can be megabytes to gigabytes (flight plan + spatial indices + cached DEM tiles). Cannot serialize through JSON.

- `Content-Type: application/geopackage+sqlite3` (OGC standard MIME type)
- `Content-Disposition: attachment; filename="mission_plan.gpkg"`
- Stream via `StreamBody` wrapping `tokio::fs::File` — low, stable memory footprint
- Alternatively, `axum_extra::response::Attachment` for convenience

This architecture prevents OOM panics even when serving massive terrain datasets to multiple field laptops simultaneously.

### 1.4 Structured Error Handling

No generic 500 errors. Every failure returns a structured JSON payload:

```json
{
  "status": 422,
  "code": "ERR_GEOID_MISSING",
  "message": "EGM2008 grid data not found for the requested coordinates. Ensure the DEM cache covers the survey area."
}
```

**Components:**
- **HTTP Status Code:** Semantically appropriate (400 syntax, 422 validation, 409 state conflict)
- **Internal Error Code:** Programmatic (e.g., `ERR_GEOID_MISSING`, `ERR_SCHEMA_MISMATCH`) — frontend triggers specific UI recovery flows without string matching
- **Human-Readable Message:** Diagnostic detail for the operator

Uses `thiserror` for library errors and `anyhow` for CLI. Error enums implement `axum::response::IntoResponse` for centralized mapping.

---

## 2. WebSocket Architecture

### 2.1 10 Hz Telemetry Broadcast

At 80 m/s, a 1 Hz update = 80 meters of blind travel. **10 Hz is the minimum** — the pilot receives visual feedback every 8 meters.

**CurrentState payload:**
- Latitude, Longitude (f64, high-precision)
- Altitude (datum-tagged: AGL or EGM2008)
- Ground Speed, True Heading (from RMC)
- Fix Quality (autonomous / DGPS / RTK Float / RTK Fixed)

**Channel: `tokio::sync::broadcast`** — multi-producer, multi-consumer. Every subscriber receives an identical copy of every message.

**Slow receiver behavior:** If a client's network stalls and the receiver buffer fills, Tokio automatically drops the oldest messages (`RecvError::Lagged`). When the network recovers, the client snaps to the latest position — no replay of stale frames. **This is intentional:** rendering 5-second-old positional data is actively dangerous for a pilot on a tight line.

### 2.2 Bidirectional Line Selection and Trigger Events

**Line selection sync:** When an operator or pilot selects a line, the client sends a JSON message up the WebSocket → the daemon's Tokio task acquires a write lock → mutates the active line in FmsState → broadcasts the state-change event back through the broadcast channel → all connected clients update simultaneously (amber highlight on the selected line).

**Trigger event relay:** The hardware trigger controller reports firing events (sequence number, coordinate, timestamp) back to the daemon over serial. The daemon wraps these in a WebSocket envelope and broadcasts them. The glass cockpit renders green confirmation dots along the flight path in real-time.

### 2.3 Mid-Mission Join: Hybrid Channel Architecture

**Problem:** If an operator restarts their device mid-flight, a new WebSocket connection can't just wait for the next 10 Hz tick. The client needs the full system context: which line is active, completion status of all lines, trigger event counts.

**Solution — two channels:**

| Channel | Type | Purpose | Behavior |
|---|---|---|---|
| `tokio::sync::broadcast` | Multi-consumer, lossy | 10 Hz transient kinematic telemetry | High-frequency, drops stale frames for slow receivers |
| `tokio::sync::watch` | Single-value, always-current | Full system state snapshot | Retains only the latest value. Always readable. |

**Connection flow:**
1. New WebSocket upgrade is accepted.
2. Handler immediately reads the `watch` channel → sends the full state as the **first initialization frame**.
3. Client parses the heavy init frame (all line statuses, active line, trigger counts, sensor config).
4. Handler enters the `broadcast` loop for 10 Hz kinematic deltas.

**Result:** Zero-latency resynchronization regardless of when a client joins or rejoins.

---

## 3. State Management and Concurrency

### 3.1 FmsState: `Arc<RwLock<FmsState>>`

**FmsState fields:**
1. **Flight Plan** — parsed geometry, all lines and waypoints
2. **Current GPS State** — latest NMEA solution (coordinates, speed, heading, fix quality)
3. **Active Line** — reference to the currently selected/navigated line
4. **Line Completion Status** — per-line progress percentage and success/failure flags
5. **Trigger Event Log** — historical ledger of every coordinate where the controller fired

**Why RwLock, not Mutex:**
- The read-to-write ratio is heavily skewed: many WebSocket readers (glass cockpit, operator laptop, observer tablets) vs. sparse writers (NMEA parser, REST PUT).
- `Mutex` enforces mutual exclusion for all access. A slow WebSocket client serializing a large JSON payload would block the critical NMEA parser from updating the aircraft's position.
- `RwLock` allows infinite concurrent readers. Only writers acquire exclusive access.
- Must use **`tokio::sync::RwLock`** (not `std::sync::RwLock`) if the lock is held across `.await` points. Tokio's implementation uses an internal FIFO queue that prevents writer starvation.

| Primitive | Role | Justification |
|---|---|---|
| `Arc<T>` | Atomic reference counting | Shares ownership across Tokio tasks and OS threads |
| `RwLock<T>` | Read-write lock | Multiple WebSocket readers don't block each other; exclusive access only for NMEA writer |
| `tokio::sync::RwLock` | Async-aware locking | Prevents OS thread blocking across `.await`; FIFO queue prevents writer starvation |

### 3.2 The Async Boundary Rule in Practice

Heavy geodesic math (UTM projection, R-tree intersection, Karney distance) runs on rayon threads. If an Axum handler locks a Tokio worker for 500 ms of computation, the entire network layer stutters — NMEA ingestion halts, WebSocket telemetry freezes.

**Every handler that calls the math engine must use `tokio::task::spawn_blocking`.** This moves the closure off the Tokio executor onto a dedicated synchronous thread pool. The async handler awaits the result without blocking the network threads.

### 3.3 WAL-Mode Persistence for Crash Recovery

The FmsState is RAM-resident for nanosecond access. But it must survive power losses (frequent in aging aircraft electrical systems) and daemon restarts.

**Strategy:** A low-priority background Tokio task periodically:
1. Acquires a read lock on FmsState.
2. Clones the line completion matrix.
3. Writes it to the GeoPackage via `spawn_blocking`.

**SQLite WAL mode (`PRAGMA journal_mode=WAL`):**
- Default SQLite uses rollback journal: exclusive lock during write, blocks all readers.
- WAL mode appends changes to a separate `-wal` file: **readers never block writers, writers never block readers.**
- The React UI can `GET` the flight plan from the same GeoPackage while the daemon is writing completion status — both succeed without contention.
- WAL contents merge back into the main database during automatic background checkpoint operations.

---

## 4. Service Discovery: mDNS/DNS-SD

### 4.1 Zero-Configuration Discovery

The daemon broadcasts `_irontrack._tcp.local.` via mDNS (RFC 6762/6763) using the `mdns-sd` Rust crate. Clients issue mDNS queries to `224.0.0.251:5353`. The daemon responds with its IP and port. The client populates a list of available daemons — the user connects with a single click.

### 4.2 Why mDNS Fails in Aircraft (and How to Handle It)

| Failure Mode | Cause | Impact |
|---|---|---|
| **Low-rate multicast airtime hogging** | 802.11 transmits multicast at the lowest basic data rate | Chatty mDNS consumes disproportionate WiFi airtime |
| **No multicast ACKs** | Standard 802.11 doesn't acknowledge multicast frames | mDNS packets are exceptionally vulnerable to loss and collision |
| **AP isolation / multicast filtering** | Commercial routers enable these by default | UDP 5353 is blackholed — mDNS completely neutralized |
| **Intel WiFi chipset bug** | Specific chipsets block multicast after sleep/resume | Host becomes completely blind to mDNS after waking from airplane mode |

### 4.3 Fallback Chain

1. **mDNS discovery** (attempt first, 3-second timeout)
2. **Cached last-known-good IP** from persistent local storage (tried on timeout)
3. **Manual IP entry** (fallback UI if both above fail)

**Operational recommendation:** Configure a static DHCP reservation for the daemon host on the aircraft router. The client caches this IP and attempts a direct unicast connection before falling back to the slower multicast query on subsequent launches.

---

## 5. Network Resilience

### 5.1 WiFi Drop Immunity via the Real-Time Boundary

If WiFi dies mid-flight, the trigger controller continues autonomously (pre-loaded waypoints, direct NMEA UART, no network dependency). Data acquisition is perfectly insulated from all network topology failures. The only impact: the pilot temporarily loses the glass cockpit telemetry feed.

### 5.2 WebSocket Reconnection: Exponential Backoff with Jitter

When a WebSocket drops:
1. Wait base interval (1 second).
2. If reconnection fails, double the interval (2s, 4s, 8s...) up to a maximum threshold.
3. Apply randomized jitter to prevent thundering-herd reconnection from multiple clients.
4. On successful reconnection: REST query for updated config + `watch` channel init frame for full state resync.

**Localhost immunity:** If the glass cockpit runs on the same physical device as the daemon (e.g., ruggedized tablet), it connects via `127.0.0.1` — the loopback interface never touches the WiFi adapter and is immune to RF interference or router failure. The pilot never loses navigation guidance even if the cabin network collapses.

### 5.3 Multi-Interface Binding

Binding to `0.0.0.0:3000` listens on all active interfaces (e.g., `eth0` for hardwired LiDAR scanner + `wlan0` for operator UI). The OS routing table multiplexes traffic naturally — no need for separate Axum instances on different ports.

---

## 6. LAN Security

### 6.1 Threat Model

Aircraft cabin WiFi is frequently open or uses a shared PSK. The network is geographically local but fundamentally untrusted.

**Attack vectors without auth:**
- **Passive eavesdropping:** Any device in range sniffs the flight plan and aircraft position.
- **State manipulation:** An accidental HTTP PUT from another device alters sensor parameters or deletes lines mid-mission.
- **Spoofing:** A rogue device advertises itself as the IronTrack daemon via mDNS and feeds false navigation guidance.

The financial cost of a ruined acquisition flight (fuel + crew) makes minimum viable security an architectural mandate.

### 6.2 Bearer Token Middleware

A cryptographically secure random token is generated during daemon configuration and stored in `irontrack.toml`. Injected into authorized client devices.

**REST protection:** Axum `middleware::from_fn` wraps all sensitive routes. The middleware:
1. Extracts the `Authorization` header.
2. Verifies `Bearer <token>` format via constant-time comparison.
3. Valid → passes request to handler. Invalid → returns 401 Unauthorized.

**WebSocket protection:** Browser WebSocket APIs don't support custom headers during the HTTP upgrade. Token is passed via query parameter: `ws://host:3000/ws?token=XYZ`. Extracted and verified before `ws.on_upgrade` — the TCP connection is severed immediately if auth fails.

### 6.3 TLS Strategy

**v1.0:** Plaintext HTTP over WPA2/WPA3 LAN. Bearer tokens prevent unauthorized access but are transmitted in plaintext (vulnerable to packet sniffing on open WiFi).

**Future:** Generate a local Root CA during daemon installation. Distribute the CA cert to operator devices. Enable TLS without public CA dependency or browser security warnings. Full zero-trust security on the edge without Let's Encrypt or public internet access.
