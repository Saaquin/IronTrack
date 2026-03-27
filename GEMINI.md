# IronTrack — Claude Code Project Context

## What This Is
IronTrack is a GPLv3-licensed open-source Flight Management System (FMS) for aerial survey.
It replaces fragmented legacy toolchains with a single unified system from planning desk to
aircraft cockpit. Universal data container: the **GeoPackage**.

Solo-founder FOSS project. Proof-of-concept adopter: Eagle Mapping.
Full architecture and release roadmap: `docs/IronTrack_Manifest_v2.md`.

## System Layers
Core Engine (CLI, sync, rayon) → Network Daemon (Axum/Tokio, v0.4) → Trigger Controller
(embedded firmware, v0.8) → Glass Cockpit (Tauri, v0.6) → Planning Web UI (React, v0.7).
**Current scope: Core Engine only.**

## Architecture Rules (Hard Constraints)
- **Headless CLI only** through v0.3. Output = GeoPackage + stdout.
- **No async on math.** Geodesics, CRS transforms, DEM queries, photogrammetry, datum
  conversions → `rayon`. Tokio only for network I/O. Bridge via `spawn_blocking`.
- **f64 everywhere.** Never f32 for coordinates. f32 gives ~1m precision at planetary scale.
- **Two-Phase I/O Rule.** No blocking I/O inside a Rayon `par_iter`. Sequential warm-up,
  then parallel math.
- **Precision Guard.** Any f64 → f32 cast for external crates MUST be followed by an explicit
  clamp or wrap to prevent rounding-induced OutOfBounds errors.
- **Feature-Matched Testing.** Synthetic fixtures MUST replicate production features
  (DEFLATE+PREDICTOR=3) to prevent clean-room test passes that fail on real data.
- **GeoCoord as Primary Exchange.** All internal APIs use `GeoCoord` (or radian slices).
  Raw f64 degrees only at CLI/IO boundary.
- **Bulk-First API Design.** Engines provide bulk methods (`elevations_at(&[f64], &[f64])`)
  for cache-friendly SoA processing and Rayon parallelism.
- **Karney only** for geodesic distance. No Haversine (0.3% error). No Vincenty (antipodal
  convergence failure → thread starvation).
- **Karney-Krüger 6th order** for WGS84 → UTM. No legacy Redfearn truncations.
- **Kahan summation** for all distance/trajectory accumulations.
- **SoA memory layout** for bulk coordinates: `{lats: Vec<f64>, lons: Vec<f64>, ...}`.
- **GeoPackage** as primary data store. SQLite WAL mode, bundled via rusqlite.
- **Datum-tagged altitudes.** Every altitude carries an explicit `AltitudeDatum` tag.
  Conversion chains through WGS84 Ellipsoidal as pivot. See `src/datum.rs`.
- **DSM safety warning is mandatory and non-suppressible.** Copernicus is an X-band SAR DSM;
  X-band penetrates 2–8 m into forest canopy. Warning → stderr + GeoPackage + GeoJSON.
- **GPLv3 header on every .rs file.** See header template at end of this file.

## Current Status
- v0.1 complete. v0.2 in progress.
- Remaining v0.2 work and full roadmap: `.agents/roadmap.md`
- GeoPackage content requirements (irontrack_metadata, attribution, etc.): `.agents/geopackage.md`

## Documentation Map
Full table in `.agents/docs_index.md`. Key refs for active work:
- `05_vertical_datum_engineering.md` — datum conversion math
- `08_copernicus_dem.md` — attribution requirements, DSM warnings
- `09_dsm_vs_dtm.md` — DSM vs DTM AGL safety
- `11_autopilot_formats.md` — QGC .plan, DJI WPML/KMZ, datum per platform

## Dependencies
```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
rusqlite = { version = "0.31", features = ["bundled"] }
rayon = "1.10"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
geographiclib-rs = "0.2"
byteorder = "1"
dirs = "5"
reqwest = { version = "0.12", features = ["blocking"] }
thiserror = "1"
anyhow = "1"
egm96 = { version = "0.2", default-features = false }   # vendored via [patch.crates-io]

[patch.crates-io]
egm96 = { path = "vendor/egm96" }
```

## Module Structure
```
src/
├── main.rs              # CLI entry (clap). No business logic here.
├── lib.rs               # Library root. Re-exports all modules.
├── types.rs             # WGS84 constants, GeoCoord, AltitudeDatum, SoA types, SensorParams
├── error.rs             # DatumError, DemError, GeodesyError, GpkgError, IoError, PhotogrammetryError
├── datum.rs             # FlightLine::to_datum() — EGM2008↔EGM96↔WGS84↔AGL (rayon)
├── legal.rs             # Copernicus attribution strings, DSM safety warning, license constants
├── geodesy/             # karney.rs, utm.rs, geoid.rs
├── dem/                 # mod.rs (TerrainEngine), copernicus.rs, srtm.rs, cache.rs
├── photogrammetry/      # mod.rs, sensor.rs, flightlines.rs, lidar.rs [v0.2]
├── gpkg/                # mod.rs, init.rs, binary.rs, rtree.rs
└── io/                  # mod.rs, geojson.rs, kml.rs [v0.2], qgc.rs [v0.2], dji.rs [v0.2], mdb.rs [v0.2]
```

## Code Conventions
- Rust 2021 edition. `cargo fmt` + `cargo clippy -- -D warnings` clean.
- `thiserror` for library errors, `anyhow` for CLI binary.
- No `unwrap()` in library code. `expect("reason")` only in main.rs.
- Unit tests in-module (`#[cfg(test)]`). Integration tests in `tests/`.
- Commit messages: imperative mood, reference milestone (e.g. "v0.2: add QGC export").

## Testing
- Unit test stubs in every module; integration stubs in `tests/` marked `#[ignore]`.
- Dev deps: `tempfile`, `approx` (f64 assertions — never use `==`).
- Fixtures in `tests/fixtures/`. Use `env!("CARGO_MANIFEST_DIR")` to locate them.
- Geodesy regression suite must pass at `--release`.
- Full strategy: `.agents/testing.md`

## Agent Overflow
- `.agents/roadmap.md` — release roadmap, v0.2 remaining work checklist
- `.agents/docs_index.md` — full documentation table
- `.agents/geodesy.md` — Karney, UTM, geoid, GeoCoord exchange protocol
- `.agents/geopackage.md` — binary format, R-tree SQL, WAL tuning, content requirements
- `.agents/photogrammetry.md` — GSD/FOV, dynamic AGL, overlap math
- `.agents/terrain.md` — SRTM + Copernicus specs, bilinear interpolation, Two-Phase I/O
- `.agents/testing.md` — test strategy, GeodTest datasets, CI/CD, Precision Guard
- `.agents/autopilot.md` — QGC .plan, DJI WPML/KMZ, MAVLink, datum per platform
- `.agents/lidar.md` — LiDAR sensor model, point density, USGS QL0-QL2

## GPLv3 Header (every .rs file)
```rust
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
```
