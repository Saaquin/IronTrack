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

use crate::error::PhotogrammetryError;
use crate::types::SensorParams;

/// Built-in sensor presets.
pub mod presets {
    use crate::types::SensorParams;

    /// DJI Phantom 4 Pro / Pro V2.0
    /// 1-inch CMOS, 20 MP, 8.8 mm lens
    pub fn phantom4pro() -> SensorParams {
        SensorParams::new(8.8, 13.2, 8.8, 5472, 3648).unwrap()
    }

    /// DJI Mavic 3 (Classic / Enterprise)
    /// 4/3 CMOS, 20 MP, Hasselblad L-Format 24 mm equiv = 12.29 mm actual
    pub fn mavic3() -> SensorParams {
        SensorParams::new(12.29, 17.3, 13.0, 5280, 3956).unwrap()
    }

    /// Phase One iXM-100 with RSM 50mm lens
    /// 100 MP, 53.4×40 mm back-illuminated CMOS
    pub fn ixm100() -> SensorParams {
        SensorParams::new(50.0, 53.4, 40.0, 11664, 8750).unwrap()
    }
}

/// Resolve a sensor preset name or custom parameters.
pub fn resolve_sensor(
    name: &str,
    custom_focal_length_mm: Option<f64>,
    custom_sensor_width_mm: Option<f64>,
    custom_sensor_height_mm: Option<f64>,
    custom_image_width_px: Option<u32>,
    custom_image_height_px: Option<u32>,
) -> Result<SensorParams, PhotogrammetryError> {
    if let Some(f) = custom_focal_length_mm {
        return SensorParams::new(
            f,
            custom_sensor_width_mm.ok_or_else(|| {
                PhotogrammetryError::InvalidSensor("missing sensor_width_mm".into())
            })?,
            custom_sensor_height_mm.ok_or_else(|| {
                PhotogrammetryError::InvalidSensor("missing sensor_height_mm".into())
            })?,
            custom_image_width_px.ok_or_else(|| {
                PhotogrammetryError::InvalidSensor("missing image_width_px".into())
            })?,
            custom_image_height_px.ok_or_else(|| {
                PhotogrammetryError::InvalidSensor("missing image_height_px".into())
            })?,
        );
    }

    match name.to_lowercase().as_str() {
        "phantom4pro" => Ok(presets::phantom4pro()),
        "mavic3" => Ok(presets::mavic3()),
        "ixm100" => Ok(presets::ixm100()),
        other => Err(PhotogrammetryError::InvalidSensor(format!(
            "unknown sensor preset: {other}"
        ))),
    }
}

impl SensorParams {
    /// Horizontal field of view in radians.
    ///
    /// Derived from the half-angle formed by the sensor width and focal length:
    ///   FOV_h = 2 * atan(sensor_width / (2 * focal_length))
    pub fn fov_horizontal(&self) -> f64 {
        2.0 * (self.sensor_width_mm / (2.0 * self.focal_length_mm)).atan()
    }

    /// Vertical field of view in radians.
    ///
    /// Same derivation as fov_horizontal but using the sensor height axis.
    pub fn fov_vertical(&self) -> f64 {
        2.0 * (self.sensor_height_mm / (2.0 * self.focal_length_mm)).atan()
    }

    /// Ground Sample Distance in metres/pixel for both axes at a given AGL.
    ///
    /// Returns `(gsd_x, gsd_y)` where x is along the sensor width axis
    /// (typically cross-track) and y is along the sensor height axis
    /// (typically along-track). Derived from similar-triangles collinearity:
    ///   GSD = (AGL * sensor_dimension) / (focal_length * pixel_count)
    pub fn gsd_at_agl(&self, agl_m: f64) -> (f64, f64) {
        debug_assert!(
            agl_m > 0.0 && agl_m.is_finite(),
            "agl_m must be positive and finite"
        );
        let gsd_x =
            (agl_m * self.sensor_width_mm) / (self.focal_length_mm * self.image_width_px as f64);
        let gsd_y =
            (agl_m * self.sensor_height_mm) / (self.focal_length_mm * self.image_height_px as f64);
        (gsd_x, gsd_y)
    }

    /// Worst-case (largest) GSD in metres/pixel at the given AGL.
    ///
    /// Per docs/03, the binding constraint for project accuracy compliance is
    /// always the axis with the lower spatial resolution (higher GSD value).
    /// Taking the max guarantees the entire image meets the target threshold.
    pub fn worst_case_gsd(&self, agl_m: f64) -> f64 {
        let (gsd_x, gsd_y) = self.gsd_at_agl(agl_m);
        gsd_x.max(gsd_y)
    }

    /// Ground swath width in metres (cross-track / perpendicular to flight).
    ///
    /// Derived from the horizontal FOV half-angle and the AGL:
    ///   swath_w = 2 * AGL * tan(FOV_h / 2)
    /// Algebraically equivalent to AGL * sensor_width / focal_length.
    pub fn swath_width(&self, agl_m: f64) -> f64 {
        2.0 * agl_m * (self.fov_horizontal() / 2.0).tan()
    }

    /// Ground swath height in metres (along-track / parallel to flight).
    ///
    /// Same derivation as swath_width but using the vertical FOV.
    pub fn swath_height(&self, agl_m: f64) -> f64 {
        2.0 * agl_m * (self.fov_vertical() / 2.0).tan()
    }

    /// Effective cross-track swath width under a crab (yaw) angle.
    ///
    /// When the aircraft crabs into the wind, the sensor footprint rotates
    /// relative to the ground track, reducing the contiguous coverage strip
    /// perpendicular to the flight line:
    ///
    ///   W_eff = W_s × cos(|WCA|) − H_f × sin(|WCA|)
    ///
    /// Clamped to 0.0 when the crab angle is too extreme for any usable
    /// coverage. \[Doc 21 §2.3, Doc 03 §Geometric Impact\]
    pub fn effective_swath_width(&self, agl_m: f64, wca_rad: f64) -> f64 {
        let w = self.swath_width(agl_m);
        let h = self.swath_height(agl_m);
        let abs_wca = wca_rad.abs();
        (w * abs_wca.cos() - h * abs_wca.sin()).max(0.0)
    }

    /// AGL required to achieve a target worst-case GSD, in metres.
    ///
    /// Rearranges the worst-case GSD equation. The binding axis is whichever
    /// has the larger GSD-per-metre-of-AGL ratio (i.e. the coarser pixel pitch
    /// relative to focal length). We solve for the AGL where that axis's GSD
    /// equals the target:
    ///   AGL = target_gsd / max(sensor_w/(f*W), sensor_h/(f*H))
    pub fn required_agl_for_gsd(&self, target_gsd_m: f64) -> f64 {
        debug_assert!(
            target_gsd_m > 0.0 && target_gsd_m.is_finite(),
            "target_gsd_m must be positive and finite"
        );
        /*
         * Each axis contributes a GSD-per-unit-AGL rate. The binding dimension
         * (the one that produces the worst-case GSD at any given altitude) is
         * the one with the highest rate. Dividing the target GSD by that rate
         * gives the maximum AGL at which worst_case_gsd == target_gsd_m.
         */
        let rate_x = self.sensor_width_mm / (self.focal_length_mm * self.image_width_px as f64);
        let rate_y = self.sensor_height_mm / (self.focal_length_mm * self.image_height_px as f64);
        target_gsd_m / rate_x.max(rate_y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /*
     * Reference sensor: Phase One iXM-100
     *   Sensor: 53.4 × 40.0 mm
     *   Resolution: 11664 × 8750 px
     *   Focal length: 50 mm
     *
     * Hand-calculated reference values (derived below for review):
     *   pixel_pitch = 53.4 / 11664 = 0.004578703... mm
     *   FOV_h = 2 * atan(53.4 / 100) = 2 * atan(0.534) ≈ 0.98079 rad
     *   FOV_v = 2 * atan(40.0 / 100) = 2 * atan(0.400) ≈ 0.76008 rad
     *   @ 1000 m AGL:
     *     GSD_x = 1000 * 53.4 / (50 * 11664) = 53400 / 583200 ≈ 0.091558 m/px
     *     GSD_y = 1000 * 40.0 / (50 * 8750)  = 40000 / 437500  ≈ 0.091429 m/px
     *     worst  = GSD_x ≈ 0.091558 m/px
     *     swath_w = 1000 * 53.4 / 50 = 1068.0 m  (tan cancels atan algebraically)
     *     swath_h = 1000 * 40.0 / 50 = 800.0 m
     *   required_agl for 0.05 m/px:
     *     rate_x = 53.4 / (50 * 11664) = 9.15584... × 10⁻⁵
     *     agl = 0.05 / 9.15584e-5 ≈ 546.06 m
     */

    fn ixm100() -> SensorParams {
        SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        }
    }

    #[test]
    fn pixel_pitch_ixm100() {
        let s = ixm100();
        // 53.4 mm / 11664 px = 0.004578703... mm/px
        assert_abs_diff_eq!(s.pixel_pitch(), 53.4 / 11664.0_f64, epsilon = 1e-12);
    }

    #[test]
    fn fov_horizontal_ixm100() {
        let s = ixm100();
        let expected = 2.0_f64 * (53.4_f64 / (2.0 * 50.0)).atan();
        assert_abs_diff_eq!(s.fov_horizontal(), expected, epsilon = 1e-12);
    }

    #[test]
    fn fov_vertical_ixm100() {
        let s = ixm100();
        let expected = 2.0_f64 * (40.0_f64 / (2.0 * 50.0)).atan();
        assert_abs_diff_eq!(s.fov_vertical(), expected, epsilon = 1e-12);
    }

    #[test]
    fn fov_horizontal_less_than_180_degrees() {
        // Any valid (positive, finite) sensor must produce FOV < π.
        assert!(ixm100().fov_horizontal() < std::f64::consts::PI);
        assert!(ixm100().fov_vertical() < std::f64::consts::PI);
    }

    #[test]
    fn gsd_at_agl_1000m_ixm100() {
        let s = ixm100();
        let (gsd_x, gsd_y) = s.gsd_at_agl(1000.0);
        // GSD_x = 1000 * 53.4 / (50 * 11664)
        let expected_x = 1000.0 * 53.4 / (50.0 * 11664.0_f64);
        // GSD_y = 1000 * 40.0 / (50 * 8750)
        let expected_y = 1000.0 * 40.0 / (50.0 * 8750.0_f64);
        assert_abs_diff_eq!(gsd_x, expected_x, epsilon = 1e-10);
        assert_abs_diff_eq!(gsd_y, expected_y, epsilon = 1e-10);
        // Numeric spot-check: both axes near ~9.15 cm/px at 1000 m AGL.
        assert_abs_diff_eq!(gsd_x, 0.091558, epsilon = 1e-5);
        assert_abs_diff_eq!(gsd_y, 0.091429, epsilon = 1e-5);
    }

    #[test]
    fn gsd_linear_with_agl() {
        // Doubling AGL must exactly double GSD — verifies the linear model.
        let s = ixm100();
        let (x1, y1) = s.gsd_at_agl(500.0);
        let (x2, y2) = s.gsd_at_agl(1000.0);
        assert_abs_diff_eq!(x2, 2.0 * x1, epsilon = 1e-12);
        assert_abs_diff_eq!(y2, 2.0 * y1, epsilon = 1e-12);
    }

    #[test]
    fn worst_case_gsd_is_gsd_x_for_ixm100() {
        // For iXM-100, GSD_x is very slightly larger than GSD_y at any AGL.
        // GSD_x/AGL = 53.4/(50*11664) ≈ 9.1558e-5
        // GSD_y/AGL = 40.0/(50*8750)  ≈ 9.1429e-5
        let s = ixm100();
        let (gsd_x, _gsd_y) = s.gsd_at_agl(1000.0);
        assert_abs_diff_eq!(s.worst_case_gsd(1000.0), gsd_x, epsilon = 1e-15);
    }

    #[test]
    fn swath_width_at_1000m_ixm100() {
        let s = ixm100();
        // swath_w = 2 * AGL * tan(FOV_h / 2)
        // tan(atan(sensor_w / (2*f))) = sensor_w / (2*f), so this simplifies to:
        // swath_w = AGL * sensor_w / f = 1000 * 53.4 / 50 = 1068.0 m
        assert_abs_diff_eq!(s.swath_width(1000.0), 1068.0, epsilon = 1e-9);
    }

    #[test]
    fn swath_height_at_1000m_ixm100() {
        let s = ixm100();
        // swath_h = AGL * sensor_h / f = 1000 * 40.0 / 50 = 800.0 m
        assert_abs_diff_eq!(s.swath_height(1000.0), 800.0, epsilon = 1e-9);
    }

    #[test]
    fn swath_linear_with_agl() {
        let s = ixm100();
        assert_abs_diff_eq!(
            s.swath_width(500.0),
            s.swath_width(1000.0) / 2.0,
            epsilon = 1e-9
        );
        assert_abs_diff_eq!(
            s.swath_height(500.0),
            s.swath_height(1000.0) / 2.0,
            epsilon = 1e-9
        );
    }

    #[test]
    fn required_agl_for_gsd_round_trips_worst_case() {
        // required_agl_for_gsd and worst_case_gsd must be exact inverses.
        let s = ixm100();
        let target = 0.05; // 5 cm/px
        let agl = s.required_agl_for_gsd(target);
        assert_abs_diff_eq!(s.worst_case_gsd(agl), target, epsilon = 1e-12);
    }

    #[test]
    fn required_agl_for_gsd_spot_check_ixm100() {
        let s = ixm100();
        // rate_x = 53.4 / (50 * 11664) = 9.15584e-5
        // agl = 0.05 / 9.15584e-5 ≈ 546.06 m
        let agl = s.required_agl_for_gsd(0.05);
        assert_abs_diff_eq!(agl, 546.06, epsilon = 0.01);
    }

    #[test]
    fn required_agl_proportional_to_target_gsd() {
        // required_agl is linear in target_gsd: double the target → double the AGL.
        let s = ixm100();
        let agl1 = s.required_agl_for_gsd(0.05);
        let agl2 = s.required_agl_for_gsd(0.10);
        assert_abs_diff_eq!(agl2, 2.0 * agl1, epsilon = 1e-9);
    }

    /// Sensor where the vertical axis has a larger pixel pitch than the horizontal.
    ///
    /// Constructed to force GSD_y > GSD_x at any AGL:
    ///   rate_x = 10 / (50 * 1000) = 2.0e-4  m/px per m AGL
    ///   rate_y = 20 / (50 *  500) = 8.0e-4  m/px per m AGL  (4× larger — y is binding)
    fn vertical_binding_sensor() -> SensorParams {
        SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 10.0,
            sensor_height_mm: 20.0,
            image_width_px: 1000,
            image_height_px: 500,
        }
    }

    #[test]
    fn worst_case_gsd_is_gsd_y_for_vertical_binding_sensor() {
        let s = vertical_binding_sensor();
        let (_gsd_x, gsd_y) = s.gsd_at_agl(1000.0);
        assert_abs_diff_eq!(s.worst_case_gsd(1000.0), gsd_y, epsilon = 1e-15);
        // Numeric check: GSD_y = 1000 * 20 / (50 * 500) = 0.8 m/px
        assert_abs_diff_eq!(gsd_y, 0.8, epsilon = 1e-12);
    }

    #[test]
    fn required_agl_for_gsd_vertical_binding_round_trips() {
        let s = vertical_binding_sensor();
        let target = 0.1; // 10 cm/px
        let agl = s.required_agl_for_gsd(target);
        assert_abs_diff_eq!(s.worst_case_gsd(agl), target, epsilon = 1e-12);
        // rate_y = 8.0e-4, so agl = 0.1 / 8.0e-4 = 125.0 m
        assert_abs_diff_eq!(agl, 125.0, epsilon = 1e-10);
    }

    /// Verify that the Phantom 4 Pro pixel pitch (from types.rs tests) is
    /// still accessible via the method implemented in sensor.rs.
    #[test]
    fn pixel_pitch_phantom4pro_cross_module() {
        let s = SensorParams {
            focal_length_mm: 8.8,
            sensor_width_mm: 13.2,
            sensor_height_mm: 8.8,
            image_width_px: 5472,
            image_height_px: 3648,
        };
        assert_abs_diff_eq!(s.pixel_pitch(), 13.2 / 5472.0_f64, epsilon = 1e-15);
    }

    // -- effective_swath_width (Phase 7B) --

    #[test]
    fn effective_swath_zero_crab_equals_nominal() {
        let s = ixm100();
        let agl = 1000.0;
        assert_abs_diff_eq!(
            s.effective_swath_width(agl, 0.0),
            s.swath_width(agl),
            epsilon = 1e-10,
        );
    }

    #[test]
    fn effective_swath_90_deg_crab_is_zero() {
        let s = ixm100();
        // At 90° crab the footprint is perpendicular — no contiguous coverage.
        assert_abs_diff_eq!(
            s.effective_swath_width(1000.0, std::f64::consts::FRAC_PI_2),
            0.0,
            epsilon = 1e-10,
        );
    }

    #[test]
    fn effective_swath_symmetric_in_crab_angle() {
        let s = ixm100();
        let agl = 1000.0;
        let theta = 0.15; // ~8.6 degrees
        assert_abs_diff_eq!(
            s.effective_swath_width(agl, theta),
            s.effective_swath_width(agl, -theta),
            epsilon = 1e-12,
        );
    }

    #[test]
    fn effective_swath_known_value_5deg() {
        // iXM-100 at 1000 m: W=1068.0, H=800.0
        // WCA = 5° = 0.08726646 rad
        // W_eff = 1068 * cos(5°) - 800 * sin(5°)
        //       = 1068 * 0.99619 - 800 * 0.08716
        //       = 1063.93 - 69.73
        //       ≈ 994.20
        let s = ixm100();
        let wca_rad = 5.0_f64.to_radians();
        let w_eff = s.effective_swath_width(1000.0, wca_rad);
        let expected = 1068.0 * wca_rad.cos() - 800.0 * wca_rad.sin();
        assert_abs_diff_eq!(w_eff, expected, epsilon = 0.01);
        assert!(w_eff < s.swath_width(1000.0));
        assert!(w_eff > 0.0);
    }

    #[test]
    fn effective_swath_clamped_at_zero() {
        // Extreme crab angle where formula goes negative → clamped to 0.
        let s = ixm100();
        // At 60° crab: 1068*cos60 - 800*sin60 = 534 - 692.8 = -158.8 → 0
        let w_eff = s.effective_swath_width(1000.0, 60.0_f64.to_radians());
        assert_abs_diff_eq!(w_eff, 0.0, epsilon = 1e-10);
    }
}
