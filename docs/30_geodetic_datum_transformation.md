# Geodetic Transformation Architectures for US-Domestic Aerial Survey

> **Source document:** 30_Geodetic-Datum-Transformation.docx
>
> **Scope:** Complete extraction — all content is unique to the project. Covers GEOID18 architecture, NAVD88→WGS84 transformation pipeline, binary grid format, biquadratic interpolation, 3DEP/Copernicus terrain fusion, WGS84 Ellipsoidal as canonical pivot datum, and NSRS 2022 modernization.

---

## 1. GEOID18 Architecture

### 1.1 Hybridization Process

GEOID18 is the definitive hybrid geoid model for converting between NAD83(2011) epoch 2010.0 ellipsoidal heights and NAVD88 orthometric heights within CONUS, Puerto Rico, and USVI. Alaska, Hawaii, and Pacific territories use the legacy GEOID12B.

**Step 1 — Gravimetric foundation (xGEOID19B):**
A purely gravimetric 1'×1' gravity grid computed from surface gravity measurements, satellite altimetry-derived marine anomalies, GRAV-D airborne gravity data, and SRTM 3-arcsecond topographic reductions.

**Step 2 — Hybridization (warping to benchmarks):**
The gravimetric base is warped to fit 32,357 GPS-on-Benchmark (GPSBM) observations across CONUS. At each benchmark, a residual is computed:

```
Δ = h_GNSS - H_NAVD88 - N_gravimetric
```

The NGS removes a continental-scale tilt and bias using a 2D planar surface. Remaining residuals are modeled via Least Squares Collocation (LSC) with multiple Gaussian covariance functions. This corrector surface is superimposed on the gravimetric model, forcing GEOID18 to honor the physical survey monuments.

**Outlier rejection thresholds:**
- Relative residual > 2 cm for stations < 5 km apart → excluded
- Relative residual > 3 cm for stations < 10 km apart → excluded
- Relative residual > 5 cm for stations < 50 km apart → excluded

**Result:** GEOID18 achieves a standard deviation of **1.39 cm** relative to the CONUS benchmark network — an 18% improvement over GEOID12B.

### 1.2 Why GEOID18 ≠ EGM2008

| Parameter | GEOID18 | EGM2008 |
|---|---|---|
| Model type | Hybrid (gravimetric + benchmark warping) | Gravimetric (spherical harmonic expansion) |
| Horizontal frame | NAD83(2011) epoch 2010.0 | WGS84 (geocentric) |
| Vertical datum | NAVD88 | Global MSL (WGS84 orthometric) |
| Grid resolution | 1 arc-minute (~2 km) | 5 arc-minutes (~9–10 km) |
| NAVD88 tilt | Retained (~1.5 m cross-country) | Excluded (independent of US leveling) |

**Three sources of discrepancy:**

1. **Datum offset:** NAD83 origin is offset ~2.2 m from Earth's center of mass. GEOID18 inherits this geometric shift. EGM2008 (WGS84-native) does not.
2. **NAVD88 tilt:** NAVD88 was constrained to a single tide gauge (Father Point, Quebec), ignoring sea surface topography. This creates a ~1.5 m cross-country tilt that GEOID18 preserves. EGM2008 is free of this artifact.
3. **Resolution:** EGM2008's ~10 km nodal spacing introduces significant omission errors in mountainous terrain where gravity fluctuates rapidly over short distances.

**Aggregate discrepancy:** 0.5–1.5+ meters across CONUS, worst in the Pacific Northwest and Rocky Mountains.

**IronTrack mandate:** GEOID18 must be used for all US-domestic orthometric↔ellipsoidal conversions. EGM2008 is not a substitute.

---

## 2. The NAVD88 → WGS84 Transformation Pipeline

### 2.1 Step 1: Apply GEOID18 Undulation

Convert from NAVD88 orthometric height to NAD83 ellipsoidal height:

```
h_NAD83 = H_NAVD88 + N_GEOID18
```

where `N_GEOID18` is the biquadratic-interpolated geoid undulation at the coordinate.

**Critical:** This yields a coordinate in NAD83(2011) epoch 2010.0 — NOT WGS84. The 2.2 m geocentric offset and tectonic drift remain unresolved.

### 2.2 Step 2: 14-Parameter Helmert Transformation

The relationship between NAD83 and WGS84 is time-dependent. The North American plate drifts ~2.5 cm/year relative to the global frame.

**14 parameters:** 7 static baseline values at reference epoch `t₀` (3 translations `Tx,Ty,Tz`, 3 rotations `Rx,Ry,Rz`, 1 scale factor `s`) + 7 corresponding velocity rates of change.

**Procedure:**
1. Convert geographic (lat, lon, h_NAD83) to Cartesian ECEF (X, Y, Z).
2. Compute time-dependent parameters:
```
T(t) = T(t₀) + Ṫ × (t - t₀)
R(t) = R(t₀) + Ṙ × (t - t₀)
s(t) = s(t₀) + ṡ × (t - t₀)
```
3. Apply the Helmert matrix:
```
[X']     [Tx]         [1   -Rz   Ry] [X]
[Y'] =   [Ty] + (1+s) [Rz   1   -Rx] [Y]
[Z']WGS84[Tz]         [-Ry  Rx   1 ] [Z]NAD83
```
4. Convert ECEF back to geographic (lat, lon, h_WGS84).

### 2.3 Why You Cannot Skip the Helmert Step

The "NAD83 ≈ WGS84" approximation was true in 1987. Since WGS84 was redefined to align with ITRF in 1994, the frames have diverged due to unmodeled tectonic drift:

- **Horizontal error:** 1.0–2.2 m across CONUS (up to 2.6 m on the Pacific coast)
- **Vertical error:** up to 1 m (> 0.5 m in Alaska)

For survey-grade work, skipping the Helmert transformation invalidates collinearity equations in photogrammetric BBA, corrupts PPK trajectory resolution, violates flight safety margins during terrain-following, and destroys LiDAR swath overlap geometry.

**IronTrack mandate:** The full 14-parameter Helmert transformation is enforced as an absolute mathematical imperative for all 3DEP-derived coordinates.

---

## 3. Binary Grid Format and Rust Implementation

### 3.1 NGS Binary Grid Format

44-byte header followed by packed f32 undulation values.

| Byte Offset | Field | Type | Description |
|---|---|---|---|
| 0x00–0x07 | `glamn` | f64 | Southernmost latitude bound (decimal degrees) |
| 0x08–0x0F | `glomn` | f64 | Westernmost longitude bound (decimal degrees) |
| 0x10–0x17 | `dla` | f64 | Latitude spacing (decimal degrees, typically 1/60°) |
| 0x18–0x1F | `dlo` | f64 | Longitude spacing (decimal degrees) |
| 0x20–0x23 | `nla` | i32 | Number of latitude rows |
| 0x24–0x27 | `nlo` | i32 | Number of longitude columns |
| 0x28–0x2B | `ikind` | i32 | Data type flag (1 = f32) |

Data starts at byte 44. Row-major ordering: South→North, West→East. First value = SW corner. Last value = NE corner.

**CONUS grid (g2018u0.bin):** 4201 columns × 2041 rows = 34,297,008 bytes (~34.3 MB).

File size formula: `44 + (nla × nlo × 4)` bytes.

### 3.2 Endianness Warning

A 2019 NGS compilation error corrupted the Little-Endian versions of the GEOID18 grids, introducing **decimeter-level errors**. The Rust engine must:
- Download the canonical **Big-Endian** binary files.
- Parse using `f32::from_be_bytes()` for architecture-agnostic deserialization.

### 3.3 Biquadratic Interpolation (Mandatory)

NGS mandates biquadratic interpolation for survey-grade precision. Bilinear is rejected.

**Why:** Bilinear interpolation is C⁰ continuous (value continuous, but first derivative discontinuous at grid boundaries). This creates angular "seams" that, when fed into a terrain-following autopilot PID loop, induce artificial step-functions and pitch chatter.

**Biquadratic** fits quadratic polynomials across a 3×3 window of nodes, ensuring C¹ continuity (continuous first derivative) that mirrors the slowly varying equipotential gravity field.

**Algorithm:**
1. Identify which of 4 quadrants around the central node the query point occupies.
2. Select an asymmetric 3×3 matrix of nodes based on the quadrant.
3. For each of 3 latitude rows: fit a quadratic through 3 longitude nodes, evaluate at query longitude.
4. Fit a final quadratic through the 3 interpolated row values, evaluate at query latitude.

### 3.4 Rust Memory Architecture

**Memory-mapped I/O via `memmap2`:** Map the 34.3 MB binary file directly into the process address space. The OS kernel pages sectors from disk into L1/L2 cache on demand — no user-space data copying.

**Concurrent access:** A read-only `Mmap` instance is `Send + Sync` in Rust. It can be shared across unlimited rayon parallel threads without lock contention.

**Byte offset formula for random access:**
```
offset = 44 + ((row × nlo) + col) × 4
```

Each thread slices the 36 bytes (9 × f32) needed for the 3×3 biquadratic window and evaluates the polynomial in parallel.

**Precision:** f32 values from the grid must be cast to **f64 before** polynomial multiplication. Executing biquadratic weights in f32 risks mantissa truncation that accumulates into millimeter-level drift over long survey lines.

**Deployment:** Do not embed via `include_bytes!` (inflates binary by 34 MB). Treat as an immutable sidecar asset — bundled alongside the GeoPackage, fetched as a runtime download, cached in the app data directory, and memory-mapped on daemon initialization.

---

## 4. Multi-Source Terrain Fusion: 3DEP + Copernicus

### 4.1 The Boundary Cliff Problem

- **3DEP:** Orthometric heights in NAVD88, horizontal in NAD83.
- **Copernicus:** Orthometric heights in EGM2008, horizontal in WGS84.

The discrepancy between NAVD88 and EGM2008 can exceed 1 meter. The horizontal shift between NAD83 and WGS84 exceeds 2 meters. Blindly stitching tiles without geodetic reconciliation creates a mathematical cliff at the boundary.

An aircraft crossing this cliff interprets the datum discontinuity as a sudden terrain step. The autopilot attempts an aggressive pitch correction → destabilizes the sensor → smears imagery → destroys side-lap → pushes the airframe toward structural limits.

Compounding factor: Copernicus is a DSM (canopy surface), 3DEP is a DTM (bare earth). The vertical discontinuity at the boundary is datum mismatch PLUS canopy height.

### 4.2 Canonical Pivot Datum: WGS84 Ellipsoidal

**All terrain is transformed to WGS84 Ellipsoidal on ingestion.** This is IronTrack's internal canonical datum.

**Rationale:**
1. The WGS84 ellipsoid is a smooth, closed-form geometric surface with no gravitational anomalies — fast and deterministic for math.
2. Both EGM2008 and GEOID18 are reversible translation layers between their orthometric surfaces and the ellipsoid.
3. The entire internal spatial R-tree, flight waypoints, and GeoPackage microtiles exist in a pure unified space. Rust's type system enforces datum tagging — untagged altitudes cause compilation failures.

**Ingestion pipelines:**

| Source | Conversion Path |
|---|---|
| Copernicus (EGM2008, WGS84) | `h_WGS84 = H_copernicus + N_EGM2008` |
| 3DEP (NAVD88, NAD83) | `h_NAD83 = H_3DEP + N_GEOID18` → then 14-parameter Helmert → `h_WGS84` |

**Export pipeline:** Traverse the conversion graph outward from the WGS84 pivot:
- DJI export: WGS84 → subtract EGM96 N → EGM96 orthometric
- NAVD88 export: WGS84 → inverse Helmert → NAD83 → subtract GEOID18 N → NAVD88
- ArduPilot: WGS84 Ellipsoidal directly

### 4.3 Resolution Mismatch Handling

3DEP: 1 m resolution. Copernicus: 30 m resolution. At the transition zone:
- Use 3DEP wherever available (higher resolution, more accurate vertical datum).
- Fall back to Copernicus only where 3DEP coverage is absent.
- Apply path-smoothing algorithms (B-spline, elastic band) across the transition to prevent abrupt altitude commands.

---

## 5. NSRS 2022 Modernization

### 5.1 What's Being Replaced

| Legacy | Replacement | Key Difference |
|---|---|---|
| NAD83 | **NATRF2022** | Geocentric (aligned to ITRF2020), incorporates Euler Pole parameters for intra-plate tectonic rotation |
| NAVD88 | **NAPGD2022** | Purely gravity-based vertical datum, no single-tide-gauge constraint, no 1.5 m tilt |
| GEOID18 | **GEOID2022** | Purely gravimetric (no benchmark warping), based on GRAV-D airborne gravity. Eliminates accumulated NAVD88 leveling errors. |

### 5.2 Timeline

- **2024–2025:** Beta testing commenced. NATRF2022, NAPGD2022, SPCS2022 released to Beta. NGS NCAT tool in Beta 1.
- **2026:** FGCS vote on formal approval expected mid-2026.
- **Post-approval:** Federal Register Notice → multi-month transition → NAVD88/NAD83 legally deprecated.

### 5.3 IronTrack Architectural Preparations

**1. Epoch-aware coordinate structs:**
NATRF2022 is a dynamic frame modeling intra-plate drift. Coordinates cannot be treated as static. All spatial structs (`Point`, `Waypoint`, `FlightLine`) must include a mandatory `epoch: f64` field. Every transformation module must accept temporal epoch for time-dependent Helmert parameters.

**2. Pluggable GeopotentialModel trait:**
GEOID18 cannot be hardcoded. Abstract the grid-lookup behind a polymorphic Rust trait:

```rust
trait GeopotentialModel: Send + Sync {
    fn undulation(&self, lat_deg: f64, lon_deg: f64) -> Result<f64>;
    fn grid_resolution_arcmin(&self) -> f64;
}
```

This allows hot-swapping GEOID18 → GEOID2022 when the new grids are published, and running parallel legacy/modernized projects during the multi-year industry transition.

**3. Multi-GNSS OPUS 6 compatibility:**
Legacy OPUS-Projects 5 (GPS-only) is being deprecated. The modernized OPUS 6 processes multi-constellation data (GPS + Galileo + GLONASS). IronTrack's trigger event logs and RINEX-adjacent metadata must capture full multi-constellation observables for compatibility.
