# IronTrack — Engine Technical Reference

This document provides a technical overview of the IronTrack Core Engine implementation as of v0.1/v0.2-alpha. It describes the software architecture, module responsibilities, and critical implementation details for maintainers.

## 1. Core Architecture

IronTrack is designed as a synchronous, high-performance compute engine. It follows a strict separation between I/O-bound operations and CPU-bound mathematical processing.

### 1.1 Concurrency Model: The Two-Phase I/O Rule
To maximize performance on multi-core systems while avoiding thread starvation in the `rayon` thread pool, the engine follows the **Two-Phase I/O Rule**:
1.  **Phase 1 (Sequential Warm-up):** All required external data (DEM tiles, geoid grids) is identified and loaded into memory or an in-memory cache sequentially.
2.  **Phase 2 (Parallel Math):** The mathematical processing (flight line generation, altitude adjustment, datum conversion) is dispatched via `rayon::par_iter()`. This phase performs NO blocking I/O.

### 1.2 Data Layout: Structure of Arrays (SoA)
The engine utilizes a Structure of Arrays (SoA) layout for bulk coordinate data (e.g., in `FlightLine`). This layout is cache-friendly, allowing for efficient SIMD pre-fetching and auto-vectorization by the Rust compiler.

---

## 2. Module Reference

### 2.1 `src/types.rs`: Foundational Data Structures
Defines the core primitive types used across the system.
- `GeoCoord`: WGS84 coordinates stored internally as radians.
- `UtmCoord`: Easting/Northing with zone and hemisphere.
- `FlightLine`: An SoA container for waypoints (`lats`, `lons`, `elevations`).
- `SensorParams`: Physical camera/sensor specifications.
- `AltitudeDatum`: Enum for vertical reference tagging (`Egm2008`, `Egm96`, `Wgs84Ellipsoidal`, `Agl`).

### 2.2 `src/geodesy/`: Geodetic Computations
- `karney.rs`: High-precision geodesic inverse and direct solvers wrapping `geographiclib-rs`. Accuracy: ~15 nanometers.
- `utm.rs`: Implementation of the **Karney-Krüger 6th-order series** for WGS84 ↔ UTM projection. Provides sub-millimeter precision within the UTM zone.
- `geoid.rs`: Lookup and bilinear interpolation for geoid undulation ($N$). Supports **EGM96** (via vendored crate) and **EGM2008** (via native grid interpolation).

### 2.3 `src/dem/`: Terrain Subsystem
- `copernicus.rs`: Native parser for **Copernicus GLO-30** GeoTIFFs. Supports FLOAT32 samples with DEFLATE + FloatingPoint Predictor compression.
- `cache.rs`: A cross-platform microtile cache. Missing tiles are automatically fetched from the Copernicus AWS S3 bucket.
- `mod.rs` (`TerrainEngine`): The high-level API for terrain queries. It manages tile loading and provides bulk query methods (`elevations_at`) following the Two-Phase I/O rule.

### 2.4 `src/photogrammetry/`: Survey Planning Logic
- `sensor.rs`: Derives GSD, FOV, and swath width from physical sensor parameters.
- `flightlines.rs`: Generates parallel flight lines over a bounding box.
- `overlap.rs` (Internal to `flightlines.rs`): Performs dynamic AGL adjustment. If a terrain peak reduces side-lap below the target threshold, the waypoint altitude is raised locally.

### 2.5 `src/datum.rs`: Vertical Datum Transformation
Implements `FlightLine::to_datum()`. This is a high-precision conversion pipeline that chains all transformations through the WGS84 ellipsoidal height as a central pivot. 
- Example: $H_{EGM2008} \to h_{WGS84} \to H_{EGM96}$.
- Includes `rayon` parallelization for bulk waypoint conversion.

### 2.6 `src/gpkg/`: GeoPackage Implementation
- `init.rs`: SQLite database initialization with WAL mode and mandatory OGC metadata tables.
- `binary.rs`: Manual serialization of `StandardGeoPackageBinary`. Supports **LINESTRINGZ** (3D geometries) with 48-byte envelopes.
- `rtree.rs`: Implementation of the R-tree spatial index. Registers custom SQLite functions (`ST_MinX`, etc.) to parse GPKG blobs directly for trigger-based synchronization.

---

## 3. Critical Workflows

### 3.1 Flight Plan Generation
1.  CLI parses arguments into `SensorParams` and `FlightPlanParams`.
2.  `generate_flight_lines` creates a flat-terrain baseline using Karney geodesics.
3.  If `--terrain` is enabled:
    - `TerrainEngine` pre-fetches tiles for the bounding box.
    - `validate_overlap` checks for terrain interference.
    - `adjust_for_terrain` raises altitudes to maintain overlap/GSD.
4.  `to_datum` converts the resulting lines to the target vertical datum.
5.  Exporters write to `.gpkg` and `.geojson`.

### 3.2 Automated DEM Ingestion
1.  Engine receives a coordinate query.
2.  `DemCache` maps the coordinate to a Copernicus GLO-30 tile name (e.g., `Copernicus_DSM_COG_10_N49_00_W123_00_DEM`).
3.  If file exists in `%APPDATA%/irontrack/cache/dem/`, it is loaded.
4.  If not, the engine constructs an S3 URL and downloads the tile.
5.  Bilinear interpolation is applied to the raw FLOAT32 grid posts to return the orthometric height ($H$).

---

## 4. Safety and Compliance

- **DSM Warning:** Because Copernicus is a Digital Surface Model (not bare-earth DTM), the engine prints a mandatory safety warning regarding 2–8m canopy penetration bias.
- **Precision Guard:** All coordinate math uses `f64`. Floating-point drift in distance accumulation is prevented using **Kahan summation**.
- **Licensing:** Every `.rs` file must carry the GPLv3 header. Copernicus attribution is injected into all exported files.
