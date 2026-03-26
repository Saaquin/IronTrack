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

use thiserror::Error;

// ---------------------------------------------------------------------------
// Datum errors
// ---------------------------------------------------------------------------

/// Errors from altitude datum conversions (EGM2008 ↔ EGM96 ↔ WGS84 ↔ AGL).
///
/// The two variants wrap the underlying error types from the geoid and terrain
/// subsystems so callers can distinguish between "the geoid model failed" and
/// "the DEM tile was not available" without inspecting nested error chains.
#[derive(Debug, Error)]
pub enum DatumError {
    /// An EGM2008 or EGM96 geoid undulation lookup failed (coordinate out of
    /// bounds or non-finite input).
    #[error("geoid undulation lookup failed: {0}")]
    Geoid(#[from] GeodesyError),
    /// A terrain elevation query failed during AGL ↔ ellipsoidal conversion.
    /// Terrain access failures (tile not cached, ocean/void pixel) propagate
    /// here unchanged so the caller can apply the same graceful fallbacks as
    /// the rest of the terrain pipeline.
    #[error("terrain elevation query failed during datum conversion: {0}")]
    Terrain(#[from] DemError),
}

// ---------------------------------------------------------------------------
// DEM errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DemError {
    /// The requested tile file is absent from the local cache.
    /// The payload contains the tile filename and a download URL.
    #[error("DEM tile not found: {0}")]
    TileNotFound(String),
    /// The DEM provider confirmed this coordinate has no elevation data —
    /// the Copernicus S3 bucket returned HTTP 404, meaning no tile was ever
    /// produced for this location (open ocean, polar void, or other no-data
    /// region). Use `TerrainEngine::query` for a graceful sea-level fallback.
    #[error("no DEM data for this location (ocean or void coverage): {0}")]
    NoData(String),
    /// The coordinate is outside valid WGS84 bounds (|lat| > 90° or non-finite).
    /// Use `TerrainEngine::query` for a graceful sea-level fallback at these coordinates.
    #[error("coordinate outside valid WGS84 bounds: lat={lat:.4}°, lon={lon:.4}°")]
    OutsideCoverage { lat: f64, lon: f64 },
    #[error("DEM parse error: {0}")]
    Parse(String),
    #[error("DEM cache failure: {0}")]
    CacheFailure(String),
    /// Wraps a geoid model error that occurred during ellipsoidal height conversion.
    #[error("geoid model error: {0}")]
    Geoid(#[from] GeodesyError),
    #[error("DEM I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Geodesy errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum GeodesyError {
    #[error("coordinate out of bounds: lat={lat}, lon={lon}")]
    OutOfBounds { lat: f64, lon: f64 },
    #[error("projection failure: {0}")]
    ProjectionFailure(String),
}

// ---------------------------------------------------------------------------
// GeoPackage errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum GpkgError {
    /// Wraps rusqlite errors from all database operations.
    #[error("GeoPackage database error: {0}")]
    Database(#[from] rusqlite::Error),
    /// Raised when the initial schema setup (PRAGMAs, mandatory tables,
    /// R-tree triggers) fails for a reason not captured by rusqlite::Error.
    #[error("GeoPackage initialisation failure: {0}")]
    Init(String),
    #[error("GeoPackage binary serialization error: {0}")]
    Serialization(String),
    #[error("GeoPackage I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// I/O errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum IoError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
}

// ---------------------------------------------------------------------------
// Photogrammetry errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum PhotogrammetryError {
    #[error("invalid sensor parameters: {0}")]
    InvalidSensor(String),
    #[error("overlap out of range (must be 0.0..1.0): {0}")]
    InvalidOverlap(f64),
    /// Raised during flight line validation when the true AGL over a specific
    /// terrain point falls below the required safety clearance. Carries both the
    /// computed AGL and the terrain elevation so the caller can report the
    /// exact geographic location of the violation.
    #[error("AGL too low for terrain clearance: agl={agl_m:.1}m above terrain={terrain_m:.1}m")]
    InsufficientAgl { agl_m: f64, terrain_m: f64 },
    /// Raised by the flight planner when the AGL computed from a target GSD
    /// is below the platform's minimum safe operating altitude. This is a
    /// mission-level constraint — the sensor physics are valid, but the
    /// resulting altitude is operationally unsafe for the platform type.
    ///
    /// `computed_m`: AGL that would satisfy the GSD target (from sensor math).
    /// `min_safe_m`: platform minimum from `MissionParams::min_clearance_m`.
    #[error(
        "computed AGL {computed_m:.1}m is below minimum safe operating altitude {min_safe_m:.1}m"
    )]
    UnsafeAltitude { computed_m: f64, min_safe_m: f64 },
    /// General parameter validation failure for non-sensor, non-overlap inputs
    /// (e.g., degenerate bounding box, non-finite azimuth). Carries a
    /// human-readable description of the violated constraint.
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Error Display output must be human-readable (not just the variant name).
    #[test]
    fn dem_error_tile_not_found_display() {
        let e = DemError::TileNotFound("N51W001".to_string());
        assert!(e.to_string().contains("N51W001"));
    }

    #[test]
    fn dem_error_cache_failure_display() {
        let e = DemError::CacheFailure("permission denied".to_string());
        assert!(e.to_string().contains("permission denied"));
    }

    #[test]
    fn geodesy_error_out_of_bounds_display() {
        let e = GeodesyError::OutOfBounds {
            lat: 91.0,
            lon: 0.0,
        };
        assert!(e.to_string().contains("91"));
    }

    #[test]
    fn geodesy_error_projection_failure_display() {
        let e = GeodesyError::ProjectionFailure("zone 0 invalid".to_string());
        assert!(e.to_string().contains("zone 0 invalid"));
    }

    #[test]
    fn gpkg_error_init_display() {
        let e = GpkgError::Init("missing geometry_columns table".to_string());
        assert!(e.to_string().contains("missing geometry_columns table"));
    }

    #[test]
    fn gpkg_error_serialization_display() {
        let e = GpkgError::Serialization("bad envelope".to_string());
        assert!(e.to_string().contains("bad envelope"));
    }

    #[test]
    fn photo_error_insufficient_agl_display() {
        let e = PhotogrammetryError::InsufficientAgl {
            agl_m: 10.0,
            terrain_m: 50.0,
        };
        let msg = e.to_string();
        assert!(msg.contains("10") && msg.contains("50"));
    }

    #[test]
    fn photo_error_invalid_overlap_display() {
        let e = PhotogrammetryError::InvalidOverlap(1.1);
        assert!(e.to_string().contains("1.1"));
    }

    #[test]
    fn photo_error_invalid_input_display() {
        let e = PhotogrammetryError::InvalidInput("bbox has zero width".into());
        assert!(e.to_string().contains("bbox has zero width"));
    }

    #[test]
    fn photo_error_unsafe_altitude_display() {
        let e = PhotogrammetryError::UnsafeAltitude {
            computed_m: 8.5,
            min_safe_m: 30.0,
        };
        let msg = e.to_string();
        assert!(msg.contains("8.5"), "expected computed AGL in: {msg}");
        assert!(msg.contains("30"), "expected min safe AGL in: {msg}");
    }
}
