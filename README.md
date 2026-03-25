# IronTrack

**IronTrack** is a high-performance, headless Rust compute engine for automated aerial survey flight planning. It replaces legacy proprietary GIS flight management software with a modern, memory-safe, and highly concurrent FOSS alternative.

The engine ingests Digital Elevation Models (DEMs), calculates photogrammetrically correct flight lines over variable terrain, and exports validated mission plans to industry-standard GeoPackage and GeoJSON formats.

## Key Features

*   **High-Precision Geodesy:**
    *   **Karney Geodesic Solver:** 15-nanometer accuracy for distance and azimuth (via `geographiclib-rs`).
    *   **Karney-Krüger 6th-order UTM:** Sub-millimeter WGS84 ↔ UTM projection using the full series expansion.
    *   **Geoid Models:** Integrated EGM96 and EGM2008 undulation lookups ($h = H + N$).
*   **Terrain-Aware Planning:**
    *   **Copernicus GLO-30 Support:** Native parsing of Cloud-Optimized GeoTIFFs (FLOAT32, DEFLATE+Predictor=3).
    *   **Dynamic AGL Adjustment:** Automatically raises flight altitudes to maintain target GSD and side-lap over ridges and peaks.
    *   **Two-Phase I/O:** Optimized concurrency model using `rayon` for math-heavy processing without blocking on DEM tile I/O.
*   **Enterprise I/O:**
    *   **GeoPackage (OGC):** Binary geometry serialization (LINESTRINGZ), R-tree spatial indexing, and WAL-mode SQLite performance.
    *   **GeoJSON (RFC 7946):** Full 3D feature collection export with per-line altitude metadata.
*   **Performance First:**
    *   **Headless-First:** Designed for CLI automation and backend integration (no GUI overhead).
    *   **Memory Safe:** Written in 100% Rust to eliminate memory-corruption vulnerabilities common in legacy C++ GIS toolchains.

## Status: v0.1 (Compute Engine)

IronTrack is currently in its initial release phase (v0.1). The core compute engine is fully functional and has been verified against NIMA TR8350.2 and Karney's GeodTest reference data.

### Supported Workflows:
*   `irontrack terrain`: Query orthometric height, geoid undulation, and ellipsoidal height.
*   `irontrack plan`: Generate terrain-adjusted flight lines over a bounding box for a given sensor (Phantom 4 Pro, Mavic 3, iXM-100).

## Installation

Requires the Rust 2021 stable toolchain.

```bash
git clone https://github.com/irontrack-project/irontrack.git
cd irontrack
cargo build --release
```

The resulting binary will be at `target/release/irontrack`.

## Quick Start

### Query Terrain
```powershell
./irontrack terrain --lat 49.2827 --lon -123.1207
```
*(Note: Requires Copernicus GLO-30 tiles in the local cache or an active internet connection for auto-download.)*

### Generate a Flight Plan
```powershell
./irontrack plan \
  --min-lat 49.0 --min-lon -123.1 \
  --max-lat 49.1 --max-lon -123.0 \
  --gsd-cm 3.0 --sensor phantom4pro \
  --terrain \
  --output mission.gpkg --geojson mission.geojson
```

## Documentation

Detailed technical specifications and architectural analyses are located in the `docs/` directory:
*   [Geodesy Foundations](docs/02_geodesy_foundations.md)
*   [Photogrammetry Spec](docs/03_photogrammetry_spec.md)
*   [GeoPackage Architecture](docs/04_geoPackage_architecture.md)

## License

IronTrack is licensed under the **GNU General Public License v3.0 or later**. See the [LICENSE](LICENSE) file for the full text.

*Copyright (C) 2026 IronTrack Project Contributors*
