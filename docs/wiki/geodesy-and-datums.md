# Geodesy & Datums

IronTrack's geodesy layer provides sub-millimetre coordinate transformations and multi-datum altitude conversion. Every coordinate in the system flows through this layer before storage or export.

## Overview

IronTrack uses WGS84 Ellipsoidal as its canonical internal datum. All terrain data, GNSS observations, and flight plan coordinates are converted to WGS84 Ellipsoidal on ingestion and converted to the target datum only on export. This "hub-and-spoke" design eliminates cumulative errors from chaining orthometric-to-orthometric transforms.

Three geodetic operations underpin everything:

1. **Karney geodesic** — Distance and azimuth between any two points on the ellipsoid.
2. **Karney-Krüger UTM** — Conformal projection for local Cartesian work (corridor offsets, polygon clipping).
3. **Geoid undulation lookup** — Converting between ellipsoidal height (GNSS) and orthometric height (maps/DEMs).

## Karney Geodesic Solver

IronTrack uses the Karney (2013) algorithm exclusively for geodesic distance and azimuth calculations. Haversine (0.3% spherical error) and Vincenty (antipodal convergence failure) are both rejected by project policy.

**Why Karney matters:** Vincenty's iterative method fails to converge for near-antipodal points. In a `rayon` parallel context, a single stalled Vincenty call can cause work-stealing starvation across the thread pool. Karney's Newton root-finding converges in 2–4 iterations for all point pairs globally, with 15-nanometre accuracy.

**Functions available:**

- `geodesic_inverse(a, b)` — Returns distance (metres) and forward/reverse azimuths between two points.
- `geodesic_direct(start, azimuth, distance)` — Returns the destination point and arrival azimuth given a start point, bearing, and distance.

All coordinates are stored internally in radians. Degree conversion happens only at the CLI/IO boundary.

## Karney-Krüger UTM Projection

UTM projection uses a 6th-order Krüger series, not the legacy 4th-order Redfearn method. The coefficients α₁…₆ (forward) and β₁…₆ (inverse) are hard-coded rational fractions from Karney (2011), Table 1.

**Transformation pipeline (forward):**

1. Compute conformal latitude χ from geodetic latitude φ.
2. Derive Gauss-Schreiber transverse Mercator coordinates (ξ', η').
3. Apply the 6th-order Krüger correction series.
4. Scale by the UTM scale factor (k₀ = 0.9996) and apply false easting/northing.

**Round-trip accuracy:** 1×10⁻⁹ degrees (~0.1 mm on the ground), verified across all 60 zones.

**Functions available:**

- `utm_zone(lon_deg)` — Returns the zone number (1–60).
- `wgs84_to_utm(coord)` — Standard forward projection into the natural zone.
- `wgs84_to_utm_in_zone(coord, zone, hemisphere)` — Forces projection into a specific zone (used for corridor mapping that crosses zone boundaries).
- `utm_to_wgs84(utm)` — Inverse projection back to geodetic coordinates.

## Geoid Models

Two geoid models are available for converting between ellipsoidal height (h) and orthometric height (H):

```
h = H + N     (orthometric → ellipsoidal)
H = h − N     (ellipsoidal → orthometric)
```

where N is the geoid undulation (positive when the geoid surface is above the ellipsoid).

### EGM2008 (Primary)

- Accuracy: ~0.5 m RMS globally.
- Backed by the `egm2008` crate (FlightAware, BSD-3-Clause, pure Rust).
- Tide system: Tide-Free (same as Copernicus GLO-30, so no correction is needed).
- Used as the default for all Copernicus DEM operations.

### EGM96 (Legacy)

- Accuracy: ~1 m RMS globally.
- Backed by the `egm96` crate with embedded spherical harmonic coefficients.
- Required for DJI export compatibility (DJI autopilots expect EGM96 altitudes).

### Undulation range

Geoid undulation varies from approximately −105 m (Indian Ocean) to +85 m (Indonesia). Within CONUS the range is roughly −10 m to −40 m.

## Altitude Datum Conversion

The `FlightLine::to_datum()` method converts flight line altitudes between four vertical datums through a two-phase pipeline:

**Phase 1 — Normalize to WGS84 Ellipsoidal:**

| Source Datum | Conversion |
|---|---|
| Wgs84Ellipsoidal | No-op |
| Egm2008 | h = H + N(EGM2008) |
| Egm96 | h = H + N(EGM96) |
| Agl | h = terrain_ellipsoidal + agl_offset |

**Phase 2 — Convert to target datum:**

| Target Datum | Conversion |
|---|---|
| Wgs84Ellipsoidal | No-op |
| Egm2008 | H = h − N(EGM2008) |
| Egm96 | H = h − N(EGM96) |
| Agl | agl = h − terrain_ellipsoidal |

This means an EGM2008 → EGM96 conversion always routes through WGS84 Ellipsoidal, preventing model-difference accumulation.

## PhantomData Datum Safety

All coordinates carry their datum as a compile-time type parameter via Rust's `PhantomData`. The `AltitudeDatum` enum tracks the vertical reference for each `FlightLine`:

- `Egm2008` — Copernicus GLO-30 native.
- `Egm96` — SRTM native, DJI export.
- `Wgs84Ellipsoidal` — GNSS receiver native.
- `Agl` — Above Ground Level (relative to terrain surface).

The compiler rejects mixing coordinates from different datums without an explicit `to_datum()` call. This prevents a class of bugs where altitudes from different reference frames are silently combined.

## Coordinate Types

### GeoCoord

The fundamental geodetic coordinate:

- `lat`, `lon` stored in radians (f64).
- `height` in metres above the WGS84 ellipsoid.
- Constructed via `from_degrees()` or `new()` (radians). Both validate ranges and reject non-finite values.

### FlightLine (Structure-of-Arrays)

Flight lines use SoA layout for cache-friendly bulk processing:

- `lats: Vec<f64>` — Radians.
- `lons: Vec<f64>` — Radians.
- `elevations: Vec<f64>` — Metres, in the datum specified by `altitude_datum`.

The three vectors are always the same length, enforced atomically by the `push()` method. Private fields prevent external code from breaking this invariant.

### WGS84 Constants

- Semi-major axis: a = 6,378,137.0 m
- Inverse flattening: 1/f = 298.257223563
- Derived: semi-minor axis (b), first eccentricity squared (e²), third flattening (n) for the Karney-Krüger series.

---

## Deep Dive: Implementation Notes

### Numerical Stability

- **Kahan summation** is mandatory for all distance and trajectory accumulations. The `KahanSum` type from `math::numerics` tracks a running compensation term to maintain sub-micrometre accuracy over 100+ km flight lines.
- The `fast_math` compiler flag is rejected project-wide because it destroys the Kahan compensation mechanism.

### Geoid Edge Cases

- **±180° longitude wrapping:** The EGM2008 evaluator receives f32 inputs. At extreme longitudes, f64→f32 rounding can push 179.9999999° to 180.0f32. The geoid module wraps this to −180.0 before evaluation.
- **Polar convergence (>85°):** UTM is valid only to ±84°. Coordinates beyond this range return an error; UPS projection is not implemented (not needed for aerial survey).
- **Ocean lookups:** Never rejected. Satellite altimetry marine geoid data is needed for coastal survey work.

### Epoch Management

Tectonic plate drift accumulates ~2.5 cm/year (40 cm over 16 years). IronTrack records `observation_epoch` in GeoPackage metadata but does not perform epoch propagation internally — that's delegated to external tools (HTDP/NCAT) when analytical-grade precision is required.

### Geodetic Area Calculation

IronTrack uses Karney's ellipsoidal polygon method for all area computations (hectares, acres). The Shoelace formula applied to UTM coordinates introduces ~3.5% error for large regions and fails entirely across zone boundaries.

### Reference Documents

- `02_geodesy_foundations.md` — Algorithm selection rationale.
- `05_vertical_datum_engineering.md` — h = H + N derivation, datum stack.
- `30_geodetic_datum_transformation.md` — WGS84 pivot justification, GEOID18 details.
- `31_14param_helmert.md` — NAD83 ↔ WGS84 with epoch management.
- `32_geoid_edge_cases.md` — Polar, antimeridian, and ocean handling.
