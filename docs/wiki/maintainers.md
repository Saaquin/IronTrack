# Maintainers

## Core Engine Architecture (v0.3.1)

The IronTrack engine is ~40,000 lines of Rust across 44 source files organized into 11 modules. All compute is synchronous and parallelized via `rayon`; `tokio` is reserved for network I/O in the daemon layer.

### 1. Geodesy Subsystem (`src/geodesy/`)

- `karney.rs`: Thin wrapper for `geographiclib-rs`. Solves inverse/direct geodesic problems with 15 nm accuracy.
- `utm.rs`: Karney-Krüger 6th-order WGS84 ↔ UTM projection. Coefficients α₁…₆ and β₁…₆ are hard-coded rational fractions from Karney (2011). No third-party bindings.
- `geoid.rs`: Dual-model lookup (EGM96/EGM2008). Bilinear interpolation over 1-arcminute grids.

### 2. Terrain Subsystem (`src/dem/`)

- `copernicus.rs`: Native `tiff` crate parser for Copernicus GLO-30 FLOAT32 tiles. Handles DEFLATE+Predictor=3 compression in memory.
- `cache.rs`: Cross-platform tile manager. Automatically fetches missing tiles from AWS S3 via `reqwest`.
- `TerrainEngine` (`mod.rs`): Implements the Two-Phase I/O rule — pre-warms tiles sequentially, then dispatches batch queries via `rayon::par_iter`.

### 3. Photogrammetry Subsystem (`src/photogrammetry/`)

- `sensor.rs` (373 lines): GSD/FOV/swath math. Three built-in presets (Phantom 4 Pro, Mavic 3, iXM-100) plus custom sensor parameters.
- `flightlines.rs` (2,257 lines): Parallel flight line generator with Kahan-summed geodesic tracing. Terrain-aware AGL adjustment samples DEM per-waypoint.
- `corridor.rs` (719 lines): Centerline offset polylines with miter/bevel joins, self-intersection removal, and waypoint resampling.
- `lidar.rs` (382 lines): LiDAR swath geometry and point density kinematics (PRR, scan rate, FOV, loss fraction).

### 4. Trajectory Subsystem (`src/trajectory/`) — New in v0.3

- `elastic_band.rs`: Energy minimization solver for terrain-following path smoothing.
- `bspline_smooth.rs`: Cubic B-spline C² fitting with convex hull safety guarantee (if control polygon satisfies AGL, entire curve is safe).
- `pursuit.rs`: Dynamic look-ahead pure pursuit controller.
- `validation.rs`: Synthetic test suite (flat, sinusoidal, step, sawtooth terrain profiles).

### 5. Math Subsystem (`src/math/`)

- `dubins.rs` (858 lines): Six Dubins path types (LSL, RSR, LSR, RSL, LRL, RLR) with clothoid transitions, arc discretization, and jerk-limited roll.
- `routing.rs` (507 lines): Nearest-neighbor + 2-opt TSP heuristic with Karney geodesic cost matrix and line-reversal logic.
- `geometry.rs` (228 lines): Polygon with holes, point-in-polygon, bounding box computation.
- `numerics.rs` (174 lines): Kahan summation for compensated floating-point accumulation.

### 6. I/O Subsystem (`src/gpkg/` and `src/io/`)

- `gpkg/`: OGC GeoPackage with LINESTRINGZ geometry, 48-byte 3D envelopes, R-tree spatial index, and custom `ST_` functions registered in SQLite.
- `io/geojson.rs`: RFC 7946 FeatureCollection with per-line altitude ranges.
- `io/kml.rs`: KML/KMZ import for boundary and centerline ingestion.
- `io/qgc.rs`: QGroundControl `.plan` JSON export.
- `io/dji.rs`: DJI WPML `.kmz` export.

### 7. Datum Subsystem (`src/datum.rs`)

`FlightLine::to_datum()` converts between EGM2008, EGM96, WGS84 Ellipsoidal, and AGL altitude references. All transforms happen at ingestion time (never during flight execution). WGS84 Ellipsoidal is the canonical pivot datum. Parallelized via `rayon`.

### 8. Network Daemon (`src/network/`) — v0.4 Target

- `server.rs` (1,004 lines): Axum REST API + WebSocket server. REST endpoints under `/api/v1/` for mission CRUD, terrain queries, and route optimization. WebSocket at `/ws` for 10 Hz telemetry broadcast using `tokio::sync::broadcast` and `watch` channels.
- `nmea.rs` (239 lines): NMEA sentence parser for GPS telemetry decoding.
- `telemetry.rs` (57 lines): Data structures for `CurrentState`, `SystemStatus`, `LineStatus`.
- `serial_manager.rs` (102 lines): USB serial port manager with VID/PID auto-detection (u-blox, CP2102/FTDI). Feature-gated behind `serial`.

### 9. AI Integration (`src/ai/`)

- `client.rs` / `prompts.rs`: Claude API client for photogrammetry Q&A. Requires `ANTHROPIC_API_KEY`.

## Module Map

```
src/
├── main.rs              # CLI entry (clap) — terrain, plan, daemon, ask
├── lib.rs               # 11 module declarations
├── types.rs             # WGS84 constants, Coordinate<D>, AltitudeDatum, SoA, SensorParams
├── error.rs             # DatumError, DemError, GeodesyError, GpkgError, IoError
├── datum.rs             # FlightLine::to_datum() — EGM2008↔EGM96↔WGS84↔AGL (rayon)
├── legal.rs             # Copernicus attribution, DSM warning, license constants
├── geodesy/             # karney.rs, utm.rs, geoid.rs
├── dem/                 # mod.rs (TerrainEngine), copernicus.rs, cache.rs
├── math/                # dubins.rs, geometry.rs, numerics.rs, routing.rs
├── trajectory/          # elastic_band.rs, bspline_smooth.rs, pursuit.rs, validation.rs
├── network/             # server.rs, telemetry.rs, nmea.rs, serial_manager.rs
├── photogrammetry/      # sensor.rs, flightlines.rs, corridor.rs, lidar.rs
├── gpkg/                # init.rs, binary.rs, rtree.rs
├── io/                  # geojson.rs, kml.rs, qgc.rs, dji.rs
└── ai/                  # client.rs, prompts.rs
```

## Coding Conventions

- **f64 Everywhere.** No `f32` for coordinates or geodetic math. Cast grid `f32` → `f64` before polynomial math.
- **Radians Internal.** `GeoCoord` stores radians. Degrees are only for CLI/IO boundaries.
- **PhantomData Datum Safety.** All coordinates use `Coordinate<Datum>` with `PhantomData`. The compiler rejects mixing datums without explicit transformation.
- **No Async on Math.** Geodesy, CRS, DEM, photogrammetry, and trajectory modules must not use `.await` or tokio primitives. `rayon` only.
- **Kahan Summation.** All distance/trajectory accumulations use `KahanSum` from `math::numerics`.
- **SoA Layout.** Bulk coordinate storage uses `{lats: Vec<f64>, lons: Vec<f64>}`, not `Vec<(f64, f64)>`.
- **Karney Only.** Geodesic distance via Karney. No Haversine. No Vincenty.
- **WAL Mandatory.** Every SQLite connection sets `PRAGMA journal_mode = WAL`. SQLITE_BUSY is a fatal violation. No VACUUM in flight.
- **GPLv3 Headers.** Every `.rs` file must include `SPDX-License-Identifier: GPL-3.0-or-later` in the first 3 lines. Trigger controller firmware uses MIT.
- **Error Propagation.** Functions that can fail return `Result<T, E>`. No silent `unwrap_or(default)` on mission-critical values without `log::warn!`.

## Testing

12 integration test files in `tests/`:

| File | Coverage |
|------|----------|
| `geodesy.rs` | Karney inverse/direct, UTM zone projection |
| `datum.rs` | Altitude datum conversion (EGM2008, EGM96, Ellipsoidal, AGL) |
| `terrain_following_validation.rs` | B-spline, elastic band, pursuit paths |
| `terrain_integration.rs` | DEM caching, elevation queries |
| `pipeline.rs` | End-to-end photogrammetry + terrain |
| `dem.rs` | DEM loading, caching |
| `gpkg.rs` | GeoPackage creation, spatial indexing |
| `io.rs` | KML/GeoJSON parsing |
| `v02_integration.rs` | Full integration (Phantom 4 Pro, 3 cm GSD) |
| `daemon_integration.rs` | FMS server startup, endpoint tests |
| `photogrammetry.rs` | GSD, swath, overlap validation |

Run all checks with:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Roadmap

### Completed

- **v0.1 — Foundation:** DEM ingestion, flight lines, GeoPackage/GeoJSON, Karney geodesics, EGM2008.
- **v0.2 — Correctness & Safety:** Datum-tagged altitudes, EGM96, DSM warnings, QGC/DJI/KML exports, corridor support, Dubins paths, LiDAR planning, TSP routing.
- **v0.3 — Terrain-Following Trajectories:** Elastic Band + B-spline C² curves, convex hull safety, dynamic pursuit, synthetic validation suite.

### In Progress

- **v0.4 — Networked Daemon:** Axum REST/WebSocket API, NMEA parsing, mDNS service discovery, RTK degradation handling, USB VID/PID auto-detection, hot-plug serial, ephemeral token auth, WAL persistence. (Scaffolding complete — validation and field-hardening in progress.)

### Planned

- **v0.5 — Atmospheric & Energy:** Wind triangle, crab angle, drag models, battery endurance, 20% reserve enforcement.
- **v0.6 — Glass Cockpit MVP:** Tauri 2.0, Track Vector CDI, Canvas 2D rendering, WebSocket telemetry display.
- **v0.7 — Web Planning UI:** React + MapLibre GL JS, Shapefile/DXF/GeoJSON import, FAA airspace integration (advisory only), sensor library schema.
- **v0.8 — Trigger Controller:** STM32/RP2040 embedded Rust `no_std`, NMEA FSM, dead reckoning, GPIO trigger, dual-trigger LiDAR+camera. MIT licensed.
- **v0.9 — 3DEP & US Domestic:** 3DEP AWS S3, GEOID18 (big-endian only), microtile caching, Helmert 7-parameter transforms.
- **v1.0 — Production Release:** Full E2E testing, Eagle Mapping field validation, PPK integration, NSRS 2022 readiness.

## Key Research Docs

The full set of 52 research documents lives in `docs/`. The most relevant for current and upcoming work:

- **Doc 28** — IronTrack Daemon Design (REST, WebSocket, state management, mDNS, security) — primary v0.4 reference.
- **Doc 34** — Terrain Following Planning (Elastic Band + B-spline hybrid).
- **Doc 50** — Dubins Mathematical Extension (6 paths, clothoid, 3D airplane, wind r_safe).
- **Doc 39** — Flight Path Optimization (ATSP, NN+Or-opt, boustrophedon, multi-block).
- **Doc 38** — UAV Endurance Modeling (power models, energy costing) — primary v0.5 reference.
- **Doc 25** — Aircraft Cockpit Design (Track Vector CDI, Canvas 2D) — primary v0.6 reference.
