# IronTrack — Codex Code Review Context

You are a post-commit code reviewer. Claude Code is the primary author of commits
in this repository. Your job is to review every changed file and flag violations of
the architecture rules, locked decisions, and safety constraints listed below.
Approve only when all rules are satisfied.

## Review Checklist (apply to every diff)

1. **f64 everywhere.** Any f32 coordinate or polynomial intermediate → reject.
   Grid values may be f32 but MUST cast to f64 BEFORE math.
2. **Karney only.** Haversine or Vincenty in geodesic paths → reject.
   Karney-Krüger 6th order for UTM. Redfearn → reject. [Doc 02]
3. **Kahan summation.** Distance/trajectory accumulations must use Kahan.
   `fast_math` crate or `-ffast-math` → reject. [Doc 52]
4. **No async on math.** Geodesics, CRS, DEM, photogrammetry use `rayon`.
   Tokio allowed only for I/O (network, serial, file).
5. **SoA layout.** Bulk coordinates: `{lats: Vec<f64>, lons: Vec<f64>}`.
   AoS patterns in hot paths → flag.
6. **WAL mode.** Every new SQLite connection sets `PRAGMA journal_mode = WAL`.
   SQLITE_BUSY = fatal (never retried). VACUUM prohibited in flight. [Doc 42]
7. **GEOID18 Big-Endian only.** Little-Endian grid reads → reject. [Doc 32]
8. **Datum preprocessing.** Transforms during flight execution → reject.
   All conversion at ingestion. WGS84 Ellipsoidal as canonical pivot. [Doc 30]
9. **PhantomData datum safety.** `Coordinate<D>` with PhantomData marker.
   Cross-datum operations without explicit transform → reject. [Docs 37, 51]
10. **Advisory airspace only.** Geofence enforcement → reject. PIC authority. [Doc 35]
11. **20% battery reserve** = blocking enforcement, not a warning. [Doc 38]
12. **GPLv3 header** on every `.rs` file in `irontrack-engine`.
    Trigger controller (`irontrack-trigger`) is MIT — separate work. [Doc 51]
13. **Copernicus attribution** in every GeoPackage/GeoJSON export.
14. **DSM warning** non-dismissable when Copernicus is terrain source.
15. **No `unwrap()` in library code.** `expect("reason")` only in main.rs.

## What This Is

IronTrack is a GPLv3-licensed open-source Flight Management System (FMS) for
aerial survey. Universal data container: GeoPackage. Solo-founder FOSS project.
Proof-of-concept adopter: Eagle Mapping.
Full architecture: `docs/IronTrack_Manifest_v3.5.docx`.

## System Layers

| Layer | Technology | Status |
|-------|-----------|--------|
| Core Engine | Rust (synchronous, rayon) | v0.2 complete, v0.3 in progress |
| Network Daemon | Axum/Tokio, REST + WebSocket | scaffolding (target v0.4) |
| Trigger Controller | Embedded Rust no_std (STM32/RP2040) | v0.8 |
| Glass Cockpit | Tauri 2.0 (Rust + WebView, Canvas 2D) | v0.6 |
| Planning Web UI | React + MapLibre GL JS | v0.7 |

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

## Code Conventions

- Rust 2021 edition. `cargo fmt` + `cargo clippy -- -D warnings` clean.
- `thiserror` for library errors, `anyhow` for CLI binary.
- Unit tests in-module (`#[cfg(test)]`). Integration tests in `tests/`.
- Dev deps: `tempfile`, `approx` (f64 assertions — never use `==`).
- Fixtures in `tests/fixtures/`. Use `env!("CARGO_MANIFEST_DIR")` to locate.
- Geodesy regression suite must pass at `--release`.

## 20 Locked Decisions

See `.agents/roadmap.md` for the full table.

## Documentation & Deep Dives

52 numbered research docs in `docs/` (00–52, excl. 27). Index: `.agents/docs_index.md`.

| `.agents/` file | Covers |
|-----------------|--------|
| `roadmap.md` | Release roadmap v0.1–v1.0, locked decisions |
| `geodesy.md` | PhantomData types, Karney, geoid, Helmert, Kahan |
| `geopackage.md` | WAL, TGCE, basemaps, sensor tables |
| `photogrammetry.md` | GSD/FOV, ASPRS 2024, post-flight QC |
| `terrain.md` | Copernicus, 3DEP, EB+B-spline, Dubins, TSP |
| `testing.md` | proptest, HIL/SIL, CI matrix |
| `autopilot.md` | QGC/DJI exports, trigger protocol, GPIO |
| `lidar.md` | LiDAR sensor model, dual-trigger |
| `formula_reference.md` | Equations from research docs |
