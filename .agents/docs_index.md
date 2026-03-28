# Documentation Index

All research docs live in `docs/`. 52 numbered docs (00–52, excluding 27) plus `99_engine_technical_reference.md`.
Docs 02–10 have formula gaps (images) — use `docs/formula_reference.md` and `docs/formula_reference_08_10.md`.
Docs 11+ have formulas inline. Docs 22–52 have markdown extractions in `docs/extractions/`.

| Doc | Contents | Status |
|-----|----------|--------|
| 00 | Project Charter — scope, milestones, Eagle Mapping | — |
| 01 | Legal Framework — GPLv3, IP carve-out, CLA | — |
| 02 | Geodesy Foundations — WGS84, Karney, UTM Karney-Krüger, SoA, SIMD, Kahan | — |
| 03 | Photogrammetry Spec — GSD, FOV, swath, dynamic AGL, collinearity | — |
| 04 | GeoPackage Architecture — binary header, R-tree triggers, WAL pragmas | — |
| 05 | Vertical Datum Engineering — EGM96, EGM2008, NAVD88, GEOID18, ISA | — |
| 06 | Rust Architecture — FFI, async traps, SIMD, rayon vs tokio, mmap | — |
| 07 | Corridor Mapping — polyline offsets, Dubins, bank angles, clothoids | — |
| 08 | Copernicus DEM — TanDEM-X, X-band penetration, EGM2008 ref, licensing | — |
| 09 | DSM vs DTM — canopy penetration (2–8m), AGL safety, FABDEM | — |
| 10 | COG Ingestion — Cloud-Optimized GeoTIFF, HTTP range, tiling | — |
| 11 | Autopilot Formats — QGC .plan, MAVLink, DJI WPML/KMZ, datum per platform | — |
| 12 | EGM2008 Grid — .grd/.gtx/.pgm, 1-arcmin, bilinear vs bicubic, Tide-Free | — |
| 13 | LiDAR Planning — PRR, scan rate, point density, canopy penetration, USGS QL | — |
| 14 | GeoTIFF/COG — Rust ecosystem, PREDICTOR=3, DEFLATE reversal | — |
| 15 | 3DEP Access — USGS API, NAVD88, 1-meter resolution, AWS S3 COG | — |
| 16 | Aerial Wind Data — wind triangle, atmospheric models, forecast ingestion | — |
| 17 | UAV Survey Analysis — multirotor/fixed-wing kinematics, battery, regulatory | — |
| 18 | Oblique Photogrammetry — multi-camera, trapezoidal footprints, facade | — |
| 19 | WGS84 Epoch Management — ITRF realizations, tectonic drift, PPK | — |
| 20 | NA Airspace Data — FAA LAANC, NASR, AIXM, SWIM, TFRs, NAV CANADA | — |
| 21 | Dynamic Footprint Geometry — 6DOF ray casting, polygon intersection, IMU error | — |
| 22 | Embedded Trigger Controller — STM32/RP2040, NMEA FSM, 1000 Hz dead reckoning, GPIO | ✅ |
| 23 | NMEA Data Requirements — sentence catalog, RTK degradation, baud, PPS | ✅ |
| 24 | Survey Photogrammetry PPK — event marking, open/closed loop, RINEX boundary | ✅ |
| 25 | Aircraft Cockpit Design — Track Vector CDI, ARINC 661, Canvas 2D, B612 | ✅ |
| 26 | Tauri Integration — IPC bridge, irontrack:// tiles, update governance | ✅ |
| 28 | IronTrack Daemon Design — REST, WebSocket, Arc<RwLock>, mDNS, serial | ✅ |
| 29 | Avionics Serial Communication — UART, CP2102/FTDI, CRC-32, RTS/CTS | ✅ |
| 30 | Geodetic Datum Transformation — GEOID18, 14-param Helmert, canonical pivot | ✅ |
| 31 | NAD83 Epoch Management — realization history, 40 cm drift, analytical boundary | ✅ |
| 32 | Geoid Interpolation — bilinear/biquadratic/bicubic error tables, SIMD f64x4 | ✅ |
| 33 | 3DEP Data Integration — AWS S3 COG, resolution fallback, microtile caching | ✅ |
| 34 | Terrain-Following Planning — Elastic Band, B-spline C², convex hull safety | ✅ |
| 35 | Airspace & Obstacle Data — FAA ADDS, 4D TFR, DOF 10×, AC 25-11B, advisory-only | ✅ |
| 36 | ASPRS Data Standards — 2023 Ed.2 RMSE-only, NVA/VVA, GSD heuristics, GCP reqs | ✅ |
| 37 | Aviation Safety Principles — DO-178C DAL, PhantomData, cksumvfs, fail-safe | ✅ |
| 38 | UAV Endurance Modeling — power models, drag polar, Peukert, 20% reserve | ✅ |
| 39 | Flight Path Optimization — ATSP, boustrophedon, NN+Or-opt, multi-block | ✅ |
| 40 | Sensor Trigger Library — camera specs, pin-level GPIO, MEP timing, PPS | ✅ |
| 41 | LiDAR System Control — continuous emission, dual-trigger hybrid, vendor suites | ✅ |
| 42 | GeoPackage Extensions — TGCE 32-bit TIFF, basemaps, WAL, VACUUM prohibition | ✅ |
| 43 | Boundary Import Formats — Shapefile, .gdb rejected, DXF, GeoJSON, CSV-WKT | ✅ |
| 44 | Testing Strategy — proptest, HIL/SIL, golden-file, CI matrix, Eagle Mapping | ✅ |
| 45 | Aerial Sensor Library — 10 cameras + 9 LiDAR, irontrack_sensors schema | ✅ |
| 46 | Post-Flight QC — XTE, 6DOF footprint, heat map, automated re-fly | ✅ |
| 47 | Daemon Field Hardware — Pi 4/5, NUC, tablets, Veronte, DO-160 power | ✅ |
| 48 | Avionics Hardware Integration — power buses, TVS, LC filter, grounding, connectors | ✅ |
| 49 | Computational Geometry — winding number, Weiler-Atherton, straight skeleton, spade | ✅ |
| 50 | Dubins Mathematical Extension — 6 paths, clothoid, 3D airplane, wind r_safe | ✅ |
| 51 | GPLv3 Compliance — dependency audit, firmware boundary, SDK isolation, CLA/DCO | ✅ |
| 52 | Rust Performance — AVX2 SIMD, rayon tuning, memmap2, PGO, Criterion | ✅ |
| 99 | Engine Technical Reference — modules, concurrency, Two-Phase I/O, safety | — |

## Supporting Files
| File | Contents |
|------|----------|
| `IronTrack_Manifest_v3.5.docx` | Authoritative spec (21 sections, 520 paragraphs, 20 locked decisions) |
| `formula_reference.md` | Equations from docs 02–07, 30, 32, 34, 38, 39, 46, 49, 50 |
| `formula_reference_08_10.md` | Equations from docs 08–10 |
