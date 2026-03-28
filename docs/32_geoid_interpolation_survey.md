# Geoid Grid Interpolation for Survey-Grade Aerial Flight Management

> **Source document:** 32_Geoid-Interpolation-Survey.docx
>
> **Scope:** Complete extraction — all content is unique. Covers interpolation method comparison with published error data, continuity requirements for autopilot dynamics, GEOID18 mandated algorithm, edge cases, mmap performance, SIMD acceleration, and NGA validation datasets.

---

## 1. Interpolation Methods Compared

### 1.1 Bilinear (2×2 window, C⁰ continuous) — REJECTED

Uses the nearest 4 grid points. Linear interpolation first along longitude, then latitude.

**Fatal flaw:** C⁰ continuous only — the surface value is continuous but its first derivative (slope) is discontinuous at every grid cell boundary. When the aircraft crosses a boundary, the computed geoid gradient changes instantaneously. In a PID terrain-following loop, this creates a step-function in the derivative term → elevator deflection → **pitch chatter** → gyro-stabilized mount exceedance → rolling shutter artifacts → GNSS antenna phase center shift → degraded PPK ambiguity resolution.

At 80 m/s on a 2.5' grid, the aircraft crosses a grid boundary approximately every 57 seconds. Each crossing triggers the discontinuity.

### 1.2 Biquadratic (3×3 window, NGS INTG standard)

Uses 9 grid points in a 3×3 window. Sequential quadratic fit: row-wise through 3 longitude points per row (3 rows), evaluate at query longitude → 3 intermediate values → column-wise quadratic through the 3 values, evaluate at query latitude.

The specific 3×3 window shifts based on which quadrant of the central cell the query point occupies (quadrant-aware shifting).

**This is the mandatory algorithm for GEOID18.** The NGS INTG software uses biquadratic exclusively. The 34.3 MB grid was generated with the explicit mathematical assumption that downstream users would query it using the 3×3 biquadratic window. Using bilinear violates the algorithmic contract.

### 1.3 Bicubic (4×4 window, C¹ continuous) — RECOMMENDED FOR EGM2008

Uses 16 grid points. Fits a cubic polynomial surface with 16 coefficients requiring function values, first derivatives, and mixed cross-derivatives at the 4 corners.

**Key advantage:** Enforces **C¹ continuity** — both the surface and its first derivative are perfectly smooth across all grid cell boundaries. Eliminates autopilot pitch chatter entirely.

GeographicLib's optimized implementation uses a 12-point stencil (drops 4 extreme corners) to mitigate over-parameterization and reduce grid quantization noise.

---

## 2. Empirical Error Data (GeographicLib GeoidEval Benchmarks)

### 2.1 EGM96 Grid (15' × 15', ~27.8 km node spacing)

| Method | Max Absolute Error | RMS Error |
|---|---|---|
| Bilinear | **1.152 m** | 40.0 mm |
| Bicubic | 0.169 m (16.9 cm) | 7.0 mm |

**Verdict:** Bilinear on EGM96 is catastrophically insufficient. Even bicubic on EGM96 has 16.9 cm worst-case — marginally acceptable at best. If EGM96 output is required (DJI export), evaluate the full degree-360 spherical harmonic series or use bicubic as a minimum.

### 2.2 EGM2008 Grid (2.5' × 2.5', ~4.6 km node spacing)

| Method | Max Absolute Error | RMS Error |
|---|---|---|
| Bilinear | **0.135 m (13.5 cm)** | 3.2 mm |
| Bicubic | **0.031 m (3.1 cm)** | 0.8 mm |

**Verdict:** Bilinear on EGM2008 still peaks at 13.5 cm — violates ASPRS 5 cm vertical RMSEz tolerance. **Bicubic on EGM2008 (max 3.1 cm, RMS 0.8 mm) is the minimum acceptable for survey-grade work.**

### 2.3 Summary Decision Matrix

| Grid | Bilinear | Biquadratic | Bicubic |
|---|---|---|---|
| EGM96 (15') | **DISQUALIFIED** (1.15 m) | Not standard for EGM96 | Marginal (16.9 cm) |
| EGM2008 (2.5') | **DISQUALIFIED** (13.5 cm) | Acceptable (NGS uses for domestic grids) | **SURVEY-GRADE** (3.1 cm) |
| GEOID18 (1') | Violates NGS INTG contract | **MANDATED** (NGS standard) | Overkill (grid is already 1' dense) |

---

## 3. IronTrack Algorithm Assignments

| Grid | Algorithm | Rationale |
|---|---|---|
| **GEOID18** | Biquadratic (3×3 quadrant-aware) | NGS INTG standard. Federal compliance. |
| **EGM2008** | Bicubic (4×4 / 12-point stencil) | Suppresses max error to 3.1 cm. C¹ continuity eliminates pitch chatter. |
| **EGM96** | Vendored egm96 crate (full harmonic evaluation) | Grid interpolation on 15' spacing has unacceptable error. Direct harmonic synthesis preferred. |

---

## 4. Geographic Edge Cases

### 4.1 ±180° Longitude Wrap-Around

Global grids span 0°→360° or −180°→+180°. An interpolation window crossing the antimeridian requests array indices outside the loaded matrix.

**Solution:** Pad the array with "ghost columns" — physically duplicate the first 3 columns at the end of the array in RAM during initial load. The CPU blindly increments indices without branching or modulo checks in the real-time loop.

### 4.2 Polar Convergence (> 85° Latitude)

Longitude meridians converge toward zero physical distance at the poles. Rectilinear interpolation assumes relatively constant orthogonal cell geometry. At exactly 90° latitude, all longitudes represent the same physical point.

**Solution:** Constrain the polynomial to be strictly independent of longitude when evaluating coordinates at or near the poles. Without this, the interpolator produces undefined behavior, divide-by-zero panics, or wildly oscillating values.

### 4.3 Ocean Lookups

Geoid models are NOT undefined over water. EGM2008 incorporates massive volumes of satellite altimetry data mapping the marine geoid. These undulation values are required for coastal photogrammetry and bathymetric LiDAR correlation against MSL.

**Rule:** The FMS must never mask, zero out, or reject geoid lookups over ocean polygons.

---

## 5. Computational Performance

### 5.1 Memory Architecture: mmap via memmap2

Map binary grids directly into the process virtual address space. The OS kernel handles paging — specific grid cells are fetched from SSD into L1/L2 cache only when accessed.

- EGM2008 2.5' grid: ~149 MB
- GEOID18 Big-Endian grid: 34.3 MB

No heap allocation, no buffered I/O initialization latency. The mmap instance is `Send + Sync` in Rust — shared across unlimited rayon parallel threads without lock contention.

### 5.2 Scalar Performance (Waypoint Computation)

| Method | Throughput (single thread) | 60,000 waypoints |
|---|---|---|
| Bilinear | 4–8 million/sec | ~8–15 ms |
| Bicubic | 1–2 million/sec | **~30–60 ms** |

60,000 bicubic interpolations in under 60 ms. **The "bilinear is faster" argument is completely obsolete on modern mmap architectures.** Bicubic is unquestionably fast enough for offline flight plan generation.

### 5.3 SIMD Acceleration (Real-Time DEM Rendering)

For 60 FPS SVS/HITS cockpit rendering, dynamically reprojecting a 1000×1000 pixel DEM raster (1 million vertices) requires massive throughput.

**Approach:** Pack multiple f64 coordinates into AVX2 registers (f64x4 = 4 coordinates per 256-bit register). Biquadratic and bicubic polynomials consist entirely of branchless FMA (fused multiply-add) instructions — ideal for SIMD.

| Acceleration | Speedup | Use Case |
|---|---|---|
| Scalar | 1× (baseline) | Waypoint computation (sufficient) |
| **f64x4 AVX2** | **4–15×** | Real-time DEM rendering, SVS/HITS cockpit display |

1 million DEM vertices datum-shifted in < 15 ms with SIMD. Not needed for standard waypoint computation but essential for the glass cockpit 3D terrain view.

Rust implementation via `core::simd` (nightly) or portable crates like `wide`.

---

## 6. Precision Requirements

### 6.1 f32 → f64 Cast Before Polynomial Math

Grid files store undulation values as f32 (4 bytes, ~7 significant digits, 1 mm quantization floor). All interpolation coefficients, coordinate normalizations, and polynomial evaluations must execute in **f64**.

Executing polynomial weights in f32 risks mantissa truncation that accumulates to millimeter-level trajectory drift over 100 km survey lines.

### 6.2 Kahan Summation

Distance accumulation along flight trajectories must use Kahan summation to prevent floating-point drift during iterative additions over hundreds of kilometers.

### 6.3 Validation Target

The final interpolated geoid value must consistently reproduce the expected undulation to 3 decimal places (0.001 m = 1 mm), matching the quantization floor of the original NGA source grids.

---

## 7. NGA Validation Dataset

**Official test file:** `Und_min2.5x2.5_egm2008_isw=82_WGS84_TideFree`

Published by the NGA specifically for validating third-party EGM2008 interpolation implementations. Contains synthesized test points with known spherical harmonic undulation values.

**Validation procedure:**
1. Extract geographic coordinates from the NGA test file.
2. Feed them into the IronTrack bicubic interpolator.
3. Compare output against published control values.
4. Prove that deviation is bounded by ≤ 3.1 cm (the theoretical maximum for bicubic on the 2.5' grid).

Exact nanometer parity with the full 2159-degree harmonic expansion is mathematically impossible (polynomial spline vs. global summation). The test proves the error is within the known theoretical bounds of the chosen method.
