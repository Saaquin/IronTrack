# IronTrack — Claude Code Prompt Playbook

> These are sequential prompts to paste into Claude Code sessions.
> Work through them in order. Each prompt is a self-contained session.
> Claude Code reads your CLAUDE.md automatically, so it already knows the project context.

---

## PHASE 0: Project Scaffolding

### Prompt 0A — Initialize the Workspace

```
Read CLAUDE.md to understand this project. Then set up the full project structure:

1. Ensure Cargo.toml has the correct [package] metadata:
   - name = "irontrack"
   - version = "0.1.0"
   - edition = "2021"
   - license = "GPL-3.0-or-later"
   - description = "Open-source flight management and aerial survey planning engine"
   - Add all the core dependencies listed in CLAUDE.md

2. Create the module directory structure under src/:
   - src/main.rs (CLI entry point using clap)
   - src/lib.rs (library root, re-exports all modules)
   - src/geodesy/mod.rs (Karney geodesic, UTM projection, geoid)
   - src/dem/mod.rs (SRTM tile parsing, caching, interpolation)
   - src/photogrammetry/mod.rs (sensor model, GSD, FOV, swath)
   - src/gpkg/mod.rs (GeoPackage creation, binary serialization)
   - src/io/mod.rs (GeoJSON export, flight reporting)
   - src/types.rs (core data types, SoA coordinate structs)
   - src/error.rs (error types using thiserror)

3. Every .rs file gets the GPLv3 header from CLAUDE.md.

4. src/main.rs should parse a simple --help with clap and print "IronTrack v0.1.0-dev"

5. Make sure `cargo build` and `cargo clippy` pass clean.

Do NOT implement any logic yet — just the skeleton with empty modules and placeholder structs.
```

### Prompt 0B — Core Type Definitions

```
Read CLAUDE.md and docs/02_geodesy_foundations.md. Now implement src/types.rs with the foundational data types:

1. WGS84 ellipsoid constants as a module-level const block:
   - Semi-major axis a = 6378137.0 m
   - Inverse flattening 1/f = 298.257223563
   - Derive: flattening f, semi-minor axis b, eccentricity squared e2, second eccentricity squared e_prime2, third flattening n

2. Geographic coordinate struct (lat/lon/height in f64, all radians internally, with degree constructors)

3. UTM coordinate struct (easting, northing, zone number, hemisphere enum)

4. SoA flight line struct per CLAUDE.md:
   struct FlightLine {
       lats: Vec<f64>,
       lons: Vec<f64>,
       elevations: Vec<f64>,
   }

5. Sensor parameters struct:
   - focal_length_mm: f64
   - sensor_width_mm: f64
   - sensor_height_mm: f64
   - image_width_px: u32
   - image_height_px: u32
   - Method: pixel_pitch() -> f64

6. Implement Display traits for readable output.

Write unit tests verifying the WGS84 derived constants against known reference values.
```

### Prompt 0C — Error Types and License File

```
1. Implement src/error.rs using thiserror with these error variants:
   - DemError (tile not found, parse failure, cache failure)
   - GeodesyError (invalid coordinates, projection failure)
   - GpkgError (database init failure, serialization failure, wraps rusqlite::Error)
   - IoError (wraps std::io::Error)
   - PhotogrammetryError (invalid sensor params, overlap violation)

2. Create a LICENSE file in the project root containing the full text of GPLv3.
   You can use a placeholder comment pointing to https://www.gnu.org/licenses/gpl-3.0.txt

3. Create a minimal README.md:
   - Project name and one-line description
   - "⚠️ Under active development — not yet functional"
   - Build instructions (cargo build --release)
   - License: GPLv3

4. Verify cargo build and cargo test pass.
```

### Prompt 0D — GitHub Actions CI

```
Create .github/workflows/ci.yml with a basic CI pipeline:

1. Triggers on push to main and all pull requests
2. Matrix: ubuntu-latest, stable Rust
3. Steps:
   - Checkout
   - Install Rust stable via dtolnay/rust-toolchain
   - cargo fmt --check
   - cargo clippy -- -D warnings
   - cargo build --release
   - cargo test

Keep it simple. No caching or fancy stuff yet.
```

---

## PHASE 1: Milestone 1 — Dynamic Terrain Ingestion

### Prompt 1A — SRTM Tile Parser

```
Read CLAUDE.md and docs/02_geodesy_foundations.md Section 1.

Implement src/dem/srtm.rs — the SRTM HGT file parser:

1. An SRTM1 .hgt file is exactly 25,934,402 bytes:
   - 3601 rows × 3601 columns × 2 bytes per sample
   - Each sample is a signed 16-bit big-endian integer (elevation in meters)
   - Void/no-data value is -32768
   - The filename encodes the SW corner: N47W122.hgt means lat=47°N, lon=122°W

2. Implement:
   - parse_hgt_filename(name: &str) -> Result<(f64, f64)> to extract the SW corner coordinates
   - SrtmTile struct that loads and owns the raw elevation grid
   - SrtmTile::from_file(path: &Path) -> Result<SrtmTile>
   - SrtmTile::elevation_at(lat: f64, lon: f64) -> Option<f64> using bilinear interpolation
     between the four surrounding grid posts

3. Bilinear interpolation math:
   - Convert lat/lon to fractional row/col indices
   - lat index: (tile_north_edge - lat) * 3600.0
   - lon index: (lon - tile_west_edge) * 3600.0
   - Interpolate between 4 surrounding posts
   - Return None for void pixels (-32768)

4. Write tests using a small synthetic 3×3 or 5×5 grid to verify interpolation accuracy.
   Don't rely on downloading real SRTM tiles for unit tests — create test fixtures.
```

### Prompt 1B — Cache Manager

```
Read CLAUDE.md. Implement src/dem/cache.rs — the DEM tile cache:

1. Use the `dirs` crate to locate a cross-platform cache directory:
   - dirs::cache_dir() / "irontrack" / "srtm"
   - Create the directory tree if it doesn't exist

2. Implement DemCache struct with:
   - cache_dir: PathBuf
   - new() -> Result<Self> (creates dirs)
   - tile_path(lat: i32, lon: i32) -> PathBuf (generates the expected .hgt filename)
   - has_tile(lat: i32, lon: i32) -> bool
   - get_tile(lat: i32, lon: i32) -> Result<SrtmTile> (loads from cache, returns DemError if missing)

3. For v0.1, downloading tiles is a stretch goal. For now, implement a
   stub download method that returns a clear error message:
   "Tile N47W122.hgt not in cache. Download from https://e4ftl01.cr.usgs.gov/ and place in {cache_dir}"

4. Implement a DemProvider that composes the cache with tile lookups:
   - elevation_at(lat: f64, lon: f64) -> Result<f64>
   - Determines which tile contains the coordinate
   - Loads from cache (lazy — only load tiles when first queried)
   - Handles tile boundaries (a point exactly on a tile edge)

5. Write tests for path generation and the tile-selection logic.
```

### Prompt 1C — EGM96 Geoid Lookup

```
Read CLAUDE.md and docs/05_vertical_datum_engineering.md Sections 2-3.

Implement src/geodesy/geoid.rs — the EGM96 geoid undulation lookup:

1. Background: DEMs give orthometric height (H). GNSS gives ellipsoidal height (h).
   The relationship is: h = H + N, where N is the geoid undulation from EGM96.
   N can range from roughly -105m to +85m globally.

2. The egm96 crate provides a simple API. Use it if it compiles cleanly.
   If not, implement a minimal grid interpolator:
   - The EGM96 15-arc-minute grid is a 721×1441 grid of f32 values
   - Bilinear interpolation at arbitrary lat/lon

3. Implement:
   - geoid_undulation(lat_deg: f64, lon_deg: f64) -> Result<f64>
     Returns N in meters.
   - orthometric_to_ellipsoidal(lat: f64, lon: f64, ortho_h: f64) -> Result<f64>
     Returns h = ortho_h + N
   - ellipsoidal_to_orthometric(lat: f64, lon: f64, ellip_h: f64) -> Result<f64>
     Returns H = ellip_h - N

4. Write tests against known EGM96 reference values:
   - Near the Indian Ocean gravity low: N ≈ -105m
   - Near Indonesia gravity high: N ≈ +85m
   - Continental US: N is typically between -10m and -40m
```

### Prompt 1D — Integration: Terrain Query Pipeline

```
Read CLAUDE.md. Now wire the DEM + geoid modules together in src/dem/mod.rs:

1. Create a TerrainEngine that combines DemProvider and geoid lookup:
   - terrain_elevation(lat: f64, lon: f64) -> Result<f64>
     Returns orthometric height from DEM
   - ellipsoidal_terrain_elevation(lat: f64, lon: f64) -> Result<f64>
     Returns DEM elevation converted to ellipsoidal height (applies geoid correction)

2. Implement a simple CLI subcommand in main.rs:
   irontrack terrain --lat 47.6062 --lon -122.3321
   Should output:
   Location: 47.6062°N, 122.3321°W
   Orthometric elevation: 56.0 m (MSL)
   Geoid undulation (N): -22.3 m
   Ellipsoidal elevation: 33.7 m (WGS84)

   (Obviously this only works if the user has the SRTM tile cached locally)

3. Handle the "no tile" case gracefully with a clear message telling the user
   where to download it.

4. Write an integration test in tests/terrain_integration.rs that creates
   a tiny synthetic .hgt file in a temp directory, points the cache at it,
   and verifies the full pipeline returns correct values.
```

---

## PHASE 2: Milestone 2 — Sensor Geometry and Flight Lines

### Prompt 2A — Photogrammetric Math Core

```
Read CLAUDE.md and docs/03_photogrammetry_spec.md.

Implement src/photogrammetry/sensor.rs — the core sensor geometry math:

1. Using the SensorParams struct from types.rs, implement:

   - pixel_pitch(&self) -> f64
     sensor_width_mm / image_width_px as f64 (in mm)

   - fov_horizontal(&self) -> f64
     2.0 * (sensor_width_mm / (2.0 * focal_length_mm)).atan()  (radians)

   - fov_vertical(&self) -> f64
     2.0 * (sensor_height_mm / (2.0 * focal_length_mm)).atan()  (radians)

   - gsd_at_agl(&self, agl_m: f64) -> (f64, f64)
     Returns (gsd_x, gsd_y) in meters/pixel
     gsd_x = (agl * sensor_width_mm) / (focal_length_mm * image_width_px as f64)
     gsd_y = (agl * sensor_height_mm) / (focal_length_mm * image_height_px as f64)

   - worst_case_gsd(&self, agl_m: f64) -> f64
     max(gsd_x, gsd_y) — the binding constraint per docs/03

   - swath_width(&self, agl_m: f64) -> f64
     2.0 * agl * (fov_horizontal / 2.0).tan()

   - swath_height(&self, agl_m: f64) -> f64
     2.0 * agl * (fov_vertical / 2.0).tan()

   - required_agl_for_gsd(&self, target_gsd_m: f64) -> f64
     Rearranged from worst-case GSD: solve for AGL

2. Write thorough unit tests with a known sensor (e.g., Phase One iXM-100:
   sensor 53.4×40.0 mm, 11664×8750 px, 50mm lens).
   Verify GSD at 1000m AGL, swath width, etc. against hand calculations.
```

### Prompt 2B — Flight Line Generator

```
Read CLAUDE.md and docs/02_geodesy_foundations.md Section 4, and docs/03_photogrammetry_spec.md.

Implement src/photogrammetry/flightlines.rs — the flight line spacing calculator:

1. Implement FlightPlanParams:
   - sensor: SensorParams
   - target_gsd_m: f64
   - side_lap_percent: f64 (default 30.0)
   - end_lap_percent: f64 (default 60.0)
   - flight_altitude_msl: Option<f64>

2. Implement static (flat terrain) calculations first:
   - nominal_agl from target GSD
   - flight_line_spacing = swath_width * (1.0 - side_lap_percent / 100.0)
   - photo_interval = swath_height * (1.0 - end_lap_percent / 100.0)

3. Implement generate_flight_lines() that takes:
   - A rectangular bounding box (min_lat, min_lon, max_lat, max_lon)
   - Flight direction azimuth (degrees from north)
   - FlightPlanParams
   Returns a Vec<FlightLine> where each FlightLine contains the waypoint
   coordinates along that line.

4. For now, implement the flat-terrain version only. We'll add DEM-aware
   dynamic adjustment in the next prompt.

5. Use Karney's geodesic algorithm (geographiclib-rs) to:
   - Calculate the geodesic distance across the bounding box
   - Step along each flight line at the correct photo interval
   - Offset parallel lines by the computed spacing

6. Write tests: for a known bounding box and sensor, verify the number of
   flight lines and approximate spacing matches hand calculations.
```

### Prompt 2C — Dynamic Terrain-Aware Overlap Validation

```
Read CLAUDE.md and docs/03_photogrammetry_spec.md section on Dynamic AGL.

Extend src/photogrammetry/flightlines.rs with terrain awareness:

1. Implement validate_overlap() that:
   - Takes a Vec<FlightLine> and access to the TerrainEngine
   - For each proposed exposure point on each flight line:
     a. Query the DEM for the highest elevation within the projected swath footprint
     b. Calculate the actual AGL = flight_altitude_msl - max_terrain_elevation
     c. Calculate the resulting swath width at that actual AGL
     d. Check if adjacent flight lines still achieve the required side-lap
   - Returns a validation report: Vec of problem points where overlap drops below threshold

2. Implement adjust_for_terrain() that:
   - Takes the flat-terrain flight plan and the TerrainEngine
   - At each exposure point, if the terrain peak would violate the overlap constraint:
     Option A: Raise the flight altitude locally (terrain-following)
     Option B: Insert additional flight lines where the gap occurs
   - For v0.1, implement Option A (altitude adjustment) — it's simpler
   - Return the adjusted flight plan

3. This is CPU-intensive — use rayon's par_iter for the DEM queries across
   all exposure points. This is exactly the workload pattern described in
   docs/06_rust_architecture_analysis.md for Rayon.

4. Test with a synthetic DEM that has a known ridge, and verify the validator
   catches the overlap violation and the adjuster raises altitude over the ridge.
```

---

## PHASE 3: Milestone 3 — GeoPackage I/O

### Prompt 3A — GeoPackage Initialization

```
Read CLAUDE.md and docs/04_geopackage_architecture.md thoroughly.

Implement src/gpkg/init.rs — GeoPackage database creation from scratch:

1. Create a new SQLite database via rusqlite with these PRAGMA statements
   executed immediately on connection:
   - PRAGMA application_id = 1196444487;  (0x47504B47 = "GPKG")
   - PRAGMA user_version = 10300;          (GeoPackage 1.3.0)
   - PRAGMA journal_mode = WAL;
   - PRAGMA synchronous = NORMAL;
   - PRAGMA cache_size = -64000;           (64MB cache)
   - PRAGMA page_size = 4096;

2. Create the mandatory metadata tables with exact DDL:
   - gpkg_spatial_ref_sys (with the 3 required seed records: EPSG:4326, srs_id -1, srs_id 0)
   - gpkg_contents
   - gpkg_geometry_columns
   - gpkg_extensions

3. Implement GeoPackage struct:
   - new(path: &Path) -> Result<Self>  (creates and initializes)
   - open(path: &Path) -> Result<Self> (opens existing, validates PRAGMA application_id)
   - register_feature_table(name, srs_id, geometry_type, z_flag, m_flag, bbox) -> Result<()>
     Creates the user feature table, registers in gpkg_contents and gpkg_geometry_columns

4. Write a test that creates a GeoPackage, opens it with a fresh connection,
   and verifies the PRAGMAs and mandatory tables exist.
```

### Prompt 3B — Binary Geometry Serialization

```
Read docs/10_Rust_COG_Ingestion.md and docs/08_copernicusDEM.md.

Update src/main.rs to build the v0.1 pipeline. We are NOT downloading monolithic SRTM ZIPs. We are using cloud-native streaming.

Add a new subcommand `generate-survey`:
1. Takes arguments: `--polygon` (path to GeoJSON), `--altitude-agl` (f64), `--gsd` (f64), `--sensor-width` (u32), `--sensor-height` (u32), `--focal-length` (f64), `--overlap-forward` (f64), `--overlap-lateral` (f64).
2. The execution pipeline:
   a. Parse the input polygon and calculate the WGS84 bounding box.
   b. Use `reqwest` and `tokio` to query the STAC API for Copernicus GLO-30 COGs intersecting the bounding box.
   c. Perform async byte-range HTTP requests to extract ONLY the specific DEM tiles required. Do not download the entire COG.
   d. Cache the extracted micro-tiles in `%APPDATA%/irontrack/cache/dem/` using the `dirs` crate.
   e. Generate the flight lines over the bounding box, utilizing the cached Copernicus DEM data to adjust line spacing and altitude dynamically based on the DSM terrain.
   f. Export the final validated lines to GeoPackage and GeoJSON.

Write an end-to-end integration test with a synthetic polygon that mocks the HTTP request to verify the pipeline logic.
```

### Prompt 3C — R-Tree Spatial Index

```
Read docs/04_geopackage_architecture.md section on R-Trees.

Implement src/gpkg/rtree.rs — the spatial index setup:

1. For a feature table <t> with geometry column <c> and primary key <i>,
   implement create_spatial_index() that:

   a. Registers in gpkg_extensions:
      INSERT INTO gpkg_extensions VALUES('<t>', '<c>', 'gpkg_rtree_index', 'write-only', ...)

   b. Creates the R-tree virtual table:
      CREATE VIRTUAL TABLE rtree_<t>_<c> USING rtree(id, minx, maxx, miny, maxy)

   c. Creates all 7 synchronization triggers:
      - Insert trigger (new row with geometry → insert into R-tree)
      - Update 1 (geometry updated, id unchanged → update R-tree)
      - Update 2 (geometry moved to different row id → delete old + insert new)
      - Update 3 (both cases → delete old + insert new)
      - Update 4 (geometry set to NULL → delete from R-tree)
      - Delete trigger (row deleted → delete from R-tree)
      - Update from NULL to geometry → insert into R-tree

2. The triggers use ST_MinX, ST_MaxX, ST_MinY, ST_MaxY functions.
   Since we're writing our own GeoPackage (not using SpatiaLite),
   we need to register custom SQLite functions that parse the GPKG binary
   header envelope bytes to extract the bbox values directly.
   This is far more performant than decoding full WKB.

3. Test: create a GeoPackage, insert a flight line geometry, and verify
   the R-tree shadow table contains the correct bounding box.
```

### Prompt 3D — GeoJSON Export

```
Read CLAUDE.md. Implement src/io/geojson.rs:

1. Implement GeoJSON FeatureCollection export for flight lines:
   - Each FlightLine becomes a Feature with LineString geometry
   - Properties include: line_number, altitude_msl, gsd, swath_width, overlap_percent

2. Implement write_geojson(path: &Path, lines: &[FlightLine], metadata: &FlightMetadata) -> Result<()>

3. Use serde_json for serialization. Build the GeoJSON structure manually
   rather than pulling in a heavy GIS crate — it's a simple format:
   {
     "type": "FeatureCollection",
     "features": [
       {
         "type": "Feature",
         "geometry": { "type": "LineString", "coordinates": [[lon, lat], ...] },
         "properties": { "line_number": 1, "altitude_msl": 1500.0, ... }
       }
     ]
   }

4. Note: GeoJSON spec (RFC 7946) mandates WGS84 coordinates in [longitude, latitude] order.
   Make sure we don't accidentally swap them.

5. Test: generate a simple 3-line flight plan, export to GeoJSON, parse it back,
   and verify the coordinates round-trip correctly.
```

### Prompt 3E — Full Pipeline Integration

```
Read CLAUDE.md. Wire everything together for the v0.1 CLI:

1. Implement the main CLI workflow in src/main.rs using clap subcommands:

   irontrack plan \
     --bbox "47.5,-122.4,47.7,-122.2" \
     --sensor-focal-length 50.0 \
     --sensor-width 53.4 \
     --sensor-height 40.0 \
     --image-width 11664 \
     --image-height 8750 \
     --target-gsd 0.05 \
     --side-lap 30 \
     --end-lap 60 \
     --output plan.gpkg \
     --geojson plan.geojson

2. The pipeline:
   a. Parse CLI args into SensorParams and FlightPlanParams
   b. Generate flat-terrain flight lines over the bounding box
   c. If SRTM tiles are available in cache, run terrain-aware validation and adjustment
   d. Export to GeoPackage (with R-tree index and metadata)
   e. Export to GeoJSON
   f. Print a summary: number of lines, total distance, estimated flight time, GSD achieved

3. Also implement the terrain query subcommand from Prompt 1D if not already done.

4. Write an end-to-end integration test that runs the full pipeline with
   synthetic data and verifies both output files are created and valid.

5. After this works, open the .gpkg file in QGIS to visually verify the flight lines.
   Open the .geojson in geojson.io. This is your v0.1 smoke test.
```

---

## NOTES FOR CLAUDE CODE SESSIONS

- **One prompt per session is fine.** Don't rush. Each prompt is designed to be a focused, testable unit.
- **Always run `cargo test` at the end** of each prompt before moving on.
- **If something breaks**, paste the error into Claude Code and say "fix this." It's good at that.
- **Read the docs/** files when Claude Code asks or when you want to go deeper. Tell it: "Read docs/04_geopackage_architecture.md and then implement..."
- **You can skip ahead** if a prompt feels trivial, but don't skip Phase 0 — the scaffolding matters.
- **After Phase 3**, you have a working v0.1 MVP. The next steps would be: SRTM tile auto-download, UTM projection module (manual Karney-Krüger), corridor mapping from Doc 7, and eventually a Tauri or React frontend.
