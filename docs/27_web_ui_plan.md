# React Web Interface: Architecture and Implementation Strategy

> **Source document:** 27_Web_UI_Plan.docx
>
> **Scope:** Complete extraction — all content is unique to the project. Covers map library selection, client-side boundary parsing, polygon editing, real-time streaming preview, sensor library UI, export lifecycle, multi-user collaboration, and LAN security.

---

## 1. Map Library Selection

### 1.1 Evaluation

| Library | Rendering | Large Dataset Performance | License | GPLv3 Compatible | Air-Gapped Deployment |
|---|---|---|---|---|---|
| Leaflet | DOM (SVG/Canvas) | Poor (DOM bottleneck) | BSD 2-Clause | Yes | Yes |
| OpenLayers | Canvas/WebGL | Moderate | BSD 2-Clause | Yes | Yes |
| Mapbox GL JS | WebGL | Excellent | **Commercial / Proprietary** | **No** | Restricted (telemetry) |
| **MapLibre GL JS** | **WebGL / WebGPU** | **Excellent** | **BSD-style (FOSS)** | **Yes** | **Yes** |

### 1.2 Decision: MapLibre GL JS

MapLibre is the only library combining GPU-accelerated WebGL rendering (60 FPS with thousands of vector features), FOSS licensing compatible with GPLv3, and unrestricted air-gapped deployment. It is an open-source fork of Mapbox GL JS v1.x maintained by a robust community.

**React integration:** `react-map-gl` provides idiomatic React bindings for declarative viewport state, source, and layer management.

---

## 2. Boundary Ingestion and Geometry Editing

### 2.1 Client-Side Parsing (No Daemon Involvement)

All boundary file parsing happens in the browser to minimize network payload and backend processing.

**GeoJSON:** Native — `FileReader` → `JSON.parse()` → MapLibre `GeoJSONSource`. No external library needed.

**KML/KMZ:**
1. Read uploaded file as `ArrayBuffer`.
2. If KMZ: extract with `jszip`, isolate the `.kml` text payload.
3. Parse XML via DOM parser.
4. Transform to GeoJSON using `@tmcw/togeojson` (extracts `<Polygon>`, `<LineString>`, `<Point>`).
5. Discard styling assets — IronTrack operates on pure geometry.

**Shapefile (.shp/.shx/.dbf/.prj):**
1. User uploads a `.zip` containing all Shapefile components.
2. Parse with `shapefile.js` or `shapefile-geojson-js` — uses `DataView` and `ArrayBuffer` to read binary geometry headers.
3. If `.prj` file present: reproject to WGS84 using `proj4js`.

### 2.2 Polygon Editing Tools

**Library: `maplibre-gl-geo-editor`** — extends the Geoman library with survey-grade precision tools.

**Vertex snapping:** Continuously calculates Euclidean distance between cursor and all rendered vertices/edges within a local bounding box. When cursor enters a configurable pixel tolerance (default 15–18 px), the active vertex coordinates snap to the target geometry. Vertex snapping is prioritized over edge snapping for topological integrity.

**Exclusion zones (interior polygon holes):**
1. Operator draws an overlapping polygon over the area to exclude.
2. On completion, the React app uses Turf.js:
   - `turf.booleanContains` or `turf.intersect` detects the overlap.
   - `turf.difference` subtracts the new polygon from the primary block.
3. The result is a valid GeoJSON polygon with interior rings (exterior counter-clockwise, holes clockwise per RFC 7946).
4. Map source is updated with the restructured geometry.

**Multi-polygon support:** Full `MultiPolygon` type support for disjoint survey blocks planned simultaneously.

**Undo/redo:** Every destructive operation pushes a deep clone of the GeoJSON state onto an undo stack (default 50 operations). `Ctrl+Z` / `Ctrl+Y` traverses the history.

---

## 3. Real-Time Plan Preview via WebSocket Streaming

### 3.1 Latency Requirements

Human perception thresholds for interactive systems:

| Latency | Perception |
|---|---|
| < 100 ms | Instantaneous |
| 100–300 ms | Noticeable but acceptable |
| > 500 ms | Disrupts direct manipulation illusion |

As an operator adjusts AGL, sidelap, GSD, or sensor parameters, the recomputed flight lines must appear within 300 ms.

### 3.2 Why WebSockets (Not Polling)

HTTP short-polling requires new TCP connections, TLS negotiation, and verbose headers per update — unacceptable overhead for continuous parameter adjustment. WebSockets provide a persistent, full-duplex TCP connection over a single socket with minimal protocol overhead after the initial HTTP upgrade handshake.

### 3.3 Partial Streaming Architecture

Generating a terrain-following flight plan with thousands of trigger points takes seconds for large areas. Waiting for the full result before sending a monolithic GeoJSON freezes the UI.

**Solution:** The Rust math engine yields individual flight lines as they are generated. A `tokio::sync::broadcast` channel in the Axum state pushes each GeoJSON Feature to all connected WebSocket clients immediately.

```rust
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, _receiver) = socket.split();
    let mut rx = state.tx.subscribe();
    tokio::spawn(async move {
        while let Ok(geojson_feature_string) = rx.recv().await {
            if sender.send(Message::Text(geojson_feature_string)).await.is_err() {
                break;
            }
        }
    });
}
```

Flight lines appear on the map one by one as they're computed — the operator sees progress immediately rather than waiting for the entire block.

### 3.4 Controlling React Re-render Chaos

At 20 features/second from WebSocket, naive `setState` calls trigger 20 React reconciliation cycles/second — thousands of virtual DOM comparisons, causing severe UI blocking.

**Solution — two layers:**

1. **TanStack Query (React Query) for state:** Intercept WebSocket messages with `queryClient.setQueryData()` for optimistic, partial cache updates. Avoids direct `useState` thrashing.

```javascript
websocket.onmessage = (event) => {
    const newFlightLine = JSON.parse(event.data);
    queryClient.setQueryData(['flightPlan', planId], (oldData) => {
        if (!oldData) return { type: 'FeatureCollection', features: [newFlightLine] };
        return { ...oldData, features: [...oldData.features, newFlightLine] };
    });
};
```

2. **Direct MapLibre source update:** Use a `useRef` hook for the map instance. Call `map.getSource('flight-lines').setData(updatedFeatureCollection)` inside a `requestAnimationFrame` loop. This batches high-frequency network updates into the browser's native rendering cycle, bypassing the React virtual DOM entirely for map operations.

---

## 4. Sensor Library UI

### 4.1 Table Component: TanStack Table (Headless)

| Feature | React Data Table Component | TanStack Table |
|---|---|---|
| Architecture | Component-based (opinionated UI) | **Headless (logic only)** |
| Bundle size | ~94 KB | **~15 KB** |
| Styling flexibility | Moderate (themed API) | **Infinite (complete DOM control)** |
| Virtualization | Not built-in | **Fully supported** |
| IronTrack suitability | Low | **High** |

TanStack Table encapsulates sorting, filtering, and pagination logic but renders zero HTML/CSS. The development team builds a completely bespoke visual interface matching the dark industrial aesthetic (blacks, earth tones, amber accents).

### 4.2 Comparative Analytics

When multiple sensors are selected, a dynamic side-by-side comparison matrix shows the operational deltas: focal length, pixel pitch, sensor dimensions, resulting GSD and swath width at the configured AGL. The UI visually flags hardware that is mathematically incapable of fulfilling the mission profile (e.g., a LiDAR sensor whose max PRR cannot achieve the target point density at the planned altitude).

### 4.3 Profile Save/Load

Tuned sensor configurations are serialized to JSON, sent via REST POST to the daemon, and written into the GeoPackage's `irontrack_metadata` table. The profile becomes intrinsically linked to the flight plan and flows to field laptops and cockpit displays without external config files.

---

## 5. Export Lifecycle

### 5.1 Format Matrix

| Format | Purpose | Datum Handling |
|---|---|---|
| **GeoPackage (.gpkg)** | Primary artifact. SQLite + WAL, flight lines, sensor profiles, R-tree indices, cached DEM microtiles. | Internal canonical (EGM2008 or WGS84 Ellipsoidal) |
| **GeoJSON** | Lightweight web visualization, external GIS portals | WGS84 |
| **QGroundControl (.plan)** | ArduPilot/PX4 autopilots. MAVLink mission items array. | WGS84 Ellipsoidal |
| **DJI (.kmz)** | DJI enterprise FMS. WPML schema. | **EGM96 (mandatory datum shift: EGM2008 → WGS84 → EGM96)** |
| **Snapshot MDB** | Legacy Eagle Mapping acquisition pipeline compatibility | Per legacy spec |

### 5.2 Pre-Flight Metadata Verification Modal

Before any export, the UI surfaces a mandatory verification screen:

- **Schema version** — ensures planning engine matches the airborne daemon.
- **Copernicus attribution** — mandatory legal compliance strings.
- **DSM penetration warning** — if terrain source is a DSM (Copernicus), a non-dismissable safety warning: "AGL clearance over forests may be UNDERESTIMATED by 2–8 m. Verify obstacle clearance manually."
- **Altitude datum** — explicitly displays the datum tag for the export format.

The DSM warning is hardcoded into the export payload and cannot be suppressed.

### 5.3 Batch Export

The "Batch Export" function generates all formats simultaneously:

1. React dispatches a POST to the daemon with export parameters.
2. Rust backend synthesizes all formats, compresses into a ZIP archive in memory.
3. Streams the binary buffer to the client (`Content-Type: application/octet-stream`).
4. React creates a `Blob` from the response, generates a temporary `URL.createObjectURL()`, creates an invisible `<a>` element with a `download` attribute, and fires a programmatic `.click()`.
5. File saves without page navigation.

---

## 6. Multi-User Collaboration

### 6.1 Why Not CRDTs

CRDTs (Conflict-free Replicated Data Types) power real-time co-editing in Google Docs and Figma. They guarantee mathematical convergence of concurrent edits without a central arbiter.

**But CRDTs are rejected for IronTrack v1.0.** In a spatial context, a CRDT can merge two concurrent edits perfectly at the data level while producing a semantically dangerous result: User A increases altitude while User B narrows line spacing — the merged plan violates aerodynamic stability margins or terrain-following safety constraints. Silent algorithmic merging of flight parameters could cause terrain collision.

| Feature | Pessimistic Locking | CRDTs |
|---|---|---|
| Concurrency | Serialized (turn-based) | Fully concurrent |
| Conflict resolution | Prevented by design | Mathematical convergence |
| **Aviation safety risk** | **Low** | **High (semantic contradictions)** |

### 6.2 Granular Optimistic Locking via WebSocket

The Axum daemon is the single source of truth.

1. Operator opens an editing subsystem (e.g., Sensor Configuration modal, Polygon Draw tool).
2. React client emits a **lock request** over WebSocket specifying the subsystem.
3. Daemon registers the lock and broadcasts `STATE_LOCKED` to all other connected clients.
4. Other operators' UIs **gray out** the locked subsystem — read-only visibility, no conflicting input.
5. When the editing operator saves or closes, the lock is released and broadcast.

**Race condition backstop:** If network latency allows two clients to submit before the lock propagates, the daemon enforces **Last-Write-Wins** at the database transaction level. The rejected client receives a notification prompting a state resync.

---

## 7. Security Architecture

### 7.1 Threat Model

The daemon runs on isolated LANs — field laptops, NUCs, ruggedized tablets in remote airstrips. No public internet. Heavyweight IAM (OAuth 2.0, OIDC, RBAC) is unnecessary fragility.

But treating a LAN as implicitly secure is dangerous. On a crowded airfield or shared office network, an unauthenticated API allows any device on the subnet to submit DELETE requests, overwrite flight plans, or corrupt trigger waypoints.

### 7.2 Ephemeral Token Authentication

**Daemon startup:** The Rust core generates a cryptographically secure, randomized Bearer token for the session. The token is printed to stdout on the host terminal.

**Client gateway:** When an operator navigates to the React interface, they enter the session token. The app stores it in `sessionStorage`.

**REST requests:** Axios interceptor appends the token:
```
Authorization: Bearer <token>
```

**WebSocket:** Browser WebSocket APIs don't support custom headers during the upgrade handshake. The token is passed as a query parameter:
```
ws://192.168.1.100:3000/ws?token=<token>
```

The Axum `WebSocketUpgrade` extractor parses the query string, verifies the token, and either accepts the upgrade or terminates with HTTP 401.

### 7.3 TLS

For v1.0, the LAN deployment model does not require TLS. If the daemon is exposed beyond the local network in future, TLS can be added via `axum-server` with `rustls` without architectural changes.
