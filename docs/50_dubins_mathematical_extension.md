# Dubins Path Mathematics: Wind-Corrected, Clothoid-Smoothed 3D Trajectory Generation

> **Source document:** 50_Dubins-Mathematical-Extension.docx
>
> **Scope:** Complete extraction — all content is unique. Covers the 6 canonical Dubins path types with analytical geometry, clothoid (Euler spiral) C² smoothing via Fresnel integrals, 3D Dubins Airplane altitude management, wind correction angles, and ground-track-referenced turn radius expansion.

---

## 1. Classical Dubins Path Formulation

### Kinematics
Vehicle state: (x, y, θ). Constant velocity V, bounded turn rate |u| ≤ u_max. Minimum turn radius r_min = V / u_max.

**Pontryagin's Maximum Principle** proves: shortest path = at most 3 segments, each either maximum-curvature arc (C) or straight line (S). Six canonical types:

| Path | Family | Tangent Type | Feasibility | Straight Length |
|---|---|---|---|---|
| LSL | CSC | Outer (parallel) | Always feasible | d (center-to-center distance) |
| RSR | CSC | Outer (parallel) | Always feasible | d |
| LSR | CSC | Inner (transverse) | d ≥ 2r | √(d² - 4r²) |
| RSL | CSC | Inner (transverse) | d ≥ 2r | √(d² - 4r²) |
| LRL | CCC | Contiguous arcs | d ≤ 4r | N/A |
| RLR | CCC | Contiguous arcs | d ≤ 4r | N/A |

### Key Insights
- **Inner tangent feasibility:** If center-to-center distance < 2r → LSR/RSL impossible (circles intersect).
- **CCC feasibility:** If d > 4r → third bridging circle can't touch both → LRL/RLR impossible. For long-distance routing (most survey lines), optimal path is guaranteed CSC.
- **CCC paths** only emerge as optimal for close-proximity configurations (tight maneuvers, abort procedures).

### Analytical Solution
For each of 6 types: compute turning circle centers (perpendicular offset ±r from heading), center-to-center distance d and angle α, tangent line heading, arc lengths via angular displacement (modulo 2π for correct rotation direction). Select minimum total length.

---

## 2. Discretization into Geodetic Waypoints

### Dynamic Resolution
- Straight segments: fixed linear interval (e.g., every 5.0 m).
- Circular arcs: fixed angular resolution (e.g., every 1.0°). Linear step = r × Δθ.

### Floating-Point Safety
**Kahan summation mandatory** for parametric arc-length accumulation over thousands of steps. SoA memory layout for cache-coherent SIMD processing.

### Projection Strategy
All Dubins geometry computed in flat Cartesian space (UTM or local ENU). Waypoints back-transformed to WGS84 via **Karney-Krüger 6th-order series** (sub-mm precision). Altitude from DEM via bicubic interpolation (C¹ continuity, prevents pitch chatter at grid cell boundaries).

---

## 3. Clothoid (Euler Spiral) Transitions — C² Smoothing

### The Problem with Raw Dubins
At straight↔arc tangent junctions: curvature jumps instantaneously from κ=0 to κ=1/r. This demands infinite roll rate → physical impossibility → violent oscillations, pitch chatter, destroyed gimbal stability.

**Raw Dubins = C¹ continuous only (tangent continuous). Must upgrade to C² (curvature continuous).**

### Clothoid Mathematics
Curvature varies linearly with arc length: κ(s) = s/A² (where A = scaling parameter).

Aircraft enters transition at κ=0 (wings level), curvature ramps linearly to κ=1/r at the end of the transition zone. Transition length:
```
L_transition = V × (φ_max / ω_max)
```
Where φ_max = target bank angle, ω_max = max roll rate, V = airspeed.

### Fresnel Integral Coordinates
```
x(s) = ∫cos(s²/2A²)ds    y(s) = ∫sin(s²/2A²)ds
```
No closed-form solution. Use **Maclaurin power series** for small-angle aviation transitions:
```
x(s) ≈ s - s⁵/40A⁴ + ...
y(s) ≈ s³/6A² - s⁷/336A⁶ + ...
```

### Geometric Shift
Clothoid insertion displaces the circular arc inward by a perpendicular offset δ ("shift"). The Dubins algorithm must iteratively recalculate shifted circle centers and shortened straight segments to maintain route continuity.

---

## 4. 3D Extension: Dubins Airplane Model

### Extended Kinematics
Add altitude z and pitch angle γ to state vector. Constraint: |γ| ≤ γ_max (max climb/dive angle).

### Decoupled Methodology
1. Project poses onto 2D horizontal plane → compute planar Dubins path length L_2D.
2. Required altitude change: Δh = z_final - z_initial.
3. Maximum possible altitude change: Δh_max = L_2D × tan(γ_max).

### Three Altitude Classes

| Class | Condition | Strategy |
|---|---|---|
| **Low** | |Δh| ≤ Δh_max | Shallow constant flight path angle. Turns become ascending/descending helices. |
| **Medium** | |Δh| slightly > Δh_max | Artificially lengthen horizontal path (increase r or extend straight segments) until L_2D × tan(γ_max) ≥ |Δh|. |
| **High** | |Δh| >> Δh_max | Inject helical holding spirals (loiter circles) at start or end to gain/lose altitude in a tight geographic boundary. |

### Axis Segregation Preference
Climbing during banked turns couples pitch and roll → increased load factor → reduced stall margin → destabilized sensor payload. **Advanced FMS biases altitude changes onto straight segments only.** Level-flight climb/descent, establish altitude, then initiate level banked turn.

---

## 5. Wind Correction

### Straight Segments: Crab Angle
Aircraft must "crab" into wind to maintain ground track. Wind Correction Angle:
```
WCA = arcsin(V_crosswind / V_TAS)
```
FMS dynamically offsets heading at every discrete waypoint by WCA.

### Turning Segments: Ground-Track Turn Radius
At constant bank angle in wind: ground speed fluctuates harmonically (min upwind, max downwind). Turn radius R ∝ V_ground² → produces asymmetric trochoid, not a circle.

To force a **circular ground track** (required for boundary constraints): autopilot must continuously modulate bank angle throughout the turn — shallow upwind, steep downwind.

**Safety critical:** On the downwind leg, required bank angle to maintain the planned radius at inflated ground speed may exceed structural limits or induce accelerated stall.

### Wind-Corrected Minimum Radius
```
r_safe = (V_TAS + V_wind)² / (g × tan(φ_max))
```
Uses worst-case maximum ground speed (full tailwind) at maximum safe bank angle. This expanded radius becomes the baseline input for ALL Dubins CSC/CCC geometric derivations → guarantees the circular ground tracks are flyable without over-banking.

---

## 6. IronTrack Integration Summary

| Layer | Algorithm | Purpose |
|---|---|---|
| Lateral path | 6-type Dubins (CSC + CCC) | Minimum-distance 2D routing between oriented poses |
| Curvature smoothing | Clothoid (Euler spiral) insertion | C² continuity — finite jerk, bounded roll rate |
| Vertical path | Dubins Airplane (decoupled 3D) | Altitude management with axis segregation preference |
| Wind compensation | WCA for straights + expanded r_safe for turns | Inertial ground-track compliance despite atmospheric drift |
| Discretization | Karney-Krüger UTM + Kahan summation + SoA | Sub-mm geodetic waypoints from flat-earth math |
| Terrain following | Bicubic DEM interpolation + B-spline C² fitting | Smooth vertical profile with convex hull collision guarantee |
