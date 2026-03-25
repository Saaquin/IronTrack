# Maintainers

## Core Engine Architecture (v0.1)

The IronTrack engine is built on four functional pillars:

1.  **Geodesy Subsystem (`src/geodesy/`):**
    - `karney.rs`: Thin wrapper for `geographiclib-rs`. Solves inverse/direct problems with 15nm accuracy.
    - `utm.rs`: **Karney-Krüger 6th-order** WGS84 ↔ UTM projection. No third-party bindings; coefficients $\alpha_{1..6}$ and $\beta_{1..6}$ are hard-coded rational fractions from Karney (2011).
    - `geoid.rs`: Dual-model lookup (EGM96/EGM2008). Uses bilinear interpolation over 1-arcminute grids.

2.  **Terrain Subsystem (`src/dem/`):**
    - `copernicus.rs`: Native `tiff` crate parser for **Copernicus GLO-30** FLOAT32 tiles. Handles DEFLATE+Predictor=3 compression in memory.
    - `cache.rs`: Cross-platform tile manager. Automatically fetches missing tiles from AWS S3 via `reqwest`.
    - `TerrainEngine`: Implements the **Two-Phase I/O rule**. Pre-warms tiles sequentially, then dispatches batch queries via `rayon::par_iter`.

3.  **Photogrammetry Subsystem (`src/photogrammetry/`):**
    - `sensor.rs`: Core GSD/FOV/Swath math.
    - `flightlines.rs`: Parallel flight line generator. Implements **Kahan-summed** geodesic tracing to eliminate floating-point drift over 100+ km trajectories.
    - `overlap.rs`: Dynamic AGL adjustment. Samples 5 DEM points per exposure (center + 4 corners) and raises altitude to maintain side-lap/GSD.

4.  **I/O Subsystem (`src/gpkg/` and `src/io/`):**
    - `GeoPackage`: Implements standard OGC binary geometry (LINESTRINGZ) with 48-byte 3D envelopes.
    - `RTree`: Registers custom ST_ functions (`ST_MinX` etc.) in SQLite to parse GPKG blobs directly for trigger-based spatial index synchronization.
    - `GeoJSON`: RFC 7946 compliant FeatureCollection with per-line altitude ranges.

## Coding Conventions

- **f64 Everywhere:** No `f32` for coordinates.
- **Radians Internal:** `GeoCoord` stores radians. Degrees are only for CLI/IO boundaries.
- **No Async on Math:** All compute is synchronous and parallelized via `rayon`. `tokio` is only for network I/O.
- **GPLv3 Headers:** Every `.rs` file must contain the mandatory header.

## Future Roadmap (v0.2)

- **Autopilot Export:** DJI WPML/KMZ and QGroundControl `.plan` (JSON).
- **KML Import:** Arbitrary polygon boundary clipping.
- **LiDAR Support:** Point density models and swath-width conflicts.
- **Kinematics:** Dubins Path generation for fixed-wing turn geometries.
