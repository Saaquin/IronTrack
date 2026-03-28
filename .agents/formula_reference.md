# IronTrack Formula Reference

> Comprehensive equation catalog from research documents.
> Covers docs 02–07, 30, 32, 34, 38, 39, 46, 49, 50 in UTF-8 notation.
> See also: `formula_reference_08_10.md` for docs 08–10.

## NOTE: This file extends the existing formula_reference.md
The original file (docs 02-07, 30, 32, 34) remains valid. The following
sections ADD equations from the newly processed Docs 36-52.

---

## Doc 38 — Aerodynamic Power and Energy

### Hover Power (Momentum Theory)
P_hover = (m × g)^1.5 / √(2 × ρ × A)

### Fixed-Wing Drag Polar
C_D = C_D0 + C_L² / (π × e × AR)

### Power Required (Level Flight)
P = 0.5 × ρ × V³ × S × C_D0 + (2 × W²) / (ρ × V × π × b² × e)

### Propeller Advance Ratio
J = V_TAS / (n × D)
Ct(J) = a₀ + a₁J + a₂J² + a₃J³
η = J × Ct / Cp

### Peukert Effect
t_actual = t_rated × (C_rated / I_actual)^k    [k ≈ 1.05 for LiPo]

### Voltage Sag
V_terminal = V_oc - I × R_internal - V_polarization

### Per-Segment Energy
E_line = P(V_TAS) × d / V_GS
E_turn: load factor n = 1/cos(φ), induced drag ∝ n²
E_climb = m × g × V_z (additive)
E_regen ≈ 0.067 × E_total (fixed-wing descent)
E_total = Σ(E_line + E_turn + E_climb - E_regen)   [Kahan summed]
Feasible: E_total_adjusted ≤ 0.80 × E_battery_nominal

---

## Doc 39 — Flight Path Optimization

### ATSP Objective
min Σ c_ij × x_ij    subject to assignment + DFJ subtour elimination

### Boustrophedon Detection
If azimuth_variance(all_lines) = 0 AND adjacent distances ≤ min_turn_radius → O(n log n) sort

### XTE (Perpendicular Vector Distance)
XTE = |((P - A) × (B - A))| / |B - A|    [sign = drift direction L/R]

---

## Doc 46 — Post-Flight QC

### GSD (Terrain-Adjusted)
GSD = (AGL × sensor_width_mm) / (focal_length_mm × resolution_x)

### Forward Overlap
overlap_fwd = 1 - (trigger_spacing / footprint_length)

### Footprint Dimensions
footprint_length = GSD × resolution_y
footprint_width = GSD × resolution_x

### 6DOF Rotation Matrix (Euler → DCM)
R = Rz(κ) × Ry(φ) × Rx(ω)
R_total = R_boresight × R_imu_to_nav

---

## Doc 49 — Computational Geometry

### Shoelace Area (Planar Only)
A = 0.5 × |Σ(x_i × y_{i+1} - x_{i+1} × y_i)|

### Karney Ellipsoidal Area
Computes area directly on WGS84 oblate ellipsoid via geographiclib-rs.
No projection distortion. No zone boundary problem. Sub-meter accuracy at any scale.

### Rotating Calipers MBR
Freeman-Shapira theorem: MBR has at least one side collinear with a convex hull edge.
O(n) after O(n log n) hull. Pure vector algebra (dot products, cross products — no trig).

---

## Doc 50 — Dubins Paths

### Dubins Kinematics
ẋ = V cos(θ),  ẏ = V sin(θ),  θ̇ = u,  |u| ≤ u_max
r_min = V / u_max

### Turn Circle Centers
Right: (x + r sin(θ), y - r cos(θ))
Left:  (x - r sin(θ), y + r cos(θ))

### Inner Tangent Feasibility
L_s = √(d² - 4r²)    [requires d ≥ 2r]

### CCC Feasibility (Law of Cosines)
cos(α) = d² / (8r²) - 1    [requires d ≤ 4r]

### Clothoid Curvature
κ(s) = s / A²
L_transition = V × (φ_max / ω_max)

### Fresnel Integrals (Maclaurin)
x(s) ≈ s - s⁵/40A⁴ + ...
y(s) ≈ s³/6A² - s⁷/336A⁶ + ...

### Wind Correction Angle
WCA = arcsin(V_crosswind / V_TAS)

### Wind-Corrected Turn Radius
r_safe = (V_TAS + V_wind)² / (g × tan(φ_max))
