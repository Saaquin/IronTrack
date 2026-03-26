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

/*
 * Flat-terrain flight line generation (Milestone 2A).
 *
 * Algorithm overview:
 *  1. Decompose the bounding box corners into cross-track / along-track
 *     components relative to the flight azimuth using Karney geodesic inverse.
 *  2. Determine the number of parallel lines needed to cover the cross-track
 *     span (formula from docs/formula_reference.md §4).
 *  3. For each line: compute the origin via a cross-track geodesic offset from
 *     the box centre, then step waypoints along the flight azimuth using Karney
 *     geodesic direct with Kahan compensated summation.
 *  4. Lines are generated in parallel with Rayon; each line is fully
 *     independent, satisfying the "No I/O inside par_iter" constraint.
 *
 * DEM-aware dynamic AGL adjustment is planned for Milestone 2C.
 */

use geographiclib_rs::{DirectGeodesic, Geodesic, InverseGeodesic};
use rayon::prelude::*;

use crate::dem::TerrainEngine;
use crate::error::{DemError, PhotogrammetryError};
use crate::geodesy::geoid::{Egm2008Model, GeoidModel};
use crate::types::{AltitudeDatum, FlightLine, SensorParams};

impl FlightLine {
    /// Convert all altitudes in this flight line to a target vertical datum.
    ///
    /// Chaining through the WGS84 ellipsoidal pivot is the only safe path for
    /// orthometric-to-orthometric transformations (e.g. EGM2008 → EGM96).
    /// AGL conversions require access to the `TerrainEngine` to query surface
    /// elevations at each waypoint.
    ///
    /// # Errors
    /// Propagates `DemError` if AGL conversion is requested and a DEM tile
    /// is missing or corrupt.
    pub fn to_datum(
        &self,
        target: AltitudeDatum,
        engine: &TerrainEngine,
    ) -> Result<FlightLine, DemError> {
        if self.altitude_datum == target {
            return Ok(self.clone());
        }

        let egm96 = GeoidModel::new();
        let egm2008 = Egm2008Model::new();

        // Phase 1: Convert current elevations to WGS84 ellipsoidal (h).
        let ellipsoidal_h: Vec<f64> = match self.altitude_datum {
            AltitudeDatum::Wgs84Ellipsoidal => self.elevations().to_vec(),
            AltitudeDatum::Egm2008 => self
                .lats()
                .iter()
                .zip(self.lons().iter().zip(self.elevations().iter()))
                .map(|(&la, (&lo, &h))| {
                    let n = egm2008.undulation(la.to_degrees(), lo.to_degrees())?;
                    Ok(h + n)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| DemError::Geoid(e))?,
            AltitudeDatum::Egm96 => self
                .lats()
                .iter()
                .zip(self.lons().iter().zip(self.elevations().iter()))
                .map(|(&la, (&lo, &h))| {
                    let n = egm96.undulation(la.to_degrees(), lo.to_degrees())?;
                    Ok(h + n)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| DemError::Geoid(e))?,
            AltitudeDatum::Agl => {
                // To get ellipsoidal h from AGL, we need: h = terrain_ortho + N + AGL
                self.lats()
                    .iter()
                    .zip(self.lons().iter().zip(self.elevations().iter()))
                    .map(|(&la, (&lo, &agl))| {
                        let report = engine.query(la.to_degrees(), lo.to_degrees())?;
                        Ok::<f64, DemError>(report.ellipsoidal_m + agl)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        // Phase 2: Convert from WGS84 ellipsoidal (h) to target datum.
        let final_elevations: Vec<f64> = match target {
            AltitudeDatum::Wgs84Ellipsoidal => ellipsoidal_h,
            AltitudeDatum::Egm2008 => self
                .lats()
                .iter()
                .zip(self.lons().iter().zip(ellipsoidal_h.iter()))
                .map(|(&la, (&lo, &h))| {
                    let n = egm2008.undulation(la.to_degrees(), lo.to_degrees())?;
                    Ok(h - n)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| DemError::Geoid(e))?,
            AltitudeDatum::Egm96 => self
                .lats()
                .iter()
                .zip(self.lons().iter().zip(ellipsoidal_h.iter()))
                .map(|(&la, (&lo, &h))| {
                    let n = egm96.undulation(la.to_degrees(), lo.to_degrees())?;
                    Ok(h - n)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| DemError::Geoid(e))?,
            AltitudeDatum::Agl => {
                // AGL = h_aircraft - h_terrain_ellipsoidal
                self.lats()
                    .iter()
                    .zip(self.lons().iter().zip(ellipsoidal_h.iter()))
                    .map(|(&la, (&lo, &h))| {
                        let report = engine.query(la.to_degrees(), lo.to_degrees())?;
                        Ok::<f64, DemError>(h - report.ellipsoidal_m)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        let mut new_line = FlightLine::new().with_datum(target);
        for i in 0..self.len() {
            new_line.push(self.lats()[i], self.lons()[i], final_elevations[i]);
        }

        Ok(new_line)
    }
}


// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/*
 * Normalise an azimuth delta to the half-open interval (-180°, 180°].
 *
 * Rust's `%` operator returns the sign of the dividend, so raw modulo is not
 * sufficient for negative deltas (e.g., -270° % 360° == -270°, not 90°).
 */
fn normalize_delta(deg: f64) -> f64 {
    let mut a = deg % 360.0;
    if a > 180.0 {
        a -= 360.0;
    } else if a <= -180.0 {
        a += 360.0;
    }
    a
}

// ---------------------------------------------------------------------------
// BoundingBox
// ---------------------------------------------------------------------------

/// Axis-aligned WGS84 bounding box in decimal degrees.
///
/// All four fields are public for direct construction. Validation
/// (finite, positive extent) is performed by `generate_flight_lines`.
pub struct BoundingBox {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

impl BoundingBox {
    /// Arithmetic centre of the box in (lat, lon) degrees.
    pub fn center(&self) -> (f64, f64) {
        (
            (self.min_lat + self.max_lat) * 0.5,
            (self.min_lon + self.max_lon) * 0.5,
        )
    }

    /*
     * Four corners in (lat, lon) degree pairs, ordered SW, SE, NW, NE.
     * Used to find the bounding cross-track and along-track extents via
     * geodesic projections from the box centre.
     */
    fn corners(&self) -> [(f64, f64); 4] {
        [
            (self.min_lat, self.min_lon), // SW
            (self.min_lat, self.max_lon), // SE
            (self.max_lat, self.min_lon), // NW
            (self.max_lat, self.max_lon), // NE
        ]
    }
}

// ---------------------------------------------------------------------------
// FlightPlanParams
// ---------------------------------------------------------------------------

/// Photogrammetric parameters that drive flight line spacing and photo interval.
#[derive(Clone)]
pub struct FlightPlanParams {
    /// Camera / sensor geometry for GSD and swath calculations.
    pub sensor: SensorParams,
    /// Target worst-case ground sample distance (metres/pixel).
    pub target_gsd_m: f64,
    /// Lateral (cross-track) image overlap, percent. Default: 30.0.
    pub side_lap_percent: f64,
    /// Forward (along-track) image overlap, percent. Default: 60.0.
    pub end_lap_percent: f64,
    /// Planned MSL flight altitude stored in waypoint elevations (metres).
    ///
    /// This field is **metadata for the autopilot** — it does not change the
    /// photogrammetric calculations. Line spacing and photo interval are always
    /// derived from `nominal_agl()` (the AGL required for the target GSD),
    /// regardless of this value.
    ///
    /// Intended use: when the survey area has a known terrain offset, set this
    /// to `nominal_agl + terrain_elevation_msl` so waypoints carry the correct
    /// MSL altitude. Example: target GSD → nominal_agl = 182 m, terrain at
    /// 150 m MSL → `flight_altitude_msl = Some(332.0)`.
    ///
    /// If `None`, waypoints are tagged with `nominal_agl` (flat-terrain,
    /// terrain = 0 m MSL assumed).
    pub flight_altitude_msl: Option<f64>,
}

impl FlightPlanParams {
    /// Construct with default overlaps (side 30 %, end 60 %) and no MSL override.
    ///
    /// Returns `Err(InvalidSensor)` if `target_gsd_m` is not finite or ≤ 0.
    pub fn new(sensor: SensorParams, target_gsd_m: f64) -> Result<Self, PhotogrammetryError> {
        if !target_gsd_m.is_finite() || target_gsd_m <= 0.0 {
            return Err(PhotogrammetryError::InvalidSensor(format!(
                "target_gsd_m must be positive and finite, got {target_gsd_m}"
            )));
        }
        Ok(Self {
            sensor,
            target_gsd_m,
            side_lap_percent: 30.0,
            end_lap_percent: 60.0,
            flight_altitude_msl: None,
        })
    }

    /// Builder: set lateral overlap percent. Returns `Err` if outside [0, 100).
    pub fn with_side_lap(mut self, pct: f64) -> Result<Self, PhotogrammetryError> {
        if !pct.is_finite() || !(0.0..100.0).contains(&pct) {
            return Err(PhotogrammetryError::InvalidOverlap(pct));
        }
        self.side_lap_percent = pct;
        Ok(self)
    }

    /// Builder: set forward overlap percent. Returns `Err` if outside [0, 100).
    pub fn with_end_lap(mut self, pct: f64) -> Result<Self, PhotogrammetryError> {
        if !pct.is_finite() || !(0.0..100.0).contains(&pct) {
            return Err(PhotogrammetryError::InvalidOverlap(pct));
        }
        self.end_lap_percent = pct;
        Ok(self)
    }

    /// AGL needed to achieve the target worst-case GSD (metres).
    pub fn nominal_agl(&self) -> f64 {
        self.sensor.required_agl_for_gsd(self.target_gsd_m)
    }

    /// Cross-track spacing between adjacent flight lines (metres).
    ///
    /// W = swath_width × (1 − side_lap / 100)  [formula_reference.md §4]
    pub fn flight_line_spacing(&self) -> f64 {
        self.sensor.swath_width(self.nominal_agl()) * (1.0 - self.side_lap_percent / 100.0)
    }

    /// Along-track distance between consecutive photo exposures (metres).
    ///
    /// B = swath_height × (1 − end_lap / 100)  [formula_reference.md §4]
    pub fn photo_interval(&self) -> f64 {
        self.sensor.swath_height(self.nominal_agl()) * (1.0 - self.end_lap_percent / 100.0)
    }
}

// ---------------------------------------------------------------------------
// FlightPlan — contract between the photogrammetry engine and the I/O layer
// ---------------------------------------------------------------------------

/// A complete, self-describing flight plan ready for serialisation.
///
/// Bundles the mission parameters (`params`), the flight direction
/// (`azimuth_deg`), and the computed waypoint data (`lines`) into a single
/// value so that the I/O layer (GeoPackage, GeoJSON) never needs to receive
/// these as separate arguments. This eliminates two categories of latent bug:
///
/// 1. **Azimuth mismatch** — `validate_overlap` and `adjust_for_terrain`
///    now read the azimuth from the same struct that was used to generate the
///    lines, making it impossible to accidentally pass a different value.
///
/// 2. **Unit confusion** — all coordinate-producing paths go through
///    `FlightLine::iter_lonlat_deg`, which converts radians → degrees and
///    enforces the RFC 7946 / GeoPackage [longitude, latitude] order at the
///    export gate.
///
/// Construct via [`generate_flight_lines`].
pub struct FlightPlan {
    /// Mission and sensor parameters used to generate this plan.
    pub params: FlightPlanParams,
    /// Flight direction, decimal degrees clockwise from north, in `[0, 360)`.
    pub azimuth_deg: f64,
    /// Ordered flight lines. Each line is a waypoint sequence in the SoA
    /// [`FlightLine`] format (lat/lon in radians, elevation in metres MSL).
    pub lines: Vec<FlightLine>,
}

// ---------------------------------------------------------------------------
// Flight line generation
// ---------------------------------------------------------------------------

/// Generate a grid of parallel flight lines covering `bbox` under a flat-terrain
/// (constant AGL) assumption.
///
/// Returns a `Vec<FlightLine>` ordered from left to right relative to the
/// flight direction (negative to positive cross-track offset from box centre).
/// Each `FlightLine` stores:
///   - `lats` / `lons` in **radians** (WGS84 geodetic)
///   - `elevations` set to the planned MSL flight altitude
///
/// # Geometry
/// The bounding box is decomposed into cross-track (perpendicular) and
/// along-track (parallel) extents via a planar projection of each corner's
/// Karney geodesic bearing and distance from the box centre. This
/// approximation is accurate to < 0.1 % for survey areas up to ~200 km.
///
/// # Errors
/// - `PhotogrammetryError::InvalidInput` — non-finite or degenerate bbox,
///   non-finite azimuth.
/// - `PhotogrammetryError::InvalidOverlap` — overlap settings that produce
///   a zero or negative line spacing or photo interval.
pub fn generate_flight_lines(
    bbox: &BoundingBox,
    azimuth_deg: f64,
    params: &FlightPlanParams,
) -> Result<FlightPlan, PhotogrammetryError> {
    // --- Input validation --------------------------------------------------

    let coords_ok = bbox.min_lat.is_finite()
        && bbox.max_lat.is_finite()
        && bbox.min_lon.is_finite()
        && bbox.max_lon.is_finite();
    if !coords_ok || bbox.min_lat >= bbox.max_lat || bbox.min_lon >= bbox.max_lon {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "bbox must have positive, finite extent; got ({:.4},{:.4}) to ({:.4},{:.4})",
            bbox.min_lat, bbox.min_lon, bbox.max_lat, bbox.max_lon,
        )));
    }
    /*
     * Reject coordinates outside the WGS84 domain before they reach the
     * Karney solver. This matches the invariant enforced by GeoCoord and
     * TerrainEngine elsewhere in the codebase.
     */
    if bbox.min_lat < -90.0 || bbox.max_lat > 90.0 || bbox.min_lon < -180.0 || bbox.max_lon > 180.0
    {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "bbox coordinates outside WGS84 bounds: lat [{:.4},{:.4}], lon [{:.4},{:.4}]",
            bbox.min_lat, bbox.max_lat, bbox.min_lon, bbox.max_lon,
        )));
    }
    if !azimuth_deg.is_finite() {
        return Err(PhotogrammetryError::InvalidInput(
            "azimuth_deg must be finite".into(),
        ));
    }
    // Normalise to [0, 360) for consistent cross_azi computation and tracing.
    let azimuth_deg = {
        let a = azimuth_deg % 360.0;
        if a < 0.0 {
            a + 360.0
        } else {
            a
        }
    };

    let spacing = params.flight_line_spacing();
    let photo_interval = params.photo_interval();

    if !spacing.is_finite() || spacing <= 0.0 {
        return Err(PhotogrammetryError::InvalidOverlap(params.side_lap_percent));
    }
    if !photo_interval.is_finite() || photo_interval <= 0.0 {
        return Err(PhotogrammetryError::InvalidOverlap(params.end_lap_percent));
    }

    /*
     * MSL altitude stored in each waypoint's elevation slot.
     * For the flat-terrain case without an explicit MSL override, the terrain
     * is treated as 0 m MSL and the aircraft flies at nominal_agl above it.
     */
    let (flight_alt_m, datum) = match params.flight_altitude_msl {
        Some(msl) => (msl, AltitudeDatum::Egm2008),
        None => (params.nominal_agl(), AltitudeDatum::Agl),
    };

    // --- Decompose bbox corners into cross-track / along-track extents -----

    /*
     * For each corner we compute the signed projection onto the cross-track
     * and along-track axes using the planar approximation:
     *
     *   delta = normalize(azi_to_corner − flight_azi)  [in radians]
     *   cross = s12 × sin(delta)   (positive = right of flight)
     *   along = s12 × cos(delta)   (positive = ahead of flight)
     *
     * s12 and azi_to_corner come from the Karney inverse geodesic — the most
     * accurate available (~15 nm) and free of the convergence failures that
     * make Vincenty unsafe in Rayon (CLAUDE.md).
     */
    let geod = Geodesic::wgs84();
    let (center_lat, center_lon) = bbox.center();

    let mut cross_min = f64::INFINITY;
    let mut cross_max = f64::NEG_INFINITY;
    let mut along_min = f64::INFINITY;
    let mut along_max = f64::NEG_INFINITY;

    for (c_lat, c_lon) in bbox.corners() {
        // 4-tuple inverse → (s12, azi1, azi2, a12)
        let (s12, azi_to, _azi2, _a12): (f64, f64, f64, f64) =
            geod.inverse(center_lat, center_lon, c_lat, c_lon);
        let delta_rad = normalize_delta(azi_to - azimuth_deg).to_radians();
        let cross = s12 * delta_rad.sin();
        let along = s12 * delta_rad.cos();
        cross_min = cross_min.min(cross);
        cross_max = cross_max.max(cross);
        along_min = along_min.min(along);
        along_max = along_max.max(along);
    }

    let cross_span = cross_max - cross_min; // metres, always >= 0
    let along_span = along_max - along_min; // metres, always >= 0

    // --- Grid dimensions ---------------------------------------------------

    /*
     * Number of lines: lines are positioned at cross_min, cross_min+spacing,
     * …, cross_min+(n−1)×spacing. The last line must reach or exceed cross_max
     * to guarantee coverage at the right edge:
     *   (n−1) × spacing ≥ cross_span  →  n = ⌈cross_span / spacing⌉ + 1
     *
     * For a degenerate (zero-width) cross-span, a single centre line suffices.
     */
    let n_lines = if cross_span < 1e-9 {
        1_usize
    } else {
        (cross_span / spacing).ceil() as usize + 1
    };

    /*
     * Waypoints per line: same logic — step from 0 to along_span in
     * photo_interval steps, always including an endpoint.
     */
    let n_waypoints = if along_span < 1e-9 {
        1_usize
    } else {
        (along_span / photo_interval).ceil() as usize + 1
    };

    // Cross-track direction is 90° clockwise from the flight azimuth.
    let cross_azi = azimuth_deg + 90.0;

    // --- Generate lines (parallel via Rayon) --------------------------------

    /*
     * Each closure is self-contained: it holds a copy of the Geodesic
     * (which is Copy) and all scalar parameters. No shared mutable state.
     */
    let lines: Vec<FlightLine> = (0..n_lines)
        .into_par_iter()
        .map(|i| {
            // Cross-track offset of this line from the box centre (metres).
            let cross_offset = cross_min + i as f64 * spacing;

            // Origin on the cross-track axis.
            let (orig_lat, orig_lon): (f64, f64) =
                geod.direct(center_lat, center_lon, cross_azi, cross_offset);

            /*
             * First waypoint: step `along_min` from origin along the flight
             * direction. along_min is negative for a symmetric box (the far
             * edge is "behind" the centre), so geod.direct receives a negative
             * s12 and correctly steps in the reverse direction.
             */
            let (start_lat, start_lon): (f64, f64) =
                geod.direct(orig_lat, orig_lon, azimuth_deg, along_min);

            /*
             * Step waypoints forward using Kahan compensated summation to
             * prevent floating-point drift over long flight lines. Each
             * waypoint is placed via an independent direct geodesic from the
             * line start, so errors do not compound across steps.
             */
            let mut line = FlightLine::new().with_datum(datum);
            let mut kahan_sum = 0.0_f64;
            let mut kahan_c = 0.0_f64;

            for j in 0..n_waypoints {
                let (wp_lat, wp_lon): (f64, f64) =
                    geod.direct(start_lat, start_lon, azimuth_deg, kahan_sum);

                // FlightLine stores geodetic coordinates in radians.
                line.push(wp_lat.to_radians(), wp_lon.to_radians(), flight_alt_m);

                // Advance the Kahan accumulator (skip on the last iteration).
                if j + 1 < n_waypoints {
                    let y = photo_interval - kahan_c;
                    let t = kahan_sum + y;
                    kahan_c = (t - kahan_sum) - y;
                    kahan_sum = t;
                }
            }

            line
        })
        .collect();

    Ok(FlightPlan {
        params: params.clone(),
        azimuth_deg,
        lines,
    })
}

// ---------------------------------------------------------------------------
// Terrain-aware overlap validation and altitude adjustment (Milestone 2C)
// ---------------------------------------------------------------------------

/// Per-exposure-point side-lap quality report produced by [`validate_overlap`].
pub struct OverlapCheck {
    /// Index of the parent [`FlightLine`] in the input slice.
    pub line_index: usize,
    /// Index of this waypoint within the parent flight line.
    pub point_index: usize,
    /// Waypoint latitude, decimal degrees.
    pub lat: f64,
    /// Waypoint longitude, decimal degrees.
    pub lon: f64,
    /// Peak orthometric terrain elevation within the camera footprint (metres).
    pub terrain_peak_m: f64,
    /// Effective AGL = waypoint MSL altitude − terrain_peak_m (metres).
    pub actual_agl_m: f64,
    /// Actual cross-track swath width at effective AGL (metres).
    pub actual_swath_m: f64,
    /// Actual side-lap at the planned line spacing and effective AGL (percent).
    pub actual_overlap_pct: f64,
    /// `true` if `actual_overlap_pct` is below the target `side_lap_percent`.
    pub violation: bool,
}

/*
 * Append 5 DEM sample points (centre + 4 footprint corners) for a single
 * exposure point to flat lat/lon accumulator vectors.
 *
 * Corner positions use two sequential Karney direct geodesic calls:
 *   1. Offset ±half_cross_m from centre along the cross-track axis
 *      (azimuth + 90°).
 *   2. Offset ±half_along_m from that intermediate point along the flight
 *      azimuth.
 *
 * This is a small-angle approximation — accurate to sub-metre for footprint
 * diagonals under ~2 km, which covers all practical survey altitudes.
 *
 * Points pushed in order: centre, NE, SE, NW, SW (5 total; N = along+,
 * S = along−, E = cross+, W = cross−).
 */
#[allow(clippy::too_many_arguments)]
fn push_footprint_samples(
    lat_deg: f64,
    lon_deg: f64,
    azimuth_deg: f64,
    half_cross_m: f64,
    half_along_m: f64,
    geod: &Geodesic,
    lats: &mut Vec<f64>,
    lons: &mut Vec<f64>,
) {
    // Centre.
    lats.push(lat_deg);
    lons.push(lon_deg);

    let cross_azi = azimuth_deg + 90.0;

    for &cross_sign in &[1.0_f64, -1.0_f64] {
        // Intermediate: step cross-track from the exposure centre.
        let (mid_lat, mid_lon): (f64, f64) =
            geod.direct(lat_deg, lon_deg, cross_azi, cross_sign * half_cross_m);

        // Two corners along the flight direction from this cross-track offset.
        for &along_sign in &[1.0_f64, -1.0_f64] {
            let (c_lat, c_lon): (f64, f64) =
                geod.direct(mid_lat, mid_lon, azimuth_deg, along_sign * half_along_m);
            lats.push(c_lat);
            lons.push(c_lon);
        }
    }
}

/// Validate side-lap overlap at every exposure point across all flight lines.
///
/// For each waypoint, five DEM samples are taken (centre + four footprint
/// corners). The peak terrain within the footprint is used to compute the
/// effective AGL and actual side-lap overlap, which are compared against the
/// target threshold stored in `params.side_lap_percent`.
///
/// ## Two-Phase I/O
/// All terrain queries are dispatched as a single batch to
/// [`TerrainEngine::elevations_at`], which handles tile pre-warming
/// internally. No I/O occurs inside a Rayon closure.
///
/// # Errors
/// Propagates [`DemError`] from the bulk terrain query (tile not found,
/// parse error, I/O failure).
pub fn validate_overlap(
    plan: &FlightPlan,
    terrain: &TerrainEngine,
) -> Result<Vec<OverlapCheck>, DemError> {
    let geod = Geodesic::wgs84();
    let params = &plan.params;
    let azimuth_deg = plan.azimuth_deg;
    let nominal_agl = params.nominal_agl();
    let half_cross = params.sensor.swath_width(nominal_agl) / 2.0;
    let half_along = params.sensor.swath_height(nominal_agl) / 2.0;
    let spacing = params.flight_line_spacing();

    // --- Phase 1: collect all DEM sample coordinates (sequential) -----------

    let total_waypoints: usize = plan.lines.iter().map(|l| l.len()).sum();
    let mut sample_lats: Vec<f64> = Vec::with_capacity(total_waypoints * 5);
    let mut sample_lons: Vec<f64> = Vec::with_capacity(total_waypoints * 5);

    /*
     * Flat metadata list — one entry per waypoint — so the parallel pass can
     * reconstruct (line_index, point_index, lat_deg, lon_deg, waypoint_alt)
     * from the global waypoint index without touching the input slice inside
     * a Rayon closure.
     */
    let mut wp_meta: Vec<(usize, usize, f64, f64, f64)> = Vec::with_capacity(total_waypoints);

    for (li, line) in plan.lines.iter().enumerate() {
        for (pi, (&lat_rad, (&lon_rad, &alt_m))) in line
            .lats()
            .iter()
            .zip(line.lons().iter().zip(line.elevations().iter()))
            .enumerate()
        {
            let lat_deg = lat_rad.to_degrees();
            let lon_deg = lon_rad.to_degrees();
            push_footprint_samples(
                lat_deg,
                lon_deg,
                azimuth_deg,
                half_cross,
                half_along,
                &geod,
                &mut sample_lats,
                &mut sample_lons,
            );
            wp_meta.push((li, pi, lat_deg, lon_deg, alt_m));
        }
    }

    // --- Bulk DEM query (Two-Phase I/O inside elevations_at) ----------------

    let elevations = terrain.elevations_at(&sample_lats, &sample_lons)?;

    // --- Phase 2: compute per-waypoint reports (parallel via Rayon) ---------

    /*
     * Each waypoint owns 5 consecutive elevation samples starting at
     * global_idx * 5. Finding the peak and computing derived quantities
     * is pure arithmetic — no I/O inside par_iter.
     */
    let reports: Vec<OverlapCheck> = wp_meta
        .into_par_iter()
        .enumerate()
        .map(|(global_idx, (li, pi, lat_deg, lon_deg, alt_m))| {
            let base = global_idx * 5;
            let terrain_peak_m = elevations[base..base + 5]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);

            let actual_agl_m = alt_m - terrain_peak_m;
            let actual_swath_m = params.sensor.swath_width(actual_agl_m);
            let actual_overlap_pct = (1.0 - spacing / actual_swath_m) * 100.0;
            /*
             * When AGL ≤ 0 the aircraft is at or below the terrain surface.
             * `swath_width` is linear in AGL and returns a non-positive value,
             * which makes the overlap formula above produce a large positive
             * percentage — a false "no violation" signal. Override: any
             * non-positive AGL is unconditionally a violation, regardless of
             * what the algebra produces.
             */
            let violation = actual_agl_m <= 0.0 || actual_overlap_pct < params.side_lap_percent;

            OverlapCheck {
                line_index: li,
                point_index: pi,
                lat: lat_deg,
                lon: lon_deg,
                terrain_peak_m,
                actual_agl_m,
                actual_swath_m,
                actual_overlap_pct,
                violation,
            }
        })
        .collect();

    Ok(reports)
}

/// Adjust flight line waypoint altitudes to maintain the target GSD over terrain.
///
/// For each waypoint, five terrain samples are taken (centre + four footprint
/// corners). The planned altitude is raised to
/// `max(planned_alt, terrain_peak + nominal_agl)` so that the sensor footprint
/// never compresses below the target GSD resolution.
///
/// Lines and waypoints are returned in the same order as `lines`. Only the
/// elevation component is modified — lat/lon coordinates are unchanged.
///
/// ## Two-Phase I/O
/// All terrain queries are dispatched as a single batch to
/// [`TerrainEngine::elevations_at`]. No I/O occurs inside a Rayon closure.
///
/// # Errors
/// Propagates [`DemError`] from the bulk terrain query.
pub fn adjust_for_terrain(
    plan: &FlightPlan,
    terrain: &TerrainEngine,
) -> Result<FlightPlan, DemError> {
    let geod = Geodesic::wgs84();
    let params = &plan.params;
    let azimuth_deg = plan.azimuth_deg;
    let nominal_agl = params.nominal_agl();
    let half_cross = params.sensor.swath_width(nominal_agl) / 2.0;
    let half_along = params.sensor.swath_height(nominal_agl) / 2.0;

    // --- Phase 1: collect all DEM sample coordinates (sequential) -----------

    let total_waypoints: usize = plan.lines.iter().map(|l| l.len()).sum();
    let mut sample_lats: Vec<f64> = Vec::with_capacity(total_waypoints * 5);
    let mut sample_lons: Vec<f64> = Vec::with_capacity(total_waypoints * 5);

    // (lat_rad, lon_rad, planned_alt_m) per waypoint for the Rayon pass.
    let mut wp_data: Vec<(f64, f64, f64)> = Vec::with_capacity(total_waypoints);

    for line in &plan.lines {
        for (&lat_rad, (&lon_rad, &alt_m)) in line
            .lats()
            .iter()
            .zip(line.lons().iter().zip(line.elevations().iter()))
        {
            push_footprint_samples(
                lat_rad.to_degrees(),
                lon_rad.to_degrees(),
                azimuth_deg,
                half_cross,
                half_along,
                &geod,
                &mut sample_lats,
                &mut sample_lons,
            );
            wp_data.push((lat_rad, lon_rad, alt_m));
        }
    }

    // --- Bulk DEM query (Two-Phase I/O inside elevations_at) ----------------

    let elevations = terrain.elevations_at(&sample_lats, &sample_lons)?;

    // --- Phase 2: compute adjusted altitudes (parallel via Rayon) -----------

    /*
     * Terrain-following floor strategy from docs/03:
     *   adjusted_alt = max(planned_alt, terrain_peak + nominal_agl)
     *
     * The aircraft never descends below the altitude needed to achieve the
     * target GSD regardless of how high the terrain is within the footprint.
     */
    let adjusted: Vec<(f64, f64, f64)> = wp_data
        .into_par_iter()
        .enumerate()
        .map(|(global_idx, (lat_rad, lon_rad, planned_alt))| {
            let base = global_idx * 5;
            let terrain_peak_m = elevations[base..base + 5]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);

            let required_alt = terrain_peak_m + nominal_agl;
            (lat_rad, lon_rad, planned_alt.max(required_alt))
        })
        .collect();

    // Reconstruct FlightLine objects preserving the original line structure.
    let mut out_lines: Vec<FlightLine> = Vec::with_capacity(plan.lines.len());
    let mut flat_idx = 0_usize;
    for line in &plan.lines {
        let n = line.len();
        let mut new_line = FlightLine::new().with_datum(AltitudeDatum::Egm2008);
        for i in 0..n {
            let (lat_rad, lon_rad, alt_m) = adjusted[flat_idx + i];
            new_line.push(lat_rad, lon_rad, alt_m);
        }
        flat_idx += n;
        out_lines.push(new_line);
    }

    Ok(FlightPlan {
        params: plan.params.clone(),
        azimuth_deg: plan.azimuth_deg,
        lines: out_lines,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use geographiclib_rs::{Geodesic, InverseGeodesic};

    /*
     * Reference sensor: DJI Phantom 4 Pro
     *   Focal: 8.8 mm | Sensor: 13.2 × 8.8 mm | Resolution: 5472 × 3648 px
     *
     * The Phantom 4 Pro has square pixels (13.2/5472 == 8.8/3648), so:
     *   pixel_pitch = 1/3648 mm/px (both axes identical)
     *   nominal_agl  = 0.05 / (1/3648) = 182.4 m  for target GSD 5 cm/px
     *   swath_width  = 182.4 × 13.2 / 8.8 = 273.6 m
     *   swath_height = 182.4 × 8.8  / 8.8 = 182.4 m
     *   spacing      = 273.6 × 0.70       = 191.52 m  (30 % side lap)
     *   interval     = 182.4 × 0.40       = 72.96 m   (60 % end lap)
     *
     * Test bbox (0.0°, 0.0°) → (0.01°, 0.01°) near equator:
     *   cross_span ≈ 1113 m  (east-west, for N-S flight azimuth 0°)
     *   along_span ≈ 1111 m  (north-south)
     *   n_lines     = ⌈1113/191.52⌉ + 1 = 6 + 1 = 7
     *   n_waypoints = ⌈1111/72.96⌉  + 1 = 16 + 1 = 17
     */

    fn phantom4pro() -> SensorParams {
        SensorParams {
            focal_length_mm: 8.8,
            sensor_width_mm: 13.2,
            sensor_height_mm: 8.8,
            image_width_px: 5472,
            image_height_px: 3648,
        }
    }

    fn equator_bbox() -> BoundingBox {
        BoundingBox {
            min_lat: 0.0,
            min_lon: 0.0,
            max_lat: 0.01,
            max_lon: 0.01,
        }
    }

    // --- FlightPlanParams unit tests ----------------------------------------

    #[test]
    fn flight_plan_params_defaults() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        assert_abs_diff_eq!(p.side_lap_percent, 30.0, epsilon = 1e-12);
        assert_abs_diff_eq!(p.end_lap_percent, 60.0, epsilon = 1e-12);
        assert!(p.flight_altitude_msl.is_none());
    }

    #[test]
    fn nominal_agl_matches_sensor_required_agl() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let expected = p.sensor.required_agl_for_gsd(0.05);
        assert_abs_diff_eq!(p.nominal_agl(), expected, epsilon = 1e-12);
    }

    #[test]
    fn spacing_formula_phantom4pro() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        // swath_width = 182.4 × (13.2/8.8) = 273.6 m; spacing = 273.6 × 0.70
        assert_abs_diff_eq!(p.flight_line_spacing(), 191.52, epsilon = 1e-6);
    }

    #[test]
    fn interval_formula_phantom4pro() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        // swath_height = 182.4 × (8.8/8.8) = 182.4 m; interval = 182.4 × 0.40
        assert_abs_diff_eq!(p.photo_interval(), 72.96, epsilon = 1e-6);
    }

    #[test]
    fn with_side_lap_changes_spacing() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05)
            .unwrap()
            .with_side_lap(60.0)
            .unwrap();
        // spacing = 273.6 × 0.40 = 109.44 m
        assert_abs_diff_eq!(p.flight_line_spacing(), 109.44, epsilon = 1e-6);
    }

    #[test]
    fn with_side_lap_100_is_error() {
        let result = FlightPlanParams::new(phantom4pro(), 0.05)
            .unwrap()
            .with_side_lap(100.0);
        assert!(result.is_err());
    }

    #[test]
    fn with_end_lap_100_is_error() {
        let result = FlightPlanParams::new(phantom4pro(), 0.05)
            .unwrap()
            .with_end_lap(100.0);
        assert!(result.is_err());
    }

    #[test]
    fn zero_target_gsd_is_error() {
        assert!(FlightPlanParams::new(phantom4pro(), 0.0).is_err());
    }

    #[test]
    fn negative_target_gsd_is_error() {
        assert!(FlightPlanParams::new(phantom4pro(), -1.0).is_err());
    }

    // --- generate_flight_lines integration tests ----------------------------

    #[test]
    fn generates_correct_line_count_near_equator() {
        /*
         * cross_span ≈ 1113 m, spacing = 191.52 m
         * n_lines = ⌈1113/191.52⌉ + 1 = 6 + 1 = 7
         */
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &p).unwrap();
        assert_eq!(plan.lines.len(), 7, "expected 7 flight lines");
    }

    #[test]
    fn generates_correct_waypoint_count_per_line() {
        /*
         * along_span ≈ 1111 m, photo_interval = 72.96 m
         * n_waypoints = ⌈1111/72.96⌉ + 1 = 16 + 1 = 17
         */
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &p).unwrap();
        for (i, line) in plan.lines.iter().enumerate() {
            assert_eq!(
                line.len(),
                17,
                "line {i}: expected 17 waypoints, got {}",
                line.len()
            );
        }
    }

    #[test]
    fn waypoints_go_north_for_azimuth_zero() {
        // For azimuth 0° (north), successive waypoints must have increasing latitude.
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &p).unwrap();
        let lats = plan.lines[0].lats();
        assert!(
            lats.last().unwrap() > lats.first().unwrap(),
            "last waypoint latitude should exceed first for northward flight"
        );
    }

    #[test]
    fn adjacent_lines_spaced_by_flight_line_spacing() {
        /*
         * For N-S flight, lines are offset E-W. The geodesic distance between
         * the first waypoints of adjacent lines should equal the flight line
         * spacing to within 1 m (the planar decomposition error over a 1 km box
         * is sub-metre; the dominant residual is the ~0.1 % approximation).
         */
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let spacing = p.flight_line_spacing();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &p).unwrap();
        let geod = Geodesic::wgs84();

        for pair in plan.lines.windows(2) {
            let l0 = &pair[0];
            let l1 = &pair[1];
            let s12: f64 = geod.inverse(
                l0.lats()[0].to_degrees(),
                l0.lons()[0].to_degrees(),
                l1.lats()[0].to_degrees(),
                l1.lons()[0].to_degrees(),
            );
            assert_abs_diff_eq!(s12, spacing, epsilon = 1.0);
        }
    }

    #[test]
    fn first_waypoints_near_south_edge_for_azimuth_zero() {
        // The southernmost waypoint of any line should be within one photo
        // interval of the southern bbox edge (within ~75 m for this sensor).
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let interval = p.photo_interval();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &p).unwrap();
        for line in &plan.lines {
            let south_lat = line.lats()[0].to_degrees();
            assert!(
                south_lat < equator_bbox().min_lat + interval / 110_574.0,
                "first waypoint lat {south_lat:.6}° should be near south edge",
            );
        }
    }

    #[test]
    fn generate_lines_eastward_flight_swaps_axes() {
        /*
         * For azimuth 90° (east), lines run E-W and are spaced N-S.
         * The number of lines should now depend on the N-S span (~1111 m)
         * while waypoints step along the E-W span (~1113 m).
         * n_lines = ⌈1111/191.52⌉ + 1 = 6 + 1 = 7 (within ±1 of N-S case)
         */
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 90.0, &p).unwrap();
        // At equator lat=lon span is nearly equal, so expect the same count.
        assert!(
            plan.lines.len() >= 6 && plan.lines.len() <= 8,
            "expected 6–8 lines for eastward flight, got {}",
            plan.lines.len()
        );
    }

    #[test]
    fn invalid_bbox_min_equals_max_is_error() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let degenerate = BoundingBox {
            min_lat: 49.0,
            min_lon: -123.0,
            max_lat: 49.0,
            max_lon: -122.0,
        };
        assert!(generate_flight_lines(&degenerate, 0.0, &p).is_err());
    }

    #[test]
    fn invalid_bbox_inverted_coords_is_error() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let inverted = BoundingBox {
            min_lat: 0.01,
            min_lon: 0.01,
            max_lat: 0.0,
            max_lon: 0.0,
        };
        assert!(generate_flight_lines(&inverted, 0.0, &p).is_err());
    }

    #[test]
    fn bbox_outside_wgs84_bounds_is_error() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        // Latitude > 90
        let oob_lat = BoundingBox {
            min_lat: 89.0,
            min_lon: 0.0,
            max_lat: 91.0,
            max_lon: 1.0,
        };
        assert!(generate_flight_lines(&oob_lat, 0.0, &p).is_err());
        // Longitude > 180
        let oob_lon = BoundingBox {
            min_lat: 0.0,
            min_lon: 179.0,
            max_lat: 1.0,
            max_lon: 181.0,
        };
        assert!(generate_flight_lines(&oob_lon, 0.0, &p).is_err());
    }

    #[test]
    fn non_finite_azimuth_is_error() {
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        assert!(generate_flight_lines(&equator_bbox(), f64::NAN, &p).is_err());
        assert!(generate_flight_lines(&equator_bbox(), f64::INFINITY, &p).is_err());
    }

    // --- FlightPlan struct tests (Milestone 2D) ------------------------------

    #[test]
    fn flight_plan_preserves_azimuth() {
        // The azimuth stored in the returned FlightPlan must match what was passed in.
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 45.0, &p).unwrap();
        assert_abs_diff_eq!(plan.azimuth_deg, 45.0, epsilon = 1e-12);
    }

    #[test]
    fn flight_plan_preserves_params() {
        // Mission parameters cloned into FlightPlan must match the originals.
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let target = p.target_gsd_m;
        let side = p.side_lap_percent;
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &p).unwrap();
        assert_abs_diff_eq!(plan.params.target_gsd_m, target, epsilon = 1e-15);
        assert_abs_diff_eq!(plan.params.side_lap_percent, side, epsilon = 1e-15);
    }

    #[test]
    fn iter_lonlat_deg_order_and_units() {
        /*
         * iter_lonlat_deg must return (longitude, latitude) in decimal degrees
         * — not (latitude, longitude) and not radians.
         *
         * For a north-bound flight starting at the equator (lat ≈ 0°,
         * lon ≈ 0°), the first waypoint's longitude should be near 0° and
         * the returned pair must be (lon, lat), not (lat, lon).
         */
        let p = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &p).unwrap();
        let line = &plan.lines[0];

        // Verify the iterator length matches the waypoint count.
        assert_eq!(line.iter_lonlat_deg().count(), line.len());

        // Spot-check the first pair: both should be small degree values near
        // [0, 0.01]° — not radian-scale values (~0.0–0.0002 rad would be
        // ~0°–0.01°, and ~0.0–0.0175 rad would be ~0°–1°; radian values
        // outside [-π, π] would be a clear mistake).
        let (lon_deg, lat_deg) = line.iter_lonlat_deg().next().unwrap();
        assert!(
            lon_deg.abs() <= 0.02,
            "expected lon near 0°, got {lon_deg:.6}° (possible lat/lon swap or radian leak)"
        );
        assert!(
            lat_deg.abs() <= 0.02,
            "expected lat near 0°, got {lat_deg:.6}° (possible lat/lon swap or radian leak)"
        );
        // The pair must be in (lon, lat) order: lon is the first element.
        // For a north-south grid, the first (western-most) line has
        // longitude < latitude at this location (both near 0, but lon is
        // consistently below the line's latitude for lines east of centre).
        // We verify the two fields are not swapped by checking the raw slices:
        let raw_lon_rad = line.lons()[0];
        let raw_lat_rad = line.lats()[0];
        assert_abs_diff_eq!(lon_deg, raw_lon_rad.to_degrees(), epsilon = 1e-12);
        assert_abs_diff_eq!(lat_deg, raw_lat_rad.to_degrees(), epsilon = 1e-12);
    }

    // --- Terrain-aware tests (Milestone 2C) ---------------------------------

    /*
     * Build a FLOAT32 TIFF in memory with Compression=8 (DEFLATE) and
     * Predictor=3 (FloatingPoint) — the exact combination used by production
     * Copernicus COG tiles. Using the same compression path in tests enforces
     * the Feature-Matched Testing constraint from CLAUDE.md: uncompressed
     * fixtures would exercise a code path that never appears in production
     * and could silently pass even if the DEFLATE/predictor decode regresses.
     *
     * Encoding (inverse of tiff::decoder::fp_predict_f32, applied per row):
     *   1. For each row, extract big-endian bytes and arrange into 4 byte
     *      planes: [b0 of all pixels | b1 | b2 | b3] — cols×4 bytes per row.
     *   2. Apply forward horizontal differencing right-to-left:
     *      buf[k] = buf[k] - buf[k-1]  for k in (1..len).rev()
     *   3. Concatenate all encoded rows and zlib-DEFLATE compress.
     *
     * The TIFF IFD has 11 entries (10 standard + Predictor tag 317).
     */
    fn make_flat_f32_tiff(value: f32, rows: u32, cols: u32) -> Vec<u8> {
        use flate2::write::ZlibEncoder;
        use flate2::Compression as Flate2Compression;

        let rows_n = rows as usize;
        let cols_n = cols as usize;
        let be_bytes = value.to_bits().to_be_bytes();

        // Encode with FloatingPoint predictor, row by row.
        let mut encoded: Vec<u8> = Vec::with_capacity(rows_n * cols_n * 4);
        for _ in 0..rows_n {
            let mut row: Vec<u8> = Vec::with_capacity(cols_n * 4);
            for plane in 0..4_usize {
                for _ in 0..cols_n {
                    row.push(be_bytes[plane]);
                }
            }
            for k in (1..row.len()).rev() {
                row[k] = row[k].wrapping_sub(row[k - 1]);
            }
            encoded.extend_from_slice(&row);
        }

        // Zlib-compress (tiff crate uses ZlibDecoder for Compression=8).
        let mut enc = ZlibEncoder::new(Vec::new(), Flate2Compression::default());
        std::io::Write::write_all(&mut enc, &encoded).expect("zlib write");
        let compressed = enc.finish().expect("zlib finish");

        // Build TIFF header + IFD (11 entries, tags in ascending order).
        const IFD_ENTRIES: u16 = 11;
        let data_offset: u32 = 8 + 2 + IFD_ENTRIES as u32 * 12 + 4;
        let data_bytes = compressed.len() as u32;

        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&42u16.to_le_bytes());
        buf.extend_from_slice(&8u32.to_le_bytes());
        buf.extend_from_slice(&IFD_ENTRIES.to_le_bytes());

        let mut entry = |tag: u16, typ: u16, cnt: u32, val: u32| {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&typ.to_le_bytes());
            buf.extend_from_slice(&cnt.to_le_bytes());
            buf.extend_from_slice(&val.to_le_bytes());
        };
        entry(256, 3, 1, cols);
        entry(257, 3, 1, rows);
        entry(258, 3, 1, 32);
        entry(259, 3, 1, 8); // Compression = DEFLATE
        entry(262, 3, 1, 1);
        entry(273, 4, 1, data_offset);
        entry(277, 3, 1, 1);
        entry(278, 3, 1, rows);
        entry(279, 4, 1, data_bytes);
        entry(317, 3, 1, 3); // Predictor = FloatingPoint
        entry(339, 3, 1, 3); // SampleFormat = IEEE float

        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&compressed);
        buf
    }

    /*
     * Build a TerrainEngine backed by flat synthetic tiles at the given
     * orthometric elevation (metres).
     *
     * The equator bbox is (0.0°, 0.0°) → (0.01°, 0.01°). Footprint corners
     * extend ≈±137 m cross-track and ≈±91 m along-track from each waypoint.
     * The westernmost / southernmost lines sit near lon=0° and lat=0°, so
     * their corners can stray into the three neighbouring tiles:
     *   N00 E000  (lat 0–1°, lon  0–1°)  — primary
     *   N00 W001  (lat 0–1°, lon −1–0°)  — west footprint corners
     *   S01 E000  (lat −1–0°, lon 0–1°)  — south footprint corners
     *   S01 W001  (lat −1–0°, lon −1–0°) — SW footprint corners
     *
     * All four tiles are written with the same uniform elevation so that the
     * bilinear peak is consistent across every exposure point.
     */
    fn flat_terrain_engine(elevation_m: f32) -> (tempfile::TempDir, crate::dem::TerrainEngine) {
        use std::fs;
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let tiff = make_flat_f32_tiff(elevation_m, 10, 10);
        for (tile_lat, tile_lon) in [(0, 0), (0, -1), (-1, 0), (-1, -1)] {
            let filename = crate::dem::cache::tile_filename(tile_lat, tile_lon);
            fs::write(dir.path().join(&filename), &tiff).expect("write synthetic TIFF");
        }
        let engine = crate::dem::TerrainEngine::with_cache_dir(dir.path().to_path_buf())
            .expect("build TerrainEngine");
        (dir, engine)
    }

    #[test]
    fn validate_overlap_flat_terrain_no_violations() {
        /*
         * Terrain = 0 m. Nominal AGL = 182.4 m, swath = 273.6 m, spacing = 191.52 m.
         * Actual overlap = (1 − 191.52/273.6) × 100 ≈ 30 % == target → no violation.
         */
        let (_dir, engine) = flat_terrain_engine(0.0);
        let params = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &params).unwrap();
        let reports = validate_overlap(&plan, &engine).unwrap();

        assert_eq!(
            reports.len(),
            plan.lines.len() * plan.lines[0].len(),
            "one report per waypoint"
        );
        for r in &reports {
            assert!(
                !r.violation,
                "expected no violation at line {}, point {}",
                r.line_index, r.point_index
            );
            assert_abs_diff_eq!(r.terrain_peak_m, 0.0, epsilon = 1e-3);
        }
    }

    #[test]
    fn validate_overlap_elevated_terrain_causes_violations() {
        /*
         * Terrain = 100 m. Waypoints were generated at flat-terrain altitude
         * (182.4 m MSL) so actual_agl = 82.4 m.
         *   actual_swath = 82.4 × (13.2/8.8) = 123.6 m  (< spacing 191.52 m)
         *   actual_overlap = (1 − 191.52/123.6) × 100 ≈ −54.9 %  → violation
         */
        let (_dir, engine) = flat_terrain_engine(100.0);
        let params = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let nominal_agl = params.nominal_agl();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &params).unwrap();
        let reports = validate_overlap(&plan, &engine).unwrap();

        assert!(
            reports.iter().any(|r| r.violation),
            "expected violations with elevated terrain"
        );
        // Spot-check the first report.
        let r = &reports[0];
        assert_abs_diff_eq!(r.terrain_peak_m, 100.0, epsilon = 0.5);
        assert_abs_diff_eq!(r.actual_agl_m, nominal_agl - 100.0, epsilon = 1.0);
        assert!(r.actual_overlap_pct < params.side_lap_percent);
    }

    #[test]
    fn adjust_for_terrain_no_change_over_flat_terrain() {
        /*
         * Terrain = 0 m. max(nominal_agl, 0 + nominal_agl) = nominal_agl.
         * All waypoint altitudes must be unchanged.
         */
        let (_dir, engine) = flat_terrain_engine(0.0);
        let params = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let nominal_agl = params.nominal_agl();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &params).unwrap();
        let adjusted = adjust_for_terrain(&plan, &engine).unwrap();

        for line in &adjusted.lines {
            for &alt in line.elevations() {
                assert_abs_diff_eq!(alt, nominal_agl, epsilon = 1e-9);
            }
        }
    }

    #[test]
    fn adjust_for_terrain_raises_waypoints_over_ridge() {
        /*
         * Terrain = 200 m. Nominal AGL = 182.4 m.
         *   required_alt = 200 + 182.4 = 382.4 m
         *   planned_alt  = 182.4 m  (flat-terrain generation)
         *   adjusted_alt = max(182.4, 382.4) = 382.4 m
         */
        let (_dir, engine) = flat_terrain_engine(200.0);
        let params = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let nominal_agl = params.nominal_agl();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &params).unwrap();
        let adjusted = adjust_for_terrain(&plan, &engine).unwrap();

        let expected = 200.0_f64 + nominal_agl;
        for line in &adjusted.lines {
            for &alt in line.elevations() {
                assert_abs_diff_eq!(alt, expected, epsilon = 1e-6);
            }
        }
    }

    #[test]
    fn adjust_for_terrain_preserves_line_and_waypoint_count() {
        let (_dir, engine) = flat_terrain_engine(50.0);
        let params = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &params).unwrap();
        let adjusted = adjust_for_terrain(&plan, &engine).unwrap();

        assert_eq!(
            adjusted.lines.len(),
            plan.lines.len(),
            "line count must be unchanged"
        );
        for (orig, adj) in plan.lines.iter().zip(adjusted.lines.iter()) {
            assert_eq!(
                adj.len(),
                orig.len(),
                "waypoint count must be unchanged per line"
            );
        }
    }

    #[test]
    fn adjust_for_terrain_preserves_coordinates() {
        /*
         * Only the elevation component should change — lat/lon must be
         * bit-for-bit identical to the originals.
         */
        let (_dir, engine) = flat_terrain_engine(50.0);
        let params = FlightPlanParams::new(phantom4pro(), 0.05).unwrap();
        let plan = generate_flight_lines(&equator_bbox(), 0.0, &params).unwrap();
        let adjusted = adjust_for_terrain(&plan, &engine).unwrap();

        for (orig, adj) in plan.lines.iter().zip(adjusted.lines.iter()) {
            for (&lat_o, &lat_a) in orig.lats().iter().zip(adj.lats().iter()) {
                assert_abs_diff_eq!(lat_o, lat_a, epsilon = 1e-15);
            }
            for (&lon_o, &lon_a) in orig.lons().iter().zip(adj.lons().iter()) {
                assert_abs_diff_eq!(lon_o, lon_a, epsilon = 1e-15);
            }
        }
    }
}
