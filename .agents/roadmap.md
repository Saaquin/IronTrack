# IronTrack Release Roadmap & Work Tracker

## Release Roadmap (Manifest v3.5)

| Release | Theme | Key Deliverables | Source Docs |
|---------|-------|-----------------|-------------|
| v0.1 ✅ | Foundation | DEM ingestion (SRTM, Copernicus), flight line gen, GeoPackage/GeoJSON export, Karney geodesics, EGM2008, bilinear terrain interpolation, CLI | 02-04, 08 |
| v0.2 ✅ | Correctness & Safety | Datum-tagged altitudes, EGM96, DSM warnings, Copernicus attribution, QGC/DJI/MDB exports, LiDAR planning, KML/KMZ import, schema versioning, corridors, Dubins, TSP | 05, 07, 09, 11-13 |
| v0.3 🔨 | Terrain-Following | EB + B-spline C² (convex hull safety), corridor mapping, Dubins+clothoid, NN+Or-opt TSP, wind-corrected turn radius, bank angle physics, synthetic validation | 34, 39, 50 |
| v0.4 | Networked Daemon | Axum REST/WS, NMEA parsing, RTK degradation, mDNS, WAL persistence, VID/PID auto-detect, hot-plug, mock telemetry, ephemeral token auth | 28, 29 |
| v0.5 | Atmospheric & Energy | Wind triangle, crab angle, multirotor/fixed-wing drag, Peukert battery, per-segment energy, 20% reserve enforcement, endurance estimation | 16, 38 |
| v0.6 | Glass Cockpit MVP | Tauri 2.0, Track Vector CDI, Canvas 2D, B612, ARINC 661, 20mm targets, WebSocket telemetry, auto-detecting daemon | 25, 26 |
| v0.7 | Web Planning UI | React+MapLibre, client-side parsing (KML/KMZ/SHP/GeoJSON/CSV-WKT/DXF), TanStack Table sensor library, irontrack:// tiles, FAA airspace+TFR 4D+DOF, advisory-only | 35, 42, 43, 45 |
| v0.8 | Trigger Controller | Embedded Rust (STM32/RP2040), NMEA FSM, UBX, 1000 Hz dead reckoning, COBS+CRC-32+RTS/CTS, opto-isolated GPIO, PPS µs timestamping, MAVLink hybrid, dual-trigger LiDAR+camera, HIL testing | 22, 29, 40, 41 |
| v0.9 | 3DEP & US Domestic | 3DEP AWS S3 COG range requests, resolution fallback, microtile caching (OGC TIFF + f64 SoA), GEOID18 biquadratic + Helmert per-pixel, MBlend feathering, epoch-aware structs, GeopotentialModel trait | 30, 33, 42 |
| v1.0 | Production Release | Full E2E testing, Eagle Mapping field validation (Trackair parallel + live flight + ASPRS 2024), post-flight QC (XTE + 6DOF + heat map + auto re-fly), PPK open/closed-loop, dual-antenna, NSRS 2022 prep, docs, community release | 36, 44, 46 |

## v0.3 Active Work — Terrain-Following Trajectories

- [ ] **Phase 5A:** Elastic Band energy minimization solver [Doc 34]
- [ ] **Phase 5B:** Cubic B-spline fitting with convex hull safety [Doc 34]
- [ ] **Phase 5C:** Dynamic look-ahead pure pursuit [Doc 34]
- [ ] **Phase 5D:** Synthetic validation suite (flat/sinusoidal/step/sawtooth) [Doc 34]

## 20 Locked Architectural Decisions (Manifest v3.5 §19)

| Decision | Source |
|----------|--------|
| Advisory-only airspace | Doc 35 |
| CRDT rejection | Doc 28 |
| Navigational vs analytical epoch boundary | Docs 30, 31 |
| Big-Endian GEOID18 only | Doc 32 |
| Preprocessing mandate (no on-the-fly Helmert) | Doc 30 |
| CH340 rejected (CP2102/FTDI only) | Doc 29 |
| Bilinear disqualified for geoid (bicubic EGM2008, biquadratic GEOID18) | Doc 32 |
| PhantomData compile-time datum safety | Doc 37 |
| SQLITE_BUSY = fatal violation | Doc 42 |
| VACUUM prohibited in flight | Doc 42 |
| 20% battery reserve = blocking enforcement | Doc 38 |
| NN + Or-opt as TSP pipeline | Doc 39 |
| .gdb native ingestion rejected | Doc 43 |
| v1.0 CRS: WGS84 + UTM only | Doc 43 |
| Sensor library inside GeoPackage | Doc 45 |
| fast_math REJECTED | Doc 52 |
| Greiner-Hormann clipping disqualified | Doc 49 |
| spade over delaunator | Doc 49 |
| EXIF timestamps disqualified | Doc 40 |
| Firmware is a separate work (MIT, not GPLv3) | Doc 51 |
