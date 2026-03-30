# IronTrack

[![CI](https://github.com/Saaquin/IronTrack/actions/workflows/ci.yml/badge.svg)](https://github.com/Saaquin/IronTrack/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

High-performance open-source Flight Management System for aerial survey. IronTrack replaces fragmented legacy toolchains with a single unified Rust engine from planning desk to aircraft cockpit.

## What It Does

IronTrack generates terrain-aware, geodetically precise flight plans for photogrammetric cameras, LiDAR sensors, and corridor surveys. It ingests global elevation data, computes survey geometry, optimises flight line ordering, and exports directly to autopilot-native formats.

**Who it's for:** Survey operators, photogrammetry engineers, and UAS service providers who need sub-centimetre geodetic accuracy in their flight planning without stitching together spreadsheets, GIS plugins, and manufacturer apps.

**Proof-of-concept adopter:** Eagle Mapping.

## Key Capabilities

- **Terrain-aware planning** — Copernicus GLO-30 DSM with automatic tile fetching. Per-waypoint AGL adjustment maintains GSD and overlap over ridges and valleys.
- **Precision geodesy** — Karney geodesic solver (15 nm accuracy), Karney-Krueger 6th-order UTM, dual-geoid lookup (EGM96/EGM2008). PhantomData compile-time datum safety.
- **Terrain-following trajectories** — Elastic Band energy minimisation + cubic B-spline C2 smoothing with mathematically guaranteed clearance (convex hull property).
- **Multiple mission types** — Area grids, corridor offsets (1/3/5 parallel lines), LiDAR point density planning.
- **Procedure turns** — Six Dubins path types with clothoid (Euler spiral) transitions for smooth, flyable curves.
- **Route optimisation** — Nearest-neighbour + 2-opt TSP heuristic with Karney geodesic cost matrix.
- **Multi-format export** — 3D GeoPackage (LINESTRINGZ + R-tree), RFC 7946 GeoJSON, QGroundControl `.plan`, DJI `.kmz`.
- **Networked daemon** (v0.4) — Axum REST API + WebSocket telemetry server with NMEA parsing, USB serial manager, and mock telemetry mode.

## Quick Start

### Requirements

- Rust 1.75+ (stable)
- No external C libraries required (`rusqlite` uses bundled SQLite)

### Build

```bash
git clone https://github.com/Saaquin/IronTrack.git
cd IronTrack
cargo build --release
```

### Usage Examples

**Query terrain elevation:**

```bash
irontrack terrain --lat 49.2827 --lon -123.1207
```

**Plan a photogrammetric survey:**

```bash
irontrack plan \
  --min-lat 49.0 --min-lon -123.1 \
  --max-lat 49.1 --max-lon -123.0 \
  --gsd-cm 3.0 --sensor phantom4pro \
  --terrain --optimize-route \
  --output survey.gpkg --geojson survey.geojson
```

**Plan a corridor survey:**

```bash
irontrack plan \
  --kml centerline.kml \
  --corridor --corridor-lines 3 \
  --gsd-cm 5.0 --sensor mavic3 \
  --output corridor.gpkg
```

**Start the FMS daemon:**

```bash
irontrack daemon --port 8080 --mock-telemetry
```

### Output

Open `.gpkg` files in QGIS, import `.plan` files into QGroundControl, or drag `.geojson` files into geojson.io for quick preview. DJI `.kmz` files load directly into DJI Pilot.

## Architecture

IronTrack is a 4-layer system. The Core Engine (this repo) is complete through v0.3; the daemon layer is in active v0.4 development.

| Layer | Technology | Status |
|-------|-----------|--------|
| Core Engine | Rust (synchronous, `rayon`) | v0.3.1 |
| Network Daemon | Axum/Tokio, REST + WebSocket | v0.4 in progress |
| Trigger Controller | Embedded Rust `no_std` (STM32/RP2040) | Planned v0.8 |
| Glass Cockpit | Tauri 2.0 (Rust + WebView) | Planned v0.6 |
| Planning Web UI | React + MapLibre GL JS | Planned v0.7 |

### Module Map

```
src/
+-- main.rs              # CLI (clap) -- terrain, plan, daemon, ask
+-- geodesy/             # Karney geodesic, UTM, EGM96/EGM2008 geoid
+-- dem/                 # Copernicus GLO-30 tile cache + TerrainEngine
+-- photogrammetry/      # Flight lines, corridor, LiDAR, sensor geometry
+-- trajectory/          # Elastic band, B-spline, pure pursuit
+-- math/                # Dubins paths, TSP routing, Kahan summation
+-- network/             # Axum REST/WS server, NMEA, serial manager
+-- gpkg/                # GeoPackage (SQLite + R-tree spatial index)
+-- io/                  # GeoJSON, KML, QGroundControl, DJI export
+-- datum.rs             # Altitude datum conversion (EGM2008/EGM96/WGS84/AGL)
+-- types.rs             # Coordinate types, WGS84 constants, SoA layout
```

## Documentation

The `docs/` folder contains 52 numbered research documents covering the mathematical and engineering foundations. See [`docs/README.md`](docs/README.md) for a themed index.

The [GitHub Wiki](https://github.com/Saaquin/IronTrack/wiki) provides accessible overviews for each subsystem: geodesy, terrain, flight planning, and the daemon API.

## Running Tests

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## License

IronTrack Core Engine is licensed under the [GNU General Public License v3.0](LICENSE). Trigger controller firmware (separate repository) is MIT licensed.

Copernicus GLO-30 DEM data is provided by the European Space Agency under the Copernicus programme. Attribution is automatically included in all exported files.
