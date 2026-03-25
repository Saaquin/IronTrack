# Maintainers

## What The Executable Does Today

The binary currently exposes two workflows:

- `irontrack terrain` queries orthometric height, geoid undulation, and ellipsoidal height at one coordinate.
- `irontrack plan` builds a survey plan from a bounding box and sensor model, optionally applies terrain-aware validation/altitude adjustment, then writes GeoPackage output and optional GeoJSON.

The main orchestration lives in `src/main.rs`.

## Runtime Flow

### Terrain Query

The terrain path is:

1. CLI input validation in `src/main.rs`
2. `TerrainEngine::query()` in `src/dem/mod.rs`
3. `DemProvider` cache lookup / tile load in `src/dem/cache.rs`
4. Copernicus TIFF decode + bilinear interpolation in `src/dem/copernicus.rs`
5. EGM2008 correction in `src/geodesy/geoid.rs`

### Flight Planning

The planning path is:

1. Resolve sensor preset or custom sensor in `src/main.rs`
2. Build `FlightPlanParams`
3. Generate flat-terrain lines in `src/photogrammetry/flightlines.rs`
4. If `--terrain` is set:
   `validate_overlap()` checks footprint-vs-terrain overlap
   `adjust_for_terrain()` raises waypoint altitudes where needed
5. Export:
   `GeoPackage::insert_flight_plan()` writes `LINESTRING` rows
   `write_geojson()` writes RFC 7946 output

## Module Map

- `src/types.rs`: shared value types, WGS84 constants, `FlightLine`, `SensorParams`
- `src/error.rs`: library error enums
- `src/dem/`: cache, Copernicus parser, terrain engine
- `src/geodesy/geoid.rs`: EGM96 and EGM2008 support
- `src/photogrammetry/sensor.rs`: FOV, GSD, swath, required AGL
- `src/photogrammetry/flightlines.rs`: line generation, overlap checks, terrain adjustment
- `src/gpkg/`: GeoPackage init, binary geometry encoding, R-tree support
- `src/io/geojson.rs`: GeoJSON export

## Important Invariants

- Internal flight-line coordinates are stored as radians in `FlightLine`.
- GeoJSON and GeoPackage export should use `FlightLine::iter_lonlat_deg()` to avoid lat/lon swaps.
- Terrain bulk queries must go through `TerrainEngine::elevations_at()` to preserve the two-phase I/O rule.
- Copernicus DEM heights are orthometric; GNSS/autopilot heights are ellipsoidal.
- `adjust_for_terrain()` changes waypoint elevations only. It preserves line ordering and lat/lon coordinates.

## Build And Verification

Current local checks that pass:

- `cargo build`
- `cargo test`
- `cargo clippy -- -D warnings`

One practical caveat from this workspace: direct `cargo run -- terrain ...` attempts to create the default cache under the user cache directory. In this sandbox that path is not writable, so live-download validation was not possible here. Tests that use `TerrainEngine::with_cache_dir()` do pass.

## Known Maintenance Risks

- The geodesy integration-test suite in `tests/geodesy.rs` is still mostly ignored stubs, so Karney/UTM regressions are not yet protected at the integration level.
- GeoPackage is currently the primary export path, but it only writes 2D `LINESTRING` geometry. Waypoint altitude is not preserved in the geometry payload.
- GeoJSON writes 3D coordinates, but its per-feature `altitude_msl_m` property is mission metadata, not a guaranteed reflection of per-waypoint terrain-adjusted altitudes.

## Suggested Next Documentation Additions

- A fixture guide for synthetic Copernicus TIFF generation
- A maintainer page for GeoPackage binary layout and trigger behavior
- A release-readiness checklist for v0.1
