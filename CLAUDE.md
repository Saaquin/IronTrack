# IronTrack — Claude Code Project Context

## What This Is
IronTrack is a GPLv3-licensed open-source Flight Management System (FMS) for aerial survey.
It replaces fragmented legacy toolchains with a single unified system from planning desk to
aircraft cockpit. Universal data container: the **GeoPackage**.

Solo-founder FOSS project. Proof-of-concept adopter: Eagle Mapping.
Full architecture: `docs/IronTrack_Manifest_v3.5.docx` (21 sections, 520 paragraphs, 52 research docs).

## System Layers (4-Layer Architecture)
| Layer | Technology | Status |
|-------|-----------|--------|
| Core Engine | Rust (synchronous, rayon) | **v0.2 complete**, v0.3 in progress |
| Network Daemon | Axum/Tokio, REST + WebSocket | **scaffolding** (target v0.4) |
| Trigger Controller | Embedded Rust no_std (STM32/RP2040) | v0.8 |
| Glass Cockpit | Tauri 2.0 (Rust + WebView, Canvas 2D) | v0.6 |
| Planning Web UI | React + MapLibre GL JS | v0.7 |

## Architecture Rules (Hard Constraints)
- **Headless CLI only** through v0.3. Output = GeoPackage + stdout.
- **No async on math.** Geodesics, CRS, DEM, photogrammetry → `rayon`. Tokio only for I/O.
- **f64 everywhere.** Never f32 for coordinates. Cast grid f32→f64 BEFORE polynomial math.
- **PhantomData datum safety.** All coordinates use `Coordinate<Datum>` with PhantomData.
  Compiler rejects mixing datums without explicit transformation. [Docs 37, 51]
- **Kahan summation** for all distance/trajectory accumulations. fast_math REJECTED. [Doc 52]
- **SoA memory layout** for bulk coordinates: `{lats: Vec<f64>, lons: Vec<f64>}`.
- **Karney only** for geodesic distance. No Haversine. No Vincenty. [Doc 02]
- **Karney-Krüger 6th order** for UTM. No Redfearn. [Doc 02]
- **WGS84 Ellipsoidal** as canonical pivot datum. All terrain converted on ingestion. [Doc 30]
- **Preprocessing mandate.** Datum transforms NEVER on-the-fly during flight. [Doc 30]
- **GEOID18 Big-Endian only.** Little-Endian corrupted (2019 NGS). [Doc 32]
- **WAL mode mandatory.** SQLITE_BUSY = fatal violation (not transient retry). [Doc 42]
- **VACUUM prohibited in flight.** Use PRAGMA optimize. [Doc 42]
- **Advisory only for airspace.** Never enforce geofences. PIC retains authority. [Doc 35]
- **20% battery reserve** = blocking enforcement (not just warning). [Doc 38]
- **GPLv3 header on every .rs file.** Trigger controller firmware: MIT (separate work). [Doc 51]

## 20 Locked Decisions (see `.agents/roadmap.md` for full table)

## Documentation Map
Full table in `.agents/docs_index.md`. **52 numbered research documents** (00–52, excl. 27)
plus `99_engine_technical_reference.md` in `docs/`.

Key refs for active work (v0.3):
- `34_terrain_following_planning.md` — Elastic Band + B-spline hybrid
- `50_dubins_mathematical_extension.md` — 6 Dubins paths, clothoid, 3D airplane, wind r_safe
- `39_flight_path_optimization.md` — ATSP, NN+Or-opt, boustrophedon, multi-block
- `07_corridor_mapping.md` — polyline offsets, bank angles

Key refs for upcoming releases:
- `28_irontrack_daemon_design.md` — REST, WebSocket, state management (v0.4)
- `38_uav_endurance_modeling.md` — power models, energy costing (v0.5)
- `25_aircraft_cockpit_design.md` — Track Vector CDI, Canvas 2D (v0.6)
- `43_boundary_import_formats.md` — Shapefile/DXF/GeoJSON/CSV-WKT (v0.7)
- `42_geopackage_extensions.md` — TGCE, basemaps, WAL details (v0.7)
- `45_aerial_sensor_library.md` — irontrack_sensors schema (v0.7)
- `22_embedded_trigger_controller.md` — NMEA FSM, dead reckoning, GPIO (v0.8)
- `40_sensor_trigger_library.md` — pin-level specs, MEP timing (v0.8)
- `41_lidar_system_control.md` — dual-trigger hybrid (v0.8)
- `30_geodetic_datum_transformation.md` — GEOID18, Helmert, 3DEP (v0.9)
- `44_irontrack_testing_strategy.md` — full testing strategy (v1.0)
- `46_post_flight_qc.md` — XTE, 6DOF footprint, auto re-fly (v1.0)

## Module Structure
```
src/
├── main.rs              # CLI entry (clap)
├── lib.rs               # Library root
├── types.rs             # WGS84 constants, Coordinate<D>, AltitudeDatum, SoA, SensorParams
├── error.rs             # DatumError, DemError, GeodesyError, GpkgError, IoError
├── datum.rs             # FlightLine::to_datum() — EGM2008↔EGM96↔WGS84↔AGL (rayon)
├── legal.rs             # Copernicus attribution, DSM warning, license constants
├── geodesy/             # karney.rs, utm.rs, geoid.rs
├── dem/                 # mod.rs (TerrainEngine), copernicus.rs, cache.rs
├── math/                # dubins.rs, geometry.rs, routing.rs
├── network/             # server.rs (Axum), telemetry.rs, nmea.rs, serial_manager.rs
├── photogrammetry/      # sensor.rs, flightlines.rs, corridor.rs, lidar.rs
├── gpkg/                # init.rs, binary.rs, rtree.rs
└── io/                  # geojson.rs, kml.rs, qgc.rs, dji.rs
```

## Agent Overflow (`.agents/` Directory)
| File | Contents | Key Source Docs |
|------|----------|-----------------|
| `roadmap.md` | Release roadmap (v0.1–v1.0), v0.3 checklist, 20 locked decisions | Manifest v3.5 |
| `docs_index.md` | Full 52-doc table with status | All |
| `geodesy.md` | PhantomData types, Karney, geoid models, Helmert, epoch, Kahan | 02, 30-32, 37 |
| `geopackage.md` | WAL, TGCE, basemaps, sensor tables, irontrack_metadata | 04, 42, 45 |
| `photogrammetry.md` | GSD/FOV, ASPRS 2024, post-flight QC, computational geometry | 03, 36, 46, 49 |
| `terrain.md` | Copernicus, 3DEP, EB+B-spline, Dubins, TSP, energy, performance | 08-10, 33-34, 38-39, 50, 52 |
| `testing.md` | proptest, HIL/SIL, CI matrix, Eagle Mapping acceptance | 44 |
| `autopilot.md` | QGC/DJI exports, trigger protocol, sensor GPIO, hardware integration | 11, 22, 29, 40-41, 48 |
| `lidar.md` | LiDAR sensor model, system control, dual-trigger, USGS QL | 13, 41, 45 |
| `formula_reference.md` | Equations from docs 02-07, 30, 32, 34, 38, 39, 46, 49, 50 | — |

## Agent Routing Guide
**By task type:**
- Geodesy/datum/CRS → `.agents/geodesy.md`
- DEM/terrain/trajectory → `.agents/terrain.md`
- Flight line planning → `.agents/photogrammetry.md`
- GeoPackage schema → `.agents/geopackage.md`
- Export formats/triggers → `.agents/autopilot.md`
- LiDAR planning → `.agents/lidar.md`
- Testing → `.agents/testing.md`
- Math/equations → `.agents/formula_reference.md`
- Roadmap/decisions → `.agents/roadmap.md`
- Doc lookup → `.agents/docs_index.md`
