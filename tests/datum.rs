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

//! Integration tests for `FlightLine::to_datum` — altitude datum conversions.
//!
//! These tests exercise the safety-critical datum transform pipeline that chains
//! through the WGS84 ellipsoidal pivot. Wrong altitude datums produce wrong
//! flight altitudes — every conversion path must be verified.

use approx::assert_abs_diff_eq;
use tempfile::TempDir;

use irontrack::dem::cache::tile_filename;
use irontrack::dem::TerrainEngine;
use irontrack::geodesy::geoid::{Egm2008Model, GeoidModel};
use irontrack::types::{AltitudeDatum, FlightLine};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a `TerrainEngine` backed by an empty cache directory.
///
/// Suitable for non-AGL tests where the engine parameter is required by
/// `to_datum` but never queried (no AGL path is taken).
fn empty_engine() -> (TempDir, TerrainEngine) {
    let dir = TempDir::new().unwrap();
    let engine =
        TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("build TerrainEngine");
    (dir, engine)
}

/// Build a `FlightLine` from decimal-degree coordinates.
///
/// Converts lat/lon to radians before pushing (FlightLine stores radians
/// internally).
fn flight_line_from_degrees(
    datum: AltitudeDatum,
    points: &[(f64, f64, f64)], // (lat_deg, lon_deg, elevation_m)
) -> FlightLine {
    let mut line = FlightLine::new().with_datum(datum);
    for &(lat_deg, lon_deg, elev) in points {
        line.push(lat_deg.to_radians(), lon_deg.to_radians(), elev);
    }
    line
}

/// Build a DEFLATE-compressed FLOAT32 TIFF with uniform elevation.
///
/// Matches the Copernicus DEM GLO-30 format: DEFLATE compression,
/// FloatingPoint predictor, single-strip, big-endian byte planes.
fn make_flat_f32_tiff(value: f32, rows: u32, cols: u32) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    let rows_n = rows as usize;
    let cols_n = cols as usize;
    let be_bytes = value.to_bits().to_be_bytes();

    // FloatingPoint predictor: separate into byte planes, then delta-encode.
    let mut encoded: Vec<u8> = Vec::with_capacity(rows_n * cols_n * 4);
    for _ in 0..rows_n {
        let mut row: Vec<u8> = Vec::with_capacity(cols_n * 4);
        for &byte in &be_bytes {
            for _ in 0..cols_n {
                row.push(byte);
            }
        }
        for k in (1..row.len()).rev() {
            row[k] = row[k].wrapping_sub(row[k - 1]);
        }
        encoded.extend_from_slice(&row);
    }

    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut enc, &encoded).unwrap();
    let compressed = enc.finish().unwrap();

    // TIFF header + IFD (11 entries).
    const IFD_ENTRIES: u16 = 11;
    let data_offset: u32 = 8 + 2 + IFD_ENTRIES as u32 * 12 + 4;
    let data_bytes = compressed.len() as u32;

    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes());
    buf.extend_from_slice(&IFD_ENTRIES.to_le_bytes());

    let mut entry = |tag: u16, typ: u16, cnt: u32, val: u32| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&cnt.to_le_bytes());
        buf.extend_from_slice(&val.to_le_bytes());
    };
    entry(256, 3, 1, cols); // ImageWidth
    entry(257, 3, 1, rows); // ImageLength
    entry(258, 3, 1, 32); // BitsPerSample
    entry(259, 3, 1, 8); // Compression = DEFLATE
    entry(262, 3, 1, 1); // PhotometricInterpretation
    entry(273, 4, 1, data_offset); // StripOffsets
    entry(277, 3, 1, 1); // SamplesPerPixel
    entry(278, 3, 1, rows); // RowsPerStrip
    entry(279, 4, 1, data_bytes); // StripByteCounts
    entry(317, 3, 1, 3); // Predictor = FloatingPoint
    entry(339, 3, 1, 3); // SampleFormat = IEEE float

    buf.extend_from_slice(&0u32.to_le_bytes()); // Next IFD = 0 (none)
    buf.extend_from_slice(&compressed);
    buf
}

/// Build a `TerrainEngine` backed by flat synthetic tiles at the given
/// orthometric elevation.
///
/// Writes four tiles around the equator/prime-meridian junction so that
/// coordinates near (0.5°, 0.5°) are fully covered.
fn flat_terrain_engine(elevation_m: f32) -> (TempDir, TerrainEngine) {
    let dir = TempDir::new().unwrap();
    let tiff = make_flat_f32_tiff(elevation_m, 10, 10);
    for (tile_lat, tile_lon) in [(0, 0), (0, -1), (-1, 0), (-1, -1)] {
        let filename = tile_filename(tile_lat, tile_lon);
        std::fs::write(dir.path().join(&filename), &tiff).unwrap();
    }
    let engine =
        TerrainEngine::with_cache_dir(dir.path().to_path_buf()).expect("build TerrainEngine");
    (dir, engine)
}

// ---------------------------------------------------------------------------
// 1. Identity transform — to_datum(current_datum) returns unchanged values
// ---------------------------------------------------------------------------

#[test]
fn identity_transform_egm2008_returns_unchanged() {
    let (_dir, engine) = empty_engine();
    let points = [(39.7, -104.9, 1500.0), (47.5, -121.5, 800.0)];
    let line = flight_line_from_degrees(AltitudeDatum::Egm2008, &points);
    let original = line.clone();

    let result = line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("identity transform");

    assert_eq!(result.altitude_datum, AltitudeDatum::Egm2008);
    assert_eq!(result.len(), original.len());
    for i in 0..original.len() {
        assert_eq!(
            result.lats()[i],
            original.lats()[i],
            "lat mismatch at index {i}"
        );
        assert_eq!(
            result.lons()[i],
            original.lons()[i],
            "lon mismatch at index {i}"
        );
        assert_eq!(
            result.elevations()[i],
            original.elevations()[i],
            "elevation mismatch at index {i}"
        );
    }
}

#[test]
fn identity_transform_wgs84_returns_unchanged() {
    let (_dir, engine) = empty_engine();
    let line = flight_line_from_degrees(AltitudeDatum::Wgs84Ellipsoidal, &[(39.7, -104.9, 1500.0)]);

    let result = line
        .to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
        .expect("identity transform");

    assert_eq!(result.altitude_datum, AltitudeDatum::Wgs84Ellipsoidal);
    assert_eq!(result.elevations()[0], 1500.0);
}

#[test]
fn identity_transform_egm96_returns_unchanged() {
    let (_dir, engine) = empty_engine();
    let line = flight_line_from_degrees(AltitudeDatum::Egm96, &[(47.5, -121.5, 900.0)]);

    let result = line
        .to_datum(AltitudeDatum::Egm96, &engine)
        .expect("identity transform");

    assert_eq!(result.altitude_datum, AltitudeDatum::Egm96);
    assert_eq!(result.elevations()[0], 900.0);
}

// ---------------------------------------------------------------------------
// 2. WGS84 Ellipsoidal ↔ EGM2008 round-trip (sub-millimeter)
// ---------------------------------------------------------------------------

#[test]
fn wgs84_to_egm2008_roundtrip_sub_millimeter() {
    let (_dir, engine) = empty_engine();
    let original_h = 1500.0_f64;
    let line = flight_line_from_degrees(
        AltitudeDatum::Wgs84Ellipsoidal,
        &[(47.5, -121.5, original_h)],
    );

    // Forward: WGS84 → EGM2008
    let egm2008_line = line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("WGS84 → EGM2008");
    assert_eq!(egm2008_line.altitude_datum, AltitudeDatum::Egm2008);

    // The orthometric height must differ from the ellipsoidal by the undulation.
    let egm2008 = Egm2008Model::new();
    let expected_n = egm2008.undulation(47.5, -121.5).expect("undulation lookup");
    assert_abs_diff_eq!(
        egm2008_line.elevations()[0],
        original_h - expected_n,
        epsilon = 1e-9
    );

    // Reverse: EGM2008 → WGS84
    let recovered = egm2008_line
        .to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
        .expect("EGM2008 → WGS84");
    assert_eq!(recovered.altitude_datum, AltitudeDatum::Wgs84Ellipsoidal);
    assert_abs_diff_eq!(recovered.elevations()[0], original_h, epsilon = 1e-6);
}

// ---------------------------------------------------------------------------
// 3. WGS84 Ellipsoidal ↔ EGM96 round-trip (sub-millimeter)
// ---------------------------------------------------------------------------

#[test]
fn wgs84_to_egm96_roundtrip_sub_millimeter() {
    let (_dir, engine) = empty_engine();
    let original_h = 1234.567_f64;
    let line = flight_line_from_degrees(
        AltitudeDatum::Wgs84Ellipsoidal,
        &[(39.7, -104.9, original_h)],
    );

    // Forward: WGS84 → EGM96
    let egm96_line = line
        .to_datum(AltitudeDatum::Egm96, &engine)
        .expect("WGS84 → EGM96");
    assert_eq!(egm96_line.altitude_datum, AltitudeDatum::Egm96);

    let egm96 = GeoidModel::new();
    let expected_n = egm96.undulation(39.7, -104.9).expect("undulation lookup");
    assert_abs_diff_eq!(
        egm96_line.elevations()[0],
        original_h - expected_n,
        epsilon = 1e-9
    );

    // Reverse: EGM96 → WGS84
    let recovered = egm96_line
        .to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
        .expect("EGM96 → WGS84");
    assert_abs_diff_eq!(recovered.elevations()[0], original_h, epsilon = 1e-6);
}

// ---------------------------------------------------------------------------
// 4. EGM2008 → EGM96 cross-conversion via WGS84 pivot
// ---------------------------------------------------------------------------

#[test]
fn egm2008_to_egm96_cross_conversion_matches_undulation_delta() {
    let (_dir, engine) = empty_engine();
    let lat = 47.5_f64;
    let lon = -121.5_f64;
    let h_egm2008 = 1500.0_f64;

    let line = flight_line_from_degrees(AltitudeDatum::Egm2008, &[(lat, lon, h_egm2008)]);

    let egm96_line = line
        .to_datum(AltitudeDatum::Egm96, &engine)
        .expect("EGM2008 → EGM96");
    assert_eq!(egm96_line.altitude_datum, AltitudeDatum::Egm96);

    // The cross-conversion chains: H_96 = H_08 + N_2008 - N_96.
    // Equivalently: H_96 - H_08 = N_2008 - N_96 (the undulation delta).
    let egm2008 = Egm2008Model::new();
    let egm96 = GeoidModel::new();
    let n_2008 = egm2008.undulation(lat, lon).expect("EGM2008 undulation");
    let n_96 = egm96.undulation(lat, lon).expect("EGM96 undulation");
    let expected_delta = n_2008 - n_96;

    let actual_delta = egm96_line.elevations()[0] - h_egm2008;
    assert_abs_diff_eq!(actual_delta, expected_delta, epsilon = 1e-9);
}

#[test]
fn egm2008_egm96_roundtrip_via_wgs84_pivot() {
    let (_dir, engine) = empty_engine();
    let h_egm2008 = 1500.0_f64;
    let line = flight_line_from_degrees(AltitudeDatum::Egm2008, &[(47.5, -121.5, h_egm2008)]);

    let egm96_line = line
        .to_datum(AltitudeDatum::Egm96, &engine)
        .expect("EGM2008 → EGM96");
    let recovered = egm96_line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("EGM96 → EGM2008");

    assert_abs_diff_eq!(recovered.elevations()[0], h_egm2008, epsilon = 1e-6);
}

// ---------------------------------------------------------------------------
// 5. AGL conversion — orthometric minus terrain
// ---------------------------------------------------------------------------

#[test]
fn egm2008_to_agl_equals_elevation_minus_terrain() {
    // Flat terrain at 100 m orthometric. Aircraft at 350 m orthometric.
    // Expected AGL ≈ 250 m (exact because the EGM2008 undulation cancels).
    let terrain_ortho_m = 100.0_f32;
    let aircraft_ortho_m = 350.0_f64;
    let (_dir, engine) = flat_terrain_engine(terrain_ortho_m);

    let line = flight_line_from_degrees(AltitudeDatum::Egm2008, &[(0.5, 0.5, aircraft_ortho_m)]);

    let agl_line = line
        .to_datum(AltitudeDatum::Agl, &engine)
        .expect("EGM2008 → AGL");
    assert_eq!(agl_line.altitude_datum, AltitudeDatum::Agl);

    // AGL = h_ellipsoidal - terrain_ellipsoidal
    //     = (H_aircraft + N) - (H_terrain + N) = H_aircraft - H_terrain
    let expected_agl = aircraft_ortho_m - terrain_ortho_m as f64;
    assert_abs_diff_eq!(agl_line.elevations()[0], expected_agl, epsilon = 0.5);
}

#[test]
fn agl_roundtrip_through_wgs84() {
    let terrain_ortho_m = 200.0_f32;
    let aircraft_ortho_m = 500.0_f64;
    let (_dir, engine) = flat_terrain_engine(terrain_ortho_m);

    let line = flight_line_from_degrees(AltitudeDatum::Egm2008, &[(0.5, 0.5, aircraft_ortho_m)]);

    // EGM2008 → AGL → EGM2008
    let agl_line = line
        .to_datum(AltitudeDatum::Agl, &engine)
        .expect("EGM2008 → AGL");
    let recovered = agl_line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("AGL → EGM2008");

    assert_abs_diff_eq!(recovered.elevations()[0], aircraft_ortho_m, epsilon = 0.5);
}

#[test]
fn wgs84_to_agl_conversion() {
    let terrain_ortho_m = 150.0_f32;
    let (_dir, engine) = flat_terrain_engine(terrain_ortho_m);

    // Compute the expected ellipsoidal terrain height to set up a known AGL.
    let egm2008 = Egm2008Model::new();
    let n = egm2008.undulation(0.5, 0.5).expect("undulation");
    let terrain_ellipsoidal = terrain_ortho_m as f64 + n;

    // Aircraft at 100 m above terrain (AGL).
    let aircraft_ellipsoidal = terrain_ellipsoidal + 100.0;
    let line = flight_line_from_degrees(
        AltitudeDatum::Wgs84Ellipsoidal,
        &[(0.5, 0.5, aircraft_ellipsoidal)],
    );

    let agl_line = line
        .to_datum(AltitudeDatum::Agl, &engine)
        .expect("WGS84 → AGL");
    assert_abs_diff_eq!(agl_line.elevations()[0], 100.0, epsilon = 0.5);
}

// ---------------------------------------------------------------------------
// 6. Edge cases — grid boundaries
// ---------------------------------------------------------------------------

#[test]
fn edge_case_near_antimeridian() {
    // lon = 179.5° — near the EGM grid seam at ±180°.
    let (_dir, engine) = empty_engine();
    let h = 500.0_f64;
    let line = flight_line_from_degrees(AltitudeDatum::Wgs84Ellipsoidal, &[(0.0, 179.5, h)]);

    let egm2008_line = line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("WGS84 → EGM2008 near antimeridian");
    let recovered = egm2008_line
        .to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
        .expect("EGM2008 → WGS84 near antimeridian");

    assert_abs_diff_eq!(recovered.elevations()[0], h, epsilon = 1e-6);
}

#[test]
fn edge_case_near_north_pole() {
    // lat = 89° — near the polar singularity. EGM models must handle high latitudes.
    let (_dir, engine) = empty_engine();
    let h = 200.0_f64;
    let line = flight_line_from_degrees(AltitudeDatum::Wgs84Ellipsoidal, &[(89.0, 0.0, h)]);

    let egm2008_line = line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("WGS84 → EGM2008 near pole");
    let recovered = egm2008_line
        .to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
        .expect("EGM2008 → WGS84 near pole");

    assert_abs_diff_eq!(recovered.elevations()[0], h, epsilon = 1e-6);
}

#[test]
fn edge_case_near_south_pole() {
    let (_dir, engine) = empty_engine();
    let h = 2800.0_f64; // Antarctic plateau elevation
    let line = flight_line_from_degrees(AltitudeDatum::Wgs84Ellipsoidal, &[(-89.0, 45.0, h)]);

    let egm96_line = line
        .to_datum(AltitudeDatum::Egm96, &engine)
        .expect("WGS84 → EGM96 near south pole");
    let recovered = egm96_line
        .to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
        .expect("EGM96 → WGS84 near south pole");

    assert_abs_diff_eq!(recovered.elevations()[0], h, epsilon = 1e-6);
}

#[test]
fn edge_case_negative_longitude() {
    // lon = -179.5° — near the antimeridian on the negative side.
    let (_dir, engine) = empty_engine();
    let h = 300.0_f64;
    let line = flight_line_from_degrees(AltitudeDatum::Wgs84Ellipsoidal, &[(10.0, -179.5, h)]);

    let egm2008_line = line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("WGS84 → EGM2008 at lon=-179.5");
    let recovered = egm2008_line
        .to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
        .expect("EGM2008 → WGS84 at lon=-179.5");

    assert_abs_diff_eq!(recovered.elevations()[0], h, epsilon = 1e-6);
}

// ---------------------------------------------------------------------------
// 7. Error case — AGL conversion with missing terrain data
// ---------------------------------------------------------------------------

#[test]
fn agl_conversion_errors_on_missing_tile() {
    // Empty cache → engine.query() returns TileNotFound for valid coordinates.
    let (_dir, engine) = empty_engine();
    let line = flight_line_from_degrees(AltitudeDatum::Wgs84Ellipsoidal, &[(39.7, -104.9, 1500.0)]);

    let result = line.to_datum(AltitudeDatum::Agl, &engine);
    assert!(
        result.is_err(),
        "AGL conversion should fail when no terrain data is available"
    );
}

#[test]
fn agl_source_errors_on_missing_tile() {
    // Converting FROM AGL also requires terrain data.
    let (_dir, engine) = empty_engine();
    let line = flight_line_from_degrees(AltitudeDatum::Agl, &[(39.7, -104.9, 100.0)]);

    let result = line.to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine);
    assert!(
        result.is_err(),
        "AGL → WGS84 should fail when no terrain data is available"
    );
}

// ---------------------------------------------------------------------------
// 8. Determinism — multi-point transform produces identical results
// ---------------------------------------------------------------------------

#[test]
fn multipoint_determinism() {
    let (_dir, engine) = empty_engine();
    let points: Vec<(f64, f64, f64)> = (0..50)
        .map(|i| {
            let lat = 30.0 + i as f64 * 0.5;
            let lon = -120.0 + i as f64 * 0.3;
            let elev = 1000.0 + i as f64 * 10.0;
            (lat, lon, elev)
        })
        .collect();

    let line = flight_line_from_degrees(AltitudeDatum::Wgs84Ellipsoidal, &points);

    // Run the same transform twice.
    let result_a = line
        .clone()
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("first transform");
    let result_b = line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("second transform");

    assert_eq!(result_a.len(), result_b.len());
    for i in 0..result_a.len() {
        assert_eq!(
            result_a.elevations()[i],
            result_b.elevations()[i],
            "non-deterministic result at waypoint {i}"
        );
    }
}

#[test]
fn multipoint_roundtrip_all_recover() {
    let (_dir, engine) = empty_engine();
    let points: Vec<(f64, f64, f64)> = vec![
        (0.0, 0.0, 100.0),
        (47.5, -121.5, 1500.0),
        (39.7, -104.9, 1700.0),
        (-33.9, 18.4, 50.0), // Cape Town
        (35.7, 139.7, 40.0), // Tokyo
    ];
    let line = flight_line_from_degrees(AltitudeDatum::Wgs84Ellipsoidal, &points);
    let original_elevations: Vec<f64> = line.elevations().to_vec();

    // WGS84 → EGM2008 → WGS84
    let converted = line
        .to_datum(AltitudeDatum::Egm2008, &engine)
        .expect("WGS84 → EGM2008");
    let recovered = converted
        .to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
        .expect("EGM2008 → WGS84");

    for i in 0..points.len() {
        assert_abs_diff_eq!(
            recovered.elevations()[i],
            original_elevations[i],
            epsilon = 1e-6
        );
    }
}
