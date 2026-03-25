# Geodesy Implementation Guide

**See also: `docs/formula_reference.md` for every equation in full UTF-8 notation.**

## WGS84 Reference Ellipsoid Constants
All derived from two defining parameters:
- Semi-major axis: a = 6_378_137.0 m
- Inverse flattening: 1/f = 298.257_223_563

Derived constants (compute once, store as module-level `const`):
- f = 1.0 / 298.257_223_563
- b = a * (1.0 - f)                          ≈ 6_356_752.314_245 m
- e² = 2.0 * f - f²                          ≈ 0.006_694_379_990_14
- e'² = e² / (1.0 - e²)                      ≈ 0.006_739_496_742_28
- n = (a - b) / (a + b)  [third flattening]   ≈ 0.001_679_220_386_38

## Karney's Geodesic Algorithm
Use `geographiclib-rs` crate. Key entry points:
```rust
use geographiclib_rs::{Geodesic, DirectGeodesic, InverseGeodesic};

let geod = Geodesic::wgs84();

// Inverse: distance and azimuth between two points
let (distance_m, azi1, azi2) = geod.inverse(lat1, lon1, lat2, lon2);

// Direct: endpoint given start, azimuth, distance
let (lat2, lon2) = geod.direct(lat1, lon1, azimuth_deg, distance_m);
```

Why Karney over alternatives:
- Haversine: spherical assumption → 0.3% error. Disqualified for survey work.
- Vincenty: antipodal convergence failure. In Rayon par_iter, one thread hangs
  for thousands of iterations → work-stealing starvation. Non-deterministic.
- Karney: Newton's method root-finding. Converges in 2-4 iterations for ALL
  point pairs including antipodes. 15-nanometer accuracy. Bounded execution.

## UTM Projection (Karney-Krüger 6th Order)
UTM zones: 60 zones, each 6° longitude. Zone number = floor((lon + 180) / 6) + 1.
Central meridian = zone * 6 - 183.

Scale factor at central meridian: k₀ = 0.9996
False Easting: 500_000 m. False Northing: 10_000_000 m (southern hemisphere only).

For v0.1, use the `proj` crate bindings or implement manually:
```rust
// Using proj crate:
use proj::Proj;
let proj = Proj::new_known_crs("EPSG:4326", "EPSG:32610", None)?; // UTM zone 10N
let (easting, northing) = proj.convert((lon, lat))?;
```

If implementing manually, the Karney-Krüger series uses coefficients α₁..α₆
derived from the third flattening n. Reference: docs/02_geodesy_foundations.md §3.2.

## EGM96 Geoid Undulation
The relationship: h = H + N
- h = ellipsoidal height (GNSS output)
- H = orthometric height (DEM values, physical elevation above MSL)
- N = geoid undulation (from EGM96 model)

The `egm96` crate provides lookup. Alternatively, the EGM96 grid is a
721×1441 array of f32 at 15-arc-minute spacing. Bilinear interpolation
between the 4 surrounding grid nodes.

Global N range: approximately -105m (Indian Ocean) to +85m (Indonesia).
CONUS range: approximately -10m to -40m.

Critical warning from docs/05: if both the planning engine AND the UAV
firmware independently apply geoid corrections, you get a double-correction
error. The engine must document which datum its output altitudes reference.

## Kahan Summation
Required for all trajectory distance accumulations. Naive summation over
10,000+ waypoints accumulates floating-point drift proportional to √n.

```rust
fn kahan_sum(values: &[f64]) -> f64 {
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;
    for &v in values {
        let y = v - compensation;
        let t = sum + y;
        compensation = (t - sum) - y;
        sum = t;
    }
    sum
}
```
Error bound drops from O(n·ε) to O(ε) — independent of array length.

## Coordinate Exchange Protocol
All library-internal APIs (Geodesy, Terrain, Photogrammetry) MUST use the `GeoCoord` struct or slices of radians. Avoid passing raw `f64` degrees except at the CLI/IO boundary.
- **Validation**: Centralize all geographic validation inside `GeoCoord::new` or `GeoCoord::from_degrees`. Downstream engines should trust the type-safety of `GeoCoord`.
- **Units**: `GeoCoord` stores radians. All math remains in radians. Degrees are for humans only.

## Structured Error Propagation
Do NOT "erase" errors by converting them to strings (e.g., `GeodesyError::OutOfBounds` -> `String`). Use `thiserror` to wrap or propagate structured errors so that callers can programmatically react to specific failure modes (e.g., coordinate bounds vs. tile missing).

## The Unit Protocol
- **Library Internal**: Use `GeoCoord` exclusively for all coordinate exchanges. It encapsulates the "Radians vs. Degrees" and "Normalised Longitude" complexity. Avoid passing raw `f64` except at the CLI/IO boundary.
- **Precision**: Never cast coordinates to `f32` unless strictly required by an external FFI/Crate (e.g., `egm2008`). If casting is mandatory, you MUST implement the **Precision Guard** (see AGENTS.md).

## Coordinate Types
Use radians internally for all trig. Provide degree constructors for API surface.

```rust
pub struct Geographic {
    pub lat_rad: f64,  // geodetic latitude
    pub lon_rad: f64,  // geodetic longitude
    pub height_m: f64, // ellipsoidal height (h) unless documented otherwise
}

impl Geographic {
    pub fn from_degrees(lat: f64, lon: f64, h: f64) -> Self {
        Self { lat_rad: lat.to_radians(), lon_rad: lon.to_radians(), height_m: h }
    }
}

pub struct Utm {
    pub easting: f64,
    pub northing: f64,
    pub zone: u8,
    pub is_north: bool,
}
```
