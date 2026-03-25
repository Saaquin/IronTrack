# LiDAR Flight Planning Implementation Guide

**Source docs:** `docs/13_lidar_planning.md`

## How LiDAR Planning Differs from Camera Planning
Camera planning optimizes for GSD (ground sample distance — meters per pixel).
LiDAR planning optimizes for point density (points per square meter). The math
is completely different. Both share: swath width from FOV, terrain-following AGL,
and the need for lateral overlap between adjacent lines.

## LiDAR Sensor Parameters

```rust
pub enum ScannerType {
    OscillatingMirror, // Bidirectional zigzag pattern
    RotatingPolygon,   // Unidirectional parallel lines
    Palmer,            // Conical/elliptical pattern (bathymetry)
    Livox,             // Non-repetitive rosette pattern
}

pub struct LidarSensorParams {
    pub pulse_repetition_rate: f64,  // Hz (e.g., 240_000.0 for 240 kHz)
    pub scan_rate: f64,              // Hz (mirror sweeps per second)
    pub fov_degrees: f64,            // total field of view
    pub scanner_type: ScannerType,
    pub loss_fraction: f64,          // fraction lost to mirror decel/blind spots (0.0–0.15)
    pub max_returns: u8,             // discrete returns per pulse (1–15)
}
```

## Core Equations

### Swath Width (same as camera)
```
W = 2 × H_agl × tan(FOV / 2)
```

### Global Point Density
The fundamental LiDAR flight planning equation:
```
ρ = PRR × (1 - LSP) / (V × W)

where:
  ρ   = average point density (pts/m²)
  PRR = pulse repetition rate (Hz)
  LSP = loss of scanning points fraction (0.0–0.15)
  V   = aircraft ground speed (m/s)
  W   = swath width (m)
```

### Worked Example
240 kHz PRR, 100m AGL, 70° FOV, 5 m/s ground speed, negligible loss:
```
W = 2 × 100 × tan(35°) ≈ 140 m
ρ = 240,000 / (5 × 140) ≈ 342 pts/m²
```

### Along-Track Line Spacing
```
d_along = V / (2 × scan_rate)    [bidirectional oscillating scanner]
d_along = V / scan_rate           [unidirectional rotating polygon]
```

### Across-Track Point Spacing
```
d_across = W / (PRR / scan_rate)
```

### Square Sampling Grid
For optimal data quality, d_across ≈ d_along. Solve for speed:
```
V = d_across × 2 × scan_rate     [bidirectional]
```

### Required Speed for Target Density
```
V_max = PRR × (1 - LSP) / (ρ_target × W)
```

### Flight Line Spacing (with overlap)
```
spacing = W × (1 - OL_lateral / 100)
```
Standard: 20–30% for flat terrain, 50% for steep terrain or USGS compliance.

## Pulse Energy vs PRR Tradeoff
**CRITICAL for vegetated terrain.** Total laser power is constant:
```
E_pulse = P_avg / PRR
```
Doubling PRR halves per-pulse energy. Under dense canopy, weak pulses only
trigger first-return (canopy top) — ground returns are lost entirely.

For dense forest:
- LOWER the PRR to maximize E_pulse
- Accept lower raw density
- Compensate with slower flight speed or more overlap
- This produces fewer but STRONGER pulses that penetrate to ground

### Laser Penetration Index
```
LPI = ground_returns / total_returns
```
Flight planner should warn if calculated E_pulse drops below threshold for
the target vegetation type.

## Canopy Penetration Probability (Beer-Lambert)
```
P_ground = e^(-LAI × k)

where:
  P_ground = probability of pulse reaching ground
  LAI = leaf area index of the canopy
  k = extinction coefficient (vegetation-dependent)
```

Penetration probability increases near nadir (short path through canopy):
```
path_length ∝ sec(scan_angle)
```
→ Restrict FOV to ≤40° over heavy vegetation to maximize ground returns.

## Slope Effects on Point Density
On a slope of angle θ perpendicular to the flight direction:
```
d_slope = d_flat / cos(θ_terrain ± scan_angle)
```
- Upslope side: points compress (higher density)
- Downslope side: points stretch (lower density, possible voids)
- If θ_terrain ≈ scan_angle on the downslope side → density approaches zero

**Mitigation:** Cross-hatching — fly orthogonal flight lines to fill downslope voids.

## USGS Quality Levels

| QL | Min Density | NVA (bare earth RMSE_z) | VVA (veg RMSE_z) | Max AGL |
|----|-------------|-------------------------|-------------------|---------|
| QL0 | ≥ 8 pts/m² | ≤ 9.25 cm | ≤ 18.5 cm | altitude-limited |
| QL1 | ≥ 8 pts/m² | ≤ 10.0 cm | ≤ 20.0 cm | ~2,000m (typical) |
| QL2 | ≥ 2 pts/m² | ≤ 10.0 cm | ≤ 20.0 cm | ~2,500m (typical) |

```rust
pub enum UsgsQualityLevel { QL0, QL1, QL2 }

pub fn validate_usgs_ql(density: f64, agl: f64) -> Option<UsgsQualityLevel> {
    if density >= 8.0 && agl <= 1500.0 { Some(UsgsQualityLevel::QL0) }
    else if density >= 8.0 { Some(UsgsQualityLevel::QL1) }
    else if density >= 2.0 { Some(UsgsQualityLevel::QL2) }
    else { None }
}
```

## Multiple-Time-Around (MTA) Range Ambiguity
At extreme PRR, light travel time exceeds pulse interval:
```
max_unambiguous_range = c / (2 × PRR)

At 1 MHz: max_range = 3×10⁸ / (2×10⁶) = 150 m
At 500 kHz: max_range = 300 m
```
If AGL > max_unambiguous_range, returns from pulse N arrive after pulse N+1
is emitted → range ambiguity. Modern scanners resolve this with pulse tagging,
but the planner should warn if AGL approaches this limit.

## Combined Camera + LiDAR Missions
When both sensors are on the same platform, their requirements conflict:

| Parameter | Camera wants | LiDAR wants | Conflict |
|-----------|-------------|-------------|----------|
| Altitude | LOW (better GSD) | HIGH (wider swath, fewer lines) | Direct |
| Speed | MODERATE (blur limit) | LOW (higher density) | Moderate |
| Overlap | 60-80% forward, 30-60% lateral | 20-50% lateral | Minor |

### Resolution Strategy (v0.1)
Let camera constraints drive altitude and speed (GSD is usually the contractual
deliverable). Report the resulting LiDAR density at camera-optimal parameters.
Warn if density falls below USGS QL target.

```rust
pub struct CombinedMissionReport {
    pub camera_gsd: f64,          // achieved GSD at planned AGL
    pub lidar_density: f64,       // achieved density at camera speed/AGL
    pub lidar_ql: Option<UsgsQualityLevel>,
    pub density_shortfall: bool,  // true if below target
    pub recommended_speed: f64,   // speed needed to hit LiDAR target (may violate blur)
}
```

### If density is insufficient:
1. Slow down (may violate camera blur constraint — check B_pixels)
2. Add dedicated LiDAR-only passes at higher altitude
3. Increase lateral overlap (more lines, same altitude)
4. Reduce LiDAR FOV to concentrate pulses

## Scanner-Specific Notes

### Oscillating Mirror
- Bidirectional: scans left-to-right then right-to-left
- Point density peaks at swath edges (mirror deceleration zone)
- LSP is highest here due to mirror turnaround time
- d_along = V / (2 × scan_rate)

### Rotating Polygon
- Unidirectional: always scans in same direction
- More uniform density distribution across swath
- No mirror deceleration loss (LSP ≈ 0 for active FOV)
- d_along = V / scan_rate

### Palmer (Conical)
- Produces elliptical/spiral ground pattern
- Constant incidence angle across swath (ideal for bathymetry)
- Massive data redundancy at swath edges (double coverage)
- Offset angle δ = FOV/4 for desired total FOV

### Livox (Non-Repetitive)
- Rosette/spirograph pattern, never repeats
- Coverage ratio C_r increases with dwell time
- Lower speed = dramatically higher coverage and resolution
- Best for hover-capable platforms (multirotor)
