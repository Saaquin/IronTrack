# Early Adopters

## What You Can Try Today

IronTrack is a high-performance headless CLI and networked daemon. There is no GUI yet (Glass Cockpit targets v0.6, Web Planning UI targets v0.7).

Four commands are currently available: `terrain`, `plan`, `daemon`, and `ask`.

## Terrain Query

```bash
irontrack terrain --lat 49.2827 --lon -123.1207
```

**What it does:**
1. Determines which Copernicus GLO-30 tile (DSM) contains the coordinate.
2. Checks your local cache (`%APPDATA%/irontrack/cache/dem/` on Windows, `~/.cache/irontrack/dem/` on Linux/macOS).
3. If missing, automatically downloads the tile from the Copernicus AWS S3 bucket.
4. Calculates EGM2008 geoid undulation (N) and returns orthometric height (MSL), undulation, and ellipsoidal height (WGS84).

## Mission Planning

### Area Survey (Photogrammetry)

```bash
irontrack plan \
  --min-lat 49.0 --min-lon -123.1 \
  --max-lat 49.1 --max-lon -123.0 \
  --gsd-cm 3.0 --sensor phantom4pro \
  --terrain \
  --output survey.gpkg --geojson survey.geojson
```

**What it does:**
1. Generates a grid of parallel flight lines over the bounding box.
2. Calculates nominal AGL required to achieve 3.0 cm GSD for the Phantom 4 Pro.
3. With `--terrain`, queries the Copernicus DEM across all waypoints and automatically raises altitude where ridges or peaks would violate side-lap overlap or the GSD target.
4. Exports a 3D GeoPackage (OGC standard) and RFC 7946 GeoJSON.

**Sensor presets:** `phantom4pro`, `mavic3`, `ixm100`, or supply custom parameters via `--focal-mm`, `--sensor-width-mm`, `--sensor-height-mm`, `--image-width-px`, `--image-height-px`.

### Corridor Survey

```bash
irontrack plan \
  --kml corridor_centerline.kml \
  --corridor --corridor-lines 3 \
  --gsd-cm 5.0 --sensor mavic3 \
  --output corridor.gpkg
```

Generates parallel offset passes along a KML centerline with miter/bevel join handling and self-intersection removal.

### LiDAR Mission

```bash
irontrack plan \
  --min-lat 49.0 --min-lon -123.1 \
  --max-lat 49.1 --max-lon -123.0 \
  --mission-type lidar \
  --prr 300000 --scan-rate 100 --fov 60 \
  --point-density 8.0 \
  --output lidar_survey.gpkg
```

Calculates swath geometry and flight speed from pulse repetition rate, scan rate, and target point density.

### Advanced Options

- **Procedure turns:** `--procedure-turns` generates Dubins path turns with clothoid transitions between flight lines.
- **Route optimization:** `--optimize-route` applies nearest-neighbor + 2-opt TSP to minimize total transit distance.
- **Terrain following:** `--terrain` adjusts AGL per-waypoint using Copernicus DEM data.
- **Export formats:** `--geojson`, `--qgc` (QGroundControl `.plan`), `--dji` (DJI `.kmz`).

## FMS Daemon (v0.4 Preview)

```bash
irontrack daemon --port 8080 --mock-telemetry
```

Starts the Flight Management System server with an Axum REST API and WebSocket telemetry stream. The `--mock-telemetry` flag generates synthetic GPS/attitude data for testing without hardware.

**REST endpoints** (under `/api/v1/`): mission creation, flight plan management, terrain queries, route optimization.

**WebSocket:** Connect at `/ws` for 10 Hz telemetry broadcast (position, attitude, active line, trigger events).

## AI Assistant

```bash
irontrack ask "What GSD will I get at 120m AGL with a Phase One iXM-100?"
```

Requires `ANTHROPIC_API_KEY`. Uses Claude for photogrammetry-aware Q&A grounded in IronTrack's sensor and geodesy models.

## Visualization

- **QGIS:** Open the `.gpkg` file. It includes a LINESTRINGZ feature layer with an R-tree spatial index.
- **GeoJSON.io:** Drag and drop the `.geojson` file to preview 3D flight lines.
- **QGroundControl:** Import the `.plan` file directly for autopilot upload.
- **DJI Pilot:** Import the `.kmz` file for DJI enterprise drones.
- **Google Earth:** The GeoJSON can be converted to KML/KMZ using standard tools.

## Known Limitations (v0.3.1)

- **DSM Bias:** Copernicus GLO-30 is a surface model. Reported altitudes include canopy. Clearance over forests may be understated by 2–8 m.
- **Rectangular Bounding Boxes or KML only:** Arbitrary polygon boundaries from Shapefile/DXF/GeoJSON are coming in v0.7.
- **No real-time hardware telemetry yet:** The daemon supports mock telemetry. Real serial GNSS integration requires the `serial` feature flag and is in active development for v0.4.
- **No wind-aware routing:** Energy-weighted TSP and wind triangle corrections are planned for v0.5.
- **No GUI:** The Glass Cockpit (Tauri 2.0) is targeted for v0.6.

## Safe To Use For

- Pre-flight planning and feasibility analysis.
- Generating high-precision geodetic references.
- Comparing GSD targets against terrain relief.
- Corridor mapping with offset polylines.
- LiDAR mission geometry and point density planning.
- Route-optimized multi-line flight plans.
- Testing daemon REST/WebSocket integrations against mock telemetry.

## Not A Good Fit Yet

- Safety-critical mission dispatch without independent validation.
- Real-time in-flight FMS operations (daemon is functional but not field-validated).
- Autonomous geofence enforcement (IronTrack is advisory only — PIC retains authority).
- Legacy SRTM-only workflows.
