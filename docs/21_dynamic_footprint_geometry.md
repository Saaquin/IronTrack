# Dynamic Footprint Geometry, Platform Physics, and Algorithmic Overlap Buffering

> **Source document:** 21_Dynamic-Footprint-Geometry.docx
>
> **Scope of this extract:** Content unique to Doc 21 that is NOT covered by existing project docs (03, 07, 18). Omitted: collinearity equations, DCM/Euler angle definitions, basic GSD/FOV formulas, wind triangle derivation, and effective swath width formula — all already authoritative in Docs 03/07.

---

## 1. Vector Ray Casting for Distorted Footprint Projection

To calculate the true ground coordinates of a dynamically distorted sensor footprint, the engine casts a spatial vector from the optical center through each of the four physical corners of the sensor array.

Let a sensor corner in the camera frame be defined as the vector:

`p_cam = [x_corner, y_corner, -f]`

where `f` is the calibrated focal length and `(x_corner, y_corner)` are the physical sensor corner coordinates relative to the principal point.

To project this vector into the world frame, multiply by the transpose of the Direction Cosine Matrix `R`:

`d_world = R^T · p_cam`

where `d_world` is the resulting direction vector in world (object) space.

To find the ground intersection point, compute the scalar multiplier `t`. For a flat datum at elevation `Z_ground`:

`t = (Z_ground - Z_aircraft) / d_world_z`

The ground coordinate for that sensor corner is then:

`P_ground = [X_aircraft, Y_aircraft, Z_aircraft] + t · d_world`

When executed for all four corners, the resulting coordinates `{P₁, P₂, P₃, P₄}` form an **irregular convex quadrilateral** — not a rectangle — on the ground plane.

**DEM intersection:** When using a high-resolution DEM instead of a flat datum, the ray-casting algorithm must iteratively search for the intersection between each 3D ray and the raster elevation grid or TIN surface. This is computationally expensive and benefits from concurrent, memory-safe architectures with spatial indexing (R-tree lookups against cached DEM tiles).

---

## 2. Isolated Effects of Pitch, Roll, and Yaw on Footprint Geometry

Understanding each rotational distortion independently is essential for building deterministic overlap buffering algorithms.

### 2.1 Pitch (φ) — Longitudinal Keystoning

When the aircraft pitches upward, the camera's optical axis sweeps forward along the flight trajectory:

- The **leading edge** of the footprint is projected farther from nadir → severe scale exaggeration.
- The **trailing edge** is pulled closer to nadir → scale compression.
- The rectangular footprint warps into an **isosceles trapezoid** along the flight direction.

**Consequence:** GSD becomes non-uniform across the image. The forward overlap between consecutive frames is compressed. If the camera trigger interval is not dynamically shortened, data gaps ("holidays") occur along-track.

### 2.2 Roll (ω) — Lateral Skew and Swath Asymmetry

When the aircraft rolls (e.g., right wing down):

- The **left edge** of the footprint sweeps outward toward the horizon.
- The **right edge** sweeps inward beneath the fuselage.
- The footprint warps into a **trapezoid along the cross-track axis**.

**Critical failure mode:** A transient roll of merely 3–4° can contract the down-wing swath boundary enough to obliterate the lateral overlap safety margin, severing photogrammetric block continuity. This is the most dangerous high-frequency distortion for sidelap.

### 2.3 Yaw (κ) — Heading Divergence and Swath Clipping

Pure yaw rotates the footprint around the vertical Z-axis:

- The footprint remains rectangular (no trapezoidal distortion if pitch and roll are zero).
- But the bounding box rotates out of alignment with the flight line, **clipping** the leading and trailing corners.
- The **contiguous usable swath width perpendicular to the flight trajectory** is reduced.

This is the same geometric mechanism as the crab angle — yaw from any source (wind correction, autopilot overshoot, turbulence) produces the same swath clipping effect.

---

## 3. True Overlap via Polygon Intersection

### 3.1 Why 1D Overlap is Invalid Under 6DOF Distortion

Standard overlap is expressed as a simple ratio: an operational requirement of 75% forward overlap means the air base `B` (distance between consecutive exposures) equals 25% of the nominal footprint length:

`B = L_nominal × (1 - OL_target)`

This arithmetic is **entirely invalid** when consecutive footprints are warped into skewed quadrilaterals by varying pitch, roll, and yaw states. The overlap boundary is no longer a straight, parallel line — it transforms into an irregular convex polygon with 3 to 8 vertices depending on distortion severity.

### 3.2 Polygon Intersection Algorithm

Given two convex quadrilaterals `Q₁` (footprint at exposure time `t₁`) and `Q₂` (footprint at exposure time `t₂`), the true overlap area is the geometric intersection `Q₁ ∩ Q₂`.

**Step 1 — Vertex Containment Tests:**
For every vertex of `Q₁`, test whether it lies inside `Q₂`. Store enclosed vertices. Repeat symmetrically: test every vertex of `Q₂` against `Q₁`.

**Step 2 — Edge-Edge Intersection:**
Test every edge of `Q₁` against every edge of `Q₂` using line segment intersection formulas. Append all intersection points to the stored array.

**Step 3 — Cyclic Sorting:**
The combined point set from Steps 1 and 2 represents the vertices of the intersection polygon, but they are unordered. Compute the centroid:

`C = (1/n) × Σ Pᵢ`

Calculate the polar angle of each vertex relative to the centroid:

`θᵢ = atan2(Yᵢ - Y_c, Xᵢ - X_c)`

Sort the vertices by `θᵢ` to produce a closed, non-self-intersecting polygon boundary.

**Step 4 — Area via the Shoelace Formula:**
With vertices ordered cyclically as `{(x₁,y₁), (x₂,y₂), ..., (xₙ,yₙ)}`:

`A = 0.5 × |Σᵢ₌₁ⁿ (xᵢ · yᵢ₊₁ - xᵢ₊₁ · yᵢ)|`

where index `n+1` wraps to index `1`.

**True overlap percentage:**

`OL_true = A_intersection / A_nominal`

If `OL_true < OL_min` (e.g., the 60% minimum for dense stereoscopic matching), the engine flags an overlap failure. This polygon intersection routine is the core of the "smart overlap-checking" algorithm — it validates every generated flight line in 3D space against the terrain model before the aircraft leaves the ground.

| Algorithm Step | Operation | Purpose |
|---|---|---|
| 1. Containment | Vertex-in-polygon testing | Identifies which footprint corners survive inside the overlap zone |
| 2. Intersection | Edge-to-edge crossing | Identifies new boundary vertices created by skewed overlaps |
| 3. Sorting | Centroid + atan2 polar sort | Organizes scattered vertices into a traceable closed polygon |
| 4. Area | Shoelace theorem | Yields exact area of the overlap region in m² |

---

## 4. IMU/INS Tolerance Limits and Positional Uncertainty

### 4.1 Error Sources

An IMU is susceptible to bias instability, scale factor errors, white noise, and random walk. During long, straight flight lines — where dynamic maneuvering is absent and error states are unobservable — these errors accumulate, causing the INS solution to drift from the true 6DOF state.

Advanced systems mitigate drift via Extended Kalman Filter (EKF) or Error-State Kalman Filter (ESKF) fusion with GNSS, but **transient angular drift between GNSS updates is physically inevitable**.

### 4.2 Error Propagation from Sensor to Ground

The critical issue is how minute angular uncertainty propagates over the flying height `H` to create large positional uncertainty on the ground.

Using a first-order Taylor series approximation, the lateral ground displacement `ΔY` caused by an unmeasured roll error `Δω` is:

`ΔY ≈ H × tan(Δω) ≈ H × Δω`  (for small angles, where `Δω` is in radians)

**Worked example:** At `H = 2000 m`, a low-grade MEMS IMU drifting by just `0.1°` (`≈ 0.00175 rad`) silently shifts the entire ground footprint laterally by `≈ 3.5 m`. The camera captures images perfectly, but the actual swath is physically offset from the planned swath.

**Non-linear scaling:** At non-zero base roll angles, the error propagation includes a secant-squared multiplier from the Taylor expansion, making the ground error grow non-linearly with existing attitude offset.

### 4.3 IMU Error Budget Table

| Flying Height (H) | IMU Grade | Angular Error (Δω) | Ground Error (ΔY) | Buffer Requirement |
|---|---|---|---|---|
| 500 m | Tactical (FOG) | 0.01° (0.00017 rad) | ~0.09 m | Negligible |
| 500 m | Industrial (MEMS) | 0.1° (0.00174 rad) | ~0.87 m | Minimal (~1% of swath) |
| 2000 m | Industrial (MEMS) | 0.1° (0.00174 rad) | ~3.5 m | Moderate (~2–3% of swath) |
| 2000 m | Commercial (low-cost) | 1.0° (0.01745 rad) | ~34.9 m | Severe (>10% of swath) |

Ground positional error scales linearly with altitude but non-linearly with existing roll angles.

---

## 5. Dynamic Overlap Buffering Algorithm

### 5.1 Problem Statement

Legacy planners handle platform instability by hardcoding 80–90% overlap regardless of platform or conditions. This prevents data gaps but causes an exponential explosion in image count — bloating field acquisition time, exhausting onboard storage, and paralyzing Structure from Motion pipelines with millions of redundant tie-points.

Dynamic Overlap Buffering actively modulates flight line spacing based on a rigorous assessment of the specific platform's aerodynamic instability and hardware IMU error budget.

### 5.2 Algorithm Inputs

**Optical sensor parameters:**
- `f` — focal length (mm)
- `S_w` — sensor physical width (mm)
- `S_h` — sensor physical height (mm)

**Mission geometry:**
- `H_agl` — planned flying height above ground level (m)
- `OL_min` — minimum target lateral overlap (e.g., 0.60 for 60%)

**Aerodynamic profile:**
- `v_w` — maximum expected crosswind speed (m/s)
- `v_a` — aircraft True Airspeed (m/s)
- `ω_max` — max expected roll angle in turbulent air (degrees)
- `φ_max` — max expected pitch angle in turbulent air (degrees)

**Hardware profile:**
- `Δω_imu` — maximum angular drift tolerance of the IMU (degrees)

### 5.3 Algorithmic Sequence

**Step 1 — Nominal Swath Dimensions:**

```
W_nominal = S_w × (H_agl / f)
L_nominal = S_h × (H_agl / f)
```

**Step 2 — Crab Angle Swath Reduction:**

Compute the worst-case Wind Correction Angle:

```
WCA = arcsin(v_w / v_a)
```

Compute the effective crab-reduced swath width:

```
W_crab = W_nominal × cos(WCA) - L_nominal × |sin(WCA)|
```

**Step 3 — Roll Instability Margin:**

Compute the lateral ground displacement caused by maximum expected roll. The inward contraction of the up-wing swath edge is a function of the FOV:

```
ΔW_roll = H_agl × (tan(FOV/2) - tan(FOV/2 - ω_max))
```

where `FOV = 2 × arctan(S_w / (2 × f))`.

**Step 4 — IMU Drift Error Margin:**

Compute the latent positional uncertainty from the IMU angular tolerance:

```
ΔW_imu = H_agl × tan(Δω_imu)
```

For small angles this simplifies to `ΔW_imu ≈ H_agl × Δω_imu` (radians).

**Step 5 — Dynamic Buffer and Final Spacing:**

Total physical buffer width:

```
B_total = (W_nominal - W_crab) + ΔW_roll + ΔW_imu
```

Convert to a percentage of nominal swath:

```
OL_buffer = B_total / W_nominal
```

Final optimized flight line spacing:

```
D_line = W_nominal × (1 - OL_min - OL_buffer)
```

### 5.4 Behavioral Implications

- A **light multirotor UAS** in turbulent 15-knot crosswinds with a low-grade MEMS IMU → aggressively narrow line spacing, heavy buffering.
- A **heavy fixed-wing aircraft** on a calm day with a tactical-grade FOG IMU → substantially wider lines, maximizing survey efficiency.

The engine optimizes flight time and storage without risking data integrity, eliminating the brute-force processing bloat of arbitrary 80/80% overlap.

---

## 6. Terrain Integration: Non-Linear Compounding of 6DOF and DEM

The buffering algorithm above operates over a flat datum. A DEM introduces a final layer of geometric complexity.

When an aircraft rolls while surveying steep topography, the 6DOF angular distortions **compound non-linearly** with topographic relief displacement. If the aircraft rolls toward a rising uphill slope, the sensor ray intersects the rising terrain much sooner in 3D space, severely and disproportionately contracting the swath width on that side.

Advanced flight engines must run the polygon intersection algorithm (§3.2) iteratively across the exact DEM surface. If the projected true overlap `OL_true` falls below `OL_min` locally due to a terrain spike or valley, the dynamic buffering algorithm must either:

1. **Locally constrict** the flight line spacing in that zone, or
2. **Inject terrain-following vertical waypoints** to maintain AGL and preserve the geometric constraints.

By coupling 6DOF kinematics with high-fidelity elevation data, the planning engine guarantees coverage integrity regardless of operational complexity.
