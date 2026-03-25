# IronTrack — Testing Guide

## Overview

Tests are the correctness contract for safety-critical aerial survey math.
A centimetre of GSD error or a metre of geodesic drift compounds across
hundreds of flight lines. Every module must have both unit and integration
coverage before a milestone is closed.

---

## File Locations

| Layer | Location | Runs with |
|-------|----------|-----------|
| Unit tests | `#[cfg(test)]` block at bottom of each `.rs` file | `cargo test` |
| Integration tests | `tests/<subsystem>.rs` | `cargo test` |
| Reference fixtures | `tests/fixtures/` | loaded by integration tests |

Integration tests call the public library API (`use irontrack::...`) exactly
as an external caller would. They must not reach into private internals.

---

## Dev Dependencies

```toml
[dev-dependencies]
tempfile = "3"    # temp files for GeoPackage write tests
approx = "0.5"   # assert_abs_diff_eq! / assert_relative_eq! for f64
```

Use `approx` for all floating-point assertions. Never use `==` on f64.
Default epsilon for geodesic results: `1e-9` (sub-nanometre). For GSD/swath
results where sensor tolerances apply: `1e-6`.

---

## Geodesy Tests (`tests/geodesy.rs`, `src/geodesy/`)

### Reference dataset — GeodTest
Karney published a 500 000-point validation set (GeodTest.dat) covering all
geodesic edge cases including nearly-antipodal points, equatorial lines, and
meridional arcs. Download from:

  https://zenodo.org/record/32156

Store a representative 1 000-row subset as `tests/fixtures/geodtest_1k.dat`.
Parse it in `tests/geodesy.rs` and assert each (lat1, lon1, az1) → (lat2, lon2)
inverse result matches to within `1e-9` degrees.

### UTM round-trip
WGS84 → UTM → WGS84 must recover the original coordinate to `1e-9` degrees.
Test across all 60 UTM zones and both hemispheres.

### Geoid undulation spot-checks
Published EGM96 reference values (NIMA TR8350.2, Appendix C):
- 0°N 0°E → N ≈ 17.16 m
- 90°N 0°E → N ≈ 13.61 m
- 51.5°N 0.1°W (London) → N ≈ 47.0 m (± 0.5 m tolerance)

Assert with `approx::assert_abs_diff_eq!(result, expected, epsilon = 0.01)`.

---

## DEM Tests (`tests/dem.rs`, `src/dem/`)

### Synthetic .hgt fixture
Do NOT use real SRTM tiles in tests (large binary files, download dependency).
Instead, generate a minimal synthetic `.hgt` file in the test:
- 1201×1201 grid (SRTM-3 format), all cells = known constant
- Write to `tempfile::NamedTempFile`, parse, assert elevation == constant

### Bilinear interpolation
Test with a hand-crafted 2×2 grid where the exact centre interpolation value
is calculable by hand. Assert to `1e-6` metres.

### Cache behaviour
- Query a tile that is not cached → cache miss, tile fetched and stored
- Query same tile again → cache hit, no I/O
- Verify cache path is inside the `dirs::cache_dir()` hierarchy

---

## Photogrammetry Tests (`tests/photogrammetry.rs`, `src/photogrammetry/`)

### GSD formula
Reference: GSD (cm/px) = (AGL_m × pixel_pitch_mm) / focal_length_mm × 100

Test with known values:
- Phantom 4 Pro: focal=8.8mm, sensor_w=13.2mm, img_w=5472px, AGL=120m
- Expected GSD ≈ 3.29 cm/px (assert within 0.01 cm/px)

### Swath width
swath_w = 2 × AGL × tan(HFOV / 2)

Verify swath width matches independently-calculated value for standard sensors.

### Overlap validation
- `forward_overlap = 0.8`, `side_overlap = 0.7` → valid, no error
- `forward_overlap = 1.1` → `PhotogrammetryError::InvalidOverlap`
- `side_overlap = -0.1` → `PhotogrammetryError::InvalidOverlap`

### Dynamic AGL
Given a terrain profile that rises 50 m over 500 m horizontal distance,
the flight altitude above MSL must increase by 50 m to maintain constant AGL.

---

## GeoPackage Tests (`tests/gpkg.rs`, `src/gpkg/`)

### Binary header encoding
The StandardGeoPackageBinary header is specified byte-for-byte in OGC 12-128r18.
Test every field individually:
- Magic bytes `[0x47, 0x50]`
- Version byte `0x00`
- Flags byte: endianness bit, empty/envelope bits
- SRS ID as little-endian i32
- Envelope (4× f64, little-endian)

Use `byteorder::LittleEndian` to read back the bytes and assert each field.
Reference: `.agents/geopackage.md §2`

### WAL mode pragma
After `gpkg::init::create(path)`, open the file with rusqlite and:
```sql
PRAGMA journal_mode;
-- must return "wal"
```

### R-tree trigger
Insert one geometry row, then:
```sql
SELECT COUNT(*) FROM rtree_<table>_geom;
-- must return 1
```
This verifies all 7 synchronisation triggers are installed and firing.
Reference: `.agents/geopackage.md §4`

### Temp files
Always create GeoPackage files under `tempfile::tempdir()`. Never write to
a fixed path — parallel test runs will collide.

---

## I/O Tests (`tests/io.rs`, `src/io/`)

### GeoJSON structural validity
Serialise a `FeatureCollection` with two point features. Assert:
- Top-level `"type": "FeatureCollection"` key exists
- `"features"` is an array of length 2
- Each feature has `"type": "Feature"`, `"geometry"`, `"properties"`
- `"geometry"."type"` is `"Point"`, coordinates array has length 2

### Coordinate precision
Serialise a coordinate pair with maximum f64 precision, deserialise it back,
assert `approx::assert_relative_eq!` within `1e-9` degrees.
GeoJSON must never truncate coordinates (no rounding to 6 decimal places).

---

## CI/CD Workflow

Target file: `.github/workflows/ci.yml`

```yaml
name: CI
on:
  push:
    branches: ["**"]
  pull_request:

jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install stable toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Format check
        run: cargo fmt --check

      - name: Clippy
        run: cargo clippy -- -D warnings

      - name: Test
        run: cargo test

      - name: Test (release, geodesy regression)
        run: cargo test --release --test geodesy
```

Running geodesy regression tests at `--release` catches optimisation-induced
floating-point behaviour changes that debug builds may hide.

---

## Anti-Patterns

- **Do not mock the filesystem** for DEM cache tests. Use `tempfile` and real
  (synthetic) `.hgt` data. Mocked I/O can hide path separator bugs on Windows.
- **Do not use `==` on f64**. Always use `approx`.
- **Do not hardcode absolute paths** in fixture loading. Use
  `std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/...")`.
- **Do not `unwrap()` in tests**. Use `expect("reason")` so failures are
  identifiable without reading the source.
- **Do not share mutable state between tests**. Rust runs tests in parallel
  by default. Each test must be fully self-contained.

## Fixture Realism
When testing external data parsers (TIFF, GeoPackage):
- **Compression**: Tests MUST include fixtures using production-level compression (e.g., DEFLATE + Floating Point Predictor).
- **Edge Cases**: Always include a "Void Straddle" test case where a query coordinate lies exactly between a valid post and a void sentinel.
- **Geoid Regression**: Geoid undulation tests must include a longitude wrap-around check at ±180° to catch `f32` rounding errors (the **Precision Guard**).
