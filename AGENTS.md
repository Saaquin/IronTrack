# IronTrack — Claude Code Project Context

## What This Is
IronTrack is a GPLv3-licensed, headless Rust compute engine for automated aerial survey
flight planning. It replaces legacy proprietary GIS flight management software. The engine
ingests Digital Elevation Models, calculates photogrammetrically correct flight lines over
variable terrain, and exports results to GeoPackage and GeoJSON.

Solo-founder FOSS project. Proof-of-concept adopter: Eagle Mapping.

## Architecture Rules (Hard Constraints)
- **Headless CLI only** for v0.1. No GUI, no TUI. Output = GeoPackage + stdout.
- **No async on math.** Geodesics, CRS transforms, DEM queries, photogrammetry → `rayon`.
  Tokio only for network I/O. Bridge via `spawn_blocking` if needed.
- **f64 everywhere.** Never f32 for coordinates. f32 gives ~1m precision at planetary scale.
- **The Two-Phase I/O Rule.** Never perform blocking I/O (Disk/Network) inside a Rayon `par_iter`. Use a sequential "warm-up" phase to load data, followed by a parallel "math" phase.
- **Precision Guard.** Any f64 -> f32 cast for external crates (e.g., EGM2008) MUST be followed by an explicit clamp or wrap (e.g., lon 180.0 -> -180.0) to prevent rounding-induced OutOfBounds errors.
- **Feature-Matched Testing.** Synthetic test fixtures (like TIFFs or GPKGs) MUST replicate production features (e.g. DEFLATE compression, Predictors) to prevent silent "clean-room" successes that fail on real data.
- **GeoCoord as Primary Exchange.** All library-internal APIs (Geodesy, Terrain, Photogrammetry) MUST use the `GeoCoord` struct (or slices of radians). Avoid passing raw `f64` degrees except at the CLI/IO boundary.
- **Bulk-First API Design.** Engines must provide bulk query methods (e.g., `elevations_at(lats: &[f64], lons: &[f64])`) to support cache-friendly SoA processing and Rayon parallelism.
- **Karney only** for geodesic distance. No Haversine (0.3% error). No Vincenty (antipodal
  convergence failure → thread starvation in Rayon work-stealing scheduler).
- **Karney-Krüger 6th order** for WGS84 → UTM. No legacy Redfearn truncations.
- **Kahan summation** for all distance/trajectory accumulations (prevents floating-point drift).
- **SoA memory layout** for bulk coordinates: `{lats: Vec<f64>, lons: Vec<f64>, ...}`
  Not `Vec<Point>`. Enables cache-friendly SIMD-width prefetching.
- **GeoPackage** as primary data store. SQLite with WAL mode, bundled via rusqlite.
- **GPLv3 header on every .rs file.** See header template at end of this file.

## v0.1 Milestones
1. **Terrain ingestion** ✅ — Copernicus GLO-30 GeoTIFF parsing (f32, DEFLATE+PREDICTOR=3),
   latitude-dependent grid width table, microtile cache (`dirs` crate), bilinear interpolation,
   EGM96 geoid undulation (h = H + N). SRTM .hgt retained as fallback.
   ⚠️ Phase 4A: upgrade geoid to EGM2008 (matches Copernicus vertical datum)
2. **Sensor geometry** ✅ — GSD, FOV, swath width, flight line spacing, side-lap validation
   over variable DEM terrain, dynamic AGL adjustment
   ⚠️ Phase 4B: add AltitudeDatum tags to FlightLine for autopilot export safety
3. **I/O** — GeoPackage creation (binary header, R-tree index, 7 triggers), GeoJSON export,
   embedded flight metadata, Copernicus attribution injection
4. **Phase 4 (post-research)** — EGM2008 geoid, datum-tagged altitudes, DSM safety warnings,
   autopilot export (QGC .plan, DJI WPML/KMZ), LiDAR sensor model, KML import

## Documentation Map
Read these for implementation detail. They live in `docs/`.

**Important:** Docs 02–10 have blank gaps where formulas were embedded images — use
`docs/formula_reference.md` and `docs/formula_reference_08_10.md` alongside them.
Docs 11–15 have formulas embedded inline (no separate reference needed).

| Doc | Contents |
|-----|----------|
| `00_project_charter.md` | Scope, milestones, Eagle Mapping relationship |
| `01_legal_framework.md` | GPLv3 mechanics, IP carve-out, CLA strategy, copyright headers |
| `02_geodesy_foundations.md` | WGS84 params, geoid/ellipsoid, Karney algorithm, UTM Karney-Krüger, SoA, SIMD, Kahan |
| `03_photogrammetry_spec.md` | GSD, FOV, swath, dynamic AGL, collinearity, sensor stabilization |
| `04_geopackage_architecture.md` | Byte-level binary format, flags bitfield, envelope codes, R-tree triggers, WAL pragmas |
| `05_vertical_datum_engineering.md` | EGM96, EGM2008, NAVD88, GEOID18, CGVD2013, barometric ISA, terrain-following |
| `06_rust_architecture_analysis.md` | FFI, async overhead trap, SIMD auto-vectorization, Rayon vs Tokio, zero-copy mmap |
| `07_corridor_mapping.md` | Polyline offsets, self-intersection clipping, catenary, bank angles, clothoids |
| `08_copernicus_dem.md` | Copernicus GLO-30/GLO-90 specs, TanDEM-X bistatic InSAR, LE90<4m, EGM2008, GPLv3-compatible license |
| `09_dsm_vs_dtm.md` | DSM vs DTM for AGL: pitch dynamics, X-band canopy penetration (2–8m), FABDEM, CHM dual-layer |
| `10_cog_ingestion.md` | COG byte-range architecture, TIFF IFD, STAC, affine transforms, latitude grid widths, Tokio, DEFLATE |
| `11_autopilot_formats.md` | QGC .plan JSON, MAVLink 2.0 MISSION_ITEM_INT, DJI WPML/KMZ, senseFly, Leica HxMap, altitude datums per platform |
| `12_egm2008_grid.md` | EGM2008 .grd/.gtx/.pgm formats, 1-arcmin 10801×21600 grid, bilinear vs bicubic (10mm max error at 1'), Tide-Free system, memory footprint, EGM96↔EGM2008 regional discrepancies |
| `13_lidar_planning.md` | LiDAR swath/density math, scanner types, ρ=PRR(1-LSP)/(V×W), slope distortion, canopy penetration Beer-Lambert, USGS QL0-QL2, combined camera+LiDAR conflicts |
| `14_geotiff_cog.md` | Rust GeoTIFF ecosystem: `tiff`/`geotiff`/`oxigdal` crates, PREDICTOR=3 support confirmed, benchmarks, manual DEFLATE+predictor reversal path |
| `15_3dep_access.md` | USGS 3DEP COGs on AWS, TNM API, STAC via Planetary Computer, NAVD88/GEOID18 vertical datum, blending equation H_egm2008 = H_navd88 + N_geoid18 − N_egm2008 |
| `formula_reference.md` | **All equations from docs 02–07 in UTF-8** |
| `formula_reference_08_10.md` | **All equations from docs 08–10 in UTF-8** |

## Dependencies (v0.1)
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
```

## Module Structure
```
src/
├── main.rs              # CLI entry (clap). No business logic here.
├── lib.rs               # Library root. Re-exports all modules.
├── types.rs             # WGS84 constants, GeoCoord, AltitudeDatum, SoA types, SensorParams
├── error.rs             # thiserror enum: DemError, GeodesyError, GpkgError, IoError, PhotoError
├── legal.rs             # Copernicus attribution strings, license constants
├── geodesy/
│   ├── mod.rs
│   ├── karney.rs        # Geodesic direct/inverse via geographiclib-rs
│   ├── utm.rs           # WGS84 ↔ UTM projection (Karney-Krüger)
│   └── geoid.rs         # EGM96 + EGM2008 undulation lookup, h ↔ H conversion
├── dem/
│   ├── mod.rs           # TerrainEngine (combines DEM + geoid), DemType enum (DSM/DTM)
│   ├── copernicus.rs    # Copernicus GLO-30 GeoTIFF parser, lat-band column table
│   ├── srtm.rs          # .hgt parser (legacy fallback), bilinear interpolation
│   └── cache.rs         # Cross-platform tile cache, S3 URL builder, atomic writes
├── photogrammetry/
│   ├── mod.rs
│   ├── sensor.rs        # FOV, GSD, swath width, pixel pitch
│   ├── flightlines.rs   # Line generation, spacing, overlap validation
│   └── lidar.rs         # [Phase 4] LiDAR sensor model, point density, USGS QL
├── gpkg/
│   ├── mod.rs
│   ├── init.rs          # DB creation, PRAGMAs, mandatory tables
│   ├── binary.rs        # StandardGeoPackageBinary encoder (header + WKB)
│   └── rtree.rs         # R-tree virtual table + 7 sync triggers
└── io/
    ├── mod.rs
    ├── geojson.rs       # RFC 7946 FeatureCollection export
    ├── kml.rs           # [Phase 4] KML/KMZ polygon import
    ├── qgc.rs           # [Phase 4] QGroundControl .plan JSON export
    └── dji.rs           # [Phase 4] DJI WPML/KMZ export
```

## Code Conventions
- Rust 2021 edition. `cargo fmt` + `cargo clippy -- -D warnings` clean.
- `thiserror` for library errors, `anyhow` for CLI binary.
- No `unwrap()` in library code. `expect("reason")` only in main.rs.
- Unit tests in-module (`#[cfg(test)]`). Integration tests in `tests/`.
- Commit messages: imperative mood, reference milestone (e.g. "M1: implement SRTM parser").

## Testing
- Unit test stubs exist in every source module. Fill them in as each module is implemented.
- Integration test stubs live in `tests/geodesy.rs`, `tests/dem.rs`, `tests/photogrammetry.rs`,
  `tests/gpkg.rs`, `tests/io.rs`. All marked `#[ignore]` until implemented.
- Dev dependencies: `tempfile` (temp .gpkg files), `approx` (f64 assertions — never use `==`).
- Test fixtures go in `tests/fixtures/`. Use `env!("CARGO_MANIFEST_DIR")` to locate them.
- Geodesy regression suite must pass at `--release` (catches optimisation-induced FP drift).
- See `.agents/testing.md` for full strategy, reference datasets, and CI/CD workflow.

## Agent Overflow
Subsystem-specific implementation guides live in `.agents/`:
- `.agents/geodesy.md` — Karney algorithm, UTM math, geoid lookup, GeoCoord exchange protocol
- `.agents/geopackage.md` — Binary format byte layout, R-tree trigger SQL, WAL tuning
- `.agents/photogrammetry.md` — GSD/FOV derivations, dynamic AGL logic, overlap math, TerrainReport
- `.agents/terrain.md` — SRTM + Copernicus format specs, bilinear interpolation, Two-Phase I/O, bulk API, cache architecture
- `.agents/testing.md` — Test strategy, reference datasets (GeodTest), CI/CD workflow,
  fixture conventions, floating-point comparison rules, Precision Guard, anti-patterns
- `.agents/autopilot.md` — [Phase 4] QGC .plan JSON schema, DJI WPML/KMZ structure, MAVLink
  MISSION_ITEM_INT, altitude datum conversion per platform, camera trigger encoding
- `.agents/lidar.md` — [Phase 4] LiDAR sensor model, point density ρ=PRR(1-LSP)/(V×W),
  scanner types, USGS QL0-QL2, combined camera+LiDAR conflict resolution

Point Claude Code at these with: "Read .agents/geopackage.md then implement..."

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
