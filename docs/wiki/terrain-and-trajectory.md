# Terrain & Trajectory

IronTrack's terrain layer ingests global elevation data and its trajectory layer produces smooth, terrain-conforming flight paths with mathematically guaranteed clearance.

## Overview

The terrain-to-trajectory pipeline has four stages:

1. **DEM Ingestion** — Copernicus GLO-30 tiles are fetched, cached, and parsed.
2. **Elastic Band** — An energy minimizer relaxes the vertical profile toward terrain while maintaining clearance.
3. **B-Spline Smoothing** — A cubic C² interpolant smooths the elastic band output with a convex hull safety guarantee.
4. **Pure Pursuit** — A dynamic look-ahead controller converts the B-spline to discrete autopilot waypoints.

## Copernicus GLO-30 DEM

IronTrack uses the Copernicus GLO-30 Digital Surface Model as its primary global elevation source. Each tile covers 1°×1° at 30-metre resolution.

**Tile format:** Cloud Optimized GeoTIFF, FLOAT32 samples, DEFLATE compression with floating-point predictor (Predictor=3). Row-major layout, north-to-south, west-to-east. Column count varies by latitude band (3,600 at the equator, narrowing to 360 near the poles).

**Height reference:** Orthometric heights above the EGM2008 geoid (Tide-Free system). IronTrack converts to WGS84 Ellipsoidal on ingestion using h = H + N.

**Void handling:** Pixels with values ≤ −32,767 or NaN represent water bodies or data gaps. Bilinear interpolation returns `None` if any of the four surrounding posts is void.

**DSM caveat:** Copernicus is X-band SAR, meaning it sees canopy tops rather than bare earth. Reported altitudes include vegetation (2–8 m overstated in forests). IronTrack displays a mandatory, non-dismissable operator warning whenever Copernicus is the terrain source.

## Two-Phase I/O

DEM operations follow a strict Two-Phase I/O pattern to keep network latency out of the parallel compute path:

**Phase 1 (Sequential):** The `TerrainEngine` identifies which 1° tiles are needed, checks the local cache, and downloads any missing tiles from the Copernicus AWS S3 bucket. Downloads use atomic file rename to handle concurrent requests safely.

**Phase 2 (Parallel):** Once all tiles are in memory, bilinear elevation queries are dispatched via `rayon::par_iter`. This phase is pure CPU work with no I/O.

The cache lives at `%APPDATA%/irontrack/cache/dem/` on Windows and `~/.cache/irontrack/dem/` on Linux/macOS.

## Bilinear Interpolation

Each elevation query maps a (lat, lon) pair to fractional grid coordinates within the tile, then performs four-post bilinear interpolation. Edge coordinates are clamped to the tile boundary (tiles do not share boundary posts with their neighbours). The antimeridian is normalized so that lon = 180° maps to the W180 tile.

## Elastic Band Energy Minimization

The elastic band solver relaxes a vertical profile from an initially safe altitude down toward terrain, balancing four competing energy terms:

**Energy terms:**

- **Tension** — Hooke's law springs between adjacent nodes pull the path toward a straight line.
- **Torsion** — Angular penalty on bending enforces smooth pitch transitions.
- **Terrain repulsion** — Asymmetric exponential barrier repels nodes approaching the clearance floor. Strength decays exponentially with distance above floor.
- **Gravity** — A weak constant downward pull prevents the path from floating arbitrarily high.

**Algorithm:** Initialization places all nodes above the highest peak plus minimum clearance. Gradient descent with a tridiagonal Jacobian iterates until the maximum node displacement falls below 0.01 m (typical convergence: 200–500 iterations for a 30 km profile).

**Hard safety constraint:** Nodes can never descend below `floor[i] = terrain[i] + min_clearance`, regardless of the energy gradient.

### Floor Modes

The clearance floor can be computed five ways:

- **TerrainFollowing** (default) — Per-point: `floor[i] = terrain[i] + clearance`. The path hugs terrain contours.
- **MeanTerrain** — Constant floor at the mean terrain elevation plus clearance. Suitable for gently rolling terrain.
- **MedianTerrain** — Constant floor at the median elevation. More robust to outlier peaks or valleys.
- **FixedMsl(alt)** — Absolute MSL altitude. Terrain clearance is still enforced as a hard minimum.
- **HighestPoint** — Level flight above the highest peak. The most conservative option.

### Default Spring Constants

```
tension:          0.1     (path shortening)
torsion:          0.05    (curvature penalty)
repulsion:        5.0     (terrain barrier)
repulsion_decay:  5.0 m   (exponential decay distance)
gravity:          0.5     (downward pull)
```

Equilibrium AGL above floor is approximately 3.5 m with these defaults.

## Cubic B-Spline Smoothing

The elastic band output is numerically stable but can contain residual high-frequency oscillations. A cubic B-spline interpolant provides C² continuity (continuous curvature), which prevents autopilot pitch chatter.

**Fitting methods:**

- **Uniform** — Equal knot spacing. Requires ≥ 4 data points.
- **Adaptive** — Knot density follows terrain complexity. Extra knots are inserted at detected feature points (peaks, cliffs, ridges); fewer knots on flat terrain. Falls back to uniform if feature detection produces fewer than 4 points.

**Internals:** Chord-length parameterization distributes parameters uniformly. A clamped uniform knot vector (multiplicity degree+1 at endpoints) is solved via the Thomas tridiagonal algorithm in O(n) time. Evaluation uses the Cox-de Boor recursive algorithm at degree 3.

### Convex Hull Safety Guarantee

The Cox-de Boor basis functions form a non-negative partition of unity. This means: if every control point clears the minimum altitude above terrain, then the entire B-spline curve is mathematically guaranteed to clear that altitude. No sampling-based check can miss a violation.

IronTrack exploits this property:

- `enforce_convex_hull_safety()` lifts any control point below the clearance floor.
- `verify_convex_hull_safety()` confirms the guarantee holds.

This is the key safety argument for the entire trajectory pipeline.

## Pure Pursuit Controller

The pursuit controller converts the continuous B-spline into a discrete sequence of autopilot waypoints with dynamically varying density.

**Dynamic look-ahead distance:**

The look-ahead adapts to two factors:

1. **Curvature** — Shrinks at ridge peaks (high curvature regions need tighter tracking), expands on flat terrain.
2. **Altitude error** — Contracts exponentially when the path altitude deviates from the target.

```
base_lookahead = min_la + (max_la - min_la) × (1 − curvature_ratio)
adaptive_lookahead = base_lookahead × exp(−error_gain × |alt_error|)
```

**Default parameters:**

- Minimum look-ahead: 50 m (high curvature).
- Maximum look-ahead: 500 m (flat terrain).
- Error gain: 0.05.

**Waypoint generation:** The B-spline is sampled at 10,000 points to build an arc-length table (using Kahan compensated summation). Waypoints are then placed at uniform along-track intervals, with each waypoint's altitude taken directly from the B-spline (clearance is already enforced by the elastic band + convex hull guarantee).

## Synthetic Validation

The `validation.rs` module provides synthetic terrain profiles for testing the full pipeline without real DEM data:

- **Flat** — Constant elevation. Verifies baseline behaviour.
- **Sinusoidal** — Rolling hills. Tests smooth tracking and energy balance.
- **Step** — Abrupt cliff. Tests rapid altitude response without overshoot.
- **Sawtooth** — Asymmetric ridges. Stress-tests the torsion/repulsion balance.

Each profile is run through the full elastic band → B-spline → pursuit pipeline, with assertions on clearance, smoothness, and waypoint density.

---

## Deep Dive: Implementation Notes

### Performance

- Tile downloads are the bottleneck. Once cached, a full 30 km terrain profile query completes in under 50 ms on a modern laptop.
- Elastic band convergence is O(iterations × nodes). The tridiagonal Jacobian structure means each iteration is O(n), not O(n²).
- B-spline fitting via Thomas algorithm is O(n). Evaluation is O(log n) per point via knot span binary search.

### Elevation Sources

The `TerrainReport` struct tracks provenance:

- `CopernicusDem` — Normal tile query.
- `SeaLevelFallback` — Coordinate is valid but tile has void data (coastal/water). Returns 0.0 m MSL.
- `OceanFallback` — Tile does not exist (open ocean). Returns 0.0 m MSL.

### Atomic Tile Downloads

If two threads request the same tile simultaneously, both may begin downloading. The cache uses atomic rename to persist: the second writer silently discovers the file already exists and discards its copy. This is a benign race with no corruption risk.

### Future: 3DEP Integration (v0.9)

USGS 3DEP LiDAR-derived bare-earth DTM will be available as a secondary source for US domestic operations. Where 3DEP coverage exists, it will take precedence over Copernicus (true DTM vs. DSM). Access will be via HTTP 206 range requests against the USGS AWS S3 bucket.

### Reference Documents

- `08_copernicus_dem.md` — Tile format, compression, resolution bands.
- `09_dsm_vs_dtm.md` — DSM bias analysis, vegetation penetration.
- `10_Rust_COG_Ingestion.md` — TIFF parsing, DEFLATE decompression.
- `33_copernicus_tile_architecture.md` — S3 bucket structure, caching strategy.
- `34_terrain_following_planning.md` — Elastic Band + B-spline hybrid design.
- `50_dubins_mathematical_extension.md` — 3D trajectory extensions.
- `52_rust_performance_optimization.md` — SoA layout, rayon tuning, AVX2.
