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

use std::fmt;

use crate::error::{GeodesyError, PhotogrammetryError};

/*
 * WGS84 ellipsoid — defining parameters only (EPSG:7030 / NGA TR8350.2 §3.2).
 * All downstream constants are derived algebraically from these two values.
 * Never hardcode b, e2, or n independently — derive them here or risk
 * inconsistency if the ellipsoid definition is ever revised.
 */

/// Semi-major axis (equatorial radius), metres. Exact defining parameter.
pub const WGS84_A: f64 = 6_378_137.0;

/// Inverse flattening. Exact defining parameter.
pub const WGS84_F_INV: f64 = 298.257_223_563;

/// Flattening:  f = 1 / (1/f)
pub const WGS84_F: f64 = 1.0 / WGS84_F_INV;

/// Semi-minor axis (polar radius), metres:  b = a(1 − f)
pub const WGS84_B: f64 = WGS84_A * (1.0 - WGS84_F);

/// First eccentricity squared:  e² = 2f − f²  =  (a² − b²) / a²
pub const WGS84_E2: f64 = 2.0 * WGS84_F - WGS84_F * WGS84_F;

/// Second eccentricity squared:  e′² = e² / (1 − e²)  =  (a² − b²) / b²
pub const WGS84_E_PRIME2: f64 = WGS84_E2 / (1.0 - WGS84_E2);

/// Third flattening:  n = (a − b) / (a + b)  =  f / (2 − f)
/// Appears in the Karney-Krüger 6th-order series expansion for UTM.
pub const WGS84_N: f64 = WGS84_F / (2.0 - WGS84_F);

// ---------------------------------------------------------------------------
// Geographic coordinate
// ---------------------------------------------------------------------------

/// WGS84 geographic coordinate.
///
/// Stored internally in radians to avoid repeated unit conversions inside
/// tight geodesic computation loops. Construct with `from_degrees` for
/// human-readable input. Both constructors validate their inputs and return
/// `Err(GeodesyError::OutOfBounds)` for values outside the WGS84 domain.
#[derive(Debug, Clone, Copy)]
pub struct GeoCoord {
    lat: f64,        // geodetic latitude  (radians), in [-π/2, π/2]
    lon: f64,        // geodetic longitude (radians), in [-π, π]
    pub height: f64, // ellipsoidal height above WGS84 (metres)
}

impl GeoCoord {
    /// Construct from decimal degrees and ellipsoidal height (m).
    ///
    /// Returns `Err` if lat is outside [-90, 90], lon outside [-180, 180],
    /// or any value is non-finite.
    pub fn from_degrees(lat_deg: f64, lon_deg: f64, height: f64) -> Result<Self, GeodesyError> {
        if !lat_deg.is_finite()
            || !lon_deg.is_finite()
            || !height.is_finite()
            || !(-90.0..=90.0).contains(&lat_deg)
            || !(-180.0..=180.0).contains(&lon_deg)
        {
            return Err(GeodesyError::OutOfBounds {
                lat: lat_deg,
                lon: lon_deg,
            });
        }
        Ok(Self {
            lat: lat_deg.to_radians(),
            lon: lon_deg.to_radians(),
            height,
        })
    }

    /// Construct from radians and ellipsoidal height (m).
    ///
    /// Returns `Err` if lat is outside [-π/2, π/2], lon outside [-π, π],
    /// or any value is non-finite.
    pub fn new(lat_rad: f64, lon_rad: f64, height: f64) -> Result<Self, GeodesyError> {
        let half_pi = std::f64::consts::FRAC_PI_2;
        let pi = std::f64::consts::PI;
        if !lat_rad.is_finite()
            || !lon_rad.is_finite()
            || !height.is_finite()
            || !(-half_pi..=half_pi).contains(&lat_rad)
            || !(-pi..=pi).contains(&lon_rad)
        {
            return Err(GeodesyError::OutOfBounds {
                lat: lat_rad.to_degrees(),
                lon: lon_rad.to_degrees(),
            });
        }
        Ok(Self {
            lat: lat_rad,
            lon: lon_rad,
            height,
        })
    }

    pub fn lat_rad(&self) -> f64 {
        self.lat
    }
    pub fn lon_rad(&self) -> f64 {
        self.lon
    }
    pub fn lat_deg(&self) -> f64 {
        self.lat.to_degrees()
    }
    pub fn lon_deg(&self) -> f64 {
        self.lon.to_degrees()
    }
}

impl fmt::Display for GeoCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lat = self.lat_deg();
        let lon = self.lon_deg();
        let lat_dir = if lat >= 0.0 { 'N' } else { 'S' };
        let lon_dir = if lon >= 0.0 { 'E' } else { 'W' };
        write!(
            f,
            "{:.6}°{}, {:.6}°{}, {:.3}m",
            lat.abs(),
            lat_dir,
            lon.abs(),
            lon_dir,
            self.height,
        )
    }
}

// ---------------------------------------------------------------------------
// UTM coordinate
// ---------------------------------------------------------------------------

/// UTM hemisphere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hemisphere {
    North,
    South,
}

/// UTM projected coordinate (easting/northing in metres + zone metadata).
pub struct UtmCoord {
    pub easting: f64,
    pub northing: f64,
    pub zone: u8,
    pub hemisphere: Hemisphere,
}

impl fmt::Display for UtmCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hemi = match self.hemisphere {
            Hemisphere::North => 'N',
            Hemisphere::South => 'S',
        };
        write!(
            f,
            "{}{} {:.3}mE {:.3}mN",
            self.zone, hemi, self.easting, self.northing,
        )
    }
}

// ---------------------------------------------------------------------------
// Flight line  (Structure-of-Arrays)
// ---------------------------------------------------------------------------

/// Structure-of-Arrays layout for a single flight line.
///
/// Fields are private to enforce the SoA length invariant:
/// `lats`, `lons`, and `elevations` are always the same length.
/// Use `push()` to add waypoints atomically, and the accessor methods
/// to read back slices. This prevents silent data corruption from
/// mismatched vector lengths in downstream geodesic and swath computations.
#[derive(Default, Clone)]
pub struct FlightLine {
    lats: Vec<f64>,       // geodetic latitudes  (radians)
    lons: Vec<f64>,       // geodetic longitudes (radians)
    elevations: Vec<f64>, // terrain elevation above MSL (metres)
}

impl FlightLine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one waypoint. All three components are written atomically,
    /// keeping the three vectors the same length at all times.
    pub fn push(&mut self, lat: f64, lon: f64, elevation: f64) {
        self.lats.push(lat);
        self.lons.push(lon);
        self.elevations.push(elevation);
    }

    /// Number of waypoints in this flight line.
    pub fn len(&self) -> usize {
        self.lats.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lats.is_empty()
    }

    pub fn lats(&self) -> &[f64] {
        &self.lats
    }

    pub fn lons(&self) -> &[f64] {
        &self.lons
    }

    pub fn elevations(&self) -> &[f64] {
        &self.elevations
    }

    /// Iterator of `(longitude_deg, latitude_deg)` pairs for this flight line.
    ///
    /// Enforces the **RFC 7946 / GeoPackage coordinate order** at the export
    /// gate. All GIS-facing code (WKB encoder, GeoJSON serialiser) must use
    /// this iterator rather than zipping `lats()` and `lons()` directly, which
    /// would silently produce swapped [lat, lon] pairs.
    ///
    /// Coordinates are converted from the internal radian representation to
    /// decimal degrees at the point of iteration — no allocations.
    pub fn iter_lonlat_deg(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.lons
            .iter()
            .zip(self.lats.iter())
            .map(|(&lo, &la)| (lo.to_degrees(), la.to_degrees()))
    }
}

impl fmt::Display for FlightLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FlightLine {{ {} waypoints }}", self.lats.len())
    }
}

// ---------------------------------------------------------------------------
// Sensor parameters
// ---------------------------------------------------------------------------

/// Camera/sensor geometry parameters for GSD, FOV, and swath calculations.
#[derive(Clone)]
pub struct SensorParams {
    pub focal_length_mm: f64,
    pub sensor_width_mm: f64,
    pub sensor_height_mm: f64,
    pub image_width_px: u32,
    pub image_height_px: u32,
}

impl SensorParams {
    /// Validated constructor. Returns `Err(InvalidSensor)` if any dimension is
    /// zero, non-positive, or non-finite. Use this in all production code paths;
    /// struct-literal construction in tests is fine when values are known-valid.
    pub fn new(
        focal_length_mm: f64,
        sensor_width_mm: f64,
        sensor_height_mm: f64,
        image_width_px: u32,
        image_height_px: u32,
    ) -> Result<Self, PhotogrammetryError> {
        /*
         * Reject any sensor geometry that would produce infinite or NaN GSD /
         * pixel-pitch values downstream. Zero pixel counts are the most likely
         * accidental input, but we also guard against non-finite floats from
         * upstream deserialization.
         */
        if !focal_length_mm.is_finite() || focal_length_mm <= 0.0 {
            return Err(PhotogrammetryError::InvalidSensor(format!(
                "focal_length_mm must be finite and positive, got {focal_length_mm}"
            )));
        }
        if !sensor_width_mm.is_finite() || sensor_width_mm <= 0.0 {
            return Err(PhotogrammetryError::InvalidSensor(format!(
                "sensor_width_mm must be finite and positive, got {sensor_width_mm}"
            )));
        }
        if !sensor_height_mm.is_finite() || sensor_height_mm <= 0.0 {
            return Err(PhotogrammetryError::InvalidSensor(format!(
                "sensor_height_mm must be finite and positive, got {sensor_height_mm}"
            )));
        }
        if image_width_px == 0 {
            return Err(PhotogrammetryError::InvalidSensor(
                "image_width_px must be non-zero".into(),
            ));
        }
        if image_height_px == 0 {
            return Err(PhotogrammetryError::InvalidSensor(
                "image_height_px must be non-zero".into(),
            ));
        }
        Ok(Self {
            focal_length_mm,
            sensor_width_mm,
            sensor_height_mm,
            image_width_px,
            image_height_px,
        })
    }

    /// Physical size of one pixel on the sensor (mm/px).
    ///
    /// Pixel pitch feeds the GSD formula:
    ///   GSD (m/px) = (AGL_m × pixel_pitch_mm) / (focal_length_mm × 1000)
    pub fn pixel_pitch(&self) -> f64 {
        self.sensor_width_mm / self.image_width_px as f64
    }
}

impl fmt::Display for SensorParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SensorParams {{ f={:.2}mm, {:.2}×{:.2}mm, {}×{}px, pitch={:.5}mm }}",
            self.focal_length_mm,
            self.sensor_width_mm,
            self.sensor_height_mm,
            self.image_width_px,
            self.image_height_px,
            self.pixel_pitch(),
        )
    }
}

// ---------------------------------------------------------------------------
// Height newtypes
// ---------------------------------------------------------------------------

/// Orthometric height above the EGM96 geoid (approximately MSL), in metres.
///
/// This is the value stored in SRTM `.hgt` files and returned by
/// `TerrainEngine::terrain_elevation`. Do not mix with `EllipsoidalM` —
/// the two differ by the geoid undulation N (typically ±100 m globally).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct OrthometricM(pub f64);

/// WGS84 ellipsoidal height, in metres.
///
/// This is the altitude referenced by GNSS receivers and autopilot targets.
/// h_ellipsoid = H_ortho + N_geoid. Returned by
/// `TerrainEngine::ellipsoidal_terrain_elevation`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct EllipsoidalM(pub f64);

/// Above-ground level (AGL) altitude, in metres.
///
/// AGL is the height of the aircraft above the terrain directly below it.
/// agl = h_ellipsoid_aircraft − h_ellipsoid_terrain. Used in photogrammetry
/// and flight line calculations. Must be positive.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct AglM(pub f64);

// ---------------------------------------------------------------------------
// Mission parameters
// ---------------------------------------------------------------------------

/// Top-level flight mission parameters passed to the planning engine.
pub struct MissionParams {
    pub agl_m: f64,
    pub forward_overlap: f64,
    pub side_overlap: f64,
    pub sensor: SensorParams,
    /// Platform minimum safe operating altitude above ground level (metres).
    ///
    /// The flight planner raises `PhotogrammetryError::UnsafeAltitude` if the
    /// AGL computed from a target GSD falls below this threshold. Typical values:
    ///   - Small multirotor drone:      30 m
    ///   - Fixed-wing UAV:              60 m
    ///   - Manned aircraft (Transport): 152 m (500 ft, FAA/TC low-level minimum)
    ///
    /// Set to 0.0 to disable the check (simulation / unit-test contexts only).
    pub min_clearance_m: f64,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    // --- WGS84 defining parameters ------------------------------------------

    // assert_eq! is safe for these two tests: the constants are defined as
    // exact f64 literals with no arithmetic, so bit-for-bit equality holds.
    #[test]
    fn wgs84_semi_major_axis() {
        assert_eq!(WGS84_A, 6_378_137.0_f64);
    }

    #[test]
    fn wgs84_inverse_flattening() {
        assert_eq!(WGS84_F_INV, 298.257_223_563_f64);
    }

    // --- WGS84 derived constants (NGA TR8350.2 §3.2 reference values) --------

    /// Semi-minor axis. Reference: 6 356 752.3142 m (NGA TR8350.2 Table 3.1).
    #[test]
    fn wgs84_semi_minor_axis() {
        assert_abs_diff_eq!(WGS84_B, 6_356_752.314_2_f64, epsilon = 1e-3);
    }

    /// Flattening reciprocal must recover the defining parameter exactly.
    #[test]
    fn wgs84_flattening_reciprocal() {
        assert_abs_diff_eq!(1.0 / WGS84_F, WGS84_F_INV, epsilon = 1e-9);
    }

    /// First eccentricity squared. Reference: 0.006 694 379 990 14.
    #[test]
    fn wgs84_e2() {
        assert_abs_diff_eq!(WGS84_E2, 0.006_694_379_990_14_f64, epsilon = 1e-14);
    }

    /// Second eccentricity squared. Reference: 0.006 739 496 742 28.
    #[test]
    fn wgs84_e_prime2() {
        assert_abs_diff_eq!(WGS84_E_PRIME2, 0.006_739_496_742_28_f64, epsilon = 1e-14);
    }

    /// Third flattening n. Reference: 1.6792203863837047 × 10⁻³
    #[test]
    fn wgs84_n() {
        assert_abs_diff_eq!(WGS84_N, 0.001_679_220_386_383_705_f64, epsilon = 1e-18);
    }

    /// b = a(1-f) must satisfy a² = b²/(1-e²) (ellipsoid identity check).
    #[test]
    fn wgs84_ellipsoid_identity() {
        let a2_from_b = WGS84_B * WGS84_B / (1.0 - WGS84_E2);
        assert_abs_diff_eq!(a2_from_b.sqrt(), WGS84_A, epsilon = 1e-6);
    }

    // --- GeoCoord -----------------------------------------------------------

    #[test]
    fn geocoord_from_degrees_roundtrip() {
        let c = GeoCoord::from_degrees(51.5, -0.1, 100.0).expect("valid coordinate");
        assert_abs_diff_eq!(c.lat_deg(), 51.5, epsilon = 1e-14);
        assert_abs_diff_eq!(c.lon_deg(), -0.1, epsilon = 1e-14);
        assert_abs_diff_eq!(c.height, 100.0, epsilon = 1e-12);
    }

    #[test]
    fn geocoord_from_degrees_to_radians() {
        let c = GeoCoord::from_degrees(90.0, 180.0, 0.0).expect("valid coordinate");
        assert_abs_diff_eq!(c.lat_rad(), std::f64::consts::FRAC_PI_2, epsilon = 1e-14);
        assert_abs_diff_eq!(c.lon_rad(), std::f64::consts::PI, epsilon = 1e-14);
    }

    #[test]
    fn geocoord_new_stores_radians() {
        let c = GeoCoord::new(1.0, 2.0, 50.0).expect("valid coordinate");
        assert_abs_diff_eq!(c.lat_rad(), 1.0, epsilon = 1e-15);
        assert_abs_diff_eq!(c.lon_rad(), 2.0, epsilon = 1e-15);
    }

    #[test]
    fn geocoord_rejects_latitude_out_of_range() {
        assert!(GeoCoord::from_degrees(90.001, 0.0, 0.0).is_err());
        assert!(GeoCoord::from_degrees(-90.001, 0.0, 0.0).is_err());
    }

    #[test]
    fn geocoord_rejects_longitude_out_of_range() {
        assert!(GeoCoord::from_degrees(0.0, 180.001, 0.0).is_err());
        assert!(GeoCoord::from_degrees(0.0, -180.001, 0.0).is_err());
    }

    #[test]
    fn geocoord_rejects_non_finite_values() {
        assert!(GeoCoord::from_degrees(f64::NAN, 0.0, 0.0).is_err());
        assert!(GeoCoord::from_degrees(0.0, f64::INFINITY, 0.0).is_err());
        assert!(GeoCoord::from_degrees(0.0, 0.0, f64::NAN).is_err());
        assert!(GeoCoord::new(f64::NAN, 0.0, 0.0).is_err());
    }

    #[test]
    fn geocoord_display_north_east() {
        let c = GeoCoord::from_degrees(51.5, 0.1, 0.0).expect("valid coordinate");
        let s = c.to_string();
        assert!(s.contains('N'), "expected N hemisphere in: {s}");
        assert!(s.contains('E'), "expected E hemisphere in: {s}");
    }

    #[test]
    fn geocoord_display_south_west() {
        let c = GeoCoord::from_degrees(-33.9, -70.7, 0.0).expect("valid coordinate");
        let s = c.to_string();
        assert!(s.contains('S'), "expected S hemisphere in: {s}");
        assert!(s.contains('W'), "expected W hemisphere in: {s}");
    }

    // --- UtmCoord -----------------------------------------------------------

    #[test]
    fn utm_display_north_zone() {
        let u = UtmCoord {
            easting: 699311.0,
            northing: 5710164.0,
            zone: 32,
            hemisphere: Hemisphere::North,
        };
        let s = u.to_string();
        assert!(s.starts_with("32N"), "expected '32N ...' in: {s}");
    }

    #[test]
    fn utm_display_south_zone() {
        let u = UtmCoord {
            easting: 350000.0,
            northing: 6250000.0,
            zone: 19,
            hemisphere: Hemisphere::South,
        };
        let s = u.to_string();
        assert!(s.starts_with("19S"), "expected '19S ...' in: {s}");
    }

    // --- FlightLine ---------------------------------------------------------

    #[test]
    fn flightline_default_is_empty() {
        let fl = FlightLine::default();
        assert!(fl.is_empty());
        assert_eq!(fl.len(), 0);
    }

    #[test]
    fn flightline_new_equals_default() {
        let a = FlightLine::new();
        let b = FlightLine::default();
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn flightline_push_maintains_soa_invariant() {
        let mut fl = FlightLine::new();
        fl.push(0.0, 0.0, 100.0);
        fl.push(0.1, 0.1, 110.0);
        fl.push(0.2, 0.2, 120.0);
        // All three vecs must always be the same length.
        assert_eq!(fl.lats().len(), fl.lons().len());
        assert_eq!(fl.lons().len(), fl.elevations().len());
        assert_eq!(fl.len(), 3);
    }

    #[test]
    fn flightline_accessor_values_match_push_order() {
        let mut fl = FlightLine::new();
        fl.push(1.0, 2.0, 300.0);
        assert_abs_diff_eq!(fl.lats()[0], 1.0, epsilon = 1e-15);
        assert_abs_diff_eq!(fl.lons()[0], 2.0, epsilon = 1e-15);
        assert_abs_diff_eq!(fl.elevations()[0], 300.0, epsilon = 1e-12);
    }

    // --- SensorParams -------------------------------------------------------

    /// DJI Phantom 4 Pro pixel pitch reference: 13.2 mm / 5472 px.
    #[test]
    fn sensor_pixel_pitch_phantom4pro() {
        let s = SensorParams {
            focal_length_mm: 8.8,
            sensor_width_mm: 13.2,
            sensor_height_mm: 8.8,
            image_width_px: 5472,
            image_height_px: 3648,
        };
        assert_abs_diff_eq!(s.pixel_pitch(), 13.2 / 5472.0_f64, epsilon = 1e-15);
    }

    #[test]
    fn sensor_display_contains_focal_length() {
        let s = SensorParams {
            focal_length_mm: 8.8,
            sensor_width_mm: 13.2,
            sensor_height_mm: 8.8,
            image_width_px: 5472,
            image_height_px: 3648,
        };
        assert!(s.to_string().contains("8.80mm"));
    }

    #[test]
    fn sensor_new_valid_params_succeeds() {
        let s = SensorParams::new(8.8, 13.2, 8.8, 5472, 3648).expect("valid Phantom 4 Pro params");
        assert_abs_diff_eq!(s.pixel_pitch(), 13.2 / 5472.0_f64, epsilon = 1e-15);
    }

    #[test]
    fn sensor_new_zero_width_px_is_error() {
        assert!(SensorParams::new(8.8, 13.2, 8.8, 0, 3648).is_err());
    }

    #[test]
    fn sensor_new_zero_height_px_is_error() {
        assert!(SensorParams::new(8.8, 13.2, 8.8, 5472, 0).is_err());
    }

    #[test]
    fn sensor_new_zero_focal_length_is_error() {
        assert!(SensorParams::new(0.0, 13.2, 8.8, 5472, 3648).is_err());
    }

    #[test]
    fn sensor_new_nan_focal_length_is_error() {
        assert!(SensorParams::new(f64::NAN, 13.2, 8.8, 5472, 3648).is_err());
    }
}
