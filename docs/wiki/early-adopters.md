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

## FMS Daemon (v0.4)

### Quick Start

```bash
irontrack daemon --port 8080 --mock-telemetry
```

On startup the daemon prints a session token:

```
Session token: a3f8c1d9e7b2...  (use this for API authentication)
Listening on http://0.0.0.0:8080
mDNS: advertising _irontrack._tcp.local.
```

The `--mock-telemetry` flag generates a synthetic aircraft flying along the loaded flight plan at 15 m/s (configurable with `--mock-speed`). This allows full testing without hardware.

### REST API

All endpoints are under `/api/v1/` and require a bearer token:

```bash
# Load a mission
curl -X POST http://localhost:8080/api/v1/mission/load \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"min_lat":49.0, "min_lon":-123.1, "max_lat":49.1, "max_lon":-123.0, "gsd_cm":3.0, "sensor":"phantom4pro"}'

# Get the flight plan as GeoJSON
curl http://localhost:8080/api/v1/mission/plan \
  -H "Authorization: Bearer <token>"

# Export to QGroundControl
curl -o mission.plan http://localhost:8080/api/v1/mission/export/qgc \
  -H "Authorization: Bearer <token>"
```

### WebSocket Telemetry

Connect at `ws://host:port/ws?token=<token>` (the token goes in the query string). On connection you receive the current system status, then a 10 Hz stream of position/attitude frames. All messages are JSON with a `type` field (`INIT`, `TELEMETRY`, `STATUS`, `TRIGGER`, `WARNING`).

### Service Discovery

The daemon advertises itself as `_irontrack._tcp.local.` via mDNS. Clients on the same LAN can discover it automatically without knowing the IP address. If mDNS fails (common on aircraft WiFi with client isolation), fall back to manual IP entry.

### Hardware GNSS

With the `serial` feature enabled, the daemon can connect to a real USB GNSS receiver:

```bash
irontrack daemon --port 8080 --gnss-vid 0x1546 --gnss-pid 0x01A9 --gnss-baud 115200
```

The daemon auto-detects the receiver by USB Vendor/Product ID (no hardcoded COM ports), handles hot-plug disconnections transparently, and monitors RTK fix quality. If RTK degrades from Fixed to Float, a warning is broadcast to all connected clients.

Supported chipsets: u-blox, CP2102, FTDI. CH340 is not supported.

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

## Known Limitations (v0.4.0)

- **DSM Bias:** Copernicus GLO-30 is a surface model. Reported altitudes include canopy. Clearance over forests may be understated by 2-8 m.
- **Rectangular Bounding Boxes or KML only:** Arbitrary polygon boundaries from Shapefile/DXF/GeoJSON are coming in v0.7.
- **Daemon is functional but not field-validated:** REST, WebSocket, NMEA, and serial are implemented and integration-tested, but have not been through Eagle Mapping field acceptance.
- **No wind-aware routing:** Energy-weighted TSP and wind triangle corrections are planned for v0.5.
- **No GUI:** The Glass Cockpit (Tauri 2.0) is targeted for v0.6.
- **Security:** Bearer tokens are transmitted in plaintext over HTTP. The daemon assumes a trusted LAN. TLS is planned for v1.0.

## Safe To Use For

- Pre-flight planning and feasibility analysis.
- Generating high-precision geodetic references.
- Comparing GSD targets against terrain relief.
- Corridor mapping with offset polylines.
- LiDAR mission geometry and point density planning.
- Route-optimized multi-line flight plans.
- Testing daemon REST/WebSocket integrations against mock telemetry.
- Developing custom clients against the REST API and WebSocket protocol.

## Not A Good Fit Yet

- Safety-critical mission dispatch without independent validation.
- Real-time in-flight FMS operations (daemon is functional but not field-validated).
- Autonomous geofence enforcement (IronTrack is advisory only — PIC retains authority).
- Legacy SRTM-only workflows.
