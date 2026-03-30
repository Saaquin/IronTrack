# IronTrack Research Documents

52 numbered research documents (00-52, excluding 27 which was merged into 26) plus reference material. These cover the mathematical foundations, engineering specifications, and domain research behind IronTrack.

## Foundations

| Doc | Title | Topic |
|-----|-------|-------|
| 00 | Project Charter | Scope, milestones, Eagle Mapping partnership |
| 01 | Legal Framework | GPLv3, IP, contributor licensing |
| 02 | Geodesy Foundations | Karney geodesic, Vincenty rejection, WGS84 |
| 03 | Photogrammetry Spec | GSD, FOV, overlap, ASPRS standards |
| 04 | GeoPackage Architecture | OGC spec, SQLite WAL, spatial index |
| 05 | Vertical Datum Engineering | h = H + N, EGM96/EGM2008, datum stacks |
| 06 | Rust Architecture Analysis | Crate selection, error handling, concurrency |

## Terrain & DEM

| Doc | Title | Topic |
|-----|-------|-------|
| 08 | Copernicus DEM | GLO-30 tile format, S3 access, resolution bands |
| 09 | DSM vs DTM / AGL | Surface model bias, canopy penetration, safety |
| 10 | Rust COG Ingestion | GeoTIFF parsing, DEFLATE, float predictor |
| 14 | GeoTIFF COG Processing | Cloud Optimized GeoTIFF internals |
| 15 | 3DEP Access & Integration | USGS LiDAR DTM, HTTP range requests |
| 33 | 3DEP Data Integration | AWS S3 access, microtile caching |

## Geodesy & Datums

| Doc | Title | Topic |
|-----|-------|-------|
| 12 | EGM2008 Grid Analysis | Tide system, interpolation precision |
| 19 | WGS84 Epoch Management | Tectonic drift, HTDP, ITRF realizations |
| 30 | Geodetic Datum Transformation | GEOID18, Helmert 7-param, WGS84 pivot |
| 31 | NAD83 Epoch Management | 14-param Helmert, plate motion models |
| 32 | Geoid Interpolation Survey | Polar, antimeridian, ocean edge cases |

## Flight Planning & Trajectory

| Doc | Title | Topic |
|-----|-------|-------|
| 07 | Corridor Mapping | Polyline offsets, miter/bevel joins, bank angles |
| 13 | LiDAR Flight Planning | Point density, swath geometry, scan rate |
| 34 | Terrain Following Planning | Elastic Band + B-spline hybrid design |
| 39 | Flight Path Optimization | ATSP, nearest-neighbour + Or-opt, boustrophedon |
| 49 | Computational Geometry | PIP, polygon clipping, buffer, triangulation |
| 50 | Dubins Mathematical Extension | 6 path types, clothoid, 3D airplane, wind |

## Sensors & Photogrammetry

| Doc | Title | Topic |
|-----|-------|-------|
| 18 | Oblique Photogrammetry | Off-nadir geometry, multi-camera rigs |
| 21 | Dynamic Footprint Geometry | 6DOF projection, terrain intersection |
| 24 | Survey Photogrammetry PPK | PPK integration, base station requirements |
| 36 | ASPRS Data Standards | Positional accuracy 2024, RMSE classes |
| 40 | Sensor Trigger Library | Pin-level specs, MEP timing, GPIO |
| 45 | Aerial Sensor Library | irontrack_sensors schema, camera database |
| 46 | Post-Flight QC | XTE, footprint coverage, auto re-fly |

## Daemon & Networking

| Doc | Title | Topic |
|-----|-------|-------|
| 28 | IronTrack Daemon Design | REST, WebSocket, state management, mDNS |
| 23 | NMEA Data Requirements | Sentence parsing, RTK quality indicators |
| 29 | Avionics Serial Communication | UART, CRC-32, RTS/CTS flow control |
| 47 | Daemon Field Hardware | Pi 4/5, NUC, tablets, power requirements |

## Cockpit, UI & Integration

| Doc | Title | Topic |
|-----|-------|-------|
| 25 | Aircraft Cockpit Design | Track Vector CDI, Canvas 2D rendering |
| 26 | Tauri Integration | IPC bridge, tile protocol, update governance |
| 43 | Boundary Import Formats | Shapefile, DXF, GeoJSON, CSV-WKT |

## Autopilot & Hardware

| Doc | Title | Topic |
|-----|-------|-------|
| 11 | Aerial Survey Autopilot Formats | QGC .plan, DJI WPML/KMZ specs |
| 22 | Embedded Trigger Controller | STM32/RP2040, NMEA FSM, dead reckoning |
| 41 | LiDAR System Control | Dual-trigger hybrid, USGS QL compliance |
| 48 | Avionics Hardware Integration | Connector pinouts, power sequencing |

## Aviation & Safety

| Doc | Title | Topic |
|-----|-------|-------|
| 16 | Aerial Wind Data | Wind triangle, ADDS API, METAR/TAF |
| 17 | UAV Survey Analysis | Multirotor vs fixed-wing, endurance |
| 20 | NA Airspace Data | FAA data sources, TFR, NOTAM |
| 35 | Airspace & Obstacle Data | Advisory geofencing, PIC authority |
| 37 | Aviation Safety Principles | PhantomData, advisory-only design |
| 38 | UAV Endurance Modeling | Power models, battery reserve, Peukert |

## Engineering & Testing

| Doc | Title | Topic |
|-----|-------|-------|
| 42 | GeoPackage Extensions | TGCE, basemaps, WAL details, VACUUM policy |
| 44 | IronTrack Testing Strategy | proptest, HIL/SIL, CI matrix |
| 51 | GPLv3 Compliance | License headers, CLA, firmware separation |
| 52 | Geospatial Performance Rust | SoA layout, AVX2, rayon tuning, Kahan |

## Reference

| Doc | Title | Topic |
|-----|-------|-------|
| 99 | Engine Technical Reference | Architecture overview, module map, workflows |
| -- | Formula Reference | Equations from docs 02-07, 30, 32, 34, 38, 39, 46, 49, 50 |
| -- | Formula Reference 08-10 | Equations from terrain/DEM docs |
| -- | IronTrack Manifest v3.5 | Full system architecture (21 sections, 520 paragraphs) |
