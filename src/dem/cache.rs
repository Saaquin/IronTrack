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

//! Cross-platform Copernicus DEM tile cache and lazy-loading DEM provider.
//!
//! Tiles are FLOAT32 Cloud Optimized GeoTIFFs stored under:
//!   `{dirs::cache_dir()}/irontrack/dem/`
//!
//! [`DemCache`] handles the filesystem layer — path generation, existence
//! checks, tile loading, and optional blocking download from the Copernicus
//! AWS S3 bucket. [`DemProvider`] wraps the cache with a `RwLock<HashMap>`
//! for lazy, in-process tile reuse across multiple queries.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::dem::copernicus::CopernicusTile;
use crate::error::DemError;

// ---------------------------------------------------------------------------
// Tile key and filename helpers (free functions, also used in tests)
// ---------------------------------------------------------------------------

/// Derive the integer SW-corner indices for the Copernicus tile that contains
/// `(lat, lon)`. Uses `floor` toward −∞ so that negative coordinates map to
/// the correct tile (e.g. lat=−0.5 → tile lat=−1, not 0).
pub fn tile_key(lat: f64, lon: f64) -> (i32, i32) {
    (lat.floor() as i32, lon.floor() as i32)
}

/// Build the Copernicus directory/filename stem for the tile whose SW corner
/// is at `(lat, lon)` in integer degrees.
///
/// Format: `Copernicus_DSM_COG_10_{NS}{LAT:02}_00_{EW}{LON:03}_00_DEM`
fn copernicus_stem(lat: i32, lon: i32) -> String {
    let ns = if lat >= 0 { 'N' } else { 'S' };
    let ew = if lon >= 0 { 'E' } else { 'W' };
    format!(
        "Copernicus_DSM_COG_10_{ns}{:02}_00_{ew}{:03}_00_DEM",
        lat.unsigned_abs(),
        lon.unsigned_abs()
    )
}

/// Build the full S3 download URL for the tile at `(lat, lon)`.
///
/// URL pattern:
/// `https://copernicus-dem-30m.s3.amazonaws.com/{STEM}/{STEM}.tif`
pub fn copernicus_url(lat: i32, lon: i32) -> String {
    let stem = copernicus_stem(lat, lon);
    format!("https://copernicus-dem-30m.s3.amazonaws.com/{stem}/{stem}.tif")
}

/// Produce the `.tif` filename for the tile whose SW corner is at `(lat, lon)`.
///
/// This is the local on-disk cache filename — the full Copernicus stem plus
/// `.tif`. Matches the naming convention of the S3 object key's leaf file.
pub fn tile_filename(lat: i32, lon: i32) -> String {
    format!("{}.tif", copernicus_stem(lat, lon))
}

// ---------------------------------------------------------------------------
// DemCache — filesystem layer
// ---------------------------------------------------------------------------

/// Filesystem interface to the Copernicus DEM tile cache directory.
///
/// The default cache root is `{dirs::cache_dir()}/irontrack/dem/`. Two
/// constructors are provided:
///
/// - [`DemCache::new`] — production use; enables automatic tile download when
///   a tile is absent from the local cache.
/// - [`DemCache::with_dir`] — tests and custom installations; disables
///   automatic download so tests never make network calls.
pub struct DemCache {
    cache_dir: PathBuf,
    /// When `true`, a missing tile triggers a blocking HTTP download from the
    /// Copernicus AWS S3 bucket. When `false`, a missing tile returns
    /// [`DemError::TileNotFound`] immediately with the download URL.
    download_missing: bool,
}

impl DemCache {
    /// Construct a production cache handle rooted at the platform default
    /// directory, enabling automatic tile download.
    ///
    /// Creates `{dirs::cache_dir()}/irontrack/dem/` if it does not already
    /// exist.
    pub fn new() -> Result<Self, DemError> {
        let root = dirs::cache_dir().ok_or_else(|| {
            DemError::CacheFailure("could not determine platform cache directory".into())
        })?;
        let cache_dir = root.join("irontrack").join("dem");
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            DemError::CacheFailure(format!(
                "failed to create cache directory {}: {e}",
                cache_dir.display()
            ))
        })?;
        Ok(Self {
            cache_dir,
            download_missing: true,
        })
    }

    /// Construct a cache handle rooted at an arbitrary path, with automatic
    /// download **disabled**.
    ///
    /// Used in tests and when the caller wants to point the engine at a
    /// pre-populated tile store without risking network calls.
    pub fn with_dir(cache_dir: PathBuf) -> Result<Self, DemError> {
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            DemError::CacheFailure(format!(
                "failed to create cache directory {}: {e}",
                cache_dir.display()
            ))
        })?;
        Ok(Self {
            cache_dir,
            download_missing: false,
        })
    }

    /// Return the full path at which the tile for `(lat, lon)` should reside.
    pub fn tile_path(&self, lat: i32, lon: i32) -> PathBuf {
        self.cache_dir.join(tile_filename(lat, lon))
    }

    /// Return `true` if the tile file exists on disk.
    pub fn has_tile(&self, lat: i32, lon: i32) -> bool {
        self.tile_path(lat, lon).is_file()
    }

    /// Load and parse the tile for `(lat, lon)` from disk or (if
    /// `download_missing` is set) from the Copernicus S3 bucket.
    ///
    /// # Errors
    ///
    /// - [`DemError::TileNotFound`] — tile absent and download disabled (or
    ///   S3 returned HTTP 404).
    /// - [`DemError::CacheFailure`] — S3 request failed with a non-404 HTTP
    ///   error, temp-file creation failed, or atomic rename failed.
    /// - [`DemError::Parse`] / [`DemError::Io`] — corrupt or unreadable TIFF.
    pub fn get_tile(&self, lat: i32, lon: i32) -> Result<CopernicusTile, DemError> {
        let path = self.tile_path(lat, lon);
        if path.is_file() {
            return CopernicusTile::from_file(&path, lat, lon);
        }

        if self.download_missing {
            self.download_tile(lat, lon)
        } else {
            let name = tile_filename(lat, lon);
            let url = copernicus_url(lat, lon);
            Err(DemError::TileNotFound(format!(
                "Tile {name} not in cache.\n\
                 Download: {url}\n\
                 Place the .tif file in: {}",
                self.cache_dir.display()
            )))
        }
    }

    /// Download the tile for `(lat, lon)` from the Copernicus S3 bucket,
    /// write it atomically to the cache directory, then parse and return it.
    ///
    /// Uses `NamedTempFile` in the same directory as the destination so that
    /// the final `persist()` rename is always same-drive, avoiding EXDEV.
    fn download_tile(&self, lat: i32, lon: i32) -> Result<CopernicusTile, DemError> {
        let url = copernicus_url(lat, lon);
        let name = tile_filename(lat, lon);

        let response = reqwest::blocking::get(&url).map_err(|e| {
            DemError::CacheFailure(format!("HTTP request failed for {name}: {e}"))
        })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            /*
             * HTTP 404 from the Copernicus S3 bucket means the tile was never
             * produced for this location. This is the definitive signal for
             * ocean, polar void, and other no-data regions — distinct from a
             * tile that is simply absent from the local cache.
             */
            return Err(DemError::NoData(format!(
                "Tile {name} does not exist on Copernicus S3 (HTTP 404). \
                 This location is likely ocean, polar void, or otherwise \
                 outside Copernicus DEM coverage. URL: {url}"
            )));
        }

        if !response.status().is_success() {
            return Err(DemError::CacheFailure(format!(
                "HTTP {} downloading {name}: {url}",
                response.status()
            )));
        }

        let bytes = response.bytes().map_err(|e| {
            DemError::CacheFailure(format!("failed to read S3 response body for {name}: {e}"))
        })?;

        /*
         * Write to a temp file in the cache directory, then rename atomically.
         * same-directory placement guarantees the rename is a same-drive move.
         */
        let mut tmp = tempfile::NamedTempFile::new_in(&self.cache_dir).map_err(|e| {
            DemError::CacheFailure(format!("failed to create temp file in cache dir: {e}"))
        })?;

        tmp.write_all(&bytes).map_err(|e| {
            DemError::CacheFailure(format!("failed to write tile data to temp file: {e}"))
        })?;

        let dest = self.tile_path(lat, lon);

        /*
         * Atomic rename: same-directory temp → destination. If two Rayon
         * threads race to download the same tile, one persist() call will
         * fail because the other already completed. If the destination now
         * exists we can safely load from it — the content is identical.
         */
        match tmp.persist(&dest) {
            Ok(_) => {}
            Err(_) if dest.is_file() => {
                // Another thread already wrote and renamed this tile.
            }
            Err(e) => {
                return Err(DemError::CacheFailure(format!(
                    "failed to persist tile to {}: {e}",
                    dest.display()
                )));
            }
        }

        CopernicusTile::from_file(&dest, lat, lon)
    }

    /// Return the cache directory path. Exposed for diagnostic / help text.
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
    }
}

// ---------------------------------------------------------------------------
// DemProvider — lazy tile loader
// ---------------------------------------------------------------------------

/// Lazy-loading DEM elevation provider.
///
/// On the first query for a given 1°×1° tile the provider loads it from the
/// [`DemCache`] and inserts it into an in-process map. Subsequent queries for
/// the same tile hit the map directly, avoiding repeated disk I/O.
///
/// The internal tile map is protected by a `RwLock` so that concurrent
/// read queries (Rayon work-stealing threads) do not block each other.
pub struct DemProvider {
    cache: DemCache,
    tiles: RwLock<HashMap<(i32, i32), CopernicusTile>>,
}

impl DemProvider {
    /// Create a provider backed by the default platform cache directory.
    /// Automatic tile download is enabled.
    pub fn new() -> Result<Self, DemError> {
        Ok(Self {
            cache: DemCache::new()?,
            tiles: RwLock::new(HashMap::new()),
        })
    }

    /// Create a provider backed by a pre-constructed [`DemCache`].
    ///
    /// Used by [`TerrainEngine::with_cache_dir`] and in tests to avoid writing
    /// to the real user cache directory.
    pub fn with_cache(cache: DemCache) -> Self {
        Self {
            cache,
            tiles: RwLock::new(HashMap::new()),
        }
    }

    /// Return the bilinearly interpolated orthometric elevation (metres above
    /// EGM2008) at `(lat, lon)`.
    ///
    /// Tile loading is lazy: the tile is fetched from disk (or downloaded) only
    /// on the first query for its 1°×1° cell, then kept in memory for the
    /// lifetime of this provider.
    ///
    /// # Errors
    ///
    /// - [`DemError::OutsideCoverage`] — coordinate has `|lat| > 90°` or is
    ///   non-finite (genuinely invalid WGS84).
    /// - [`DemError::TileNotFound`] — tile absent from cache and download
    ///   disabled, or S3 returned HTTP 404 (tile does not exist for this
    ///   coordinate), or the query lands on a void / water-body post.
    /// - [`DemError::CacheFailure`] — download or I/O error.
    /// - [`DemError::Parse`] — corrupt or wrong-format TIFF.
    pub fn elevation_at(&self, lat: f64, lon: f64) -> Result<f64, DemError> {
        if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90.0 || lon.abs() > 180.0 {
            return Err(DemError::OutsideCoverage { lat, lon });
        }

        /*
         * Normalize the antimeridian: 180°E and 180°W refer to the same
         * meridian. floor(180.0) = 180, but the canonical tile index for the
         * antimeridian is −180. Remapping ensures we look for tile W180 rather
         * than the non-existent tile E180.
         */
        let lon = if lon == 180.0 { -180.0 } else { lon };

        let key = tile_key(lat, lon);

        /*
         * Fast path: tile already loaded — take a read lock, look up, and
         * return without ever acquiring the write lock.
         */
        {
            let tiles = self
                .tiles
                .read()
                .map_err(|_| DemError::CacheFailure("tile map read lock poisoned".into()))?;
            if let Some(tile) = tiles.get(&key) {
                return tile
                    .elevation_at(lat, lon)
                    .map(|v| v as f64)
                    .ok_or_else(|| {
                        DemError::NoData(format!(
                    "void or water-body post at ({lat:.6}°, {lon:.6}°) — \
                     no elevation data at this location (ocean, lake, or DEM void)"
                ))
                    });
            }
        }

        /*
         * Slow path: load (or download) from cache, then insert under the
         * write lock. Use entry().or_insert() to handle the benign race where
         * two threads both miss the read check and one loads first.
         */
        let tile = self.cache.get_tile(key.0, key.1)?;
        let mut tiles = self
            .tiles
            .write()
            .map_err(|_| DemError::CacheFailure("tile map write lock poisoned".into()))?;
        let tile = tiles.entry(key).or_insert(tile);

        tile.elevation_at(lat, lon)
            .map(|v| v as f64)
            .ok_or_else(|| DemError::NoData(format!(
                    "void or water-body post at ({lat:.6}°, {lon:.6}°) — \
                     no elevation data at this location (ocean, lake, or DEM void)"
                )))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- tile_filename ------------------------------------------------------

    #[test]
    fn filename_north_west() {
        assert_eq!(
            tile_filename(47, -122),
            "Copernicus_DSM_COG_10_N47_00_W122_00_DEM.tif"
        );
    }

    #[test]
    fn filename_south_east() {
        assert_eq!(
            tile_filename(-34, 18),
            "Copernicus_DSM_COG_10_S34_00_E018_00_DEM.tif"
        );
    }

    #[test]
    fn filename_north_east() {
        assert_eq!(
            tile_filename(51, 0),
            "Copernicus_DSM_COG_10_N51_00_E000_00_DEM.tif"
        );
    }

    #[test]
    fn filename_south_west() {
        assert_eq!(
            tile_filename(-1, -1),
            "Copernicus_DSM_COG_10_S01_00_W001_00_DEM.tif"
        );
    }

    #[test]
    fn filename_origin() {
        assert_eq!(
            tile_filename(0, 0),
            "Copernicus_DSM_COG_10_N00_00_E000_00_DEM.tif"
        );
    }

    #[test]
    fn filename_extreme_south_west() {
        assert_eq!(
            tile_filename(-60, -180),
            "Copernicus_DSM_COG_10_S60_00_W180_00_DEM.tif"
        );
    }

    // ---- tile_key -----------------------------------------------------------

    #[test]
    fn key_interior_north_east() {
        assert_eq!(tile_key(47.5, -121.3), (47, -122));
    }

    #[test]
    fn key_interior_south_west() {
        assert_eq!(tile_key(-0.5, -0.5), (-1, -1));
    }

    #[test]
    fn key_integer_lat_maps_to_owning_tile() {
        assert_eq!(tile_key(48.0, -122.0), (48, -122));
        assert_eq!(tile_key(47.0, -122.0), (47, -122));
    }

    #[test]
    fn key_exact_east_boundary() {
        assert_eq!(tile_key(47.5, -121.0), (47, -121));
        assert_eq!(tile_key(47.5, -122.0), (47, -122));
    }

    #[test]
    fn key_just_south_of_equator() {
        assert_eq!(tile_key(-0.0001, 0.0), (-1, 0));
    }

    // ---- copernicus_url -----------------------------------------------------

    #[test]
    fn url_contains_s3_domain_and_tile_name() {
        let url = copernicus_url(47, -122);
        assert!(url.contains("copernicus-dem-30m.s3.amazonaws.com"), "{url}");
        assert!(url.contains("N47"), "{url}");
        assert!(url.contains("W122"), "{url}");
        assert!(url.ends_with(".tif"), "{url}");
    }

    // ---- DemCache path helpers ---------------------------------------------

    #[test]
    fn cache_tile_path_matches_filename() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let path = cache.tile_path(47, -122);
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "Copernicus_DSM_COG_10_N47_00_W122_00_DEM.tif"
        );
        assert!(path.starts_with(dir.path()));
    }

    #[test]
    fn has_tile_false_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        assert!(!cache.has_tile(47, -122));
    }

    #[test]
    fn has_tile_true_when_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        std::fs::write(cache.tile_path(47, -122), b"placeholder").expect("write");
        assert!(cache.has_tile(47, -122));
    }

    #[test]
    fn get_tile_missing_returns_tile_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let err = cache.get_tile(47, -122).unwrap_err();
        assert!(
            matches!(err, DemError::TileNotFound(_)),
            "expected TileNotFound, got {err:?}"
        );
    }

    #[test]
    fn get_tile_missing_error_contains_tile_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let err = cache.get_tile(47, -122).unwrap_err();
        // Error must name the missing tile unambiguously.
        assert!(err.to_string().contains("N47"), "{err}");
        assert!(err.to_string().contains("W122"), "{err}");
    }

    #[test]
    fn get_tile_missing_error_contains_download_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let err = cache.get_tile(47, -122).unwrap_err();
        assert!(
            err.to_string().contains("copernicus-dem-30m.s3.amazonaws.com"),
            "error should contain Copernicus S3 URL: {err}"
        );
    }

    // ---- DemProvider -------------------------------------------------------

    #[test]
    fn provider_missing_tile_returns_tile_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let provider = DemProvider::with_cache(cache);
        let err = provider.elevation_at(47.5, -121.5).unwrap_err();
        assert!(
            matches!(err, DemError::TileNotFound(_)),
            "expected TileNotFound, got {err:?}"
        );
    }

    #[test]
    fn provider_nan_lat_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let provider = DemProvider::with_cache(cache);
        assert!(provider.elevation_at(f64::NAN, 0.0).is_err());
    }

    // ---- WGS84 bounds guard ------------------------------------------------

    #[test]
    fn provider_lat_above_90_returns_outside_coverage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let provider = DemProvider::with_cache(cache);
        let err = provider.elevation_at(91.0, 0.0).unwrap_err();
        assert!(
            matches!(err, DemError::OutsideCoverage { .. }),
            "expected OutsideCoverage for lat=91, got {err:?}"
        );
    }

    #[test]
    fn provider_lat_below_minus90_returns_outside_coverage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let provider = DemProvider::with_cache(cache);
        let err = provider.elevation_at(-91.0, 0.0).unwrap_err();
        assert!(
            matches!(err, DemError::OutsideCoverage { .. }),
            "expected OutsideCoverage for lat=-91, got {err:?}"
        );
    }

    #[test]
    fn provider_lat_61n_is_valid_copernicus_coverage() {
        /*
         * Copernicus DEM covers latitudes well above 60°N (unlike SRTM which
         * stops at 60°N). With download disabled, lat=61° returns TileNotFound
         * (tile absent from cache), NOT OutsideCoverage.
         */
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let provider = DemProvider::with_cache(cache);
        let err = provider.elevation_at(61.0, 0.0).unwrap_err();
        assert!(
            matches!(err, DemError::TileNotFound(_)),
            "expected TileNotFound (not OutsideCoverage) for lat=61 with Copernicus, got {err:?}"
        );
    }

    #[test]
    fn provider_lon_at_180_normalizes_to_w180_tile() {
        /*
         * lon=180.0 is the antimeridian. After normalization to −180.0 it maps
         * to tile W180. With download disabled, we expect TileNotFound (tile
         * absent) — NOT OutsideCoverage (invalid coordinate).
         */
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = DemCache::with_dir(dir.path().to_path_buf()).expect("cache");
        let provider = DemProvider::with_cache(cache);
        let err = provider.elevation_at(0.0, 180.0).unwrap_err();
        assert!(
            matches!(err, DemError::TileNotFound(_)),
            "expected TileNotFound (missing W180 tile) after normalization, got {err:?}"
        );
        assert!(
            err.to_string().contains("W180"),
            "error should name W180 tile: {err}"
        );
    }
}
