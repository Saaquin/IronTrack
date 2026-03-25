# Photogrammetry Implementation Guide

## Sensor Parameters
All calculations use physical sensor dimensions, never "35mm equivalent."

```rust
pub struct SensorParams {
    pub focal_length_mm: f64,
    pub sensor_width_mm: f64,
    pub sensor_height_mm: f64,
    pub image_width_px: u32,
    pub image_height_px: u32,
}
```

Reference sensor for testing — Phase One iXM-100:
- Sensor: 53.4 × 40.0 mm, 11664 × 8750 px, 50mm lens
- Pixel pitch: 53.4 / 11664 ≈ 4.58 μm

## Core Equations

### Pixel Pitch
```
pixel_pitch_mm = sensor_width_mm / image_width_px
```

### Field of View
```
FOV_h = 2 * atan(sensor_width_mm / (2 * focal_length_mm))     // horizontal
FOV_v = 2 * atan(sensor_height_mm / (2 * focal_length_mm))    // vertical
```

### Ground Sample Distance (GSD)
```
GSD_x = (AGL * sensor_width_mm) / (focal_length_mm * image_width_px)   // m/px
GSD_y = (AGL * sensor_height_mm) / (focal_length_mm * image_height_px)  // m/px
```
Always compute both. Use **worst-case (larger value)** as the binding constraint.

### Swath Width
```
swath_w = 2 * AGL * tan(FOV_h / 2)    // perpendicular to flight direction
swath_h = 2 * AGL * tan(FOV_v / 2)    // parallel to flight direction
```

### Required AGL for Target GSD
Rearrange worst-case GSD equation. If GSD_x is the binding dimension:
```
AGL = (target_gsd * focal_length_mm * image_width_px) / sensor_width_mm
```

### Flight Line Spacing
```
spacing = swath_w * (1.0 - side_lap / 100.0)
```
Side-lap default: ≥30% for stereo reconstruction. Some projects demand 60%.

### Photo Interval (along-track)
```
baseline = swath_h * (1.0 - end_lap / 100.0)
```
End-lap default: ≥60% for Structure-from-Motion tie points.

## Dynamic AGL Over Variable Terrain
Flat-terrain assumptions fail in mountains. The engine must:

1. Discretize each flight line into sampling nodes at the photo interval.
2. At each node, use the **`TerrainEngine` bulk API** to query elevations for the swath.
3. Use `TerrainReport` to track orthometric, geoid, and ellipsoidal heights.

### TerrainReport Struct
To avoid "logic bleed" between modules and `main.rs`, use a structured report:
```rust
pub struct TerrainReport {
    pub ortho_m: f64,      // Orthometric elevation (MSL)
    pub geoid_n_m: f64,    // Geoid undulation (N)
    pub ellipsoidal_h: f64, // Ellipsoidal height (h = H + N)
}
```

### Dynamic Validation (Rayon)
Use `rayon::par_iter()` to validate entire `FlightLine` objects against the bulk `TerrainEngine`.

### Terrain Adjustment Strategy (v0.1)
Raise flight altitude locally to maintain minimum AGL:
```
required_alt = max_elev + required_agl
adjusted_alt = max(planned_alt, required_alt)
```
This produces a "terrain-following" altitude profile. More sophisticated approaches
(B-spline smoothing, elastic band minimization) are documented in docs/05 §5 for
post-MVP implementation.

## Flight Line Generation Algorithm
1. Project bounding box corners to UTM (flat Euclidean math)
2. Rotate the box by the flight azimuth
3. Compute number of lines: ceil(rotated_width / spacing) + 1
4. For each line:
   a. Compute start/end points in rotated UTM space
   b. Step along line at photo interval to generate waypoints
   c. Transform all waypoints back to WGS84
5. Use `geographiclib-rs` Geodesic::direct() for stepping along azimuths
6. Use `rayon::par_iter()` to generate lines in parallel

### Odd-Number Logic (Corridor Mapping)
For linear infrastructure corridors, always generate an odd number of lines
(3, 5, 7). This ensures the UAV finishes on the opposite end of the segment,
minimizing transit time to the next segment. See docs/07.

## Overlap Validation Report
The validator returns a struct per exposure point:
```rust
pub struct OverlapCheck {
    pub line_index: usize,
    pub point_index: usize,
    pub lat: f64,
    pub lon: f64,
    pub terrain_peak_m: f64,
    pub actual_agl_m: f64,
    pub actual_swath_m: f64,
    pub actual_overlap_pct: f64,
    pub violation: bool,  // true if below threshold
}
```
