# Flight Planning

IronTrack's flight planning layer generates photogrammetric area surveys, corridor surveys, and LiDAR missions. It handles sensor geometry, flight line layout, procedure turns, and route optimization.

## Overview

A flight plan starts with a boundary (bounding box or KML polygon) and a sensor definition, then moves through four stages:

1. **Sensor geometry** — Compute GSD, swath width/height, and required AGL from the sensor optics and target resolution.
2. **Line generation** — Lay out parallel flight lines across the survey area (grid) or along a centerline (corridor).
3. **Procedure turns** — Connect consecutive flight lines with Dubins path turns and clothoid transitions.
4. **Route optimization** — Reorder and optionally reverse flight lines to minimize total repositioning distance.

Terrain adjustment and trajectory smoothing (see [Terrain & Trajectory](terrain-and-trajectory)) can be applied on top of any plan.

## Sensor Geometry

The `SensorParams` struct holds the five intrinsic parameters of a frame camera: focal length, physical sensor dimensions (width × height in mm), and image dimensions (width × height in pixels).

### Built-in Presets

| Preset | Focal Length | Sensor Size | Image Size |
|--------|-------------|-------------|------------|
| Phantom 4 Pro | 8.8 mm | 13.2 × 8.8 mm | 5472 × 3648 px |
| Mavic 3 | 12.29 mm | 17.3 × 13.0 mm | 5280 × 3956 px |
| Phase One iXM-100 | 50.0 mm | 53.4 × 40.0 mm | 11664 × 8750 px |

Custom sensors can be specified via CLI flags (`--focal-mm`, `--sensor-width-mm`, etc.).

### Key Formulas

**Ground Sample Distance (per axis):**

```
gsd_x = AGL × sensor_width / (focal_length × image_width_px)
gsd_y = AGL × sensor_height / (focal_length × image_height_px)
```

The binding GSD is `max(gsd_x, gsd_y)` — the worst-case axis governs ASPRS compliance.

**Required AGL for a target GSD:**

```
AGL = target_gsd / max(sensor_width/(f × W_px), sensor_height/(f × H_px))
```

**Swath dimensions:**

```
swath_width  = AGL × sensor_width / focal_length    (cross-track)
swath_height = AGL × sensor_height / focal_length   (along-track)
```

**Field of View:**

```
FOV_horizontal = 2 × atan(sensor_width / (2 × focal_length))
FOV_vertical   = 2 × atan(sensor_height / (2 × focal_length))
```

## Area Survey (Grid)

Grid-based flight line generation works in geodesic space (not UTM) to avoid zone-boundary issues over large survey areas.

**Algorithm:**

1. Decompose the bounding box into cross-track and along-track components relative to the flight azimuth using Karney geodesic inverse.
2. Compute the number of parallel lines: `num_lines = ceil(cross_track_span / line_spacing)` where `line_spacing = swath_width × (1 − side_lap/100)`.
3. For each line, compute the origin via a cross-track geodesic offset from the box centre, then step waypoints along the flight azimuth using Karney geodesic direct.
4. Lines are generated in parallel via `rayon`. Each line is independent.

Waypoint spacing along each line is derived from the along-track photo interval: `interval = swath_height × (1 − end_lap/100)`.

All waypoints are stored in radians internally. Conversion to degrees occurs only at export.

## Corridor Survey

Corridor mode generates parallel offset passes along a polyline centerline (typically imported from KML). This is used for linear infrastructure: roads, pipelines, power lines, rivers.

**Algorithm:**

1. Project the WGS84 centerline into UTM (single zone, determined from the centroid).
2. Deduplicate consecutive vertices (< 1 µm tolerance).
3. Compute offset distances for each pass:
   - 1 pass: centerline only.
   - 3 passes: −w/2, 0, +w/2.
   - 5 passes: −w/2, −w/4, 0, +w/4, +w/2.
4. Generate offset polylines using miter joins (extend offset edges to intersection). When the miter spike exceeds 2× the buffer width, fall back to a bevel join (two points instead of one).
5. Remove self-intersections via iterative segment intersection scanning.
6. Resample each offset polyline at the requested waypoint interval.
7. Convert back to WGS84 and pack into FlightLines.

## LiDAR Missions

LiDAR planning replaces GSD-based geometry with point density kinematics. The primary quality metric is points per square metre rather than ground sample distance.

**Key formulas:**

```
point_density = PRR × (1 − loss) / (ground_speed × swath_width)
required_speed = PRR × (1 − loss) / (target_density × swath_width)
across_track_spacing = swath_width / (PRR / scan_rate)
along_track_spacing = ground_speed / (2 × scan_rate)
```

The along-track denominator uses `2 × scan_rate` because oscillating mirrors scan bidirectionally. The loss fraction accounts for mirror deceleration and blind spots at scan edges.

Line spacing is driven by swath overlap percentage (typically 50%), similar to photogrammetric side-lap but computed from the LiDAR swath width.

## Dubins Path Procedure Turns

Procedure turns connect consecutive flight lines with smooth, flyable curves. IronTrack evaluates all six canonical Dubins path types and selects the shortest:

**Curve-Straight-Curve (CSC):** LSL, RSR, LSR, RSL

**Curve-Curve-Curve (CCC):** LRL, RLR

The minimum turn radius is derived from physics, not user-configurable:

```
R_min = V² / (g × tan(bank_angle))
```

where V is true airspeed and bank_angle is the maximum allowed bank.

### Clothoid Transitions

Raw Dubins paths have instantaneous curvature changes at the tangent points, which would require infinite roll rate. IronTrack inserts clothoid (Euler spiral) transitions that ramp curvature linearly:

- **Entry clothoid:** Curvature ramps from 0 to 1/R over a distance proportional to the bank angle and roll rate limit.
- **Exit clothoid:** Curvature ramps from 1/R back to 0.

```
L_clothoid = V × bank_angle / max_roll_rate
```

Clothoid length is capped at 40% of the arc length to prevent endpoint drift. Integration uses a midpoint-incremental method with a minimum of 4 steps per clothoid.

The roll rate limit prevents autopilot overshoot and gimbal destabilisation on camera platforms.

## Route Optimization (TSP)

Flight line ordering is an Asymmetric Travelling Salesman Problem. IronTrack solves it with a two-phase heuristic:

**Phase 1 — Nearest Neighbour:**

Starting from the launch point, greedily visit the closest unvisited flight line. Each line can be flown in either direction; the algorithm picks the direction with the shortest entry distance.

**Phase 2 — 2-opt Improvement:**

Iteratively reverse segments of the tour when doing so reduces total repositioning distance. A swap is accepted if the gain exceeds 1×10⁻³ metres. The loop terminates when a full pass produces no improvement, or after 1,000 iterations.

All distances use the Karney geodesic (not Haversine), and total repositioning distance is accumulated with Kahan summation.

The result is a `RouteResult` containing the optimised line order, direction flags (forward/reversed), and total repositioning distance in metres.

---

## Deep Dive: Implementation Notes

### Terrain-Aware AGL Adjustment

When the `--terrain` flag is active, the flight line generator queries the Copernicus DEM at each waypoint and raises altitude where terrain would violate the side-lap or GSD target. This is a per-waypoint adjustment on top of the nominal AGL — it only raises, never lowers.

### Kahan-Summed Geodesic Tracing

Flight line waypoints are placed by repeatedly calling `geodesic_direct()` with the same azimuth and step distance. Over a 100 km line, naive floating-point summation would accumulate centimetre-scale drift. Kahan compensated summation eliminates this.

### ASPRS 2024 Compliance

IronTrack follows ASPRS Positional Accuracy Standards (2024 edition): RMSE-based accuracy classes with the 95% confidence multiplier eliminated. GSD heuristics follow the AT ≈ 0.5× pixel pitch rule. Minimum overlap is 60/30 (end/side), with 80/60 recommended.

### Rolling Shutter Warning

For survey-grade PPK missions, a non-dismissable warning is displayed when the selected sensor uses a rolling shutter. Rolling shutter violates the collinearity equation assumption that all pixels in a frame share the same exterior orientation.

### Forward Motion Compensation

```
smear_mm = V_ground × t_exposure × focal_length / AGL
```

If the computed smear exceeds the pixel pitch, the sensor will produce motion-blurred imagery. IronTrack warns the operator but does not enforce a speed limit (PIC retains authority).

### Future: Energy-Weighted TSP (v0.5)

The current TSP uses geodesic distance as the cost metric. In v0.5, repositioning costs will incorporate wind triangle calculations, climb/descent energy, and battery state. The graph structure and 2-opt logic are designed to be reusable with energy-based weights.

### Reference Documents

- `03_photogrammetry_spec.md` — GSD/FOV derivations, ASPRS standards.
- `07_corridor_mapping.md` — Polyline offsets, bank angles.
- `13_lidar_survey_planning.md` — Point density models, swath conflicts.
- `39_flight_path_optimization.md` — ATSP, NN+Or-opt, boustrophedon.
- `50_dubins_mathematical_extension.md` — Six Dubins types, clothoid, 3D airplane.
- `36_asprs_2024_positional_accuracy.md` — Accuracy class mapping.
