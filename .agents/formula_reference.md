# IronTrack — Complete Formula Reference

> This file contains every mathematical formula referenced across the IronTrack
> documentation suite. The original docs had formulas as embedded images which
> were lost during export. This is the canonical, machine-readable source of truth.
>
> Organized by document section. Place in docs/ alongside the spec documents.

---

## Doc 02: Geodesy Foundations

### §1.1 — WGS84 Reference Ellipsoid

Defining parameters:
```
a = 6_378_137.0 m                     (semi-major axis, equatorial radius)
1/f = 298.257_223_563                  (inverse flattening)
```

Derived constants:
```
f = 1/298.257_223_563                  ≈ 0.003_352_810_664_7
b = a × (1 - f)                       ≈ 6_356_752.314_245_18 m  (semi-minor axis, polar radius)
e² = 2f - f²                          ≈ 0.006_694_379_990_14     (first eccentricity squared)
e'² = e² / (1 - e²)                   ≈ 0.006_739_496_742_28     (second eccentricity squared)
n = (a - b) / (a + b) = f/(2-f)       ≈ 0.001_679_220_386_38     (third flattening)
```

Coordinate notation:
```
(X, Y, Z) = Earth-Centered Earth-Fixed Cartesian
(φ, λ, h) = geodetic latitude, longitude, ellipsoidal height
φ = geodetic latitude
λ = geodetic longitude
h = ellipsoidal height (geometric, from GNSS)
```

### §1.2 — Geoid and Height Systems

Fundamental height relationship:
```
h = H + N

where:
  h = ellipsoidal height (geometric altitude from GNSS)
  H = orthometric height (elevation above MSL, from DEMs like SRTM/3DEP)
  N = geoid undulation (offset from EGM96/EGM2008/GEOID18 models)
```

Global N range: approximately −106 m (Indian Ocean) to +85 m (Indonesia).

### §1.3 — GSD Equation

```
GSD_x = (H_agl × S_w) / (f × I_w)
GSD_y = (H_agl × S_h) / (f × I_h)

where:
  H_agl = flight height above ground level (m)
  S_w / S_h = sensor width / height (mm)
  f = focal length (mm)
  I_w / I_h = image width / height (pixels)
```

Worst-case GSD = max(GSD_x, GSD_y).

### §2.1 — Haversine Formula

Given two points (φ₁, λ₁) and (φ₂, λ₂) in radians, Earth radius R ≈ 6371 km:
```
a = sin²((φ₂ - φ₁)/2) + cos(φ₁) × cos(φ₂) × sin²((λ₂ - λ₁)/2)
c = 2 × atan2(√a, √(1 - a))
d = R × c
```

Error: 0.1% to 0.3% depending on latitude/bearing.

### §2.2 — Vincenty's Inverse Formula

Reduced latitudes:
```
U₁ = atan((1 - f) × tan(φ₁))
U₂ = atan((1 - f) × tan(φ₂))
```

Iterative loop starting with λ₀ = L (difference in longitude):
```
sin σ = √[(cos U₂ × sin λ)² + (cos U₁ × sin U₂ - sin U₁ × cos U₂ × cos λ)²]
cos σ = sin U₁ × sin U₂ + cos U₁ × cos U₂ × cos λ
σ = atan2(sin σ, cos σ)
sin α = (cos U₁ × cos U₂ × sin λ) / sin σ
cos²α = 1 - sin²α
cos 2σₘ = cos σ - (2 × sin U₁ × sin U₂) / cos²α
C = (f/16) × cos²α × [4 + f × (4 - 3 × cos²α)]
λ = L + (1 - C) × f × sin α × [σ + C × sin σ × (cos 2σₘ + C × cos σ × (-1 + 2 × cos²2σₘ))]
```

Iterate until |Δλ| < 10⁻¹² (~0.06 mm). Convergence failure at near-antipodal points.

On convergence, the ellipsoidal distance s:
```
u² = cos²α × (a² - b²) / b²
A = 1 + (u²/16384) × [4096 + u² × (-768 + u² × (320 - 175 × u²))]
B = (u²/1024) × [256 + u² × (-128 + u² × (74 - 47 × u²))]
Δσ = B × sin σ × [cos 2σₘ + (B/4) × (cos σ × (-1 + 2 × cos²2σₘ)
     - (B/6) × cos 2σₘ × (-3 + 4 × sin²σ) × (-3 + 4 × cos²2σₘ))]
s = b × A × (σ - Δσ)
```

### §2.3 — Karney's Algorithm

Newton's method root-finding on the inverse geodesic problem. Series expanded to
6th order of third flattening n. Accuracy: ~15 nanometers. Convergence guaranteed
for all point pairs including exact antipodes. Typically 2-4 iterations.

API (geographiclib-rs):
```
geod.inverse(lat1, lon1, lat2, lon2) → (s12, azi1, azi2)
geod.direct(lat1, lon1, azi1, s12)  → (lat2, lon2)
```

### §2 — Algorithm Comparison Table

| Algorithm  | Earth Model     | Max Error    | Deterministic? | Use                     |
|------------|-----------------|--------------|----------------|-------------------------|
| Haversine  | Sphere          | 0.1–0.3%     | Yes (O(1))     | Rough culling/filtering |
| Vincenty   | Oblate ellipsoid | 0.5 mm      | No (diverges)  | Deprecated              |
| Karney     | Oblate ellipsoid | 15 nm       | Yes (Newton)   | All survey calculations |

### §3.1 — UTM Projection Parameters

```
UTM zones: 60 zones, each 6° of longitude
Zone number: floor((λ° + 180) / 6) + 1
Central meridian: zone × 6 - 183

Scale factor at central meridian: k₀ = 0.9996
False Easting: E₀ = 500_000 m
False Northing: N₀ = 10_000_000 m (southern hemisphere only)
```

Standard lines of true scale: ~180 km either side of central meridian.

### §3.2 — Karney-Krüger Transformation (6th Order)

Third flattening: n = (a - b) / (a + b)

Rectifying radius (8th-order expansion):
```
A₀ = (a / (1 + n)) × (1 + n²/4 + n⁴/64 + n⁶/256)
```

Alpha coefficients (6th order, for forward projection):
```
α₁ = (1/2)n - (2/3)n² + (5/16)n³ + (41/180)n⁴ - (127/288)n⁵ + (7891/37800)n⁶
α₂ = (13/48)n² - (3/5)n³ + (557/1440)n⁴ + (281/630)n⁵ - (1983433/1935360)n⁶
α₃ = (61/240)n³ - (103/140)n⁴ + (15061/26880)n⁵ + (167603/181440)n⁶
α₄ = (49561/161280)n⁴ - (179/168)n⁵ + (6601661/7257600)n⁶
α₅ = (34729/80640)n⁵ - (3418889/1995840)n⁶
α₆ = (212378941/319334400)n⁶
```

Forward transform (φ, λ → E, N):
1. Compute conformal latitude χ from geodetic latitude φ
2. Compute ξ' = asin(sin χ / cosh(cos χ × (λ - λ₀)))
3. Compute η' = atanh(cos χ × sin(λ - λ₀) / √(sin²χ + cos²χ × cos²(λ - λ₀)))
4. Apply series:
```
   ξ = ξ' + Σ(j=1..6) α_j × sin(2j × ξ') × cosh(2j × η')
   η = η' + Σ(j=1..6) α_j × cos(2j × ξ') × sinh(2j × η')
```
5. Final coordinates:
```
   E = E₀ + k₀ × A₀ × η
   N = N₀ + k₀ × A₀ × ξ     (N₀ = 0 for northern hemisphere)
```

### §4 — Flight Line Spacing Equations

```
Baseline (along-track photo interval):
  B = footprint_height × (1 - end_lap/100)
  B = GSD_y × I_h × (1 - OL_forward/100)

Flight spacing (cross-track line separation):
  W = swath_width × (1 - side_lap/100)
  W = GSD_x × I_w × (1 - OL_lateral/100)

where:
  OL_forward = forward overlap percentage (typically 60-80%)
  OL_lateral = lateral overlap percentage (typically 30-60%)
```

### §5 — Rust Performance Patterns

Structure of Arrays (SoA):
```rust
// BAD: Array of Structures — destroys cache locality
struct Point { lat: f64, lon: f64, elevation: f64 }
let points: Vec<Point>;

// GOOD: Structure of Arrays — contiguous f64 blocks for SIMD
struct FlightLine {
    lats: Vec<f64>,
    lons: Vec<f64>,
    elevations: Vec<f64>,
}
```

Kahan compensated summation:
```
sum = 0.0
c = 0.0           (compensation accumulator)
for each value v:
    y = v - c      (compensate for previously lost bits)
    t = sum + y    (sum is large, y is small — low-order bits may be lost)
    c = (t - sum) - y   (recover lost low-order bits)
    sum = t

Error bound: O(ε) independent of array length (vs O(n×ε) for naive sum)
```

---

## Doc 03: Photogrammetry Specification

### Sensor Parameters Table

| Variable      | Symbol | Unit    | Definition                                        |
|---------------|--------|---------|---------------------------------------------------|
| Focal Length  | f      | mm      | Rear nodal point to sensor plane distance         |
| Sensor Width  | S_w    | mm      | Physical CMOS/CCD width                           |
| Sensor Height | S_h    | mm      | Physical CMOS/CCD height                          |
| Image Width   | I_w    | pixels  | Horizontal resolution                             |
| Image Height  | I_h    | pixels  | Vertical resolution                               |
| Pixel Pitch   | p      | μm      | = S_w / I_w × 1000  (single photosite dimension)  |

### Field of View (FOV)

```
FOV_h = 2 × arctan(S_w / (2 × f))     (horizontal, radians)
FOV_v = 2 × arctan(S_h / (2 × f))     (vertical, radians)
```

Reverse-engineer focal length from known FOV:
```
f = S_w / (2 × tan(FOV_h / 2))
```

### Ground Sample Distance (GSD)

```
GSD_x = (H_agl × S_w) / (f × I_w)     (meters/pixel, cross-track)
GSD_y = (H_agl × S_h) / (f × I_h)     (meters/pixel, along-track)
```

Worst-case (binding constraint): GSD = max(GSD_x, GSD_y)

Solve for required AGL given target GSD:
```
H_agl = (GSD_target × f × I_w) / S_w   (using the binding dimension)
```

### Swath Width and Footprint

Static (flat terrain):
```
W_s = 2 × H_agl × tan(FOV_h / 2)      (swath width, perpendicular to flight)
H_f = 2 × H_agl × tan(FOV_v / 2)      (footprint height, along flight)
```

Alternative from GSD:
```
W_s = GSD_x × I_w
H_f = GSD_y × I_h
```

Dynamic (variable terrain):
```
AGL(x) = H_msl - Z_terrain(x)          (actual AGL at position x)
W_worst = 2 × (H_msl - Z_max) × tan(FOV_h / 2)   (narrowest swath over highest terrain)
```

### Angular Momentum and Stabilization

```
L = I × ω

where:
  L = angular momentum vector
  I = moment of inertia tensor
  ω = angular velocity vector

τ = dL/dt

where:
  τ = net external torque
```

Gimbal pseudo-inverse control law:
```
ω_gimbal = J⁺ × τ_command

where:
  J⁺ = pseudo-inverse of the gimbal Jacobian matrix
```

Angular motion blur at image center:
```
δ_angular = f × (ω_residual × t_exposure)

where:
  f = focal length (mm)
  ω_residual = uncompensated angular rate (rad/s)
  t_exposure = shutter open time (s)
```

Example: ω = 5°/s = 0.0873 rad/s, t = 0.002s, f = 80mm
→ δ = 80 × 0.0873 × 0.002 ≈ 0.014 mm = 14 μm
At pixel pitch 4 μm → 3.5 pixels of smear.

### Crab Angle (Wind Correction)

```
WCA = arcsin((V_wind × sin(θ)) / V_tas)

where:
  WCA = wind correction angle (crab angle)
  V_wind = wind speed
  V_tas = true airspeed
  θ = angle between wind direction and course line
```

Effective swath under crab:
```
W_eff = W_s × cos(WCA) + H_f × sin(WCA)    (effective lateral coverage)
H_eff = H_f × cos(WCA) + W_s × sin(WCA)    (effective forward coverage)
```

### Linear Motion Blur (Pixel Smear)

```
d_ground = V_ground × t_exposure        (ground distance during exposure)
B_pixels = d_ground / GSD               (blur in pixels)
```

Expanded form:
```
B_pixels = (V_ground × t_exposure × f) / (H_agl × pixel_pitch)
```

Max shutter speed to limit blur to B_max pixels:
```
t_max = (B_max × GSD) / V_ground
```

Max ground speed for a given shutter speed:
```
V_max = (B_max × GSD) / t_exposure
```

Industry standard: B_pixels < 1.0 (strict) or < 1.5 (poor lighting).

### Forward Motion Compensation (FMC)

Image velocity across focal plane:
```
V_image = V_ground × (f / H_agl)        (mm/s on the sensor)
```

FMC system matches piezo actuator velocity to V_image, reducing relative motion to zero.

### Overlap Geometry

```
Photo interval (forward):
  B = H_f × (1 - OL_forward/100)

Flight line spacing (lateral):
  W = W_s × (1 - OL_lateral/100)
```

Standard requirements: forward overlap 70-80%, lateral overlap 60-70%.

### Smart Overlap Algorithm

Step 1 — Baseline altitude:
```
H_flight = Z_min + (GSD_target × f × max(I_w, I_h)) / max(S_w, S_h)
```

Step 2 — Dynamic line placement (iterative):
```
Given line_i at coordinate y_i, find y_{i+1}:
  1. Estimate y_{i+1} = y_i + W_flat (naive flat spacing)
  2. Query DEM for Z_max between y_i and y_{i+1}
  3. AGL_min = H_flight - Z_max
  4. W_actual = 2 × AGL_min × tan(FOV_h / 2)
  5. spacing_required = W_actual × (1 - OL_lateral/100)
  6. y_{i+1} = y_i + spacing_required
  7. Repeat steps 2-6 until |Δy_{i+1}| < 0.1 m
```

---

## Doc 05: Vertical Datum Engineering

### §2.2 — Tidal Geoid Systems

Relationship between tide systems (Ekman 1989):
```
N_mean = N_nontidal + (1 + k) × 0.198 × (3/2 × sin²φ - 1/2)    (meters)
N_zero = N_nontidal + k × 0.198 × (3/2 × sin²φ - 1/2)           (meters)

where:
  k ≈ 0.3 (Love number for elastic deformation)
  φ = geodetic latitude
```

### §2.3 — Height Relationships

```
h = H + N        (fundamental approximation)
H = h - N        (solving for orthometric height)
N = h - H        (solving for geoid undulation)
```

### §4.1 — GNSS Altitude Conversion

Target ellipsoidal altitude for autopilot:
```
h_target = H_desired_msl + N_local

where:
  H_desired_msl = planned orthometric flight altitude
  N_local = geoid undulation at position (from GEOID18/EGM96)
```

### §4.2 — Barometric (ISA) Altitude

International Standard Atmosphere constants:
```
T₀ = 288.15 K        (15°C at MSL)
P₀ = 1013.25 hPa     (standard MSL pressure)
L  = -0.0065 K/m     (temperature lapse rate)
g  = 9.80665 m/s²    (standard gravity)
R  = 287.053 J/(kg·K) (specific gas constant for dry air)
```

Barometric altitude from pressure (troposphere):
```
h_baro = (T₀ / L) × [1 - (P / P₀)^(R×L / g)]
```

Or equivalently:
```
h_baro = (T₀ / |L|) × [1 - (P / P₀)^(0.190_263)]
```

### §4.3 — Radar Altimetry

```
H_agl = (c × Δt) / 2

where:
  c = speed of light (≈ 3 × 10⁸ m/s)
  Δt = round-trip time of radar pulse
```

---

## Doc 07: Corridor Mapping

### Polyline Offset Curve

For a parametric curve γ(t), the offset curve at distance d:
```
γ_offset(t) = γ(t) + d × N̂(t)

where N̂(t) = unit normal vector at parameter t
```

Miter join length at vertex (angle θ between segments):
```
miter_length = d / sin(θ/2)
```

Miter limit: if miter_length / d > threshold (typically 2.0), truncate to bevel.

### Swath Width (Corridor)

```
W_s = 2 × H_agl × tan(FOV/2)
```

### Catenary Curve (Powerline Sag)

Catenary constant:
```
C = F_horizontal / (w × g)

where:
  F_horizontal = horizontal tension force (N)
  w = cable mass per unit arc length (kg/m)
  g = 9.80665 m/s²
```

Lowest sag point x₀ (between pylons at x_A and x_B):
```
x₀ = (x_A + x_B)/2 - C × arcsinh[(z_B - z_A) / (2 × C × sinh((x_B - x_A)/(2×C)))]
```

Catenary height at any point x:
```
z(x) = C × cosh((x - x₀) / C) + z_offset
```

### Coordinated Turn Physics

Bank angle for a given radius and velocity:
```
tan(φ_bank) = V² / (g × R)

φ_bank = arctan(V² / (g × R))

where:
  V = aircraft velocity (m/s)
  g = 9.80665 m/s²
  R = turn radius (m)
```

Load factor:
```
n = 1 / cos(φ_bank)        (G-forces)
```

Stall speed in a turn:
```
V_stall_turn = V_stall_level × √n
```

Turn rate:
```
ω = V / R = g × tan(φ_bank) / V     (rad/s)
```

Standard rate turn: ω = 3°/s, completing 360° in 2 minutes.

Mapping constraint: bank angle ≤ 15°–20° during data acquisition.
Minimum turn radius: R_min = V² / (g × tan(φ_max))

### Clothoid (Euler Spiral) Transition Curve

Curvature varies linearly with arc length:
```
κ(s) = s / (R × L)

where:
  κ = curvature at arc length s
  R = final radius of circular arc
  L = total length of transition curve
```

Fresnel integrals define the spiral path:
```
x(s) = ∫₀ˢ cos(t²/(2RL)) dt
y(s) = ∫₀ˢ sin(t²/(2RL)) dt
```
