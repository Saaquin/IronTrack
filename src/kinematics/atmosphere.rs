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

use geographiclib_rs::{Geodesic, InverseGeodesic};
use rayon::prelude::*;

use crate::error::KinematicsError;
use crate::photogrammetry::flightlines::FlightPlan;

/// Standard gravity (m/s^2), consistent with `math::dubins`.
const G: f64 = 9.806_65;

/// Absolute threshold below which wind is treated as zero.
const WIND_ZERO_THRESHOLD: f64 = 1e-6;

/// Maximum crab angle (degrees) before flagging a segment as potentially
/// unsafe for imagery — exceeds typical gimbal yaw compensation range.
const CRAB_ANGLE_WARNING_DEG: f64 = 20.0;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Atmospheric wind vector using meteorological convention.
///
/// The meteorological convention defines direction as the compass bearing
/// the wind blows FROM. A 270-degree wind blows from the west (westerly).
/// Speed is always non-negative.
#[derive(Debug, Clone, Copy)]
pub struct WindVector {
    /// Wind speed in m/s. Must be non-negative and finite.
    pub speed_ms: f64,
    /// Direction wind blows FROM in degrees clockwise from true north, [0, 360).
    /// Meteorological convention per Doc 16.
    pub direction_deg: f64,
}

/// Result of the wind triangle computation for a single flight segment.
#[derive(Debug, Clone, Copy)]
pub struct WindResult {
    /// Aircraft heading to maintain the desired ground track, degrees [0, 360).
    pub heading_deg: f64,
    /// Wind correction angle in degrees. Positive = heading right of track.
    pub wca_deg: f64,
    /// Resulting ground speed along the track, m/s.
    pub ground_speed_ms: f64,
}

// ---------------------------------------------------------------------------
// WindVector
// ---------------------------------------------------------------------------

impl WindVector {
    /// Validated constructor. Returns `Err` if speed is negative/non-finite
    /// or direction is non-finite.
    pub fn new(speed_ms: f64, direction_deg: f64) -> Result<Self, KinematicsError> {
        if !speed_ms.is_finite() || speed_ms < 0.0 {
            return Err(KinematicsError::InvalidParameter(format!(
                "wind speed must be non-negative and finite, got {speed_ms}"
            )));
        }
        if !direction_deg.is_finite() {
            return Err(KinematicsError::InvalidParameter(format!(
                "wind direction must be finite, got {direction_deg}"
            )));
        }
        // Normalize to [0, 360)
        let mut dir = direction_deg % 360.0;
        if dir < 0.0 {
            dir += 360.0;
        }
        Ok(Self {
            speed_ms,
            direction_deg: dir,
        })
    }

    /// True if wind is effectively zero (below threshold).
    pub fn is_calm(&self) -> bool {
        self.speed_ms < WIND_ZERO_THRESHOLD
    }
}

// ---------------------------------------------------------------------------
// Wind triangle solver
// ---------------------------------------------------------------------------

/// Solve the wind triangle: given True Airspeed, desired ground track,
/// and wind vector, compute heading, WCA, and ground speed.
///
/// # Convention
/// - `track_deg`: desired ground path direction, degrees clockwise from north
/// - `tas_ms`: true airspeed in m/s (must be positive and finite)
/// - `wind`: atmospheric wind vector (meteorological FROM convention)
///
/// # Algorithm (Doc 03, Doc 50)
///
/// ```text
/// theta = wind_from_rad - track_rad
/// sin_wca = (V_wind / TAS) * sin(theta)
/// WCA = arcsin(clamp(sin_wca, -1, 1))
/// HDG = TRK + WCA
/// GS = TAS * cos(WCA) - V_wind * cos(theta)
/// ```
///
/// # Errors
/// - `TrackInfeasible` if the crosswind component exceeds TAS
/// - `NegativeGroundSpeed` if headwind results in backward motion
/// - `InvalidParameter` if TAS is non-positive or non-finite
pub fn solve_wind_triangle(
    track_deg: f64,
    tas_ms: f64,
    wind: &WindVector,
) -> Result<WindResult, KinematicsError> {
    if !tas_ms.is_finite() || tas_ms <= 0.0 {
        return Err(KinematicsError::InvalidParameter(format!(
            "TAS must be positive and finite, got {tas_ms}"
        )));
    }

    // Degenerate case: calm wind
    if wind.is_calm() {
        let mut hdg = track_deg % 360.0;
        if hdg < 0.0 {
            hdg += 360.0;
        }
        return Ok(WindResult {
            heading_deg: hdg,
            wca_deg: 0.0,
            ground_speed_ms: tas_ms,
        });
    }

    let trk_rad = track_deg.to_radians();
    let wind_from_rad = wind.direction_deg.to_radians();

    // Angle between wind FROM direction and desired track
    let theta = wind_from_rad - trk_rad;

    // Wind correction angle (Doc 03 formula)
    let sin_wca = (wind.speed_ms / tas_ms) * theta.sin();

    // Check if crosswind component exceeds TAS (infeasible track)
    if sin_wca.abs() > 1.0 {
        return Err(KinematicsError::TrackInfeasible {
            track_deg,
            wind_ms: wind.speed_ms,
            tas_ms,
        });
    }

    // Clamp for floating-point robustness near +-1
    let wca_rad = sin_wca.clamp(-1.0, 1.0).asin();

    // Ground speed (Doc 50 formula with FROM convention)
    let gs = tas_ms * wca_rad.cos() - wind.speed_ms * theta.cos();

    // Negative ground speed means headwind exceeds forward progress
    if gs <= 0.0 {
        return Err(KinematicsError::NegativeGroundSpeed {
            track_deg,
            gs_ms: gs,
        });
    }

    // Normalize heading to [0, 360)
    let hdg_rad = trk_rad + wca_rad;
    let mut hdg_deg = hdg_rad.to_degrees() % 360.0;
    if hdg_deg < 0.0 {
        hdg_deg += 360.0;
    }

    Ok(WindResult {
        heading_deg: hdg_deg,
        wca_deg: wca_rad.to_degrees(),
        ground_speed_ms: gs,
    })
}

// ---------------------------------------------------------------------------
// Wind-corrected turn radius
// ---------------------------------------------------------------------------

/// Wind-corrected minimum turn radius (Doc 50).
///
/// Uses worst-case ground speed (full tailwind: V_TAS + V_wind) at maximum
/// bank angle. This expanded radius guarantees circular ground tracks are
/// flyable without over-banking on the downwind arc segment.
///
/// ```text
/// r_safe = (V_TAS + V_wind)^2 / (g * tan(phi_max))
/// ```
pub fn wind_corrected_radius(
    tas_ms: f64,
    wind_speed_ms: f64,
    max_bank_deg: f64,
) -> Result<f64, KinematicsError> {
    if !tas_ms.is_finite() || tas_ms <= 0.0 {
        return Err(KinematicsError::InvalidParameter(format!(
            "TAS must be positive and finite, got {tas_ms}"
        )));
    }
    if !wind_speed_ms.is_finite() || wind_speed_ms < 0.0 {
        return Err(KinematicsError::InvalidParameter(format!(
            "wind speed must be non-negative and finite, got {wind_speed_ms}"
        )));
    }
    if !max_bank_deg.is_finite() || max_bank_deg <= 0.0 || max_bank_deg >= 90.0 {
        return Err(KinematicsError::InvalidParameter(format!(
            "bank angle must be in (0, 90) degrees, got {max_bank_deg}"
        )));
    }

    let v_max = tas_ms + wind_speed_ms;
    let bank_rad = max_bank_deg.to_radians();
    Ok(v_max * v_max / (G * bank_rad.tan()))
}

// ---------------------------------------------------------------------------
// Per-segment ground speed computation
// ---------------------------------------------------------------------------

/// Compute ground speed for each segment of a flight line given wind.
///
/// For a line with N waypoints, returns N-1 ground speed values (one per
/// segment between consecutive waypoints). The ground track azimuth of
/// each segment is computed via Karney geodesic inverse.
///
/// # Arguments
/// - `lats_rad`: waypoint latitudes in radians
/// - `lons_rad`: waypoint longitudes in radians
/// - `tas_ms`: true airspeed in m/s
/// - `wind`: atmospheric wind vector
pub fn compute_segment_ground_speeds(
    lats_rad: &[f64],
    lons_rad: &[f64],
    tas_ms: f64,
    wind: &WindVector,
) -> Result<Vec<f64>, KinematicsError> {
    if lats_rad.len() != lons_rad.len() {
        return Err(KinematicsError::InvalidParameter(format!(
            "lats and lons must have equal length, got {} vs {}",
            lats_rad.len(),
            lons_rad.len()
        )));
    }
    if lats_rad.len() < 2 {
        return Ok(Vec::new());
    }

    let n = lats_rad.len() - 1;

    // Use rayon for bulk geodesic work (synchronous parallelism per arch rules).
    let results: Result<Vec<f64>, KinematicsError> = (0..n)
        .into_par_iter()
        .map(|i| {
            let geod = Geodesic::wgs84();
            let lat1 = lats_rad[i].to_degrees();
            let lon1 = lons_rad[i].to_degrees();
            let lat2 = lats_rad[i + 1].to_degrees();
            let lon2 = lons_rad[i + 1].to_degrees();

            let (_, azi1, _): (f64, f64, f64) =
                InverseGeodesic::inverse(&geod, lat1, lon1, lat2, lon2);

            // azi1 is forward azimuth in degrees [-180, 180]. Normalize to [0, 360).
            let mut track = azi1;
            if track < 0.0 {
                track += 360.0;
            }

            let result = solve_wind_triangle(track, tas_ms, wind)?;
            Ok(result.ground_speed_ms)
        })
        .collect();

    results
}

// ---------------------------------------------------------------------------
// Centralized wind application for FlightPlan
// ---------------------------------------------------------------------------

/// Apply wind triangle computation to all lines in a flight plan.
///
/// Computes per-segment ground speeds for every line and stores them via
/// `FlightLine::set_ground_speeds`. Logs a warning if any segment's
/// wind correction angle exceeds the gimbal compensation limit.
///
/// This is the single entry point for wind application — called by both
/// CLI and daemon API to avoid logic duplication.
pub fn apply_wind_to_plan(
    plan: &mut FlightPlan,
    tas_ms: f64,
    wind: &WindVector,
) -> Result<(), KinematicsError> {
    if wind.is_calm() {
        // Clear any stale ground speeds and crab angles from a previous wind application.
        for line in &mut plan.lines {
            line.clear_ground_speeds();
            line.clear_crab_angles();
        }
        return Ok(());
    }

    let mut crab_warnings = 0u32;

    for line in &mut plan.lines {
        if line.len() < 2 {
            continue;
        }

        let lats = line.lats();
        let lons = line.lons();
        let n = lats.len() - 1;

        // Use rayon for bulk geodesic inverse (synchronous parallelism per arch rules).
        let segment_results: Result<Vec<WindResult>, KinematicsError> = (0..n)
            .into_par_iter()
            .map(|i| {
                let geod = Geodesic::wgs84();
                let lat1 = lats[i].to_degrees();
                let lon1 = lons[i].to_degrees();
                let lat2 = lats[i + 1].to_degrees();
                let lon2 = lons[i + 1].to_degrees();

                let (_, azi1, _): (f64, f64, f64) =
                    InverseGeodesic::inverse(&geod, lat1, lon1, lat2, lon2);

                let mut track = azi1;
                if track < 0.0 {
                    track += 360.0;
                }

                solve_wind_triangle(track, tas_ms, wind)
            })
            .collect();

        let results = segment_results?;
        let gs_vec: Vec<f64> = results.iter().map(|r| r.ground_speed_ms).collect();
        let wca_vec: Vec<f64> = results.iter().map(|r| r.wca_deg).collect();
        crab_warnings += results
            .iter()
            .filter(|r| r.wca_deg.abs() > CRAB_ANGLE_WARNING_DEG)
            .count() as u32;

        line.set_ground_speeds(gs_vec);
        line.set_crab_angles(wca_vec);
    }

    if crab_warnings > 0 {
        log::warn!(
            "{crab_warnings} segment(s) exceed {CRAB_ANGLE_WARNING_DEG}\u{00b0} crab angle — \
             imagery quality may be degraded without gimbal yaw compensation"
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    // -- WindVector construction --

    #[test]
    fn wind_vector_valid() {
        let w = WindVector::new(10.0, 270.0).unwrap();
        assert_abs_diff_eq!(w.speed_ms, 10.0, epsilon = 1e-10);
        assert_abs_diff_eq!(w.direction_deg, 270.0, epsilon = 1e-10);
    }

    #[test]
    fn wind_vector_direction_normalization() {
        let w1 = WindVector::new(5.0, -90.0).unwrap();
        assert_abs_diff_eq!(w1.direction_deg, 270.0, epsilon = 1e-10);

        let w2 = WindVector::new(5.0, 450.0).unwrap();
        assert_abs_diff_eq!(w2.direction_deg, 90.0, epsilon = 1e-10);
    }

    #[test]
    fn wind_vector_negative_speed_rejected() {
        assert!(WindVector::new(-5.0, 0.0).is_err());
    }

    #[test]
    fn wind_vector_nan_rejected() {
        assert!(WindVector::new(f64::NAN, 0.0).is_err());
        assert!(WindVector::new(5.0, f64::NAN).is_err());
    }

    #[test]
    fn wind_vector_is_calm() {
        assert!(WindVector::new(0.0, 0.0).unwrap().is_calm());
        assert!(WindVector::new(1e-7, 0.0).unwrap().is_calm());
        assert!(!WindVector::new(1.0, 0.0).unwrap().is_calm());
    }

    // -- solve_wind_triangle --

    #[test]
    fn no_wind_returns_track_as_heading() {
        let wind = WindVector::new(0.0, 0.0).unwrap();
        let result = solve_wind_triangle(90.0, 50.0, &wind).unwrap();
        assert_abs_diff_eq!(result.heading_deg, 90.0, epsilon = 1e-10);
        assert_abs_diff_eq!(result.wca_deg, 0.0, epsilon = 1e-10);
        assert_abs_diff_eq!(result.ground_speed_ms, 50.0, epsilon = 1e-10);
    }

    #[test]
    fn pure_headwind() {
        // Track north (0), wind from north (0) = pure headwind
        let wind = WindVector::new(10.0, 0.0).unwrap();
        let result = solve_wind_triangle(0.0, 50.0, &wind).unwrap();
        assert_abs_diff_eq!(result.heading_deg, 0.0, epsilon = 1e-6);
        assert_abs_diff_eq!(result.wca_deg, 0.0, epsilon = 1e-6);
        // GS = TAS - wind_speed for pure headwind
        assert_abs_diff_eq!(result.ground_speed_ms, 40.0, epsilon = 1e-6);
    }

    #[test]
    fn pure_tailwind() {
        // Track north (0), wind from south (180) = pure tailwind
        let wind = WindVector::new(10.0, 180.0).unwrap();
        let result = solve_wind_triangle(0.0, 50.0, &wind).unwrap();
        assert_abs_diff_eq!(result.heading_deg, 0.0, epsilon = 1e-6);
        assert_abs_diff_eq!(result.wca_deg, 0.0, epsilon = 1e-6);
        // GS = TAS + wind_speed for pure tailwind
        assert_abs_diff_eq!(result.ground_speed_ms, 60.0, epsilon = 1e-6);
    }

    #[test]
    fn strong_tailwind_exceeding_tas_is_valid() {
        // Wind from south (180) at 60 m/s, TAS 50 m/s, track north
        // This is a valid flight — GS = 50 + 60 = 110 m/s
        let wind = WindVector::new(60.0, 180.0).unwrap();
        let result = solve_wind_triangle(0.0, 50.0, &wind).unwrap();
        assert_abs_diff_eq!(result.ground_speed_ms, 110.0, epsilon = 1e-6);
        assert_abs_diff_eq!(result.wca_deg, 0.0, epsilon = 1e-6);
    }

    #[test]
    fn pure_crosswind_from_east() {
        // Track north (0), wind from east (90) = crosswind from right
        let wind = WindVector::new(10.0, 90.0).unwrap();
        let result = solve_wind_triangle(0.0, 50.0, &wind).unwrap();
        // WCA should be positive (heading right of track, into the wind)
        let expected_wca = (10.0_f64 / 50.0).asin().to_degrees();
        assert_abs_diff_eq!(result.wca_deg, expected_wca, epsilon = 1e-6);
        assert!(result.heading_deg > 0.0 && result.heading_deg < 90.0);
        // GS = TAS * cos(WCA) for pure crosswind
        let expected_gs = 50.0 * expected_wca.to_radians().cos();
        assert_abs_diff_eq!(result.ground_speed_ms, expected_gs, epsilon = 1e-6);
    }

    #[test]
    fn crosswind_exceeds_tas_returns_track_infeasible() {
        // Wind from east (90) at 60 m/s, TAS 50 m/s, track north
        // Crosswind component = 60 > TAS → infeasible
        let wind = WindVector::new(60.0, 90.0).unwrap();
        let result = solve_wind_triangle(0.0, 50.0, &wind);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            KinematicsError::TrackInfeasible { .. }
        ));
    }

    #[test]
    fn headwind_exceeds_tas_returns_negative_gs() {
        // Wind from north (0) at 60 m/s, TAS 50 m/s, track north
        // Pure headwind → GS = 50 - 60 = -10 → negative
        let wind = WindVector::new(60.0, 0.0).unwrap();
        let result = solve_wind_triangle(0.0, 50.0, &wind);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            KinematicsError::NegativeGroundSpeed { .. }
        ));
    }

    #[test]
    fn invalid_tas_rejected() {
        let wind = WindVector::new(5.0, 0.0).unwrap();
        assert!(solve_wind_triangle(0.0, 0.0, &wind).is_err());
        assert!(solve_wind_triangle(0.0, -10.0, &wind).is_err());
        assert!(solve_wind_triangle(0.0, f64::NAN, &wind).is_err());
    }

    #[test]
    fn asin_domain_robustness_near_one() {
        // Wind almost exactly equal to TAS on a pure crosswind track
        // sin_wca should be clamped to avoid NaN
        let wind = WindVector::new(49.999_999_999, 90.0).unwrap();
        let result = solve_wind_triangle(0.0, 50.0, &wind);
        // Should succeed (barely feasible) with large WCA close to 90 degrees
        assert!(result.is_ok());
    }

    // -- wind_corrected_radius --

    #[test]
    fn r_safe_zero_wind_equals_r_min() {
        let r = wind_corrected_radius(30.0, 0.0, 20.0).unwrap();
        let expected = 30.0_f64.powi(2) / (G * 20.0_f64.to_radians().tan());
        assert_abs_diff_eq!(r, expected, epsilon = 0.01);
    }

    #[test]
    fn r_safe_greater_than_r_min_with_wind() {
        let r_no_wind = wind_corrected_radius(30.0, 0.0, 20.0).unwrap();
        let r_with_wind = wind_corrected_radius(30.0, 10.0, 20.0).unwrap();
        assert!(r_with_wind > r_no_wind);
    }

    #[test]
    fn r_safe_formula_matches_doc50() {
        let tas = 30.0;
        let wind = 10.0;
        let bank = 20.0;
        let r = wind_corrected_radius(tas, wind, bank).unwrap();
        let expected = (tas + wind).powi(2) / (G * bank.to_radians().tan());
        assert_abs_diff_eq!(r, expected, epsilon = 1e-6);
    }

    #[test]
    fn r_safe_invalid_params() {
        assert!(wind_corrected_radius(0.0, 5.0, 20.0).is_err());
        assert!(wind_corrected_radius(30.0, -1.0, 20.0).is_err());
        assert!(wind_corrected_radius(30.0, 5.0, 0.0).is_err());
        assert!(wind_corrected_radius(30.0, 5.0, 90.0).is_err());
    }

    // -- compute_segment_ground_speeds --

    #[test]
    fn empty_line_returns_empty() {
        let wind = WindVector::new(5.0, 0.0).unwrap();
        let gs = compute_segment_ground_speeds(&[], &[], 30.0, &wind).unwrap();
        assert!(gs.is_empty());
    }

    #[test]
    fn single_point_returns_empty() {
        let wind = WindVector::new(5.0, 0.0).unwrap();
        let gs = compute_segment_ground_speeds(
            &[51.0_f64.to_radians()],
            &[0.0_f64.to_radians()],
            30.0,
            &wind,
        )
        .unwrap();
        assert!(gs.is_empty());
    }

    #[test]
    fn northbound_segment_with_headwind() {
        // Two points going due north
        let lats = vec![51.0_f64.to_radians(), 51.01_f64.to_radians()];
        let lons = vec![0.0_f64.to_radians(), 0.0_f64.to_radians()];
        // Wind from north (headwind)
        let wind = WindVector::new(5.0, 0.0).unwrap();
        let tas = 30.0;

        let gs = compute_segment_ground_speeds(&lats, &lons, tas, &wind).unwrap();
        assert_eq!(gs.len(), 1);
        // Segment azimuth ~0° (north), so GS ≈ TAS - wind = 25 m/s
        assert_abs_diff_eq!(gs[0], 25.0, epsilon = 0.5);
    }

    // -- apply_wind_to_plan: crab angle storage (Phase 7B) --

    #[test]
    fn apply_wind_stores_crab_angles() {
        use crate::photogrammetry::flightlines::BoundingBox;
        use crate::photogrammetry::flightlines::{generate_flight_lines, FlightPlanParams};

        let sensor = crate::types::SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        };
        let params = FlightPlanParams::new(sensor, 0.05).unwrap();
        let bbox = BoundingBox {
            min_lat: 0.0,
            min_lon: 0.0,
            max_lat: 0.01,
            max_lon: 0.01,
        };
        let mut plan = generate_flight_lines(&bbox, 0.0, &params, None).unwrap();

        let wind = WindVector::new(10.0, 90.0).unwrap(); // crosswind from east
        apply_wind_to_plan(&mut plan, 50.0, &wind).unwrap();

        for line in &plan.lines {
            if line.len() >= 2 {
                let crab = line.crab_angles();
                assert!(
                    crab.is_some(),
                    "crab angles should be populated after wind application"
                );
                let angles = crab.unwrap();
                assert_eq!(angles.len(), line.len() - 1);
                // With crosswind, WCA should be non-zero
                assert!(angles.iter().any(|a| a.abs() > 0.1));
            }
        }
    }

    #[test]
    fn calm_wind_clears_crab_angles() {
        use crate::photogrammetry::flightlines::BoundingBox;
        use crate::photogrammetry::flightlines::{generate_flight_lines, FlightPlanParams};

        let sensor = crate::types::SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        };
        let params = FlightPlanParams::new(sensor, 0.05).unwrap();
        let bbox = BoundingBox {
            min_lat: 0.0,
            min_lon: 0.0,
            max_lat: 0.01,
            max_lon: 0.01,
        };
        let mut plan = generate_flight_lines(&bbox, 0.0, &params, None).unwrap();

        // First apply wind to populate crab angles
        let wind = WindVector::new(10.0, 90.0).unwrap();
        apply_wind_to_plan(&mut plan, 50.0, &wind).unwrap();

        // Then apply calm wind — should clear them
        let calm = WindVector::new(0.0, 0.0).unwrap();
        apply_wind_to_plan(&mut plan, 50.0, &calm).unwrap();

        for line in &plan.lines {
            assert!(
                line.crab_angles().is_none(),
                "calm wind should clear crab angles"
            );
        }
    }
}
