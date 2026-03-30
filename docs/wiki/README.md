# IronTrack Wiki

IronTrack is a GPLv3-licensed open-source Flight Management System for aerial survey. It replaces fragmented legacy toolchains with a single unified system from planning desk to aircraft cockpit.

This wiki is the human-readable layer above the 52 research documents in `docs/`. It is meant for maintainers and early adopters.

## Start Here

- [Engine Technical Reference](https://github.com/Saaquin/IronTrack/blob/main/docs/99_engine_technical_reference.md) — Architectural overview, module map, and critical workflows.
- [Maintainer Guide](maintainers.md) — Architecture, coding conventions, and the roadmap through v1.0.
- [User Guide (Early Adopters)](early-adopters.md) — CLI usage examples, daemon mode, and current limitations.

## Feature Guides

- [Geodesy & Datums](geodesy-and-datums) — Karney geodesics, UTM projection, EGM96/EGM2008 geoid models, PhantomData datum safety, altitude conversions.
- [Terrain & Trajectory](terrain-and-trajectory) — Copernicus DEM ingestion, Two-Phase I/O, Elastic Band + B-spline smoothing, pure pursuit, terrain-following pipeline.
- [Flight Planning](flight-planning) — Area surveys, corridor mapping, LiDAR missions, sensor geometry, Dubins turns, TSP route optimization.
- [Daemon & API](daemon-and-api) — REST endpoints, WebSocket telemetry, NMEA parsing, serial manager, mDNS discovery, state management.

## Current Engine State (v0.4.0)

The IronTrack Core Engine is a high-performance headless CLI and networked daemon built in Rust (~40,000 lines across 44 source files). Current capabilities:

- **Terrain Ingestion:** Copernicus GLO-30 DSM parsing with automatic AWS S3 tile fetching, local caching, and Two-Phase I/O (sequential pre-warm, parallel dispatch via `rayon`).
- **Geodesy:** Karney geodesic inverse/direct, Karney-Krüger 6th-order UTM projection, dual-geoid lookup (EGM96/EGM2008) with bilinear interpolation.
- **Planning:** Area-grid and corridor flight line generation, terrain-aware AGL adjustment, LiDAR point density kinematics, and custom sensor geometry (GSD, FOV, swath width).
- **Trajectory:** Elastic Band energy minimization, cubic B-spline C² smoothing with convex hull safety, dynamic look-ahead pure pursuit, and synthetic validation suite.
- **Kinematics:** Dubins path procedure turns (LSL, RSR, LSR, RSL, LRL, RLR) with clothoid transitions and jerk-limited roll.
- **Route Optimization:** Nearest-neighbor + 2-opt TSP heuristic with Karney geodesic cost matrix.
- **I/O:** 3D GeoPackage (LINESTRINGZ with R-tree), RFC 7946 GeoJSON, QGroundControl `.plan`, DJI `.kmz`, KML/KMZ import.
- **Networked Daemon (v0.4):** Axum REST API (`/api/v1/`) and WebSocket telemetry (10 Hz broadcast). NMEA parser (GGA/RMC/Trimble GGK) with stateful epoch pairing. USB serial manager with VID/PID auto-detection, hot-plug recovery, and RTK degradation detection. `deadpool-sqlite` connection pool with WAL mode, periodic PASSIVE checkpoints, and SQLITE_BUSY fail-safe shutdown. Ephemeral bearer token auth with constant-time comparison. mDNS service discovery (`_irontrack._tcp.local.`). Mock telemetry mode for hardware-free testing. 15 integration tests covering the full daemon lifecycle.

## System Layers

| Layer | Technology | Status |
|-------|-----------|--------|
| Core Engine | Rust (synchronous, `rayon`) | v0.4.0 — stable |
| Network Daemon | Axum/Tokio, REST + WebSocket | v0.4.0 — complete |
| Trigger Controller | Embedded Rust `no_std` (STM32/RP2040) | Target v0.8 |
| Glass Cockpit | Tauri 2.0 (Rust + WebView, Canvas 2D) | Target v0.6 |
| Planning Web UI | React + MapLibre GL JS | Target v0.7 |

## Relationship To The Research Docs

Use this wiki for orientation. For deep mathematical formulas and domain-specific specifications, refer to the 52 numbered research documents (00–52, excluding 27) in the repository's `docs/` directory:

- **Foundation:** `02_geodesy_foundations.md`, `03_photogrammetry_spec.md`
- **Infrastructure:** `04_geoPackage_architecture.md`, `10_Rust_COG_Ingestion.md`
- **Engineering:** `05_vertical_datum_engineering.md`, `09_dsm_vs_dtm.md`
- **Terrain & Trajectory:** `34_terrain_following_planning.md`, `50_dubins_mathematical_extension.md`
- **Daemon (v0.4):** `28_irontrack_daemon_design.md`, `47_daemon_field_hardware.md`
- **Reference:** `formula_reference.md`, `formula_reference_08_10.md`
