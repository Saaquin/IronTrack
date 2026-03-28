# ASPRS Positional Accuracy Standards and USGS LiDAR Quality Levels

> **Source document:** 36_ASPRS-Data-Standards.docx
>
> **Scope:** Complete extraction — all content is unique. Covers ASPRS 2023 Edition 2 horizontal/vertical accuracy classes, GSD-to-accuracy prediction, BBA overlap geometry, GCP network design, USGS QL0–QL3 presets, and IronTrack integration architecture.

---

## 1. ASPRS 2023 Edition 2 — Fundamental Shift

The ASPRS 2023 standard is **sensor-agnostic and data-driven**. It abandons legacy map-scale-based accuracy tiers (Class 1/2/3 tied to compilation scale) and relies **exclusively on RMSE** as the positional accuracy metric. Applies equally to photogrammetry, linear-mode LiDAR, Geiger-mode LiDAR, and IFSAR.

**95% confidence level is eliminated.** The dual-reporting of RMSE and 95% confidence (RMSE × 1.9600) caused persistent industry confusion. RMSE is the fundamental statistical truth; the 95% figure was derived from it and added no information.

---

## 2. Horizontal Accuracy Classes

Horizontal accuracy classes are **continuous, not discrete**. Any X-cm class can be specified. Defined by RMSE_H:

```
RMSE_H = √(RMSE_x² + RMSE_y²)
```

| Horizontal Class | RMSE_H (cm) | Seamline Mismatch Max (cm) |
|---|---|---|
| 5-cm | ≤ 5.0 | ≤ 2× class = 10.0 |
| 7.5-cm | ≤ 7.5 | ≤ 15.0 |
| 10-cm | ≤ 10.0 | ≤ 20.0 |
| 15-cm | ≤ 15.0 | ≤ 30.0 |
| 30-cm | ≤ 30.0 | ≤ 60.0 |
| X-cm | ≤ X | ≤ 2X |

Seamline mismatch cap prevents visible feature shearing across orthomosaic image boundaries.

---

## 3. Vertical Accuracy: NVA and VVA

### NVA (Non-Vegetated Vertical Accuracy)
Assessed in open, bare-earth terrain. Errors are normally distributed. Evaluated using RMSE_z. **NVA is the sole pass/fail criterion for data acceptance.**

### VVA (Vegetated Vertical Accuracy)
Assessed under canopy. Errors are **non-normally distributed** (dense foliage blocks pulses, multi-path GNSS errors, classification algorithm failures). Evaluated using the **95th percentile of absolute vertical errors**.

**VVA is no longer pass/fail.** The ASPRS recognized that VVA degradation is an environmental limitation, not a sensor/workflow failure. VVA must be tested and reported in metadata, but does not trigger automatic rejection.

| Vertical Class | NVA RMSE_z (cm) | VVA 95th Percentile |
|---|---|---|
| 1-cm | ≤ 1.0 | Reported as found |
| 5-cm | ≤ 5.0 | Reported as found |
| 10-cm | ≤ 10.0 | Reported as found |
| 15-cm | ≤ 15.0 | Reported as found |
| 20-cm | ≤ 20.0 | Reported as found |
| X-cm | ≤ X | Reported as found |

---

## 4. GSD-to-Accuracy Mapping

### Industry Heuristics
Although ASPRS 2023 formally decouples GSD from accuracy classes, the industry relies on established mathematical relationships:

- **AT accuracy ≈ 0.5 × final orthophoto pixel size.** Example: for a 15-cm orthophoto, the AT must achieve RMSE_H ≤ 7.5 cm.
- **Ground control accuracy ≈ 2× more rigorous than AT target.** Example: for 7.5 cm AT target, GCPs need RMSE ≤ 2.5–3.75 cm.
- **Raw optical GSD ≤ 95% of final product pixel size.** If the client requires 10-cm orthomosaic, raw GSD must be ≤ 9.5 cm. The 5% buffer accounts for anti-aliasing, cubic convolution interpolation, and terrain-induced distance variation.

### Predictive RMSE_H Formula
The ASPRS 2023 standard provides a deterministic approximation for projecting horizontal accuracy from hardware parameters:

```
RMSE_H = √(GNSS_radial² + (AGL × IMU_roll_pitch / 1.478)² + (AGL × IMU_heading / 1.478)²)
```

Where the constant 1.478 is an empirical conversion factor translating angular arc-seconds into linear ground variance based on projection geometry.

### Worked Example Table

| AGL (m) | GNSS Radial (cm) | IMU Roll/Pitch (arcsec) | IMU Heading (arcsec) | Projected RMSE_H (cm) |
|---|---|---|---|---|
| 500 | 10.0 | 10 | 15 | 10.7 |
| 1,000 | 10.0 | 10 | 15 | 12.9 |
| 1,500 | 10.0 | 10 | 15 | 15.8 |
| 2,000 | 10.0 | 10 | 15 | 19.2 |
| 2,500 | 10.0 | 10 | 15 | 22.8 |

### IronTrack UI Integration
As the operator adjusts AGL or swaps IMU hardware in the React frontend, the system should render a real-time validation badge: "The selected sensor at 1,000 m AGL achieves 4.5 cm raw GSD and a projected RMSE_H of 12.9 cm, supporting the ASPRS 15-cm Horizontal Accuracy Class."

---

## 5. Overlap Requirements for BBA

### Minimums
- **Forward overlap (endlap):** 60% minimum (basic stereo). **80% recommended** for complex terrain, urban, or survey-grade work. High endlap ensures 5–7 ray convergence per ground point, dramatically reducing Z-error.
- **Sidelap:** 30% minimum (ties strips into a block). **60% recommended** for corridor mapping or 3D urban mesh generation.

### Excessive Overlap Warning
Configurations exceeding **85% forward / 85% sidelap destroy the base-to-height (B/H) ratio**. Intersecting rays approach parallelism → geometric instability in the BBA matrix → inflated vertical RMSE. IronTrack should warn if overlap exceeds 85/85.

### Terrain-Following Overlap Preservation
At fixed AMSL altitude, rising terrain shrinks the footprint, reduces GSD, and **critically reduces overlap**, causing tie-point voids. IronTrack's Elastic Band + B-spline terrain-following pipeline guarantees C² continuity, eliminating pitch chatter that destroys planned forward overlap. The convex hull property ensures AGL never drops below target, preventing GSD inflation beyond ASPRS tolerances.

IronTrack's 6DOF footprint ray-casting dynamically recomputes overlap against the 3DEP DEM, inflating the buffer for anticipated IMU angular error and bank angles during line transitions.

---

## 6. Ground Control Point (GCP) Requirements

### Minimum Checkpoint Count: 30 (Central Limit Theorem)
The 2023 standard replaces the legacy arbitrary minimum of 20 with **30 checkpoints**. The CLT dictates that n ≥ 30 is required for the sample mean to approximate a normal distribution — essential for statistically valid uncertainty metrics when vertical errors (VVA) are non-normally distributed.

### Scaling Table

| Project Area (km²) | NVA Checkpoints Required |
|---|---|
| ≤ 500 | 30 |
| 501–750 | 35 |
| 751–1,000 | 40 |
| 1,001–1,250 | 45 |
| 1,251–1,500 | 50 |
| 1,501–1,750 | 55 |
| 1,751–2,000 | 60 |
| 2,001–2,250 | 65 |
| 2,251–2,500 | 70 |
| > 10,000 | **120 (maximum cap)** |

Checkpoints must be proportionally stratified across vegetated vs non-vegetated land cover.

### 4× Accuracy Multiplier Relaxed
The legacy requirement that GCPs be 4× more accurate than the target product (and checkpoints 3×) is **no longer mandatory**. Modern hardware (mechanical global shutters, sub-microsecond event marking) is sufficiently deterministic. However, when product accuracy approaches the checkpoint survey noise floor, survey error must be statistically factored into the final RMSE computations.

### Automated GCP Network Proposals (IronTrack Feature)
IronTrack can suggest optimal GCP placement via K-means clustering weighted by bounding box area and terrain slope variance, querying the 3DEP microtiles. Avoidance rules: slopes > 10° (vertical interpolation errors), DOF obstacle buffers (GNSS sky view), water bodies (DEM fusion masks). Export as GeoJSON for field survey crews.

---

## 7. USGS Lidar Base Specification (2022 Revision A)

### Quality Level Matrix

| QL | ANPS (m) | ANPD (pts/m²) | NVA RMSE_z (cm) | VVA 95th% (cm) | Min DEM Cell (m) |
|---|---|---|---|---|---|
| **QL0** | ≤ 0.35 | ≥ 8 | ≤ 5.0 | ≤ 10.0 | 0.5 |
| **QL1** | ≤ 0.35 | ≥ 8 | ≤ 10.0 | ≤ 20.0 | 0.5 |
| **QL2** | ≤ 0.71 | ≥ 2 | ≤ 10.0 | ≤ 20.0 | 1.0 |
| **QL3** | ≤ 1.41 | ≥ 0.5 | ≤ 20.0 | ≤ 40.0 | 2.0 |

### Key Distinctions
- **QL0 vs QL1:** Same spatial resolution (≥ 8 pts/m²), but QL0 demands ≤ 5.0 cm NVA — approaching the hardware noise floor. Requires impeccable IMU calibration and often disqualifies standard RTK rovers for checkpoint validation.
- **QL2:** 3DEP baseline minimum for new acquisitions. ≥ 2 pts/m², same accuracy as QL1.
- **QL3:** Legacy/low-budget reconnaissance. Rarely used for new commercial acquisitions.

### Point Density Formula
```
ρ = PRR × (1 - LSP) / (V × W)
W = 2 × AGL × tan(FOV / 2)
```

### QL Preset Reverse-Kinematic Solver (IronTrack)
When the operator selects "USGS QL1" from the mission dropdown:
1. **Retrieve hardware constraints** from the sensor profile (max PRR, scan rate, beam divergence, max AGL).
2. **Bound aircraft velocity** from airframe performance parameters.
3. **Optimize AGL** to satisfy ANPD ≥ 8 AND RMSE_z ≤ 10 cm (factoring GNSS/IMU precision).
4. **Apply 50% sidelap** — USGS measures ANPD as aggregate, so overlap zones effectively double single-swath density.
5. Auto-generate terrain-following flight lines that mathematically guarantee QL compliance.

---

## 8. IronTrack Integration Summary

### Predictive Compliance Dashboard
Real-time ASPRS accuracy projection in the React UI. As the operator adjusts AGL, sensor, or IMU parameters, the display updates the projected RMSE_H and RMSE_z against the selected accuracy class.

### Sensor Library Integration
The TanStack Table sensor library should include GNSS radial error and IMU angular error alongside focal length and sensor dimensions. The side-by-side comparison matrix should display "achievable ASPRS class" for each sensor at the planned AGL.

### Epoch Metadata Persistence
ASPRS accuracy classes (especially sub-10 cm) are easily violated by uncorrected tectonic drift (~40 cm between 2010.0 and 2026.2). The GeoPackage irontrack_metadata must persist horizontal_datum, vertical_datum, realization, epoch, and observation_epoch to enable downstream analytical correction.

### Rolling Shutter Warning
The 2024 ASPRS framework implicitly mandates mechanical global shutters for survey-grade PPK work. IronTrack's non-dismissable rolling-shutter warning (from Doc 24) is architecturally aligned with ASPRS requirements.
