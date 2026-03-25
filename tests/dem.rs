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

//! Integration tests for the DEM subsystem.
//!
//! Do NOT use real SRTM tiles. Generate synthetic .hgt fixtures in-process
//! using tempfile so tests have zero download dependencies.
//! See .agents/testing.md §DEM Tests for .hgt format and synthesis details.

// stub: uncomment and implement when src/dem/ is populated
// use irontrack::dem;

/// Parse a synthetic SRTM-3 .hgt file (1201×1201 i16 big-endian) where every
/// cell is a known constant elevation. Assert the parser returns that constant
/// for every query coordinate within the tile bounds.
#[test]
#[ignore = "stub: implement when srtm parser is complete"]
fn srtm_parse_synthetic_constant_elevation() {}

/// Bilinear interpolation on a hand-crafted 2×2 grid with elevations
/// [0, 100, 200, 300] m. The exact centre must interpolate to 150 m.
/// Assert within 1e-6 m.
#[test]
#[ignore = "stub: implement when bilinear interpolation is complete"]
fn bilinear_interpolation_known_2x2_grid() {}

/// Bilinear interpolation at grid corners must return the exact corner values
/// with no floating-point bleed from adjacent cells.
#[test]
#[ignore = "stub: implement when bilinear interpolation is complete"]
fn bilinear_interpolation_exact_at_corners() {}

/// First query for a tile: cache miss, tile is fetched and written to
/// the cache directory. Second query for the same tile: cache hit, no I/O.
/// Use a tempdir as the cache root so the test does not pollute the user cache.
#[test]
#[ignore = "stub: implement when tile cache is complete"]
fn cache_miss_then_hit_no_redundant_io() {}

/// Query coordinates outside tile bounds must return DemError::TileNotFound,
/// not panic or return garbage elevation.
#[test]
#[ignore = "stub: implement when srtm parser is complete"]
fn out_of_bounds_query_returns_error() {}

/// EGM96 orthometric conversion: h (ellipsoidal) = H (orthometric) + N (geoid).
/// Given synthetic H and N, assert the TerrainEngine returns h within 1e-3 m.
#[test]
#[ignore = "stub: implement when TerrainEngine is complete"]
fn terrain_engine_ellipsoidal_height_conversion() {}
