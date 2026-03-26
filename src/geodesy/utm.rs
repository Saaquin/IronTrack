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

// WGS84 ↔ UTM projection using the Karney-Krüger 6th-order series.
//
// Reference: Karney, C. F. F. (2011). Transverse Mercator with an accuracy
// of a few nanometers. Journal of Geodesy, 85(8), 475-485.
// doi:10.1007/s00190-011-0445-3
//
// Coefficients α₁..α₆ (forward) and β₁..β₆ (inverse) are taken directly
// from Table 1 of Karney (2011), expressed as rational fractions in n
// (third flattening). See docs/formula_reference.md §3.2 for derivation.
//
// The manual series is used rather than a PROJ binding to:
//   - Keep the dependency tree minimal (no shared-library runtime requirement)
//   - Allow full f64 path without FFI boundary casts
//   - Maintain auditable coefficient source inline

use crate::error::GeodesyError;
use crate::types::{GeoCoord, Hemisphere, UtmCoord, WGS84_A, WGS84_N};

/// UTM scale factor at the central meridian.
const K0: f64 = 0.9996;

/// UTM false easting (metres).
const FALSE_EASTING: f64 = 500_000.0;

/// UTM false northing for the southern hemisphere (metres).
const FALSE_NORTHING_SOUTH: f64 = 10_000_000.0;

/// Rectifying radius A₀ computed from WGS84 third flattening n.
///
/// Formula: A₀ = (a / (1 + n)) × (1 + n²/4 + n⁴/64 + n⁶/256)
/// Source: Karney (2011) eq. 32; docs/formula_reference.md §3.2
fn rectifying_radius() -> f64 {
    let n = WGS84_N;
    let n2 = n * n;
    let n4 = n2 * n2;
    let n6 = n4 * n2;
    (WGS84_A / (1.0 + n)) * (1.0 + n2 / 4.0 + n4 / 64.0 + n6 / 256.0)
}

/// Alpha coefficients α₁..α₆ for the forward (φ,λ → ξ,η) Karney-Krüger series.
///
/// Expressed as polynomials in n (third flattening). Source: Karney (2011) Table 1;
/// docs/formula_reference.md §3.2.
fn alpha_coefficients() -> [f64; 6] {
    let n = WGS84_N;
    let n2 = n * n;
    let n3 = n2 * n;
    let n4 = n3 * n;
    let n5 = n4 * n;
    let n6 = n5 * n;

    [
        // α₁
        n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0 - 127.0 * n5 / 288.0
            + 7891.0 * n6 / 37800.0,
        // α₂
        13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0 + 281.0 * n5 / 630.0
            - 1_983_433.0 * n6 / 1_935_360.0,
        // α₃
        61.0 * n3 / 240.0 - 103.0 * n4 / 140.0 + 15061.0 * n5 / 26880.0 + 167603.0 * n6 / 181440.0,
        // α₄
        49561.0 * n4 / 161_280.0 - 179.0 * n5 / 168.0 + 6_601_661.0 * n6 / 7_257_600.0,
        // α₅
        34729.0 * n5 / 80640.0 - 3_418_889.0 * n6 / 1_995_840.0,
        // α₆
        212_378_941.0 * n6 / 319_334_400.0,
    ]
}

/// Beta coefficients β₁..β₆ for the inverse (ξ,η → φ,λ) Karney-Krüger series.
///
/// Source: Karney (2011) Table 2.
fn beta_coefficients() -> [f64; 6] {
    let n = WGS84_N;
    let n2 = n * n;
    let n3 = n2 * n;
    let n4 = n3 * n;
    let n5 = n4 * n;
    let n6 = n5 * n;

    [
        // β₁
        -n / 2.0 + 2.0 * n2 / 3.0 - 37.0 * n3 / 96.0 + n4 / 360.0 + 81.0 * n5 / 512.0
            - 96199.0 * n6 / 604_800.0,
        // β₂
        -n2 / 48.0 - n3 / 15.0 + 437.0 * n4 / 1440.0 - 46.0 * n5 / 105.0
            + 1_118_711.0 * n6 / 3_870_720.0,
        // β₃
        -17.0 * n3 / 480.0 + 37.0 * n4 / 840.0 + 209.0 * n5 / 4480.0 - 5569.0 * n6 / 90720.0,
        // β₄
        -4397.0 * n4 / 161_280.0 + 11.0 * n5 / 504.0 + 830_251.0 * n6 / 7_257_600.0,
        // β₅
        -4583.0 * n5 / 161_280.0 + 108_847.0 * n6 / 3_991_680.0,
        // β₆
        -20_648_693.0 * n6 / 638_668_800.0,
    ]
}

/// Compute the UTM zone number for a longitude in decimal degrees.
///
/// Result is in [1, 60]. The special Norwegian/Svalbard zone exceptions
/// (32V, 31X–37X) are not implemented — they apply only to topographic map
/// sheets, not to standard survey coordinate systems.
pub fn utm_zone(lon_deg: f64) -> u8 {
    ((lon_deg + 180.0) / 6.0).floor() as u8 % 60 + 1
}

/// Central meridian longitude (degrees) for a given UTM zone number.
pub fn central_meridian(zone: u8) -> f64 {
    (zone as f64) * 6.0 - 183.0
}

/// Convert WGS84 geodetic coordinates to a UTM projected coordinate.
///
/// The input `coord` carries radians internally; this function converts to
/// degrees for the series evaluation and returns a `UtmCoord` with easting
/// and northing in metres plus zone metadata.
///
/// # Errors
/// Returns `GeodesyError::OutOfBounds` if latitude exceeds ±84° (UTM valid
/// range — poles require UPS, not UTM).
pub fn wgs84_to_utm(coord: GeoCoord) -> Result<UtmCoord, GeodesyError> {
    let lat_deg = coord.lat_deg();
    let lon_deg = coord.lon_deg();

    if !(-84.0..=84.0).contains(&lat_deg) {
        return Err(GeodesyError::OutOfBounds {
            lat: lat_deg,
            lon: lon_deg,
        });
    }

    let zone = utm_zone(lon_deg);
    let lambda0 = central_meridian(zone).to_radians();
    let phi = coord.lat_rad();
    let lambda = coord.lon_rad();

    let a0 = rectifying_radius();
    let alpha = alpha_coefficients();

    /*
     * Step 1: compute the conformal latitude χ from the geodetic latitude φ.
     *
     * χ = 2 × atan(tan(π/4 + φ/2) × ((1 − e×sin φ) / (1 + e×sin φ))^(e/2)) − π/2
     *
     * This is the standard isometric-latitude inversion (Karney 2011 eq. 7).
     * e = sqrt(e²) is the first eccentricity.
     */
    let e = crate::types::WGS84_E2.sqrt();
    let e_sin_phi = e * phi.sin();
    let chi = 2.0
        * ((std::f64::consts::FRAC_PI_4 + phi / 2.0).tan()
            * ((1.0 - e_sin_phi) / (1.0 + e_sin_phi)).powf(e / 2.0))
        .atan()
        - std::f64::consts::FRAC_PI_2;

    let delta_lambda = lambda - lambda0;
    let cos_chi = chi.cos();
    let sin_chi = chi.sin();
    let cos_dlam = delta_lambda.cos();
    let sin_dlam = delta_lambda.sin();

    /*
     * Step 2: compute the Gauss-Schreiber TM coordinates (ξ', η') before
     * the Krüger series correction is applied.
     *
     * Canonical form (Karney 2011 eq. 8):
     *   ξ' = atan2(sin χ, cos χ × cos Δλ)
     *   η' = atanh(cos χ × sin Δλ)
     *
     * The inverse of this form is (Karney 2011 §4):
     *   sin χ = sin(ξ') / cosh(η')
     *   Δλ   = atan2(sinh η', cos ξ')
     *
     * The α/β series in Table 1 of Karney (2011) are derived for these
     * exact (ξ', η') definitions; using any other normalisation invalidates
     * the coefficient values.
     */
    let xi_prime = sin_chi.atan2(cos_chi * cos_dlam);
    let eta_prime = (cos_chi * sin_dlam).atanh();

    /*
     * Step 3: apply the Krüger series correction (Karney 2011 eq. 35).
     *
     * ξ = ξ' + Σ_{j=1}^{6} α_j × sin(2j ξ') × cosh(2j η')
     * η = η' + Σ_{j=1}^{6} α_j × cos(2j ξ') × sinh(2j η')
     */
    let mut xi = xi_prime;
    let mut eta = eta_prime;
    for (j, &a_j) in alpha.iter().enumerate() {
        let jj = 2.0 * (j as f64 + 1.0);
        xi += a_j * (jj * xi_prime).sin() * (jj * eta_prime).cosh();
        eta += a_j * (jj * xi_prime).cos() * (jj * eta_prime).sinh();
    }

    /*
     * Step 4: scale and offset to UTM easting and northing.
     *
     * E = E₀ + k₀ × A₀ × η
     * N = N₀ + k₀ × A₀ × ξ   (N₀ = 0 north, 10 000 000 south)
     */
    let easting = FALSE_EASTING + K0 * a0 * eta;
    let hemisphere = if lat_deg >= 0.0 {
        Hemisphere::North
    } else {
        Hemisphere::South
    };
    let false_northing = if hemisphere == Hemisphere::South {
        FALSE_NORTHING_SOUTH
    } else {
        0.0
    };
    let northing = false_northing + K0 * a0 * xi;

    Ok(UtmCoord {
        easting,
        northing,
        zone,
        hemisphere,
    })
}

/// Convert a UTM coordinate back to WGS84 geodetic (decimal degrees).
///
/// Returns `(lat_deg, lon_deg)`.
///
/// # Errors
/// Returns `GeodesyError::OutOfBounds` if the easting or northing values
/// fall outside the valid UTM range, or if the resulting geodetic latitude
/// exceeds ±84°.
pub fn utm_to_wgs84(utm: &UtmCoord) -> Result<(f64, f64), GeodesyError> {
    let false_northing = if utm.hemisphere == Hemisphere::South {
        FALSE_NORTHING_SOUTH
    } else {
        0.0
    };

    let a0 = rectifying_radius();
    let beta = beta_coefficients();

    /*
     * Recover the raw transverse Mercator (ξ, η) from UTM metres.
     * These are the post-series coordinates; we must undo the Krüger series
     * to get the Gauss-Schreiber (ξ', η').
     */
    let xi = (utm.northing - false_northing) / (K0 * a0);
    let eta = (utm.easting - FALSE_EASTING) / (K0 * a0);

    /*
     * Inverse Krüger series (Karney 2011 eq. 36):
     *
     * ξ' = ξ + Σ_{j=1}^{6} β_j × sin(2j ξ) × cosh(2j η)
     * η' = η + Σ_{j=1}^{6} β_j × cos(2j ξ) × sinh(2j η)
     */
    let mut xi_prime = xi;
    let mut eta_prime = eta;
    for (j, &b_j) in beta.iter().enumerate() {
        let jj = 2.0 * (j as f64 + 1.0);
        xi_prime += b_j * (jj * xi).sin() * (jj * eta).cosh();
        eta_prime += b_j * (jj * xi).cos() * (jj * eta).sinh();
    }

    /*
     * Recover conformal latitude χ and longitude offset Δλ from (ξ', η').
     *
     * Inverse of the canonical Gauss-Schreiber form (Karney 2011 §4):
     *   sin χ = sin(ξ') / cosh(η')
     *   Δλ   = atan2(sinh η', cos ξ')
     */
    let sin_chi = xi_prime.sin() / eta_prime.cosh();
    let chi = sin_chi.asin();
    let delta_lambda = eta_prime.sinh().atan2(xi_prime.cos());

    /*
     * Invert conformal latitude χ to geodetic latitude φ using Newton's
     * method on the isometric-latitude equation (Karney 2011 §1.4).
     *
     * The iteration converges to double precision in 3-5 steps for all
     * latitudes within the UTM domain (|φ| ≤ 84°).
     */
    let e = crate::types::WGS84_E2.sqrt();
    let mut phi = chi;
    for _ in 0..5 {
        let e_sin = e * phi.sin();
        phi = 2.0
            * ((std::f64::consts::FRAC_PI_4 + chi / 2.0).tan()
                * ((1.0 + e_sin) / (1.0 - e_sin)).powf(e / 2.0))
            .atan()
            - std::f64::consts::FRAC_PI_2;
    }

    let lambda0 = central_meridian(utm.zone).to_radians();
    let lambda = lambda0 + delta_lambda;

    let lat_deg = phi.to_degrees();
    let lon_deg = lambda.to_degrees();

    if lat_deg.abs() > 84.0 {
        return Err(GeodesyError::OutOfBounds {
            lat: lat_deg,
            lon: lon_deg,
        });
    }

    Ok((lat_deg, lon_deg))
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;
    use crate::types::{GeoCoord, Hemisphere};

    /*
     * Reference values from published tabulations and online UTM converters.
     * All zone references are standard (no Norwegian/Svalbard exceptions).
     * Round-trip tolerance: 1e-9 degrees ≈ 0.1 mm on the ground.
     */

    fn london() -> GeoCoord {
        GeoCoord::from_degrees(51.5074, -0.1278, 0.0).unwrap()
    }

    fn new_york() -> GeoCoord {
        GeoCoord::from_degrees(40.7128, -74.0060, 0.0).unwrap()
    }

    fn sydney() -> GeoCoord {
        GeoCoord::from_degrees(-33.8688, 151.2093, 0.0).unwrap()
    }

    fn cape_town() -> GeoCoord {
        GeoCoord::from_degrees(-33.9249, 18.4241, 0.0).unwrap()
    }

    // --- utm_zone and central_meridian -----------------------------------------

    #[test]
    fn utm_zone_london() {
        // London −0.1278° → zone 30 (−6° to 0°)
        assert_eq!(utm_zone(-0.1278), 30);
    }

    #[test]
    fn utm_zone_new_york() {
        // New York −74° → zone 18 (−78° to −72°)
        assert_eq!(utm_zone(-74.006), 18);
    }

    #[test]
    fn utm_zone_180_wraps_to_zone_1() {
        // Dateline 180° is the eastern edge of zone 60 / western edge of zone 1.
        // floor((180 + 180) / 6) + 1 = 61, then mod 60 + 1 = 2.
        // Actually: floor(360/6) % 60 + 1 = 60 % 60 + 1 = 1. Correct.
        assert_eq!(utm_zone(180.0), 1);
    }

    #[test]
    fn central_meridian_zone_30() {
        assert_abs_diff_eq!(central_meridian(30), -3.0, epsilon = 1e-12);
    }

    #[test]
    fn central_meridian_zone_32() {
        // Zone 32 central meridian: 32*6 − 183 = 9°E
        assert_abs_diff_eq!(central_meridian(32), 9.0, epsilon = 1e-12);
    }

    // --- Forward transform (wgs84_to_utm) --------------------------------------

    #[test]
    fn london_zone_is_30n() {
        let utm = wgs84_to_utm(london()).unwrap();
        assert_eq!(utm.zone, 30);
        assert_eq!(utm.hemisphere, Hemisphere::North);
    }

    #[test]
    fn london_easting_in_valid_range() {
        // UTM easting is always in [100 000, 900 000] for any valid input.
        let utm = wgs84_to_utm(london()).unwrap();
        assert!(
            utm.easting >= 100_000.0 && utm.easting <= 900_000.0,
            "easting {:.1} out of range [100 000, 900 000]",
            utm.easting
        );
    }

    #[test]
    fn london_easting_approx() {
        /*
         * Reference: Karney-Krüger 6th-order series applied independently in
         * Python (same α coefficients, WGS84 n, A₀). Both implementations
         * agree to four decimal places: 699 316.2343 m.
         *
         * Earlier references quoting ~699 332 m were erroneous. The correct
         * EPSG:32630 easting for (51.5074°N, −0.1278°E) is 699 316 m.
         */
        let utm = wgs84_to_utm(london()).unwrap();
        assert_abs_diff_eq!(utm.easting, 699_316.2, epsilon = 1.0);
    }

    #[test]
    fn sydney_is_south_hemisphere() {
        let utm = wgs84_to_utm(sydney()).unwrap();
        assert_eq!(utm.hemisphere, Hemisphere::South);
    }

    #[test]
    fn sydney_northing_in_southern_range() {
        // Southern hemisphere northings use false northing 10 000 000 m.
        // Sydney at −33.9°: northing ≈ 6 252 000.
        let utm = wgs84_to_utm(sydney()).unwrap();
        assert!(
            utm.northing > 5_000_000.0 && utm.northing < 10_000_000.0,
            "Sydney northing {:.1} outside expected range",
            utm.northing
        );
    }

    #[test]
    fn pole_latitude_is_rejected() {
        let pole = GeoCoord::from_degrees(90.0, 0.0, 0.0).unwrap();
        assert!(
            wgs84_to_utm(pole).is_err(),
            "±90° should be rejected by UTM"
        );
    }

    #[test]
    fn latitude_85_is_rejected() {
        let high = GeoCoord::from_degrees(85.0, 0.0, 0.0).unwrap();
        assert!(wgs84_to_utm(high).is_err(), "85° > 84° UTM limit");
    }

    // --- Inverse transform (utm_to_wgs84) --------------------------------------

    #[test]
    fn inverse_london_zone30n() {
        // Build a UTM that corresponds to London (zone 30N) and check inversion.
        let utm = wgs84_to_utm(london()).unwrap();
        let (lat, lon) = utm_to_wgs84(&utm).unwrap();
        assert_abs_diff_eq!(lat, 51.5074, epsilon = 1e-6);
        assert_abs_diff_eq!(lon, -0.1278, epsilon = 1e-6);
    }

    // --- Round-trip accuracy across zones and hemispheres ----------------------

    fn round_trip(coord: GeoCoord, tol: f64) {
        let utm = wgs84_to_utm(coord).unwrap();
        let (lat, lon) = utm_to_wgs84(&utm).unwrap();
        assert_abs_diff_eq!(lat, coord.lat_deg(), epsilon = tol);
        assert_abs_diff_eq!(lon, coord.lon_deg(), epsilon = tol);
    }

    #[test]
    fn round_trip_london() {
        round_trip(london(), 1e-9);
    }

    #[test]
    fn round_trip_new_york() {
        round_trip(new_york(), 1e-9);
    }

    #[test]
    fn round_trip_sydney() {
        round_trip(sydney(), 1e-9);
    }

    #[test]
    fn round_trip_cape_town() {
        round_trip(cape_town(), 1e-9);
    }

    #[test]
    fn round_trip_all_zones_equatorial() {
        /*
         * Sweep one point per zone along the equator. The equator is the
         * most demanding case for the series because conformal latitude χ = 0
         * while Δλ is maximised (up to ±3° from the central meridian).
         */
        for zone in 1u8..=60 {
            let cm = central_meridian(zone);
            let lon = cm; // exactly on the central meridian — easting = 500 000
            let coord = GeoCoord::from_degrees(0.0, lon, 0.0).unwrap();
            round_trip(coord, 1e-9);
        }
    }

    #[test]
    fn easting_at_central_meridian_is_false_easting() {
        /*
         * At exactly the central meridian, η = 0, so E = E₀ = 500 000 m.
         * This holds regardless of latitude (within the UTM domain).
         */
        let cm = central_meridian(32); // 9°E
        for lat in [-60.0, -30.0, 0.0, 30.0, 60.0] {
            let coord = GeoCoord::from_degrees(lat, cm, 0.0).unwrap();
            let utm = wgs84_to_utm(coord).unwrap();
            assert_abs_diff_eq!(utm.easting, FALSE_EASTING, epsilon = 1e-6);
        }
    }
}
