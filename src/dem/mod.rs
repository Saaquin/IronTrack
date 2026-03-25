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

//! DEM subsystem: Copernicus DEM GLO-30 tile parser, bilinear interpolation,
//! cross-platform tile cache, and terrain engine.
//!
//! ## Module structure
//!
//! ```text
//! dem/
//! ├── mod.rs          — TerrainEngine (this file): composes DEM + geoid
//! ├── copernicus.rs   — Copernicus FLOAT32 TIFF parser + bilinear interp
//! └── cache.rs        — DemCache (filesystem + optional download) +
//!                       DemProvider (lazy tile loader with RwLock)
//! ```
//!
//! ## Height reference frames
//!
//! Copernicus DEM GLO-30 stores **orthometric heights** H (metres above the
//! EGM2008 geoid). Autopilot targets and GNSS outputs use **ellipsoidal
//! heights** h = H + N, where N is the EGM2008 geoid undulation. The
//! `TerrainEngine` applies that correction via `ellipsoidal_terrain_elevation`.
//!
//! See `docs/05_vertical_datum_engineering.md` and
//! `docs/formula_reference.md §Doc 05` for the full treatment.
//!
//! ## DSM vs DTM
//!
//! Copernicus DEM is a **Digital Surface Model** (DSM). Vegetation canopy,
//! buildings, and other above-ground features are included in the elevation
//! surface. AGL clearance over forested terrain may be understated by 2–8 m
//! depending on forest type. See `docs/09_DSMvDTM_AGL_Flight_Generation.md`.

pub mod cache;
pub mod copernicus;

use std::path::PathBuf;

use rayon::prelude::*;

use crate::dem::cache::{self as dem_cache, DemCache, DemProvider};
use crate::error::DemError;
use crate::geodesy::geoid::Egm2008Model;
use crate::types::GeoCoord;

// ---------------------------------------------------------------------------
// ElevationSource
// ---------------------------------------------------------------------------

/// Indicates how a terrain elevation query was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationSource {
    /// Elevation was read from a Copernicus DEM GLO-30 tile, corrected with
    /// EGM2008 geoid undulation.
    CopernicusDem,
    /// Coordinate is outside valid WGS84 bounds (|lat| > 90° or non-finite).
    /// All heights reported as 0 m; the geoid model cannot evaluate at
    /// invalid coordinates.
    SeaLevelFallback,
    /// The Copernicus S3 bucket confirmed no tile exists for this coordinate
    /// (HTTP 404 — open ocean, polar void, or other no-data region). Sea-level
    /// orthometric height (0 m) is assumed; EGM2008 geoid undulation is still
    /// applied for the correct ellipsoidal height. Callers should add a safety
    /// margin for tidal variation (~1 m) when using this value for flight
    /// planning over open water.
    OceanFallback,
}

// ---------------------------------------------------------------------------
// TerrainReport
// ---------------------------------------------------------------------------

/// Full terrain result for a single geographic coordinate.
///
/// Returned by [`TerrainEngine::query`] and [`TerrainEngine::query_coord`].
/// Includes both height reference frames plus the geoid undulation so that
/// callers do not need to subtract to recover N, and the data source so that
/// callers can warn users when sea-level fallback was applied.
#[derive(Debug, Clone)]
pub struct TerrainReport {
    /// Geodetic latitude queried, decimal degrees.
    pub lat: f64,
    /// Geodetic longitude queried, decimal degrees.
    pub lon: f64,
    /// Orthometric height H (metres above EGM2008 geoid ≈ MSL).
    pub orthometric_m: f64,
    /// EGM2008 geoid undulation N at this location (metres).
    /// Reported as 0.0 when `source == SeaLevelFallback`.
    pub geoid_undulation_m: f64,
    /// WGS84 ellipsoidal height h = H + N (metres).
    pub ellipsoidal_m: f64,
    /// Whether real Copernicus DEM data was used or the sea-level fallback
    /// was applied.
    pub source: ElevationSource,
}

// ---------------------------------------------------------------------------
// TerrainEngine
// ---------------------------------------------------------------------------

/// Top-level DEM access point.
///
/// Composes Copernicus DEM GLO-30 orthometric heights with EGM2008 geoid
/// undulation to deliver both orthometric (MSL) and ellipsoidal (WGS84)
/// elevations. Tiles are downloaded automatically from the Copernicus AWS S3
/// bucket on first use when constructed with [`TerrainEngine::new`].
///
/// ```text
/// TerrainEngine
///   ├── DemProvider  — lazy-loaded Copernicus tile cache + bilinear interp
///   │                  (dem::cache)
///   └── Egm2008Model — EGM2008 undulation N for h = H + N
///                      (geodesy::geoid)
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// let engine = TerrainEngine::new()?;
///
/// // Full report — safe at any valid WGS84 coordinate.
/// let report = engine.query(47.5, -121.5)?;
/// if report.source == ElevationSource::SeaLevelFallback {
///     eprintln!("Note: Invalid WGS84 coordinate — sea-level terrain assumed");
/// }
///
/// // Strict methods — propagate DemError on tile miss or void post.
/// let h_ortho     = engine.terrain_elevation(47.5, -121.5)?;
/// let h_ellipsoid = engine.ellipsoidal_terrain_elevation(47.5, -121.5)?;
/// ```
///
/// # DSM safety note
///
/// Copernicus DEM is a **Digital Surface Model**. AGL clearance calculations
/// over forested terrain must add a vegetation offset of 2–8 m depending on
/// forest type. See `docs/09_DSMvDTM_AGL_Flight_Generation.md`.
pub struct TerrainEngine {
    /// Lazy-loading Copernicus DEM tile provider.
    ///
    /// On first query for a 1°×1° tile the provider loads (or downloads) the
    /// corresponding `.tif` file and stores the parsed grid in memory.
    /// Subsequent queries for the same tile skip disk I/O entirely.
    ///
    /// Returns **orthometric** heights H (metres above EGM2008 geoid).
    dem: DemProvider,
    /// EGM2008 geoid model — provides undulation N for h = H + N.
    ///
    /// EGM2008 is the correct datum for Copernicus DEM tiles. Using EGM96
    /// against Copernicus elevations introduces a systematic bias of typically
    /// <0.5 m globally but up to 1–2 m in mountainous terrain.
    geoid: Egm2008Model,
}

impl TerrainEngine {
    /// Construct the engine using the default platform DEM tile cache.
    ///
    /// Creates `{dirs::cache_dir()}/irontrack/dem/` if it does not exist.
    /// Tiles are loaded and downloaded lazily on first query.
    pub fn new() -> Result<Self, DemError> {
        Ok(Self {
            dem: DemProvider::new()?,
            geoid: Egm2008Model::new(),
        })
    }

    /// Construct the engine using a custom DEM tile cache directory.
    ///
    /// Automatic tile download is **disabled** (suitable for tests and
    /// pre-populated installations). The directory is created if it does not
    /// exist.
    pub fn with_cache_dir(cache_dir: PathBuf) -> Result<Self, DemError> {
        let cache = DemCache::with_dir(cache_dir)?;
        Ok(Self {
            dem: DemProvider::with_cache(cache),
            geoid: Egm2008Model::new(),
        })
    }

    /// Query terrain at `(lat, lon)`, falling back to sea level for invalid
    /// WGS84 coordinates (|lat| > 90°).
    ///
    /// This is the preferred entry point for pole-to-pole coverage. When the
    /// coordinate is outside valid WGS84 bounds, all heights are reported as
    /// 0 m (the geoid model cannot evaluate at invalid coordinates).
    ///
    /// # Errors
    ///
    /// Returns [`DemError`] only for hard failures: tile corruption, I/O
    /// errors, or download failures. A missing tile for an otherwise valid
    /// coordinate returns `DemError::TileNotFound`.
    pub fn query(&self, lat: f64, lon: f64) -> Result<TerrainReport, DemError> {
        let (orthometric_m, source) = match self.dem.elevation_at(lat, lon) {
            Ok(h) => (h, ElevationSource::CopernicusDem),
            Err(DemError::OutsideCoverage { .. }) => (0.0, ElevationSource::SeaLevelFallback),
            /*
             * NoData means Copernicus S3 confirmed no tile exists (HTTP 404).
             * This is the definitive signal for open ocean and polar void
             * regions. Assume orthometric height = 0 m (sea level), but still
             * apply the EGM2008 geoid undulation so the ellipsoidal height is
             * physically meaningful for autopilot planning over water.
             */
            Err(DemError::NoData(_)) => (0.0, ElevationSource::OceanFallback),
            Err(e) => return Err(e),
        };

        /*
         * When SeaLevelFallback fires (invalid WGS84 coordinate), the geoid
         * model would also reject the coordinate — report 0 m for undulation.
         * For OceanFallback, the coordinate is valid, so the geoid undulation
         * is meaningful and should be applied.
         */
        let geoid_undulation_m = match source {
            ElevationSource::SeaLevelFallback => 0.0,
            ElevationSource::CopernicusDem | ElevationSource::OceanFallback => {
                self.geoid.undulation(lat, lon)?
            }
        };

        Ok(TerrainReport {
            lat,
            lon,
            orthometric_m,
            geoid_undulation_m,
            ellipsoidal_m: orthometric_m + geoid_undulation_m,
            source,
        })
    }

    /// Adapter: query terrain using a validated [`GeoCoord`].
    ///
    /// Extracts decimal-degree values from the coordinate and delegates to
    /// [`query`]. Intended for Phase 2 code that works with validated
    /// coordinate types rather than raw `f64` pairs.
    ///
    /// [`query`]: TerrainEngine::query
    pub fn query_coord(&self, coord: &GeoCoord) -> Result<TerrainReport, DemError> {
        self.query(coord.lat_deg(), coord.lon_deg())
    }

    /// Bilinearly interpolated **orthometric** elevation H at `(lat, lon)`.
    ///
    /// Returns metres above the EGM2008 geoid (approximately mean sea level).
    /// This is the value stored directly in Copernicus DEM GLO-30 tiles.
    ///
    /// Use [`ellipsoidal_terrain_elevation`] when you need the WGS84
    /// ellipsoidal height for GNSS / autopilot targets.
    ///
    /// # Errors
    ///
    /// Propagates [`DemError`] from [`DemProvider::elevation_at`]:
    /// - `TileNotFound` — tile absent from cache, download failed (HTTP 404
    ///   for ocean/void coverage), or the query lands on a void post.
    /// - `OutsideCoverage` — coordinate has |lat| > 90°.
    /// - `CacheFailure` — download or I/O error.
    /// - `Parse` — corrupt or wrong-format TIFF.
    ///
    /// [`ellipsoidal_terrain_elevation`]: TerrainEngine::ellipsoidal_terrain_elevation
    pub fn terrain_elevation(&self, lat: f64, lon: f64) -> Result<f64, DemError> {
        self.dem.elevation_at(lat, lon)
    }

    /// Ellipsoidal elevation h = H + N at `(lat, lon)`.
    ///
    /// Fetches the Copernicus DEM orthometric height H then adds the EGM2008
    /// geoid undulation N to produce the WGS84 ellipsoidal height h. This is
    /// the altitude value reported by GNSS receivers and used as the target
    /// for autopilot flight planning.
    ///
    /// # Errors
    ///
    /// - [`DemError::TileNotFound`] — tile absent from cache or the query
    ///   lands on a void post.
    /// - [`DemError::OutsideCoverage`] — coordinate has |lat| > 90°. Use
    ///   [`query`] for graceful handling at invalid coordinates.
    /// - [`DemError::Geoid`] — geoid undulation failed (should not happen for
    ///   valid coordinates; indicates a bug in the caller).
    /// - [`DemError::CacheFailure`] / [`DemError::Parse`] — I/O or TIFF error.
    ///
    /// [`query`]: TerrainEngine::query
    pub fn ellipsoidal_terrain_elevation(&self, lat: f64, lon: f64) -> Result<f64, DemError> {
        let h_ortho = self.dem.elevation_at(lat, lon)?;
        let n = self.geoid.undulation(lat, lon)?;
        Ok(h_ortho + n)
    }

    /// Bilinearly interpolated **orthometric** elevations for a batch of
    /// `(lats[i], lons[i])` coordinate pairs.
    ///
    /// ## Two-phase execution
    ///
    /// Rayon is CPU-bound; blocking it on network I/O causes thread starvation
    /// when a large survey spans many unloaded tiles. This method therefore
    /// separates tile loading from interpolation:
    ///
    /// 1. **Pre-warm pass (sequential)** — deduplicate tile keys, then load
    ///    (or download) each distinct tile once on the calling thread. Any
    ///    download error here is returned immediately before Rayon is invoked.
    /// 2. **Interpolation pass (parallel)** — all needed tiles are already in
    ///    the in-process cache; Rayon threads only perform bilinear math and
    ///    never block on I/O.
    ///
    /// # Errors
    ///
    /// - [`DemError::Parse`] if `lats.len() != lons.len()`.
    /// - First [`DemError`] from tile load (pre-warm phase, sequential).
    /// - First [`DemError`] from interpolation (parallel phase,
    ///   non-deterministic order).
    pub fn elevations_at(&self, lats: &[f64], lons: &[f64]) -> Result<Vec<f64>, DemError> {
        if lats.len() != lons.len() {
            return Err(DemError::Parse(format!(
                "elevations_at: lats ({}) and lons ({}) slices must have equal length",
                lats.len(),
                lons.len()
            )));
        }

        /*
         * Phase 1: collect the set of distinct tile keys needed by this batch
         * and pre-warm the in-process tile cache — downloading from S3 if
         * required — before Rayon is ever invoked. This ensures the parallel
         * phase touches only CPU-bound math and never blocks a Rayon thread on
         * network I/O.
         *
         * lon=180.0 normalization mirrors DemProvider::elevation_at so that
         * the key we pre-warm matches the key the parallel pass will look up.
         */
        let mut keys: Vec<(i32, i32)> = lats
            .iter()
            .zip(lons.iter())
            .map(|(&lat, &lon)| {
                let lon = if lon == 180.0 { -180.0 } else { lon };
                dem_cache::tile_key(lat, lon)
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();

        for (tile_lat, tile_lon) in keys {
            self.dem.elevation_at(
                tile_lat as f64 + 0.5,
                tile_lon as f64 + 0.5,
            )?;
        }

        /*
         * Phase 2: all tiles are now in the RwLock<HashMap> fast path.
         * The parallel pass performs only bilinear interpolation — no I/O.
         */
        lats.par_iter()
            .zip(lons.par_iter())
            .map(|(&lat, &lon)| self.dem.elevation_at(lat, lon))
            .collect()
    }
}
