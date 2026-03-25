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

//! Copernicus DEM GLO-30 tile: FLOAT32 TIFF parser and bilinear interpolator.
//!
//! ## Format
//!
//! Each tile is a Cloud Optimized GeoTIFF (COG) containing IEEE FLOAT32
//! samples. Tiles cover exactly 1°×1° (latitude × longitude). The void
//! sentinel is −32767.0 (distinct from i16::MIN = −32768 used by SRTM).
//! Some pixels carry NaN for water bodies or void regions; both are treated
//! as absent data.
//!
//! ## Tile dimensions
//!
//! Rows are always 3600. Columns narrow at high latitudes to maintain
//! approximately equal physical tile widths on the ground:
//!
//! | Abs latitude band | Columns |
//! |-------------------|---------|
//! | 0° – 49°          | 3600    |
//! | 50° – 59°         | 2400    |
//! | 60° – 69°         | 1800    |
//! | 70° – 79°         | 1200    |
//! | 80° – 84°         | 720     |
//! | 85° – 89°         | 360     |
//!
//! Source: `docs/10_Rust_COG_Ingestion.md §Tile geometry`.
//!
//! ## Vertical datum
//!
//! Copernicus DEM stores orthometric heights above the **EGM2008** geoid.
//! Use [`Egm2008Model`] for h = H + N conversions, not EGM96.
//!
//! ## DSM vs DTM
//!
//! Copernicus DEM is a **Digital Surface Model** (DSM), not a bare-earth DTM.
//! X-band radar scatters from the top of vegetation canopy, producing heights
//! 2–8 m above the actual ground surface depending on forest type. AGL
//! clearance calculations over forested terrain must account for this bias.
//! See `docs/09_DSMvDTM_AGL_Flight_Generation.md` for details.

use std::io::{Read, Seek};
use std::path::Path;

use crate::error::DemError;

// ---------------------------------------------------------------------------
// Latitude band → column count table
// ---------------------------------------------------------------------------

/// Return the expected column count for a Copernicus GLO-30 tile whose
/// SW corner is at integer latitude `tile_lat`.
///
/// Rows are always 3600. The actual `dim_cols` used for bilinear interpolation
/// is read from the TIFF header via `decoder.dimensions()` — this table is
/// provided for documentation and external tooling (e.g. download size
/// estimation). A tile with unexpected dimensions is not rejected by
/// `from_reader`; it will interpolate on whatever grid the TIFF declares.
pub fn col_count_for_lat(tile_lat: i32) -> usize {
    match tile_lat.unsigned_abs() {
        0..=49 => 3600,
        50..=59 => 2400,
        60..=69 => 1800,
        70..=79 => 1200,
        80..=84 => 720,
        85..=89 => 360,
        /*
         * Tiles at or beyond 90° do not exist. Return 360 as a safe lower
         * bound; tile-availability on S3 drives actual coverage, not this
         * table.
         */
        _ => 360,
    }
}

// ---------------------------------------------------------------------------
// CopernicusTile
// ---------------------------------------------------------------------------

/// Parsed Copernicus DEM GLO-30 tile.
///
/// Holds the full FLOAT32 elevation grid for one 1°×1° tile, plus the
/// SW-corner tile coordinates needed for the bilinear interpolation formula.
#[derive(Debug)]
pub struct CopernicusTile {
    /// Elevation data, row-major, north→south, west→east.
    /// Length = `dim_rows × dim_cols`. Values are metres above EGM2008.
    elevations: Vec<f32>,
    dim_rows: usize,
    dim_cols: usize,
    tile_lat: i32,
    tile_lon: i32,
}

impl CopernicusTile {
    /// Load and parse a Copernicus tile from a `.tif` file on disk.
    ///
    /// # Errors
    ///
    /// - [`DemError::Io`] — file cannot be opened.
    /// - [`DemError::Parse`] — TIFF is malformed, not FLOAT32, or dimensions
    ///   do not match the pixel count implied by the header.
    pub fn from_file(path: &Path, tile_lat: i32, tile_lon: i32) -> Result<Self, DemError> {
        let file = std::fs::File::open(path).map_err(DemError::Io)?;
        Self::from_reader(std::io::BufReader::new(file), tile_lat, tile_lon)
    }

    /// Parse a Copernicus tile from any `Read + Seek` source.
    ///
    /// Used by `from_file` and in unit tests that supply in-memory buffers.
    pub fn from_reader<R: Read + Seek>(
        reader: R,
        tile_lat: i32,
        tile_lon: i32,
    ) -> Result<Self, DemError> {
        let mut decoder = tiff::decoder::Decoder::new(reader)
            .map_err(|e| DemError::Parse(format!("TIFF decoder init failed: {e}")))?;

        let (width, height) = decoder
            .dimensions()
            .map_err(|e| DemError::Parse(format!("TIFF dimensions query failed: {e}")))?;

        let dim_cols = width as usize;
        let dim_rows = height as usize;

        let image = decoder
            .read_image()
            .map_err(|e| DemError::Parse(format!("TIFF image read failed: {e}")))?;

        /*
         * Copernicus GLO-30 tiles are FLOAT32 (SampleFormat=3). Any other
         * sample format indicates a wrong file (e.g. an SRTM .hgt converted
         * to TIFF with integer samples).
         */
        let elevations = match image {
            tiff::decoder::DecodingResult::F32(v) => v,
            _ => {
                return Err(DemError::Parse(
                    "unexpected TIFF sample format: expected FLOAT32 (Copernicus GLO-30)".into(),
                ));
            }
        };

        if elevations.len() != dim_rows * dim_cols {
            return Err(DemError::Parse(format!(
                "TIFF pixel count mismatch: header says {}×{}={}, got {} samples",
                dim_rows,
                dim_cols,
                dim_rows * dim_cols,
                elevations.len()
            )));
        }

        Ok(Self {
            elevations,
            dim_rows,
            dim_cols,
            tile_lat,
            tile_lon,
        })
    }

    /// Bilinearly interpolated elevation at `(lat, lon)` in metres above EGM2008.
    ///
    /// Returns `None` if any of the four interpolation posts contain the
    /// Copernicus void sentinel (≤ −32767.0) or are NaN.
    pub fn elevation_at(&self, lat: f64, lon: f64) -> Option<f32> {
        let dim_r = self.dim_rows as f64;
        let dim_c = self.dim_cols as f64;

        /*
         * Map lat/lon to fractional row and column indices.
         *
         * Row 0 is the northern edge of the tile (tile_lat + 1°).
         * Row dim_rows corresponds to the southern edge (tile_lat°).
         *   r = (tile_lat + 1 - lat) * dim_rows
         *
         * Col 0 is the western edge (tile_lon°).
         * Col dim_cols corresponds to the eastern edge (tile_lon + 1°).
         *   c = (lon - tile_lon) * dim_cols
         *
         * Copernicus tiles have no shared edge post (3600 not 3601), so the
         * eastern and southern edges at integer degree boundaries belong to
         * the adjacent tile. Clamping handles the rare case where a floating-
         * point coordinate lands exactly on a tile edge.
         */
        let r = ((self.tile_lat + 1) as f64 - lat) * dim_r;
        let c = (lon - self.tile_lon as f64) * dim_c;

        let r = r.clamp(0.0, dim_r - 1.0);
        let c = c.clamp(0.0, dim_c - 1.0);

        let r0 = r.floor() as usize;
        let c0 = c.floor() as usize;
        let r1 = (r0 + 1).min(self.dim_rows - 1);
        let c1 = (c0 + 1).min(self.dim_cols - 1);

        let dr = r - r0 as f64;
        let dc = c - c0 as f64;

        let h00 = self.get(r0, c0)? as f64;
        let h01 = self.get(r0, c1)? as f64;
        let h10 = self.get(r1, c0)? as f64;
        let h11 = self.get(r1, c1)? as f64;

        /*
         * Standard bilinear interpolation over the 2×2 neighbourhood.
         * The formula is exact when all four corners have the same height
         * (flat tile), giving the height unchanged.
         */
        let h = (1.0 - dr) * (1.0 - dc) * h00
            + (1.0 - dr) * dc * h01
            + dr * (1.0 - dc) * h10
            + dr * dc * h11;

        Some(h as f32)
    }

    /// Fetch a single post at `(row, col)`, returning `None` for void or NaN.
    fn get(&self, row: usize, col: usize) -> Option<f32> {
        let v = self.elevations[row * self.dim_cols + col];
        /*
         * Copernicus void sentinel is −32767.0. Accept any value ≤ −32767.0
         * (i.e. also i16::MIN = −32768.0 if a foreign tile sneaks in) and any
         * NaN (water body nodata in some tile variants).
         */
        if v.is_nan() || v <= -32767.0 {
            None
        } else {
            Some(v)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use std::io::Cursor;

    /*
     * Build a minimal valid TIFF in memory: little-endian, FLOAT32 samples,
     * no compression, single strip. The tile contains `rows × cols` pixels
     * all set to `value`.
     *
     * IFD layout:
     *   Header  : 8 bytes
     *   IFD     : 2 + 10×12 + 4 = 126 bytes  (offset 8)
     *   Data    : rows×cols×4 bytes           (offset 134)
     */
    fn make_flat_f32_tiff(value: f32, rows: u32, cols: u32) -> Vec<u8> {
        let pixel_count = (rows * cols) as usize;
        let data_bytes = (pixel_count * 4) as u32;
        const IFD_ENTRY_COUNT: u16 = 10;
        let data_offset: u32 = 8 + 2 + IFD_ENTRY_COUNT as u32 * 12 + 4;

        let mut buf: Vec<u8> = Vec::new();

        // TIFF header (little-endian)
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&42u16.to_le_bytes());
        buf.extend_from_slice(&8u32.to_le_bytes()); // IFD offset

        // Number of IFD entries
        buf.extend_from_slice(&IFD_ENTRY_COUNT.to_le_bytes());

        // IFD entry: tag(u16), type(u16), count(u32), value-or-offset(u32)
        let mut entry = |tag: u16, typ: u16, cnt: u32, val: u32| {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&typ.to_le_bytes());
            buf.extend_from_slice(&cnt.to_le_bytes());
            buf.extend_from_slice(&val.to_le_bytes());
        };

        entry(256, 3, 1, cols); // ImageWidth (SHORT)
        entry(257, 3, 1, rows); // ImageLength (SHORT)
        entry(258, 3, 1, 32); // BitsPerSample = 32
        entry(259, 3, 1, 1); // Compression = none
        entry(262, 3, 1, 1); // PhotometricInterpretation
        entry(273, 4, 1, data_offset); // StripOffsets (LONG)
        entry(277, 3, 1, 1); // SamplesPerPixel = 1
        entry(278, 3, 1, rows); // RowsPerStrip
        entry(279, 4, 1, data_bytes); // StripByteCounts (LONG)
        entry(339, 3, 1, 3); // SampleFormat = 3 (IEEE float)

        // Next IFD offset (0 = end of chain)
        buf.extend_from_slice(&0u32.to_le_bytes());

        // Pixel data
        let bytes = value.to_le_bytes();
        for _ in 0..pixel_count {
            buf.extend_from_slice(&bytes);
        }

        buf
    }

    #[test]
    fn flat_tile_returns_correct_elevation() {
        let data = make_flat_f32_tiff(300.0, 10, 10);
        let tile =
            CopernicusTile::from_reader(Cursor::new(data), 47, -122).expect("parse flat tile");
        let h = tile.elevation_at(47.5, -121.5).expect("elevation");
        assert_abs_diff_eq!(h as f64, 300.0, epsilon = 1e-4);
    }

    #[test]
    fn void_post_returns_none() {
        // Build a 2×2 tile where one corner is the void sentinel.
        let elevations: Vec<f32> = vec![100.0, 100.0, 100.0, -32767.0];
        let data_bytes: u32 = (elevations.len() * 4) as u32;
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
        entry(256, 3, 1, 2);
        entry(257, 3, 1, 2);
        entry(258, 3, 1, 32);
        entry(259, 3, 1, 1);
        entry(262, 3, 1, 1);
        entry(273, 4, 1, data_offset);
        entry(277, 3, 1, 1);
        entry(278, 3, 1, 2);
        entry(279, 4, 1, data_bytes);
        entry(339, 3, 1, 3);
        buf.extend_from_slice(&0u32.to_le_bytes());

        for v in &elevations {
            buf.extend_from_slice(&v.to_le_bytes());
        }

        let tile =
            CopernicusTile::from_reader(Cursor::new(buf), 0, 0).expect("parse tile with void");

        /*
         * Query at the SE corner of a 2×2 tile (lat=tile_lat, lon=tile_lon+1):
         *   r = (1 - 0.0) * 2 = 2.0 → clamped to 1.0
         *   c = (1.0 - 0.0) * 2 = 2.0 → clamped to 1.0
         * r0=1, c0=1 → post [1,1] = -32767.0 → None
         */
        assert!(
            tile.elevation_at(0.0, 1.0).is_none(),
            "void post must return None"
        );
    }

    #[test]
    fn col_count_table_spot_checks() {
        assert_eq!(col_count_for_lat(0), 3600);
        assert_eq!(col_count_for_lat(49), 3600);
        assert_eq!(col_count_for_lat(50), 2400);
        assert_eq!(col_count_for_lat(70), 1200);
        assert_eq!(col_count_for_lat(85), 360);
        // Negative latitudes (southern hemisphere) mirror positive.
        assert_eq!(col_count_for_lat(-45), 3600);
        assert_eq!(col_count_for_lat(-65), 1800);
    }

    /// Verify that `from_reader` correctly decodes a DEFLATE-compressed TIFF
    /// with Predictor=3 (FloatingPoint) — the compression scheme used by real
    /// Copernicus COG tiles in production.
    ///
    /// The tiff crate v0.9 implements `fp_predict_f32` (byte-plane shuffle +
    /// horizontal differencing), but the encoder has no predictor support, so
    /// we build the encoded bytes manually here to exercise the full decode path.
    ///
    /// Encoding steps (inverse of `tiff::decoder::fp_predict_f32`, applied per row):
    ///   1. For each row: extract big-endian bytes and arrange into 4 byte planes
    ///      [b0 of all pixels in row | b1 | b2 | b3] — cols×4 bytes per row.
    ///   2. Apply forward horizontal differencing right-to-left on each row's bytes:
    ///      buf[k] -= buf[k-1] for k in (1..len).rev()
    ///   3. Concatenate all encoded rows; zlib-DEFLATE compress the result
    ///      (tiff decoder uses ZlibDecoder, Compression tag value 8).
    #[test]
    fn deflate_floating_point_predictor_round_trip() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression as Flate2Compression;

        let rows: usize = 4;
        let cols: usize = 4;
        let value: f32 = 300.0;
        let n = rows * cols;

        let be_bytes = value.to_bits().to_be_bytes(); // big-endian [b0, b1, b2, b3]

        /*
         * The tiff decoder applies FloatingPoint predictor PER ROW, not per
         * strip. `fp_predict_f32` receives the raw bytes of exactly one row at
         * a time. We therefore encode row-by-row: for each row of `cols`
         * pixels, build 4 byte planes (b0 of all pixels, then b1, b2, b3),
         * apply forward horizontal differencing, then append to the stream.
         *
         * Step 3: forward horizontal differencing (right-to-left to avoid
         * overwriting values still needed as subtrahends):
         *   buf[k] = buf[k] - buf[k-1]  for k = len-1 downto 1
         * Decode reversal: cumulative sum buf[k] += buf[k-1] left-to-right.
         */
        let mut encoded_bytes: Vec<u8> = Vec::with_capacity(n * 4);
        for _ in 0..rows {
            let mut row_bytes: Vec<u8> = Vec::with_capacity(cols * 4);
            for plane in 0..4usize {
                for _ in 0..cols {
                    row_bytes.push(be_bytes[plane]);
                }
            }
            for k in (1..row_bytes.len()).rev() {
                row_bytes[k] = row_bytes[k].wrapping_sub(row_bytes[k - 1]);
            }
            encoded_bytes.extend_from_slice(&row_bytes);
        }

        /*
         * Step 4: zlib-compress. The tiff crate's DeflateReader is a
         * flate2::read::ZlibDecoder, so we must produce zlib-wrapped output.
         */
        let mut enc = ZlibEncoder::new(Vec::new(), Flate2Compression::default());
        std::io::Write::write_all(&mut enc, &encoded_bytes).expect("zlib write");
        let compressed = enc.finish().expect("zlib finish");

        /*
         * Build a minimal TIFF with Compression=8 (DEFLATE) and Predictor=3.
         * IFD entries must be in ascending tag order per the TIFF 6.0 spec.
         */
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

        entry(256, 3, 1, cols as u32); // ImageWidth
        entry(257, 3, 1, rows as u32); // ImageLength
        entry(258, 3, 1, 32); // BitsPerSample = 32
        entry(259, 3, 1, 8); // Compression = 8 (DEFLATE)
        entry(262, 3, 1, 1); // PhotometricInterpretation
        entry(273, 4, 1, data_offset); // StripOffsets
        entry(277, 3, 1, 1); // SamplesPerPixel
        entry(278, 3, 1, rows as u32); // RowsPerStrip
        entry(279, 4, 1, data_bytes); // StripByteCounts (compressed)
        entry(317, 3, 1, 3); // Predictor = 3 (FloatingPoint)
        entry(339, 3, 1, 3); // SampleFormat = 3 (IEEE float)

        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0
        buf.extend_from_slice(&compressed);

        let tile = CopernicusTile::from_reader(Cursor::new(buf), 0, 0)
            .expect("DEFLATE + FloatingPoint predictor round-trip must succeed");

        let h = tile
            .elevation_at(0.5, 0.5)
            .expect("elevation from compressed tile");
        assert_abs_diff_eq!(h as f64, value as f64, epsilon = 1e-4);
    }

    #[test]
    fn wrong_sample_format_returns_parse_error() {
        // Build a valid TIFF with U16 samples (SampleFormat=1, BitsPerSample=16).
        let rows: u32 = 4;
        let cols: u32 = 4;
        let pixel_count = (rows * cols) as usize;
        let data_bytes = (pixel_count * 2) as u32;
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
        entry(256, 3, 1, cols);
        entry(257, 3, 1, rows);
        entry(258, 3, 1, 16); // BitsPerSample = 16
        entry(259, 3, 1, 1);
        entry(262, 3, 1, 1);
        entry(273, 4, 1, data_offset);
        entry(277, 3, 1, 1);
        entry(278, 3, 1, rows);
        entry(279, 4, 1, data_bytes);
        entry(339, 3, 1, 1); // SampleFormat = 1 (unsigned int)
        buf.extend_from_slice(&0u32.to_le_bytes());

        for _ in 0..pixel_count {
            buf.extend_from_slice(&100u16.to_le_bytes());
        }

        let err = CopernicusTile::from_reader(Cursor::new(buf), 0, 0).unwrap_err();
        assert!(
            matches!(err, DemError::Parse(_)),
            "expected Parse error for non-FLOAT32 TIFF, got {err:?}"
        );
    }
}
