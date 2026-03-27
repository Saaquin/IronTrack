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

// LiDAR sensor geometry and point density kinematics.
//
// LiDAR flight planning uses fundamentally different spacing logic from
// photogrammetric (camera) missions. Line spacing is driven by swath overlap
// percentage (typically 50%), not by image side-lap and GSD. Point density
// (pts/m^2) is the primary quality metric, not pixel resolution.
//
// Core equations from docs/13_lidar_flight_planning.md and .agents/lidar.md:
//   Swath width:    W = 2 * AGL * tan(FOV/2)
//   Point density:  rho = PRR * (1 - LSP) / (V * W)
//   Along-track:    d_along = V / (2 * scan_rate)
//   Across-track:   d_across = W / (PRR / scan_rate)

use crate::error::PhotogrammetryError;

// ---------------------------------------------------------------------------
// LiDAR sensor parameters
// ---------------------------------------------------------------------------

/// Physical parameters of a LiDAR sensor head.
///
/// These drive the point density and swath geometry calculations.
/// All frequency values are in Hertz, angles in degrees.
#[derive(Debug, Clone)]
pub struct LidarSensorParams {
    /// Pulse Repetition Rate in Hz (e.g. 240_000.0 for 240 kHz).
    pub prr: f64,
    /// Mirror oscillation / scan frequency in Hz.
    pub scan_rate: f64,
    /// Total angular Field of View in degrees.
    pub fov_deg: f64,
    /// Fraction of scanning points lost to mirror deceleration / blind spots.
    /// Range: [0.0, 1.0). Typical oscillating mirror: 0.0–0.15.
    pub loss_fraction: f64,
}

impl LidarSensorParams {
    /// Validated constructor.
    ///
    /// Returns `Err` if any parameter is non-positive, non-finite, FOV
    /// outside (0, 180), or loss_fraction outside [0, 1).
    pub fn new(
        prr: f64,
        scan_rate: f64,
        fov_deg: f64,
        loss_fraction: f64,
    ) -> Result<Self, PhotogrammetryError> {
        if !prr.is_finite() || prr <= 0.0 {
            return Err(PhotogrammetryError::InvalidSensor(format!(
                "PRR must be positive and finite, got {prr}"
            )));
        }
        if !scan_rate.is_finite() || scan_rate <= 0.0 {
            return Err(PhotogrammetryError::InvalidSensor(format!(
                "scan_rate must be positive and finite, got {scan_rate}"
            )));
        }
        if !fov_deg.is_finite() || fov_deg <= 0.0 || fov_deg >= 180.0 {
            return Err(PhotogrammetryError::InvalidSensor(format!(
                "fov_deg must be in (0, 180), got {fov_deg}"
            )));
        }
        if !loss_fraction.is_finite() || !(0.0..1.0).contains(&loss_fraction) {
            return Err(PhotogrammetryError::InvalidSensor(format!(
                "loss_fraction must be in [0.0, 1.0), got {loss_fraction}"
            )));
        }
        Ok(Self {
            prr,
            scan_rate,
            fov_deg,
            loss_fraction,
        })
    }
}

// ---------------------------------------------------------------------------
// Swath width
// ---------------------------------------------------------------------------

/// Ground swath width from AGL and total FOV.
///
/// W = 2 * AGL * tan(FOV / 2)
///
/// This formula is shared between LiDAR and camera sensors — the geometry
/// is identical. Units: metres in, metres out. FOV in degrees.
pub fn swath_width(agl: f64, fov_deg: f64) -> f64 {
    2.0 * agl * (fov_deg.to_radians() / 2.0).tan()
}

// ---------------------------------------------------------------------------
// Point density
// ---------------------------------------------------------------------------

/// Global average point density in pts/m^2.
///
/// rho = PRR * (1 - LSP) / (V * W)
///
/// where PRR is pulse repetition rate (Hz), LSP is loss fraction,
/// V is ground speed (m/s), W is swath width (m).
///
/// Returns `Err` if ground_speed or agl is non-positive.
pub fn point_density(
    params: &LidarSensorParams,
    agl: f64,
    ground_speed: f64,
) -> Result<f64, PhotogrammetryError> {
    if !agl.is_finite() || agl <= 0.0 {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "AGL must be positive and finite, got {agl}"
        )));
    }
    if !ground_speed.is_finite() || ground_speed <= 0.0 {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "ground_speed must be positive and finite, got {ground_speed}"
        )));
    }
    let w = swath_width(agl, params.fov_deg);
    Ok(params.prr * (1.0 - params.loss_fraction) / (ground_speed * w))
}

// ---------------------------------------------------------------------------
// Required speed for target density
// ---------------------------------------------------------------------------

/// Solve the density equation for ground speed.
///
/// V = PRR * (1 - LSP) / (rho_target * W)
///
/// Returns the maximum ground speed (m/s) that achieves at least
/// `target_density` pts/m^2 at the given AGL.
pub fn required_speed_for_density(
    params: &LidarSensorParams,
    agl: f64,
    target_density: f64,
) -> Result<f64, PhotogrammetryError> {
    if !agl.is_finite() || agl <= 0.0 {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "AGL must be positive and finite, got {agl}"
        )));
    }
    if !target_density.is_finite() || target_density <= 0.0 {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "target_density must be positive and finite, got {target_density}"
        )));
    }
    let w = swath_width(agl, params.fov_deg);
    Ok(params.prr * (1.0 - params.loss_fraction) / (target_density * w))
}

// ---------------------------------------------------------------------------
// Point spacing (uniformity analysis)
// ---------------------------------------------------------------------------

/// Across-track point spacing for an oscillating mirror scanner.
///
/// d_across = W / (PRR / scan_rate)
///
/// This is the average distance between adjacent laser footprints along
/// a single scan line. See Doc 13 §Decoupling Along/Across-Track.
pub fn across_track_spacing(params: &LidarSensorParams, agl: f64) -> f64 {
    let w = swath_width(agl, params.fov_deg);
    let points_per_scan_line = params.prr / params.scan_rate;
    w / points_per_scan_line
}

/// Along-track point spacing for a bidirectional oscillating mirror.
///
/// d_along = V / (2 * scan_rate)
///
/// For unidirectional rotating polygon scanners, the denominator is
/// scan_rate (not 2 * scan_rate).
pub fn along_track_spacing(ground_speed: f64, scan_rate: f64) -> f64 {
    ground_speed / (2.0 * scan_rate)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn test_sensor() -> LidarSensorParams {
        LidarSensorParams::new(240_000.0, 100.0, 70.0, 0.0).unwrap()
    }

    // -----------------------------------------------------------------------
    // Swath width
    // -----------------------------------------------------------------------

    #[test]
    fn swath_width_doc_example() {
        /*
         * Doc 13 worked example:
         *   AGL = 100 m, FOV = 70°
         *   W = 2 × 100 × tan(35°) ≈ 140.04 m
         */
        let w = swath_width(100.0, 70.0);
        assert_abs_diff_eq!(w, 140.04, epsilon = 0.1);
    }

    #[test]
    fn swath_width_zero_fov_is_zero() {
        /*
         * A 0° FOV produces zero swath. We test near-zero rather than
         * exactly 0 since fov_deg=0 is rejected by the constructor.
         */
        let w = swath_width(100.0, 0.001);
        assert!(w < 0.01, "near-zero FOV should produce near-zero swath");
    }

    #[test]
    fn swath_width_linear_with_agl() {
        let w1 = swath_width(100.0, 70.0);
        let w2 = swath_width(200.0, 70.0);
        assert_abs_diff_eq!(w2, 2.0 * w1, epsilon = 1e-9);
    }

    // -----------------------------------------------------------------------
    // Point density
    // -----------------------------------------------------------------------

    #[test]
    fn point_density_doc_example() {
        /*
         * Doc 13 worked example:
         *   PRR=240,000 Hz, AGL=100m, FOV=70°, V=5 m/s, loss=0
         *   W ≈ 140 m
         *   ρ = 240,000 / (5 × 140) ≈ 342.8 pts/m²
         *
         * Using the precise swath width (140.04 m):
         *   ρ = 240,000 / (5 × 140.04) ≈ 342.75
         */
        let sensor = test_sensor();
        let rho = point_density(&sensor, 100.0, 5.0).unwrap();
        assert_abs_diff_eq!(rho, 342.75, epsilon = 1.0);
    }

    #[test]
    fn point_density_with_loss_fraction() {
        /*
         * 10% loss: ρ = 240,000 × 0.9 / (5 × 140.04) ≈ 308.5
         */
        let sensor = LidarSensorParams::new(240_000.0, 100.0, 70.0, 0.10).unwrap();
        let rho = point_density(&sensor, 100.0, 5.0).unwrap();
        assert_abs_diff_eq!(rho, 308.5, epsilon = 1.0);
    }

    #[test]
    fn point_density_inverse_with_speed() {
        /*
         * Doubling speed halves density.
         */
        let sensor = test_sensor();
        let rho1 = point_density(&sensor, 100.0, 5.0).unwrap();
        let rho2 = point_density(&sensor, 100.0, 10.0).unwrap();
        assert_abs_diff_eq!(rho1, 2.0 * rho2, epsilon = 1e-6);
    }

    #[test]
    fn point_density_rejects_zero_speed() {
        let sensor = test_sensor();
        assert!(point_density(&sensor, 100.0, 0.0).is_err());
    }

    #[test]
    fn point_density_rejects_zero_agl() {
        let sensor = test_sensor();
        assert!(point_density(&sensor, 0.0, 5.0).is_err());
    }

    // -----------------------------------------------------------------------
    // Required speed for density
    // -----------------------------------------------------------------------

    #[test]
    fn required_speed_doc_example() {
        /*
         * ρ=8 pts/m², PRR=240,000 Hz, AGL=100m, FOV=70°, loss=0
         * V = 240,000 / (8 × 140.04) ≈ 214.2 m/s
         */
        let sensor = test_sensor();
        let v = required_speed_for_density(&sensor, 100.0, 8.0).unwrap();
        assert_abs_diff_eq!(v, 214.2, epsilon = 1.0);
    }

    #[test]
    fn required_speed_round_trips_with_density() {
        /*
         * Compute speed for target density, then verify that density at
         * that speed matches the target.
         */
        let sensor = test_sensor();
        let target = 50.0;
        let v = required_speed_for_density(&sensor, 100.0, target).unwrap();
        let rho = point_density(&sensor, 100.0, v).unwrap();
        assert_abs_diff_eq!(rho, target, epsilon = 1e-6);
    }

    #[test]
    fn required_speed_rejects_zero_density() {
        let sensor = test_sensor();
        assert!(required_speed_for_density(&sensor, 100.0, 0.0).is_err());
    }

    // -----------------------------------------------------------------------
    // Point spacing
    // -----------------------------------------------------------------------

    #[test]
    fn across_track_spacing_doc_formula() {
        /*
         * d_across = W / (PRR / scan_rate)
         *          = 140.04 / (240,000 / 100)
         *          = 140.04 / 2400
         *          ≈ 0.0583 m
         */
        let sensor = test_sensor();
        let d = across_track_spacing(&sensor, 100.0);
        assert_abs_diff_eq!(d, 0.0583, epsilon = 0.001);
    }

    #[test]
    fn along_track_spacing_doc_formula() {
        /*
         * d_along = V / (2 × scan_rate)
         *         = 5.0 / (2 × 100)
         *         = 0.025 m
         */
        let d = along_track_spacing(5.0, 100.0);
        assert_abs_diff_eq!(d, 0.025, epsilon = 1e-9);
    }

    // -----------------------------------------------------------------------
    // Sensor validation
    // -----------------------------------------------------------------------

    #[test]
    fn sensor_rejects_zero_prr() {
        assert!(LidarSensorParams::new(0.0, 100.0, 70.0, 0.0).is_err());
    }

    #[test]
    fn sensor_rejects_negative_scan_rate() {
        assert!(LidarSensorParams::new(240_000.0, -1.0, 70.0, 0.0).is_err());
    }

    #[test]
    fn sensor_rejects_fov_180() {
        assert!(LidarSensorParams::new(240_000.0, 100.0, 180.0, 0.0).is_err());
    }

    #[test]
    fn sensor_rejects_loss_fraction_1() {
        assert!(LidarSensorParams::new(240_000.0, 100.0, 70.0, 1.0).is_err());
    }

    #[test]
    fn sensor_accepts_valid_params() {
        assert!(LidarSensorParams::new(240_000.0, 100.0, 70.0, 0.12).is_ok());
    }
}
