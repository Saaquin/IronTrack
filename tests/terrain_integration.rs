// IronTrack - Open-source flight management and aerial survey planning engine
// Copyright (C) 2026 [Founder Name]
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Full-pipeline integration tests for `TerrainEngine`.
//!
//! Each test creates a synthetic Copernicus-format FLOAT32 TIFF in a temporary
//! directory, constructs a `TerrainEngine` pointed at that directory (with
//! automatic download disabled), and exercises the public API end to end —
//! TIFF parsing + bilinear interpolation + EGM2008 geoid correction.
//!
//! Synthetic tiles are small (10×10 pixels) flat grids, so expected
//! orthometric elevations are exact. Geoid spot-checks use published reference
//! values with a 2 m tolerance to cover model accuracy.

use approx::assert_abs_diff_eq;
use irontrack::dem::TerrainEngine;
use irontrack::error::DemError;
use irontrack::geodesy::geoid::geoid_undulation_egm2008;

// ---------------------------------------------------------------------------
// Helper: write a synthetic Copernicus-format FLOAT32 TIFF
// ---------------------------------------------------------------------------

/*
 * Minimal valid TIFF layout (little-endian, no compression, single strip):
 *   Header : 8 bytes  (magic + IFD offset)
 *   IFD    : 2 + 10×12 + 4 = 126 bytes  (starts at offset 8)
 *   Data   : rows×cols×4 bytes           (starts at offset 134)
 *
 * 10 IFD entries cover all fields required to decode a FLOAT32 image:
 *   ImageWidth, ImageLength, BitsPerSample, Compression,
 *   PhotometricInterpretation, StripOffsets, SamplesPerPixel,
 *   RowsPerStrip, StripByteCounts, SampleFormat.
 */
fn make_flat_f32_tiff(value: f32, rows: u32, cols: u32) -> Vec<u8> {
    let pixel_count = (rows * cols) as usize;
    let data_bytes = (pixel_count * 4) as u32;
    const IFD_ENTRIES: u16 = 10;
    let data_offset: u32 = 8 + 2 + IFD_ENTRIES as u32 * 12 + 4;

    let mut buf: Vec<u8> = Vec::new();

    // TIFF header
    buf.extend_from_slice(b"II"); // little-endian
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes()); // IFD at offset 8

    // IFD entry count
    buf.extend_from_slice(&IFD_ENTRIES.to_le_bytes());

    let mut entry = |tag: u16, typ: u16, cnt: u32, val: u32| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&cnt.to_le_bytes());
        buf.extend_from_slice(&val.to_le_bytes());
    };

    entry(256, 3, 1, cols);        // ImageWidth (SHORT)
    entry(257, 3, 1, rows);        // ImageLength (SHORT)
    entry(258, 3, 1, 32);          // BitsPerSample = 32
    entry(259, 3, 1, 1);           // Compression = none
    entry(262, 3, 1, 1);           // PhotometricInterpretation
    entry(273, 4, 1, data_offset); // StripOffsets (LONG)
    entry(277, 3, 1, 1);           // SamplesPerPixel = 1
    entry(278, 3, 1, rows);        // RowsPerStrip
    entry(279, 4, 1, data_bytes);  // StripByteCounts (LONG)
    entry(339, 3, 1, 3);           // SampleFormat = 3 (IEEE float)

    // Next IFD offset (0 = end of chain)
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Pixel data — all the same value
    let bytes = value.to_le_bytes();
    for _ in 0..pixel_count {
        buf.extend_from_slice(&bytes);
    }

    buf
}

/// Write a 10×10 flat FLOAT32 TIFF tile at the correct cache filename.
fn write_flat_tile(dir: &std::path::Path, lat: i32, lon: i32, value: f32) {
    use irontrack::dem::cache::tile_filename;
    let path = dir.join(tile_filename(lat, lon));
    let data = make_flat_f32_tiff(value, 10, 10);
    std::fs::write(&path, data).expect("write synthetic tile");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Flat tile at 500 m — orthometric elevation must come back as exactly 500 m.
#[test]
fn terrain_elevation_flat_tile_returns_correct_height() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_flat_tile(dir.path(), 47, -122, 500.0);

    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    let h_ortho = engine
        .terrain_elevation(47.5, -121.5)
        .expect("terrain_elevation");
    assert_abs_diff_eq!(h_ortho, 500.0, epsilon = 1e-4);
}

/// Ellipsoidal elevation must equal H + N where N is the EGM2008 undulation.
/// Both `TerrainEngine` and the geoid module use the same `Egm2008Model`, so
/// the values must agree to floating-point precision.
#[test]
fn ellipsoidal_elevation_equals_ortho_plus_geoid_undulation() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_flat_tile(dir.path(), 47, -122, 500.0);

    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    let (lat, lon) = (47.5_f64, -121.5_f64);
    let h_ortho = engine.terrain_elevation(lat, lon).expect("h_ortho");
    let h_ellipsoid = engine
        .ellipsoidal_terrain_elevation(lat, lon)
        .expect("h_ellipsoid");

    let n = geoid_undulation_egm2008(lat, lon).expect("geoid undulation");
    assert_abs_diff_eq!(h_ellipsoid, h_ortho + n, epsilon = 1e-6);
}

/// Spot-check against a published EGM2008 reference near the origin.
///
/// At 0°N 0°E the EGM2008 undulation is close to the EGM96 value of +17.16 m.
/// A flat tile at 500 m ortho therefore yields h_ellipsoid ≈ 517 m.
/// Tolerance is 2 m to cover both model accuracy and rounding in published
/// reference tables.
#[test]
fn ellipsoidal_elevation_spot_check_origin() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Tile N00E000 covers lat [0, 1), lon [0, 1).
    write_flat_tile(dir.path(), 0, 0, 500.0);

    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    let h_ellipsoid = engine
        .ellipsoidal_terrain_elevation(0.5, 0.5)
        .expect("ellipsoidal_terrain_elevation");

    /*
     * EGM2008 undulation at 0°N 0°E is approximately +17.2 m (close to the
     * EGM96 value of +17.16 m from NIMA TR8350.2 Appendix C). Expected
     * h = 500 + 17.2 ≈ 517.2 m. Tolerance 2 m covers both model differences
     * and spherical harmonic truncation error.
     */
    assert_abs_diff_eq!(h_ellipsoid, 517.2, epsilon = 2.0);
}

/// Void post (Copernicus no-data sentinel −32767.0) must propagate as
/// `DemError::NoData` in strict mode (`terrain_elevation`), not as a raw
/// −32767 m elevation. `NoData` signals ocean/water-body/void data rather
/// than a missing tile file — `TileNotFound` would mislead the caller into
/// thinking the tile needs downloading.
#[test]
fn void_post_propagates_as_no_data() {
    use irontrack::dem::cache::tile_filename;

    let dir = tempfile::tempdir().expect("tempdir");

    /*
     * Build a 10×10 FLOAT32 TIFF where all pixels are 300 m except the post
     * at row 5, col 5, which is the Copernicus void sentinel (−32767.0).
     *
     * We query at lat=0.5, lon=0.5 in the N00E000 tile:
     *   r = (0 + 1 - 0.5) * 10 = 5.0  → r0 = 5
     *   c = (0.5 - 0) * 10     = 5.0  → c0 = 5
     * The bilinear kernel has r0=5, c0=5 as one corner, which contains the
     * void — so None propagates and TileNotFound is returned.
     */
    const ROWS: usize = 10;
    const COLS: usize = 10;
    let void_sentinel: f32 = -32767.0;
    let mut elevations: Vec<f32> = vec![300.0; ROWS * COLS];
    elevations[5 * COLS + 5] = void_sentinel;

    let data_bytes = (ROWS * COLS * 4) as u32;
    let data_offset: u32 = 8 + 2 + 10 * 12 + 4;

    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes());
    buf.extend_from_slice(&10u16.to_le_bytes());

    let mut entry = |tag: u16, typ: u16, cnt: u32, val: u32| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&cnt.to_le_bytes());
        buf.extend_from_slice(&val.to_le_bytes());
    };
    entry(256, 3, 1, COLS as u32);
    entry(257, 3, 1, ROWS as u32);
    entry(258, 3, 1, 32);
    entry(259, 3, 1, 1);
    entry(262, 3, 1, 1);
    entry(273, 4, 1, data_offset);
    entry(277, 3, 1, 1);
    entry(278, 3, 1, ROWS as u32);
    entry(279, 4, 1, data_bytes);
    entry(339, 3, 1, 3);
    buf.extend_from_slice(&0u32.to_le_bytes());

    for v in &elevations {
        buf.extend_from_slice(&v.to_le_bytes());
    }

    let path = dir.path().join(tile_filename(0, 0));
    std::fs::write(&path, &buf).expect("write tile with void");

    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    let err = engine
        .terrain_elevation(0.5, 0.5)
        .expect_err("void post must not return Ok");
    assert!(
        matches!(err, DemError::NoData(_)),
        "expected NoData for void post (not TileNotFound), got {err:?}"
    );
}

/// Void post via `TerrainEngine::query()` must return `OceanFallback` with
/// H=0 and real EGM2008 geoid undulation — the same behaviour as an ocean
/// tile (S3 HTTP 404). This is the safe coastal-operations path.
#[test]
fn void_post_via_query_returns_ocean_fallback() {
    use irontrack::dem::ElevationSource;

    let dir = tempfile::tempdir().expect("tempdir");

    /*
     * Same void-post tile as void_post_propagates_as_no_data above:
     * 10×10 grid, all 300 m except post [5,5] = −32767.0.
     */
    use irontrack::dem::cache::tile_filename;
    const ROWS: usize = 10;
    const COLS: usize = 10;
    let mut elevations: Vec<f32> = vec![300.0; ROWS * COLS];
    elevations[5 * COLS + 5] = -32767.0;

    let data_bytes = (ROWS * COLS * 4) as u32;
    let data_offset: u32 = 8 + 2 + 10 * 12 + 4;

    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes());
    buf.extend_from_slice(&10u16.to_le_bytes());

    let mut entry = |tag: u16, typ: u16, cnt: u32, val: u32| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&cnt.to_le_bytes());
        buf.extend_from_slice(&val.to_le_bytes());
    };
    entry(256, 3, 1, COLS as u32);
    entry(257, 3, 1, ROWS as u32);
    entry(258, 3, 1, 32);
    entry(259, 3, 1, 1);
    entry(262, 3, 1, 1);
    entry(273, 4, 1, data_offset);
    entry(277, 3, 1, 1);
    entry(278, 3, 1, ROWS as u32);
    entry(279, 4, 1, data_bytes);
    entry(339, 3, 1, 3);
    buf.extend_from_slice(&0u32.to_le_bytes());
    for v in &elevations {
        buf.extend_from_slice(&v.to_le_bytes());
    }

    let path = dir.path().join(tile_filename(0, 0));
    std::fs::write(&path, &buf).expect("write tile with void");

    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    /*
     * query() catches NoData → OceanFallback: orthometric = 0, geoid
     * undulation from EGM2008 still applied (coordinate is valid).
     */
    let report = engine
        .query(0.5, 0.5)
        .expect("query over void post must not error");
    assert_eq!(
        report.source,
        ElevationSource::OceanFallback,
        "void post via query() must give OceanFallback, got {:?}",
        report.source
    );
    assert_abs_diff_eq!(report.orthometric_m, 0.0, epsilon = 1e-9);
    // Geoid undulation is real (not zero) — coordinate is valid.
    assert!(
        report.geoid_undulation_m.abs() > 1.0,
        "expected non-zero EGM2008 undulation at (0.5°, 0.5°), got {}",
        report.geoid_undulation_m
    );
}

/// Missing tile must return `DemError::TileNotFound` with an actionable
/// message that includes the Copernicus S3 download URL.
#[test]
fn missing_tile_returns_tile_not_found_with_download_url() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Do NOT write any tile — the cache is empty.

    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    let err = engine
        .terrain_elevation(47.5, -121.5)
        .expect_err("missing tile should return an error");
    assert!(
        matches!(err, DemError::TileNotFound(_)),
        "expected TileNotFound, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("copernicus-dem-30m.s3.amazonaws.com"),
        "error must contain Copernicus download URL: {msg}"
    );
    assert!(
        msg.contains("N47") && msg.contains("W122"),
        "error must name the missing tile: {msg}"
    );
}

/// Negative elevation tile (below sea level, e.g. Dead Sea ≈ −430 m).
/// Orthometric heights can be negative; the engine must not clamp them.
#[test]
fn negative_elevation_preserved() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_flat_tile(dir.path(), 31, 35, -430.0);

    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    let h_ortho = engine
        .terrain_elevation(31.5, 35.5)
        .expect("negative elevation");
    assert_abs_diff_eq!(h_ortho, -430.0, epsilon = 1e-4);
}

/// Coordinates with |lat| > 90° are outside valid WGS84 bounds.
/// `TerrainEngine::query` must return `SeaLevelFallback` with all heights
/// reported as 0 m (geoid model cannot evaluate at invalid coordinates).
#[test]
fn query_outside_wgs84_bounds_returns_sea_level_fallback() {
    use irontrack::dem::ElevationSource;

    let dir = tempfile::tempdir().expect("tempdir");
    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    let report = engine
        .query(91.0, 25.0)
        .expect("query with |lat|>90 must succeed with fallback");

    assert_eq!(
        report.source,
        ElevationSource::SeaLevelFallback,
        "expected SeaLevelFallback for lat=91°"
    );
    assert_abs_diff_eq!(report.orthometric_m, 0.0, epsilon = 1e-9);
    assert_abs_diff_eq!(report.geoid_undulation_m, 0.0, epsilon = 1e-9);
    assert_abs_diff_eq!(report.ellipsoidal_m, 0.0, epsilon = 1e-9);
}

/// Bulk elevation query via `elevations_at` must return orthometric heights
/// identical to individual `terrain_elevation` calls for the same coordinates.
#[test]
fn elevations_at_bulk_matches_individual_queries() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_flat_tile(dir.path(), 47, -122, 300.0);
    write_flat_tile(dir.path(), 31, 35, 200.0);

    let engine = TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("engine");

    let lats = [47.1_f64, 47.5, 47.9, 31.2, 31.7];
    let lons = [-121.9_f64, -121.5, -121.1, 35.2, 35.8];

    let bulk = engine
        .elevations_at(&lats, &lons)
        .expect("bulk elevation query");

    for (i, (&lat, &lon)) in lats.iter().zip(lons.iter()).enumerate() {
        let single = engine
            .terrain_elevation(lat, lon)
            .expect("single elevation query");
        assert_abs_diff_eq!(bulk[i], single, epsilon = 1e-6);
    }
}
