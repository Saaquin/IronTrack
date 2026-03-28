# Geodesy Implementation Guide

**Source modules:** `src/geodesy/` — `karney.rs`, `utm.rs`, `geoid.rs`
**Datum conversion:** `src/datum.rs`
**Source docs:** 02, 05, 12, 30, 31, 32, 37
**See also:** `docs/formula_reference.md` for every equation in full UTF-8 notation.

## PhantomData Coordinate Type Safety [Docs 37, 51 — MANDATORY]
Do NOT use primitive f64 pairs or generic structs for coordinates.
Use a strongly typed `Coordinate<Datum>` struct with `std::marker::PhantomData`:
```rust
pub struct Wgs84;
pub struct Nad83_2011;
pub struct Egm2008;
pub struct Egm96;
pub struct Navd88;

pub struct Coordinate<D> {
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
    _datum: PhantomData<D>,
}
```
The compiler MUST reject mixing coordinates from different datums without an
explicit transformation function. FlightLine stores `Coordinate<Wgs84>`.
Export functions specify the output datum in the type signature.
This is a safety architecture requirement — not optional.

## WGS84 Reference Ellipsoid Constants
a = 6_378_137.0 m, 1/f = 298.257_223_563. Derived: f, b, e², e'², n (third flattening).

## Canonical Pivot Datum: WGS84 Ellipsoidal [Doc 30]
All internal coordinates in WGS84 Ellipsoidal. Ingestion paths:
- Copernicus (EGM2008): add EGM2008 undulation (bicubic) → WGS84 Ellipsoidal
- 3DEP (NAVD88/NAD83): add GEOID18 undulation (biquadratic) → NAD83 Ellipsoidal → 14-param Helmert → WGS84
Export paths: DJI → subtract EGM96 N. ArduPilot → WGS84 directly.

## Karney's Geodesic Algorithm
`geographiclib-rs` crate. Haversine disqualified (0.3% error). Vincenty disqualified
(antipodal convergence failure → work-stealing starvation in rayon). Karney: 2-4
iterations for ALL point pairs. 15 nm accuracy. [Doc 02]

## Geoid Models — Three Required [Docs 05, 12, 30, 32]
- **EGM2008:** Bicubic 4×4 stencil. Max error 3.1 cm. Bilinear DISQUALIFIED (13.5 cm).
  C¹ continuity eliminates autopilot pitch chatter at grid boundaries.
- **EGM96:** Vendored crate. DJI export only. 15-arcmin, bilinear 1.15 m max error.
- **GEOID18:** Biquadratic 3×3 per NGS INTG. Big-Endian ONLY (LE corrupted 2019 NGS).
  44-byte header, f32 packed, 4201×2041 = 34.3 MB. mmap via memmap2 (Send+Sync).
  EGM2008 is NOT a substitute (>1 m discrepancy in Pacific NW).

## 14-Parameter Helmert (NAD83 ↔ WGS84) [Docs 30, 31]
Published IGS08→NAD83(2011) coefficients, base epoch t₀=1997.0. Skipping introduces
1.0–2.2 m horizontal error. Geographic → ECEF → time-dependent rotation → ECEF → geographic.

## Epoch Management [Doc 31]
~2.5 cm/year tectonic drift. 40 cm over 16 years. Navigational boundary: imperceptible.
Analytical boundary: must resolve via HTDP/NCAT. IronTrack records observation_epoch
in GeoPackage metadata and delegates non-rigid propagation to external tools.

## Kahan Summation [Doc 02]
Mandatory for all trajectory distance accumulations. Prevents floating-point drift
over long flight lines. fast_math is REJECTED — destroys Kahan compensation. [Doc 52]

## Geoid Edge Cases [Doc 32]
- ±180° wrap: ghost columns (duplicate first 3 at end).
- Polar convergence (>85° lat): longitude-independent polynomial.
- Ocean lookups: NEVER reject (satellite altimetry marine data needed for coastal work).

## Geodetic Area [Doc 49]
Karney's ellipsoidal polygon method via geographiclib-rs. Shoelace on UTM has ~3.5%
error for large regions and can't handle zone boundary crossings. All hectare/acre
metrics must use Karney — legally defensible and scientifically exact.
