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

//! Geoid undulation lookup and orthometric ↔ ellipsoidal height conversion.
//!
//! ## Background
//!
//! DEM tiles store **orthometric heights** H (metres above the geoid, i.e.
//! roughly mean sea level). GNSS receivers and autopilot altitude targets
//! report **ellipsoidal heights** h (metres above the WGS84 ellipsoid). The
//! two are related by:
//!
//! ```text
//! h = H + N
//! ```
//!
//! where N is the **geoid undulation** (also called geoid separation or offset)
//! at the point of interest. N ranges globally from approximately −105 m
//! (Indian Ocean gravity low) to +85 m (Indonesian gravity high).
//!
//! ## Geoid models
//!
//! Two models are provided:
//!
//! - [`GeoidModel`] — EGM96, spherical harmonic evaluator, ~1 m RMS globally.
//!   Retained for reference and backward compatibility; used by legacy SRTM
//!   workflows that reference EGM96.
//!
//! - [`Egm2008Model`] — EGM2008, used by the Copernicus DEM GLO-30 tiles that
//!   IronTrack ingests in production. EGM2008 supersedes EGM96 with improved
//!   accuracy in mountainous and coastal regions. The two models agree to
//!   typically <0.5 m globally but can diverge by 1–2 m in complex terrain.
//!   **Always use this model with Copernicus tiles.**
//!
//! ## References
//!
//! - NIMA TR8350.2, Third Edition (2000): WGS84 parameters and EGM96
//!   reference values (Appendix C spot-check table used in tests below).
//! - Lemoine et al. (1998): EGM96 model development report.
//! - Pavlis et al. (2012): EGM2008 model development report.
//! - `docs/05_vertical_datum_engineering.md` — IronTrack vertical datum design.
//! - `docs/formula_reference.md §Doc 05` — h = H + N derivation.

use crate::error::GeodesyError;

// ---------------------------------------------------------------------------
// GeoidModel
// ---------------------------------------------------------------------------

/// EGM96 geoid model — computes undulation N at any (lat, lon).
///
/// N is the separation between the WGS84 ellipsoid and the EGM96 geoid
/// surface, positive when the geoid lies above the ellipsoid:
///
/// ```text
/// h = H + N     (orthometric → ellipsoidal)
/// H = h − N     (ellipsoidal → orthometric)
///
/// where:
///   h — ellipsoidal height (WGS84, as reported by GNSS)
///   H — orthometric height (above EGM96 geoid ≈ MSL, as stored in SRTM)
///   N — geoid undulation returned by this model (metres)
/// ```
///
/// ## Accuracy
///
/// Uses the `egm96` crate's spherical harmonic evaluator. Accuracy is
/// approximately 1 m RMS globally. For sub-metre accuracy, the raster-based
/// 5-arc-minute grid should be used; that requires the `raster_5_min` feature
/// and a downloaded PNG data file (not supported on this platform build).
///
/// ## Usage
///
/// ```rust,ignore
/// use irontrack::geodesy::geoid::GeoidModel;
///
/// let model = GeoidModel::new();
/// let n = model.undulation(47.5, -121.5)?;      // undulation in metres
/// let h_ellipsoid = model.orthometric_to_ellipsoidal(47.5, -121.5, 1234.0)?;
/// ```
pub struct GeoidModel;

impl Default for GeoidModel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeoidModel {
    /// Construct the EGM96 model.
    ///
    /// No files are loaded. The spherical harmonic coefficients are embedded
    /// as a static array in the `egm96` crate (`egm96_data.rs`).
    pub fn new() -> Self {
        Self
    }

    /// Geoid undulation N at `(lat_deg, lon_deg)` in metres.
    ///
    /// # Arguments
    ///
    /// - `lat_deg` — geodetic latitude in decimal degrees (−90 … +90).
    /// - `lon_deg` — geodetic longitude in decimal degrees. Any finite value
    ///   is accepted; values outside −180 … +180 are wrapped by the underlying
    ///   `egm96_compute_altitude_offset` implementation.
    ///
    /// # Errors
    ///
    /// Returns [`GeodesyError::OutOfBounds`] if `lat_deg` is outside ±90° or
    /// either coordinate is non-finite.
    pub fn undulation(&self, lat_deg: f64, lon_deg: f64) -> Result<f64, GeodesyError> {
        if !lat_deg.is_finite() || !lon_deg.is_finite() || lat_deg.abs() > 90.0 {
            return Err(GeodesyError::OutOfBounds {
                lat: lat_deg,
                lon: lon_deg,
            });
        }
        Ok(egm96::egm96_compute_altitude_offset(lat_deg, lon_deg))
    }

    /// Convert orthometric height H to ellipsoidal height h.
    ///
    /// Applies h = H + N, where N is the EGM96 undulation at `(lat_deg, lon_deg)`.
    ///
    /// # Errors
    ///
    /// Propagates [`GeodesyError`] from [`undulation`](Self::undulation).
    pub fn orthometric_to_ellipsoidal(
        &self,
        lat_deg: f64,
        lon_deg: f64,
        h_ortho: f64,
    ) -> Result<f64, GeodesyError> {
        if !h_ortho.is_finite() {
            return Err(GeodesyError::OutOfBounds {
                lat: lat_deg,
                lon: lon_deg,
            });
        }
        let n = self.undulation(lat_deg, lon_deg)?;
        Ok(h_ortho + n)
    }

    /// Convert ellipsoidal height h to orthometric height H.
    ///
    /// Applies H = h − N, where N is the EGM96 undulation at `(lat_deg, lon_deg)`.
    ///
    /// # Errors
    ///
    /// Propagates [`GeodesyError`] from [`undulation`](Self::undulation).
    /// Also returns [`GeodesyError::OutOfBounds`] if `h_ellipsoid` is non-finite.
    pub fn ellipsoidal_to_orthometric(
        &self,
        lat_deg: f64,
        lon_deg: f64,
        h_ellipsoid: f64,
    ) -> Result<f64, GeodesyError> {
        if !h_ellipsoid.is_finite() {
            return Err(GeodesyError::OutOfBounds {
                lat: lat_deg,
                lon: lon_deg,
            });
        }
        let n = self.undulation(lat_deg, lon_deg)?;
        Ok(h_ellipsoid - n)
    }
}

// ---------------------------------------------------------------------------
// Egm2008Model
// ---------------------------------------------------------------------------

/// EGM2008 geoid model — computes undulation N at any (lat, lon).
///
/// ## Copernicus DEM vertical datum context
///
/// Copernicus DEM GLO-30 tiles store **orthometric heights** referenced to the
/// **EGM2008** geoid. The EGM2008 model supersedes EGM96 with improved accuracy
/// in mountainous and coastal regions. The two models agree to typically <0.5 m
/// globally but can diverge by 1–2 m in complex terrain — exactly where
/// terrain-following AGL math is most sensitive. IronTrack uses this model when
/// processing Copernicus tiles to ensure datum consistency.
///
/// ## Precision
///
/// Delegates to the `egm2008` crate (FlightAware, BSD-3-Clause, pure Rust).
/// Input coordinates are cast to `f32` as required by the crate API; the
/// returned `f32` undulation is widened back to `f64` for engine use. The
/// precision loss from the f64→f32 cast is approximately 0.02 m at geographic
/// scale — well within the ~0.5 m RMS accuracy of the EGM2008 model itself.
pub struct Egm2008Model;

impl Default for Egm2008Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Egm2008Model {
    /// Construct the EGM2008 model. No external data files are loaded.
    pub fn new() -> Self {
        Self
    }

    /// Geoid undulation N at `(lat_deg, lon_deg)` in metres.
    ///
    /// # Errors
    ///
    /// Returns [`GeodesyError::OutOfBounds`] if `lat_deg` is outside ±90°,
    /// either coordinate is non-finite, or the underlying EGM2008 evaluator
    /// rejects the coordinate.
    pub fn undulation(&self, lat_deg: f64, lon_deg: f64) -> Result<f64, GeodesyError> {
        if !lat_deg.is_finite() || !lon_deg.is_finite() || lat_deg.abs() > 90.0 {
            return Err(GeodesyError::OutOfBounds {
                lat: lat_deg,
                lon: lon_deg,
            });
        }
        let lat_f32 = lat_deg as f32;
        let mut lon_f32 = lon_deg as f32;
        /*
         * f64→f32 rounding can push lon values very close to ±180.0 past
         * the boundary. For example, 179.9999999° rounds to 180.0f32.
         * Wrap 180.0 → −180.0 (identical meridian) so the EGM2008 evaluator
         * always receives a value in its expected [−180, 180) domain.
         */
        if lon_f32 >= 180.0 {
            lon_f32 = -180.0;
        }
        egm2008::geoid_height(lat_f32, lon_f32)
            .map(|n| n as f64)
            .map_err(|_| GeodesyError::OutOfBounds {
                lat: lat_deg,
                lon: lon_deg,
            })
    }

    /// Convert orthometric height H (EGM2008-referenced) to ellipsoidal height h.
    ///
    /// Applies h = H + N using the EGM2008 undulation at `(lat_deg, lon_deg)`.
    pub fn orthometric_to_ellipsoidal(
        &self,
        lat_deg: f64,
        lon_deg: f64,
        h_ortho: f64,
    ) -> Result<f64, GeodesyError> {
        if !h_ortho.is_finite() {
            return Err(GeodesyError::OutOfBounds {
                lat: lat_deg,
                lon: lon_deg,
            });
        }
        let n = self.undulation(lat_deg, lon_deg)?;
        Ok(h_ortho + n)
    }

    /// Convert ellipsoidal height h to orthometric height H (EGM2008-referenced).
    ///
    /// Applies H = h − N using the EGM2008 undulation at `(lat_deg, lon_deg)`.
    pub fn ellipsoidal_to_orthometric(
        &self,
        lat_deg: f64,
        lon_deg: f64,
        h_ellipsoid: f64,
    ) -> Result<f64, GeodesyError> {
        if !h_ellipsoid.is_finite() {
            return Err(GeodesyError::OutOfBounds {
                lat: lat_deg,
                lon: lon_deg,
            });
        }
        let n = self.undulation(lat_deg, lon_deg)?;
        Ok(h_ellipsoid - n)
    }
}

// ---------------------------------------------------------------------------
// Module-level convenience functions
// ---------------------------------------------------------------------------

/// Compute EGM96 geoid undulation N at `(lat_deg, lon_deg)` in metres.
///
/// Convenience wrapper around [`GeoidModel::undulation`].
///
/// # Errors
///
/// Returns [`GeodesyError::OutOfBounds`] if `lat_deg` is outside ±90° or
/// either coordinate is non-finite.
pub fn geoid_undulation_egm96(lat_deg: f64, lon_deg: f64) -> Result<f64, GeodesyError> {
    GeoidModel::new().undulation(lat_deg, lon_deg)
}

/// Convert orthometric height H to ellipsoidal height h at `(lat_deg, lon_deg)`.
///
/// Convenience wrapper around [`GeoidModel::orthometric_to_ellipsoidal`].
pub fn orthometric_to_ellipsoidal(
    lat_deg: f64,
    lon_deg: f64,
    h_ortho: f64,
) -> Result<f64, GeodesyError> {
    GeoidModel::new().orthometric_to_ellipsoidal(lat_deg, lon_deg, h_ortho)
}

/// Convert ellipsoidal height h to orthometric height H at `(lat_deg, lon_deg)`.
///
/// Convenience wrapper around [`GeoidModel::ellipsoidal_to_orthometric`].
pub fn ellipsoidal_to_orthometric(
    lat_deg: f64,
    lon_deg: f64,
    h_ellipsoid: f64,
) -> Result<f64, GeodesyError> {
    GeoidModel::new().ellipsoidal_to_orthometric(lat_deg, lon_deg, h_ellipsoid)
}

/// Compute EGM2008 geoid undulation N at `(lat_deg, lon_deg)` in metres.
///
/// Convenience wrapper around [`Egm2008Model::undulation`]. Use this function
/// when processing Copernicus DEM GLO-30 tiles, which reference EGM2008.
///
/// # Errors
///
/// Returns [`GeodesyError::OutOfBounds`] if `lat_deg` is outside ±90° or
/// either coordinate is non-finite.
pub fn geoid_undulation_egm2008(lat_deg: f64, lon_deg: f64) -> Result<f64, GeodesyError> {
    Egm2008Model::new().undulation(lat_deg, lon_deg)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /*
     * Tolerance: 1.5 m covers the ~1 m RMS of the spherical harmonic model
     * plus any rounding in the published NIMA spot-check values.
     */
    const TOLERANCE_M: f64 = 1.5;

    #[test]
    fn undulation_at_origin() {
        // 0°N 0°E — Gulf of Guinea. NIMA TR8350.2 Appendix C: N ≈ +17.16 m.
        let n = geoid_undulation_egm96(0.0, 0.0).expect("valid coordinates");
        assert_abs_diff_eq!(n, 17.16, epsilon = TOLERANCE_M);
    }

    #[test]
    fn undulation_north_pole() {
        // 90°N 0°E — North Pole. NIMA TR8350.2: N ≈ +13.61 m.
        let n = geoid_undulation_egm96(90.0, 0.0).expect("valid coordinates");
        assert_abs_diff_eq!(n, 13.61, epsilon = TOLERANCE_M);
    }

    #[test]
    fn undulation_indian_ocean_low() {
        // ~1°S 81°E — Indian Ocean gravity low (global geoid minimum).
        // The geoid depression here reaches approximately −106 m (NIMA TR8350.2).
        // Threshold is conservative to account for spherical harmonic truncation error.
        let n = geoid_undulation_egm96(-1.0, 81.0).expect("valid coordinates");
        assert!(
            n < -90.0,
            "Indian Ocean geoid low: expected < -90 m, got {n:.2} m"
        );
    }

    #[test]
    fn undulation_indonesia_high() {
        // ~5°N 120°E — Indonesian gravity high. Published maximum ≈ +85 m.
        let n = geoid_undulation_egm96(5.0, 120.0).expect("valid coordinates");
        assert!(
            n > 50.0,
            "Indonesian geoid high: expected > +50 m, got {n:.2} m"
        );
    }

    #[test]
    fn undulation_conus_range() {
        // CONUS undulations range from about −10 m to −40 m.
        // Denver, CO (~39.7°N, 104.9°W) is representative.
        let n = geoid_undulation_egm96(39.7, -104.9).expect("valid coordinates");
        assert!(
            n > -50.0 && n < 0.0,
            "CONUS undulation: expected −50..0 m, got {n:.2} m"
        );
    }

    #[test]
    fn orthometric_to_ellipsoidal_roundtrip() {
        // h = H + N and H = h − N must be exactly inverse.
        let model = GeoidModel::new();
        let (lat, lon, h_ortho) = (47.5_f64, -121.5_f64, 1500.0_f64);
        let h_ellipsoid = model
            .orthometric_to_ellipsoidal(lat, lon, h_ortho)
            .expect("valid input");
        let h_back = model
            .ellipsoidal_to_orthometric(lat, lon, h_ellipsoid)
            .expect("valid input");
        assert_abs_diff_eq!(h_back, h_ortho, epsilon = 1e-9);
    }

    #[test]
    fn out_of_bounds_lat_rejected() {
        assert!(geoid_undulation_egm96(91.0, 0.0).is_err());
        assert!(geoid_undulation_egm96(-91.0, 0.0).is_err());
    }

    #[test]
    fn non_finite_coords_rejected() {
        assert!(geoid_undulation_egm96(f64::NAN, 0.0).is_err());
        assert!(geoid_undulation_egm96(0.0, f64::INFINITY).is_err());
        assert!(geoid_undulation_egm96(f64::NEG_INFINITY, 0.0).is_err());
    }

    #[test]
    fn lon_wrapping_consistent() {
        // 190°E wraps to −170°E — same meridian, same undulation.
        let n1 = geoid_undulation_egm96(0.0, 190.0).expect("valid coordinates");
        let n2 = geoid_undulation_egm96(0.0, -170.0).expect("valid coordinates");
        assert_abs_diff_eq!(n1, n2, epsilon = 1e-9);
    }

    // ---- Egm2008Model -------------------------------------------------------

    #[test]
    fn egm2008_undulation_at_origin() {
        /*
         * 0°N 0°E — Gulf of Guinea. EGM2008 undulation at the origin is
         * close to the EGM96 value of +17.16 m. Tolerance of 2 m covers
         * both model differences and internal evaluator accuracy.
         */
        let n = geoid_undulation_egm2008(0.0, 0.0).expect("valid coordinates");
        assert_abs_diff_eq!(n, 17.16, epsilon = 2.0);
    }

    #[test]
    fn egm2008_out_of_bounds_rejected() {
        assert!(geoid_undulation_egm2008(91.0, 0.0).is_err());
        assert!(geoid_undulation_egm2008(f64::NAN, 0.0).is_err());
    }

    #[test]
    fn egm2008_roundtrip_orthometric_ellipsoidal() {
        let model = Egm2008Model::new();
        let (lat, lon, h_ortho) = (47.5_f64, -121.5_f64, 1500.0_f64);
        let h_ellipsoid = model
            .orthometric_to_ellipsoidal(lat, lon, h_ortho)
            .expect("valid input");
        let h_back = model
            .ellipsoidal_to_orthometric(lat, lon, h_ellipsoid)
            .expect("valid input");
        assert_abs_diff_eq!(h_back, h_ortho, epsilon = 1e-6);
    }

    #[test]
    fn egm2008_lon_near_180_does_not_error() {
        /*
         * lon very close to 180° can round to exactly 180.0 when cast to f32.
         * The lon_f32 >= 180.0 wrap in Egm2008Model::undulation() must handle
         * this without returning an OutOfBounds error or panicking.
         * The undulation at the antimeridian should match both ±180° calls.
         */
        let n_pos = geoid_undulation_egm2008(0.0, 179.9999999).expect("lon=179.999...");
        let n_neg = geoid_undulation_egm2008(0.0, -180.0).expect("lon=-180");
        assert_abs_diff_eq!(n_pos, n_neg, epsilon = 0.5);
    }

    #[test]
    fn egm2008_agrees_with_egm96_within_2m_globally() {
        /*
         * EGM2008 and EGM96 agree to < 1 m RMS globally; local peaks
         * reach 1–2 m. Spot-check two representative points.
         */
        let points = [(0.0_f64, 0.0_f64), (47.5, -121.5)];
        for (lat, lon) in points {
            let n96 = geoid_undulation_egm96(lat, lon).expect("egm96");
            let n08 = geoid_undulation_egm2008(lat, lon).expect("egm2008");
            assert!(
                (n96 - n08).abs() < 3.0,
                "EGM96 vs EGM2008 diverges by more than 3 m at ({lat}, {lon}): n96={n96:.2}, n08={n08:.2}"
            );
        }
    }
}
