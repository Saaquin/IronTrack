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

// StandardGeoPackageBinary serializer.
//
// Wire format (GeoPackage spec §2.1.3):
//   [0..2]  magic: 0x47 0x50 ("GP")
//   [2]     version: 0x00 (version 1)
//   [3]     flags byte (see below)
//   [4..8]  srs_id: i32 little-endian
//   [8..]   envelope: 4 × f64 LE for 2D (minx, maxx, miny, maxy) = 32 bytes
//   [40..]  WKB geometry payload
//
// Flags byte layout (bit 0 = LSB):
//   bits 7-6: reserved = 00
//   bit  5:   extended type (X) = 0 for standard geometries
//   bit  4:   empty geometry (Y) = 1 if empty
//   bits 3-1: envelope indicator (E):
//               0 = no envelope, 1 = 2D [minx,maxx,miny,maxy] (32 bytes)
//   bit  0:   byte order (B) = 1 for little-endian
//
// For flight lines: B=1, E=1, Y=0, X=0 → flags = 0b00000011 = 0x03

/// IEEE-754 quiet NaN in little-endian byte order.
/// Used for empty geometry WKB coordinates per GeoPackage spec §2.1.3.
pub const QUIET_NAN_LE: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x7F];

/// Encodes a 2D LineString as a StandardGeoPackageBinary blob.
///
/// `coords` is a slice of (longitude, latitude) pairs in decimal degrees.
/// `srs_id` is the EPSG code written into the header (typically 4326).
///
/// Returns the complete GPKG binary blob ready for insertion into a geometry
/// column. The envelope is computed from the coordinate extrema.
pub fn encode_linestring(coords: &[(f64, f64)], srs_id: i32) -> Vec<u8> {
    /*
     * WKB spec §6.1.7: a LineString must have at least 2 points. This is a
     * programming error rather than a recoverable runtime condition, so we
     * assert (not debug_assert) to catch it in release builds as well.
     */
    assert!(
        coords.len() >= 2,
        "LineString requires at least 2 points, got {}",
        coords.len()
    );
    let (minx, maxx, miny, maxy) = compute_envelope(coords);
    encode_blob(srs_id, minx, maxx, miny, maxy, |buf| {
        write_wkb_linestring(buf, coords);
    })
}

/// Encodes a 3D LineStringZ as a StandardGeoPackageBinary blob.
///
/// `coords` is a slice of (longitude, latitude, elevation_msl) triples in
/// decimal degrees and metres. The elevation is written as the WKB Z
/// coordinate and also included in the 3D envelope.
///
/// Flags: B=1 (LE), E=2 (3D envelope = 48 bytes), Y=0, X=0 → 0x05
/// WKB geometry type: 1002 = LineStringZ (ISO 19125-1: 2D + 1000 = Z variant)
pub fn encode_linestring_z(coords: &[(f64, f64, f64)], srs_id: i32) -> Vec<u8> {
    assert!(
        coords.len() >= 2,
        "LineStringZ requires at least 2 points, got {}",
        coords.len()
    );
    let (minx, maxx, miny, maxy, minz, maxz) = compute_envelope_z(coords);

    // Header(8) + 3D envelope(48) + WKB LS body(1+4+4 + n×24)
    let mut buf = Vec::with_capacity(8 + 48 + 9 + coords.len() * 24);
    buf.extend_from_slice(&[0x47, 0x50]); // magic "GP"
    buf.push(0x00); // version
    /*
     * Flags byte: B=1 (LE), E=010 (3D envelope = 6 × f64 = 48 bytes)
     * flags = (E << 1) | B = (2 << 1) | 1 = 0b00000101 = 0x05
     */
    buf.push(0x05);
    buf.extend_from_slice(&srs_id.to_le_bytes());
    // 3D envelope: minx, maxx, miny, maxy, minz, maxz
    buf.extend_from_slice(&minx.to_le_bytes());
    buf.extend_from_slice(&maxx.to_le_bytes());
    buf.extend_from_slice(&miny.to_le_bytes());
    buf.extend_from_slice(&maxy.to_le_bytes());
    buf.extend_from_slice(&minz.to_le_bytes());
    buf.extend_from_slice(&maxz.to_le_bytes());
    // WKB LineStringZ payload
    buf.push(0x01); // byte order: LE
    buf.extend_from_slice(&1002u32.to_le_bytes()); // geometry type = LineStringZ
    buf.extend_from_slice(&(coords.len() as u32).to_le_bytes()); // num_points
    for &(x, y, z) in coords {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.extend_from_slice(&z.to_le_bytes());
    }
    buf
}

/// Encodes an empty Point geometry per GeoPackage spec §2.1.3:
/// flags Y=1, E=0 (no envelope), WKB coordinate = quiet NaN.
pub fn encode_empty_point(srs_id: i32) -> Vec<u8> {
    /*
     * Empty geometry: flags byte sets Y=1, E=0 (no envelope bytes).
     * The WKB payload still contains a Point header with NaN coordinates.
     * Byte layout: magic(2) + version(1) + flags(1) + srs_id(4) + WKB.
     * flags = B=1 (LE), E=0 (no envelope), Y=1 (empty) = 0b00010001 = 0x11
     */
    let mut buf = Vec::with_capacity(4 + 4 + 21);
    buf.extend_from_slice(&[0x47, 0x50]); // magic "GP"
    buf.push(0x00); // version
    buf.push(0x11); // flags: B=1, E=0, Y=1
    buf.extend_from_slice(&srs_id.to_le_bytes());
    // WKB Point with NaN coordinates
    buf.push(0x01); // LE byte order
    buf.extend_from_slice(&1u32.to_le_bytes()); // geometry type = Point
    buf.extend_from_slice(&QUIET_NAN_LE); // x = NaN
    buf.extend_from_slice(&QUIET_NAN_LE); // y = NaN
    buf
}

// ---------------------------------------------------------------------------
// Envelope helpers — used by rtree.rs to register ST_ scalar functions.
// All functions assume the blob has a 2D envelope (flags E=1).
// ---------------------------------------------------------------------------

/// Parse minx from a 2D GPKG blob envelope (bytes 8..16, f64 LE).
pub fn parse_envelope_minx(blob: &[u8]) -> f64 {
    if blob.len() < 16 {
        return f64::NAN;
    }
    f64::from_le_bytes(blob[8..16].try_into().expect("slice length checked above"))
}

/// Parse maxx from a 2D GPKG blob envelope (bytes 16..24, f64 LE).
pub fn parse_envelope_maxx(blob: &[u8]) -> f64 {
    if blob.len() < 24 {
        return f64::NAN;
    }
    f64::from_le_bytes(blob[16..24].try_into().expect("slice length checked above"))
}

/// Parse miny from a 2D GPKG blob envelope (bytes 24..32, f64 LE).
pub fn parse_envelope_miny(blob: &[u8]) -> f64 {
    if blob.len() < 32 {
        return f64::NAN;
    }
    f64::from_le_bytes(blob[24..32].try_into().expect("slice length checked above"))
}

/// Parse maxy from a 2D GPKG blob envelope (bytes 32..40, f64 LE).
pub fn parse_envelope_maxy(blob: &[u8]) -> f64 {
    if blob.len() < 40 {
        return f64::NAN;
    }
    f64::from_le_bytes(blob[32..40].try_into().expect("slice length checked above"))
}

/// Return true if the GPKG blob represents an empty geometry.
/// Checks bit 4 (Y flag) of the flags byte (byte index 3).
pub fn parse_is_empty(blob: &[u8]) -> bool {
    blob.len() < 4 || ((blob[3] >> 4) & 0x01) == 1
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Scan `coords` to find the 3D bounding box (minx, maxx, miny, maxy, minz, maxz).
fn compute_envelope_z(coords: &[(f64, f64, f64)]) -> (f64, f64, f64, f64, f64, f64) {
    let mut minx = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxy = f64::NEG_INFINITY;
    let mut minz = f64::INFINITY;
    let mut maxz = f64::NEG_INFINITY;
    for &(x, y, z) in coords {
        if x < minx {
            minx = x;
        }
        if x > maxx {
            maxx = x;
        }
        if y < miny {
            miny = y;
        }
        if y > maxy {
            maxy = y;
        }
        if z < minz {
            minz = z;
        }
        if z > maxz {
            maxz = z;
        }
    }
    (minx, maxx, miny, maxy, minz, maxz)
}

/// Scan `coords` to find the 2D bounding box.
fn compute_envelope(coords: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let mut minx = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxy = f64::NEG_INFINITY;
    for &(x, y) in coords {
        if x < minx {
            minx = x;
        }
        if x > maxx {
            maxx = x;
        }
        if y < miny {
            miny = y;
        }
        if y > maxy {
            maxy = y;
        }
    }
    (minx, maxx, miny, maxy)
}

/// Build a full GPKG blob: magic + flags + srs_id + 2D envelope + WKB payload.
/// `write_wkb` is a closure that appends the WKB bytes onto `buf`.
fn encode_blob(
    srs_id: i32,
    minx: f64,
    maxx: f64,
    miny: f64,
    maxy: f64,
    write_wkb: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + 32 + 64);
    // Header
    buf.extend_from_slice(&[0x47, 0x50]); // magic "GP"
    buf.push(0x00); // version
    buf.push(0x03); // flags: B=1 (LE), E=1 (2D envelope), Y=0, X=0
    buf.extend_from_slice(&srs_id.to_le_bytes());
    // 2D envelope: minx, maxx, miny, maxy
    buf.extend_from_slice(&minx.to_le_bytes());
    buf.extend_from_slice(&maxx.to_le_bytes());
    buf.extend_from_slice(&miny.to_le_bytes());
    buf.extend_from_slice(&maxy.to_le_bytes());
    // WKB payload
    write_wkb(&mut buf);
    buf
}

/// Append a WKB LineString payload to `buf`.
/// Each coord is (x=longitude, y=latitude) in decimal degrees.
fn write_wkb_linestring(buf: &mut Vec<u8>, coords: &[(f64, f64)]) {
    buf.push(0x01); // byte order: LE
    buf.extend_from_slice(&2u32.to_le_bytes()); // geometry type = LineString
    buf.extend_from_slice(&(coords.len() as u32).to_le_bytes()); // num_points
    for &(x, y) in coords {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /// Helper: minimal valid 2-point LineString blob.
    fn two_point_line() -> Vec<u8> {
        encode_linestring(&[(10.0, 50.0), (20.0, 60.0)], 4326)
    }

    #[test]
    fn binary_header_magic_gp() {
        let blob = two_point_line();
        assert_eq!(blob[0], 0x47, "magic[0] must be 0x47 ('G')");
        assert_eq!(blob[1], 0x50, "magic[1] must be 0x50 ('P')");
    }

    #[test]
    fn binary_version_zero() {
        assert_eq!(two_point_line()[2], 0x00);
    }

    #[test]
    fn binary_flags_le_2d_envelope() {
        // B=1 (LE), E=1 (2D), Y=0, X=0 → 0x03
        assert_eq!(two_point_line()[3], 0x03);
    }

    #[test]
    fn binary_srs_id_4326() {
        let blob = two_point_line();
        let srs_id = i32::from_le_bytes(blob[4..8].try_into().unwrap());
        assert_eq!(srs_id, 4326);
    }

    #[test]
    fn envelope_min_max() {
        // coords: (10,50), (20,60) → minx=10, maxx=20, miny=50, maxy=60
        let blob = encode_linestring(&[(10.0, 50.0), (20.0, 60.0), (15.0, 55.0)], 4326);
        assert_abs_diff_eq!(parse_envelope_minx(&blob), 10.0, epsilon = 1e-12);
        assert_abs_diff_eq!(parse_envelope_maxx(&blob), 20.0, epsilon = 1e-12);
        assert_abs_diff_eq!(parse_envelope_miny(&blob), 50.0, epsilon = 1e-12);
        assert_abs_diff_eq!(parse_envelope_maxy(&blob), 60.0, epsilon = 1e-12);
    }

    #[test]
    fn wkb_geometry_type_is_linestring() {
        let blob = two_point_line();
        // WKB starts at offset 40 (8 header + 32 envelope)
        let wkb_type = u32::from_le_bytes(blob[41..45].try_into().unwrap());
        assert_eq!(wkb_type, 2, "WKB type 2 = LineString");
    }

    #[test]
    fn wkb_num_points() {
        let blob = two_point_line();
        let num_points = u32::from_le_bytes(blob[45..49].try_into().unwrap());
        assert_eq!(num_points, 2);
    }

    #[test]
    fn wkb_first_coordinate_lon_lat_order() {
        // (x=10.0, y=50.0) written as x then y
        let blob = two_point_line();
        let x = f64::from_le_bytes(blob[49..57].try_into().unwrap());
        let y = f64::from_le_bytes(blob[57..65].try_into().unwrap());
        assert_abs_diff_eq!(x, 10.0, epsilon = 1e-12);
        assert_abs_diff_eq!(y, 50.0, epsilon = 1e-12);
    }

    #[test]
    fn is_empty_false_for_linestring() {
        assert!(!parse_is_empty(&two_point_line()));
    }

    #[test]
    fn empty_point_magic_and_flags() {
        let blob = encode_empty_point(4326);
        assert_eq!(blob[0], 0x47);
        assert_eq!(blob[1], 0x50);
        // flags: B=1 (LE), E=0 (no envelope), Y=1 (empty) = 0x11
        assert_eq!(blob[3], 0x11);
    }

    #[test]
    fn is_empty_true_for_empty_point() {
        assert!(parse_is_empty(&encode_empty_point(4326)));
    }

    #[test]
    fn empty_point_nan_coordinates() {
        let blob = encode_empty_point(4326);
        // WKB starts at offset 8 (no envelope): byte_order(1) + type(4) + x(8) + y(8)
        let x_bytes: [u8; 8] = blob[13..21].try_into().unwrap();
        let y_bytes: [u8; 8] = blob[21..29].try_into().unwrap();
        assert_eq!(x_bytes, QUIET_NAN_LE);
        assert_eq!(y_bytes, QUIET_NAN_LE);
    }

    #[test]
    fn total_blob_size_linestring_2pts() {
        // Header(8) + envelope_2D(32) + WKB_LS(1+4+4 + 2*16) = 40 + 41 = 81 bytes
        let blob = two_point_line();
        assert_eq!(blob.len(), 81);
    }

    // -----------------------------------------------------------------------
    // encode_linestring_z tests
    // -----------------------------------------------------------------------

    fn two_point_line_z() -> Vec<u8> {
        encode_linestring_z(&[(10.0, 50.0, 100.0), (20.0, 60.0, 200.0)], 4326)
    }

    #[test]
    fn linestring_z_total_blob_size() {
        // Header(8) + envelope_3D(48) + WKB_LSZ(1+4+4 + 2*24) = 56 + 57 = 113 bytes
        assert_eq!(two_point_line_z().len(), 113);
    }

    #[test]
    fn linestring_z_flags_byte_is_0x05() {
        // B=1 (LE), E=2 (3D envelope = 48 bytes) → (2 << 1) | 1 = 0x05
        assert_eq!(two_point_line_z()[3], 0x05);
    }

    #[test]
    fn linestring_z_wkb_type_is_linestringz() {
        // WKB starts at offset 56 (8 header + 48 envelope); type at bytes 57..61
        let blob = two_point_line_z();
        let wkb_type = u32::from_le_bytes(blob[57..61].try_into().unwrap());
        assert_eq!(wkb_type, 1002, "WKB type 1002 = LineStringZ");
    }

    #[test]
    fn linestring_z_envelope_includes_z_range() {
        // encode_linestring_z(&[(10,50,100),(20,60,200)], 4326)
        // 3D envelope bytes: 8..56 → minx(8..16), maxx(16..24), miny(24..32), maxy(32..40),
        //                            minz(40..48), maxz(48..56)
        let blob = two_point_line_z();
        let minz = f64::from_le_bytes(blob[40..48].try_into().unwrap());
        let maxz = f64::from_le_bytes(blob[48..56].try_into().unwrap());
        assert_abs_diff_eq!(minz, 100.0, epsilon = 1e-12);
        assert_abs_diff_eq!(maxz, 200.0, epsilon = 1e-12);
    }

    #[test]
    fn linestring_z_first_coordinate_xyz() {
        // First coord after WKB header (1+4+4=9 bytes at offset 56): offset 65..89
        let blob = two_point_line_z();
        let x = f64::from_le_bytes(blob[65..73].try_into().unwrap());
        let y = f64::from_le_bytes(blob[73..81].try_into().unwrap());
        let z = f64::from_le_bytes(blob[81..89].try_into().unwrap());
        assert_abs_diff_eq!(x, 10.0, epsilon = 1e-12);
        assert_abs_diff_eq!(y, 50.0, epsilon = 1e-12);
        assert_abs_diff_eq!(z, 100.0, epsilon = 1e-12);
    }

    #[test]
    fn linestring_z_envelope_xy_matches_2d() {
        // The 2D x/y envelope bytes (8..40) must equal those of the equivalent 2D blob
        let blob2d = encode_linestring(&[(10.0, 50.0), (20.0, 60.0)], 4326);
        let blob3d = two_point_line_z();
        assert_eq!(&blob3d[8..40], &blob2d[8..40]);
    }
}
