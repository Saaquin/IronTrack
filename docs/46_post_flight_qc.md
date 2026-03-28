# Post-Flight QC: Coverage Analysis, Gap Detection, and Re-Fly Planning

> **Source document:** 46_Post-Flight-QC.docx
>
> **Scope:** Complete extraction — all content is unique. Covers cross-track error computation, trigger event analysis with RTK degradation, 6DOF image footprint reconstruction via collinearity equations, coverage heat map visualization on MapLibre, and automated re-fly generation with cellular decomposition + TSP routing + B-spline smoothing.

---

## 1. Actual vs Planned Track: Cross-Track Error (XTE)

### Telemetry Ingestion
Sources: NMEA log from daemon, RINEX (PPK observables), GPX/CSV from avionics. All parsed to f64 decimal degrees, datum-tagged and transformed to WGS84 Ellipsoidal via 14-parameter Helmert. Kahan summation mandatory for all cumulative distance aggregations.

### XTE Computation Methods

| Method | Earth Model | Accuracy | Use Case |
|---|---|---|---|
| Euclidean Cartesian | Flat | Meter-level drift | Sub-km multirotor only |
| Haversine | Spherical | ~0.3% error | Non-survey-grade |
| Vincenty | Ellipsoidal (WGS84) | High | Standard surveying (fails at antipodal) |
| **Karney-Krüger UTM** | Projected Cartesian | **Sub-mm** | **Survey-grade RTK/PPK fixed-wing** |

**Mandate:** Transform to local UTM (Karney-Krüger 6th-order) then compute XTE via perpendicular vector distance:
```
XTE = |((P - A) × (B - A))| / |B - A|
```
The sign of the cross product indicates drift direction (left/right) — critical because rightward drift increases left-swath sidelap while decreasing right-swath sidelap.

### Thresholding
Low-pass filter rejects transient GNSS multipath spikes. Smoothed XTE evaluated against dynamic threshold derived from mission parameters (sensor footprint width, required sidelap, ASPRS accuracy class). If XTE exceeds max deviation for a continuous distance > footprint length → line segment flagged as geometrically compromised → queued for re-fly.

---

## 2. Trigger Event Analysis: Forward Overlap

### Event Log Ingestion
Parse trigger event CSV (event_number, UTC timestamp, lat, lon, alt, fix_quality). Cross-reference timestamps with PPK trajectory for exact 3D exposure coordinates. RTK Float events (fix quality 5) flagged as spatially uncertain with widened error margins.

### Terrain-Adjusted GSD
```
GSD = (AGL × sensor_width_mm) / (focal_length_mm × resolution_x)
```
AGL is dynamic — engine queries cached 3DEP/Copernicus DEM in GeoPackage, applies bicubic (EGM2008) or biquadratic (GEOID18) interpolation for terrain elevation at each exposure coordinate.

### Footprint Dimensions
```
footprint_length = GSD × resolution_y    (along-track)
footprint_width  = GSD × resolution_x    (cross-track)
```

### Forward Overlap Calculation
```
overlap_fwd = 1 - (trigger_spacing / footprint_length)
```
Where trigger_spacing = Karney geodesic distance between consecutive exposures. If overlap_fwd < minimum threshold (60% standard, 80% for 3D/forestry) → discrete gap marker stored in GeoPackage.

**Spatial intervalometry vs temporal:** Time-based triggers (fire every N seconds) fail in variable wind — tailwind increases ground speed → increased spacing → dropped overlap. IronTrack's closest-approach derivative zero-crossing algorithm prevents this.

---

## 3. 6DOF Image Footprint Reconstruction (Doc 21)

### Why Simple Trigger Spacing Is Insufficient
Basic overlap from trigger spacing assumes zero roll/pitch/yaw and flat terrain. In reality, aircraft attitude continuously distorts the footprint into irregular quadrilaterals. Only 6DOF reconstruction proves the survey meets spec.

### Camera Pose
- **Position (X, Y, Z):** From PPK trajectory.
- **Attitude (ω, φ, κ):** From IMU at 200-400 Hz. Represented as quaternions (avoid gimbal lock), converted to 3×3 Direction Cosine Matrix (R).
- **Boresight correction:** R_total = R_boresight × R_imu-to-nav.

### Collinearity Equation Inversion (Ray Casting)
For each of the 4 sensor corners, construct a 3D ray from the optical center through the corrected image coordinate. Cast the ray downward and intersect with the DEM (biquadratic/bicubic interpolation) to find the 3D ground coordinate. Result: 4 ground coordinates forming an irregular convex quadrilateral draped over topography.

### Polygon Intersection for True Overlap

| Algorithm | Constraint | Complexity | Suitability |
|---|---|---|---|
| **Sutherland-Hodgman** | Clip polygon must be convex | O(n × m) | Excellent — standard for Doc 21 footprint clipping |
| Weiler-Atherton | Supports concave + holes | O(n²) or worse | Over-engineered for convex footprints |
| **O'Rourke (Convex)** | Both polygons must be convex | **O(n + m)** | Optimal for batch processing |
| Monte Carlo | Probabilistic | High (iterative) | Unacceptable for regulatory validation |

Intersection area computed via Shoelace formula. True overlap = intersection_area / preceding_footprint_area.

**Side-lap validation:** Intersect footprints from adjacent parallel lines. Crab angle (crosswind yaw) creates "sawtooth" overlap patterns — side-lap can drop dangerously even with perfect line spacing on the 2D map. Gaps vectorized and stored in GeoPackage R-tree spatial index.

---

## 4. Heat Map Visualization

### Architecture: Explicit Intersection Meshing (Not Additive Blending)
The Rust backend computes the topological union of all footprints, shattering the survey boundary into mutually exclusive, non-overlapping sub-polygons. Each sub-polygon carries an exact overlap depth (1-ray, 2-ray, 4-ray coverage) or absolute percentage.

Pre-computed disjoint polygons streamed to React UI via **raw binary WebSocket channel** (bypasses JSON serialization overhead for bulk coordinate data). Injected into MapLibre using `map.getSource().setData()` within `requestAnimationFrame`.

### Color Schema (Aviation Safety Standard)

| Color | Meaning | Threshold Example | Action |
|---|---|---|---|
| **Red** | Gap / failure | < 40% overlap | Immediate re-fly required |
| **Yellow/Amber** | Marginal | 40–60% overlap | May produce 2D ortho but degrades 3D DSM accuracy |
| **Green** | Optimal | > 60% overlap | Meets specification |

### Offline Rendering
Tauri `irontrack://` custom URI protocol serves embedded GeoPackage basemap tiles to MapLibre WebGL canvas with zero network latency. Full heat map analysis against actual terrain context available without internet.

---

## 5. Automated Re-Fly Planning

### Coverage Path Planning for Irregular Gaps
Red-zone gap polygons are typically non-convex (L-shapes from crosswind gusts, jagged holes from RTK dropouts). Cannot be covered efficiently by simple boustrophedon without decomposition.

**Trapezoidal Cellular Decomposition:** Slice non-convex gap polygon along vertical sweep lines from every vertex → series of strictly convex sub-polygons (trapezoids/triangles). Convex polygons are trivial to cover with boustrophedon — a straight line intersects a convex boundary exactly twice.

### Re-Fly Line Generation
1. **Generate lines** perpendicular to decomposition sweep direction within each convex cell.
2. **Spacing** from sensor profile in GeoPackage (focal length, AGL, required overlap).
3. **Route optimization** across multiple gaps: TSP (nearest-neighbor + Or-opt) minimizes non-recording transit between isolated gap areas.
4. **Dubins/clothoid transitions** for flyable turns respecting max bank angle and min turn radius.
5. **Terrain-following:** Elastic Band energy minimization → adaptive-knot Cubic B-spline (C² continuity, convex hull terrain collision guarantee).

### GeoPackage Integration
Re-fly lines appended as a new flight plan linked to the parent mission — NOT a separate file. Written via WAL mode (non-blocking, concurrent reads continue). Instantly propagated through WebSocket watch channel → appear on pilot's glass cockpit as new selectable navigational entities.

**No landing required.** No USB thumb drives. No MDB conversions. The aircraft extends the survey mission seamlessly from the cockpit.
