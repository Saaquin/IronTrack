# IronTrack Release Roadmap & v0.2 Work Tracker

## Release Roadmap

| Release | Theme | Key Deliverables |
|---------|-------|-----------------|
| v0.1 ✅ | Foundation | DEM ingestion, flight line generation, GeoPackage/GeoJSON export, Karney geodesics, EGM2008, CLI |
| v0.2 🔨 | Correctness & Safety | Datum-tagged altitudes ✅, EGM96 integration ✅, DSM safety warnings, Copernicus attribution, GeoPackage schema versioning, QGC .plan export, DJI .kmz export (EGM96), Snapshot MDB export, LiDAR planning, KML/KMZ import |
| v0.3 | Kinematics & Routing | Corridor mapping, Dubins path turns, TSP route optimization, bank angle physics |
| v0.4 | Networked FMS Daemon | Axum REST + WebSocket, NMEA GPS parsing, mission state, mock telemetry |
| v0.5 | Atmospheric & Energy | Wind triangle, crab angle compensation, drag model, endurance estimation |
| v0.6 | Glass Cockpit MVP | Tauri app, block overview, line navigation guidance, vector graphics |
| v0.7 | Web Planning UI | React interface, boundary drawing, sensor library, plan generation via API |
| v0.8 | Trigger Controller | Embedded firmware, UART NMEA, dead reckoning, GPIO pulses, serial protocol |
| v0.9 | 3DEP & US Domestic | 1-m 3DEP DEM, NAVD88/GEOID18 datum pipeline |
| v1.0 | Production Release | Eagle Mapping validation, stability, docs, community release |

## v0.2 Remaining Work

- [ ] DSM penetration safety warning — mandatory, non-suppressible: stderr + GeoPackage metadata + GeoJSON
- [ ] Copernicus attribution strings in every GeoPackage and GeoJSON output
- [ ] `irontrack_metadata` table in GeoPackage (`schema_version`, `engine_version`)
- [ ] QGroundControl `.plan` JSON export (`src/io/qgc.rs`) — read `.agents/autopilot.md`
- [ ] DJI `.kmz` export with mandatory EGM96 datum conversion (`src/io/dji.rs`)
- [ ] Snapshot-compatible MDB export (`src/io/mdb.rs`) — Eagle Mapping pipeline bridge
- [ ] LiDAR flight planning (`src/photogrammetry/lidar.rs`) — read `.agents/lidar.md`
- [ ] KML/KMZ boundary import (`src/io/kml.rs`)
- [ ] `--datum` CLI flag plumbed into `run_plan` → `FlightLine::to_datum()` (flag exists in args struct, not yet wired)
