# Terrain-Following Trajectory Generation and Smoothing Architecture

> **Source document:** 34_Terrain-Following-Planning.docx
>
> **Scope:** Complete extraction — all content is unique. Covers the aerodynamic constraint translation, Elastic Band energy minimization, Cubic B-spline C² smoothing, convex hull AGL safety guarantee, adaptive knot placement, dynamic look-ahead pure pursuit, Rust crate selection, and synthetic validation methodology.

---

## 1. The Terrain-Following Trade-Off

Raw DEM data contains high-frequency noise (sensor anomalies, radar backscatter, canopy, micro-topography). If an autopilot tracks this profile directly, it commands rapid elevator deflections → severe pitch oscillations → structural fatigue, sensor instability, loss of control.

But heavy smoothing deviates from the target AGL → GSD varies → forward-lap and side-lap gaps → photogrammetric model failure.

**The algorithm must:** maintain minimum AGL clearance (CFIT avoidance), bound max pitch rate / climb rate / load factor to airframe limits, and minimize total RMS deviation from the ideal survey altitude.

---

## 2. Aerodynamic Constraints → Geometric Bounds

Aircraft dynamic limits in the time domain map to geometric constraints on the spatial curve:

| Aerodynamic Limit | Typical Value | Spatial Equivalent | Geometric Constraint |
|---|---|---|---|
| Max pitch angle | 10°–15° | Path slope (dy/dx) | \|slope\| ≤ tan(θ_max) |
| Max vertical acceleration | 0.5–1.0 g | Path curvature (κ) | κ ≤ a_max / V² |
| Max pitch rate | 3°–5°/s | Angular spatial rate | Bounds how rapidly slope changes per meter |
| Max vertical jerk | Comfort / gimbal limit | Path "kink" (dκ/ds) | Requires C² continuity (continuous second derivative) |

**Jerk is the most critical for survey:** excessive jerk destabilizes gyro-stabilized camera gimbals and LiDAR IMUs → sensor blur, calibration drift, data loss. The curve must have continuous second derivatives.

---

## 3. Stage 1: Elastic Band Energy Minimization

The flight path is modeled as a sequence of discrete nodes connected by virtual springs in a vertical plane above the terrain. An iterative solver minimizes a global energy functional E_total = E_internal + E_external.

### 3.1 Internal Energy (Aerodynamic Shape)

**Tension springs (Hooke's Law):** Minimize distance between adjacent nodes. Drives the path to be short and straight. Reduces flight time and energy consumption. But alone creates sharp corners that violate acceleration limits.

**Torsional springs:** Penalize the bending angle between three consecutive nodes. The torsional coefficient controls curvature smoothing — ensures pitch rate and vertical acceleration stay within aerodynamic thresholds.

### 3.2 External Energy (Terrain Interaction)

**Terrain repulsion:** Asymmetric one-sided barrier potential (modified Lennard-Jones / exponential decay). If a node drops below the minimum AGL safety threshold, it experiences a massive repulsive force upward.

**Gravitational pull:** A weak, constant downward force prevents the internal tension from pulling the band infinitely high (which would destroy the photogrammetric GSD).

### 3.3 Solver Performance

Gradient descent with explicit Euler integration. Convergence criteria: max node displacement < tolerance (e.g., 0.01 m) or energy gradient → 0.

**Initialization:** Band starts above the highest peak in the bounding box, then relaxes downward onto the terrain repulsion field. This avoids local minima traps (getting snagged under overhangs).

**Performance:** For a 30 km flight line at 10 m intervals → 3,000 nodes. The Jacobian is strictly banded and tridiagonal (forces depend only on adjacent neighbors). With rayon + SIMD + SoA memory layout, convergence in 200–500 iterations → **milliseconds of compute**. Well within the latency budget for interactive React web UI preview.

### 3.4 Limitation

The EB output is a sequence of discrete linear segments. Higher-order derivatives (jerk) computed by finite differences are noisy and discretization-dependent. **EB cannot guarantee C² continuity.** Hence: stage 2.

---

## 4. Stage 2: Cubic B-Spline Smoothing

The relaxed Elastic Band nodes are fitted with a Cubic B-spline (degree p=3): a piecewise polynomial that inherently guarantees **C² continuity**.

- First derivative (slope → pitch angle): continuous ✓
- Second derivative (curvature → vertical acceleration): continuous ✓
- Third derivative (kink → jerk): bounded and finite ✓

### 4.1 The Convex Hull Property — AGL Safety Guarantee

The Cox-de Boor basis functions are non-negative and sum to 1. Therefore the B-spline curve lies strictly within the convex hull of its local control polygon.

**Exploitation for safety:** If every segment of the control polygon is at or above the minimum clearance altitude over the underlying DEM, the resulting flight path is **mathematically incapable of ever dropping below that AGL threshold.**

This transforms continuous 3D collision detection (expensive) into discrete geometric validation of control points (cheap). If a control point violates the constraint, simply translate it upward on the Z-axis until the local convex hull clears the terrain.

### 4.2 Adaptive Knot Placement

Uniform knots are wasteful over flat terrain and insufficient over jagged terrain.

**Algorithm:**
1. Compute discrete curvature of the relaxed EB nodes.
2. Apply curvature threshold segmentation to identify dominant feature points (peaks, valleys, cliff edges).
3. Cluster knots densely around feature points (flexibility to track sharp ridges without exceeding curvature limits).
4. Decimate knots heavily over flat, unvarying terrain (minimal control points, wide spans).

**Result:** Compressed flight plan, reduced GeoPackage size, improved approximation accuracy where it matters.

### 4.3 Rust Crate Selection

| Crate | Architecture | FMS Suitability |
|---|---|---|
| **`bspline` + `nalgebra`** | Pure Rust, zero-allocation, f64, rayon-compatible | **High.** Recommended path. |
| `spliny` | FFI wrapper around Fortran FITPACK | **Rejected.** Violates memory safety, ARM cross-compilation issues. |
| `enterpolation` | Flexible builder patterns, multiple curve types | Moderate. Slower than dedicated math crates due to abstraction overhead. |
| **Custom Cox-de Boor** | From scratch, optimized for f64 SoA | **High.** Maximum control over memory layout, Kahan summation, rayon threading. |

---

## 5. Dynamic Look-Ahead Pure Pursuit Tracking

### 5.1 The Problem with Static Look-Ahead

An aircraft can't instantly convert kinetic energy to potential energy. It must begin climbing well before reaching a ridge. But a fixed maximum look-ahead distance causes "corner-cutting" over ridges and flying too high through valleys → GSD destruction.

### 5.2 Kinematic Minimum Look-Ahead

```
L_a_min = (Δh × V) / V_z_max
```

where Δh = obstacle relative height, V = ground speed, V_z_max = max sustained climb rate.

### 5.3 Dynamic Adaptation

**Curvature-based:** Over flat/gentle segments → L_a expands (smooth, efficient, low-effort guidance). Approaching high-curvature segments (ridge apex) → L_a shrinks proportionally to 1/κ (tight apex tracking, no corner-cutting).

**Error-based:** If the aircraft deviates from the planned altitude (wind shear, downdraft) → L_a contracts exponentially with error magnitude → aggressive corrective deflection.

### 5.4 Integration with B-Spline

Because the B-spline is a continuous analytical function, finding the target coordinate exactly L_a meters ahead is accomplished via **arc-length parameterization** using Gaussian quadrature. No discretization error from linear-segment navigation.

---

## 6. Synthetic Validation Suite

Four deterministic test profiles injected through the complete EB → B-spline pipeline:

| Profile | Geometry | Validates | Expected Behavior |
|---|---|---|---|
| **Flat terrain** | Constant elevation, zero slope | Baseline stability, no algorithmic drift | Path at exactly target AGL. Curvature = 0. No uniform knot bloat. |
| **Sinusoidal** | Rolling harmonic waves | C² continuity, pitch rate limits, steady-state tracking | Smooths peaks, fills valleys. Max curvature exactly matches κ_max limit. |
| **Step function** | Instantaneous vertical cliff | Impulse response, max pitch angle, anticipatory look-ahead | Projects a climb ramp well before the cliff base. Smooth jerk-limited transition into max slope. |
| **Sawtooth** | Jagged high-frequency ridgelines | Convex hull safety, adaptive knot placement | Dense knot clustering at peaks. Altitude **never** drops below min AGL at any sampled point. |

The validation suite spatially samples the generated curve at high density and asserts that:
- Altitude never violates the AGL minimum (convex hull property).
- Curvature never exceeds κ_max.
- Slope never exceeds tan(θ_max).
- Altitude variance from the ideal AGL is minimized (photogrammetric efficiency metric).
