# Design Note: PhantomData Datum-Tagged Coordinates

**Status:** Proposal for v0.9
**Author:** Claude Code session, 2026-03-28
**Related docs:** 37_aviation_safety_principles.md (section 3.2), 30_geodetic_datum_transformation.md (section 4.2), .agents/geodesy.md

## Problem

A WGS84 ellipsoidal altitude and a NAD83(2011) ellipsoidal altitude differ by
1.0-2.2 metres across CONUS. If code silently adds or compares them, the
resulting shear in flight lines will not be discovered until post-processing.
The current `AltitudeDatum` enum provides runtime tagging, but nothing prevents
a developer from extracting the raw f64 and mixing datums in arithmetic.

## Proposed Pattern

Doc 37 section 3.2 describes a zero-cost compile-time solution using Rust's
`PhantomData`:

```rust
use std::marker::PhantomData;

pub struct Wgs84;
pub struct Nad83_2011;
pub struct Egm2008;
pub struct Egm96;
pub struct Navd88;

pub struct Coordinate<D> {
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
    pub epoch: f64,
    _datum: PhantomData<D>,
}

impl<D> Coordinate<D> {
    pub fn new(lat: f64, lon: f64, alt: f64, epoch: f64) -> Self {
        Self { lat, lon, alt, epoch, _datum: PhantomData }
    }
}
```

Datum conversions are explicit functions that consume one type and produce another:

```rust
pub fn wgs84_to_nad83(coord: &Coordinate<Wgs84>) -> Coordinate<Nad83_2011>;
pub fn nad83_to_wgs84(coord: &Coordinate<Nad83_2011>) -> Coordinate<Wgs84>;
pub fn navd88_to_wgs84(
    coord: &Coordinate<Navd88>,
    geoid: &Geoid18Model,
    helmert: &HelmertParams,
) -> Coordinate<Wgs84>;
```

The compiler rejects mixed-datum arithmetic at zero runtime cost:

```rust
let wgs = Coordinate::<Wgs84>::new(47.0, -122.0, 100.0, 2026.2);
let nad = wgs84_to_nad83(&wgs);
let bad = wgs.alt + nad.alt;  // COMPILE ERROR: different types
```

## Current State (v0.2)

- `GeoCoord { lat: f64, lon: f64, height: f64 }` -- no datum, no epoch
- `FlightLine { lats: Vec<f64>, lons: Vec<f64>, elevations: Vec<f64>, altitude_datum: AltitudeDatum }` -- runtime enum tag
- `AltitudeDatum { Egm2008, Egm96, Wgs84Ellipsoidal, Agl }` -- checked at export boundaries
- ~33 `GeoCoord::from_degrees()` call sites across 6 files
- `FlightLine` used in every module (io, photogrammetry, datum, network, main)

## Migration Cost

Replacing `GeoCoord` with `Coordinate<D>` requires:

1. Every function that accepts a coordinate must become generic over `D`, or
   specify which datum it expects. Functions that are datum-agnostic (e.g.
   geodesic distance) become `fn distance<D>(a: &Coordinate<D>, b: &Coordinate<D>)`
   -- same datum enforced by the type system.

2. `FlightLine` must become `FlightLine<D>` or store `Coordinate<Wgs84>`
   exclusively (with conversions at import/export boundaries).

3. The `AltitudeDatum` enum becomes redundant -- replaced by the type parameter.

4. All 33+ `GeoCoord::from_degrees()` sites need updating.

5. Export functions gain datum awareness:
   ```rust
   pub fn export_dji_kmz(lines: &[FlightLine<Wgs84>]) -> Vec<u8>;
   // Internally: Wgs84 -> Egm96 for DJI altitude format
   ```

6. The `to_datum()` method on FlightLine becomes a type-changing operation:
   ```rust
   impl FlightLine<Wgs84> {
       pub fn to_egm96(&self, geoid: &GeoidModel) -> FlightLine<Egm96>;
   }
   ```

**Estimated touch count:** Every .rs file in the project. This is the largest
possible refactor.

## Why v0.9 is the Right Moment

The v0.9 roadmap includes:

- **Epoch-aware structs** -- every coordinate gets `epoch: f64`
- **GeopotentialModel trait** -- pluggable geoid models
- **14-parameter Helmert** -- NAD83 <-> WGS84 transformation
- **GEOID18** -- NAVD88 <-> NAD83 conversion
- **NAVD88 datum path** -- new AltitudeDatum variant

All of these require touching the core coordinate types. Adding PhantomData
datum markers at the same time means one coordinated migration instead of two
separate ones. The `epoch: f64` field and the `PhantomData<D>` marker are both
additions to the same struct.

## What NOT to Do

- Do not introduce PhantomData in v0.3. The terrain-following work is complete
  and working. A type-system overhaul here would delay the release for no
  user-facing benefit.

- Do not create a parallel `Coordinate<D>` alongside `GeoCoord` and maintain
  both. That doubles the API surface and creates confusion about which to use.

- Do not use PhantomData for altitude-only types (as shown in Doc 37's minimal
  example) as a half-step. The value is in the full coordinate type, not just
  the altitude scalar.

## Open Questions for v0.9 Design

1. Should `Coordinate<D>` be the internal storage type, or should FlightLine
   continue using SoA layout (separate lat/lon/elevation vecs) with a type-level
   datum tag on the container?

2. Should datum-agnostic functions (geodesic distance, UTM projection) be
   generic over `D`, or should they accept a `Coordinate<Wgs84>` specifically
   (since all internal coordinates are WGS84)?

3. How do import parsers (KML, GeoJSON) construct coordinates when the source
   datum is unknown or declared in metadata rather than in the type system?

4. Does the `DatumContext` struct (planned for the NAVD88 path) subsume the
   transformation functions, or do they coexist?

These questions should be resolved in a v0.9 design session before
implementation begins.
