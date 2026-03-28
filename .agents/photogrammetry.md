# Photogrammetry Implementation Guide

**Source modules:** `src/photogrammetry/` — `sensor.rs`, `flightlines.rs`, `corridor.rs`, `lidar.rs`
**Source docs:** 03, 21, 36, 40, 45, 46

## Core Equations
GSD = (AGL × sensor_width_mm) / (focal_length_mm × image_width_px)
FOV = 2 × atan(sensor_dim / (2 × focal_length))
swath = 2 × AGL × tan(FOV/2)
spacing = swath × (1.0 - side_lap/100.0)
baseline = footprint_h × (1.0 - end_lap/100.0)

## Sensor Library [Doc 45]
10 cameras + 9 LiDAR in irontrack_sensors table (inside GeoPackage, not separate DB).
Key cameras: Phase One iXM-100 (3.76µm, 3fps, LEMO), Hasselblad A6D-100c (4.6µm, 1fps,
8-camera sync), Sony ILX-LR1 (243g, Molex), DJI P1 (global shutter, PSDK),
Vexcel Eagle M3 (450MP composite), Leica DMC-4X (54,400×8,320).

## ASPRS 2024 Standards [Doc 36]
RMSE only (95% confidence multiplier eliminated). Continuous X-cm horizontal classes.
NVA/VVA vertical accuracy. GSD heuristics: AT ≈ 0.5×pixel, raw GSD ≤ 95% deliverable.
BBA overlap: 60/30 min, 80/60 recommended, >85/85 destroys B/H ratio.
GCP: 30 minimum per CLT, scaling to 120 cap. USGS QL0-QL3 mapped to IronTrack solver.

## Rolling Shutter Warning [Docs 24, 40]
Rolling shutters violate the collinearity equation (each row = different spatial position).
Non-dismissable warning when mission_type = survey_grade_ppk. Mechanical global shutters
mandatory for ASPRS survey-grade work.

## FMC / Image Smear [Doc 40]
smear_mm = V_ground × t_exposure × focal_length / AGL. If smear > pixel_pitch → motion
blur → degrades BBA tie-point matching. Modern mitigation: ultra-fast leaf shutters
(1/2500-1/4000s) + gyrostabilized mounts. FMS calculates max allowable ground speed.

## Post-Flight QC [Doc 46]
- XTE: Karney-Krüger UTM perpendicular vector distance with directional sign (left/right).
- Trigger analysis: terrain-adjusted GSD, forward overlap = 1 - spacing/footprint_length.
- 6DOF footprint reconstruction: quaternion→DCM→boresight→collinearity inversion→DEM ray cast.
- Polygon intersection: Sutherland-Hodgman O(n×m) or O'Rourke O(n+m) for convex footprints.
- Coverage heat map: explicit intersection meshing, binary WebSocket, red/yellow/green.
- Automated re-fly: trapezoidal cellular decomposition → boustrophedon → TSP → Dubins/clothoid
  → EB+B-spline terrain-following → GeoPackage WAL append → instant WebSocket to cockpit.

## Computational Geometry [Doc 49]
- PIP: Sunday winding number (trig-free). R-tree AABB + SoA SIMD for bulk.
- Clipping: Weiler-Atherton/Vatti for concave. Greiner-Hormann DISQUALIFIED (epsilon perturbation).
- Buffer: straight skeleton (geo-buffer crate) for complex topology.
- MBR: rotating calipers O(n) after O(n log n) hull, pure vector algebra (no trig).
- Area: Karney ellipsoidal (Shoelace on UTM ~3.5% error for large regions).
- Triangulation: spade CDT (exact predicates, Voronoi). delaunator rejected.
