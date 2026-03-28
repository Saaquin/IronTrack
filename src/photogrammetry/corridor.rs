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

// Corridor flight line generation for linear infrastructure following.
//
// Unlike area surveys (grid of parallel lines over a polygon), corridor
// mapping generates flight lines that follow a user-supplied centerline
// polyline with parallel offsets at a specified buffer width.
//
// Algorithm:
//   1. Project centerline vertices from WGS84 to UTM (single zone).
//   2. Generate offset polylines at evenly spaced distances across the
//      corridor width, using miter joins at vertices with a bevel
//      fallback when the miter spike exceeds 2x buffer_width.
//   3. Remove self-intersections caused by tight curves in the offsets.
//   4. Resample each offset polyline at the requested waypoint interval.
//   5. Convert waypoints back to WGS84 and pack into FlightLine structs.
//
// All geometry is synchronous CPU work via rayon. No async.

use rayon::prelude::*;

use crate::error::PhotogrammetryError;
use crate::geodesy::utm::{utm_to_wgs84, utm_zone, wgs84_to_utm_in_zone};
use crate::types::{AltitudeDatum, FlightLine, GeoCoord, Hemisphere, UtmCoord};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Parameters for a corridor survey flight plan.
pub struct CorridorParams {
    /// Centerline vertices in (latitude, longitude) decimal degrees.
    pub centerline: Vec<(f64, f64)>,
    /// Total corridor width in metres (offsets extend to +/-width/2).
    pub buffer_width: f64,
    /// Number of parallel flight lines (1, 3, or 5).
    pub num_passes: u8,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate corridor flight lines from a centerline polyline.
///
/// Each flight line follows the centerline (or a parallel offset) resampled
/// at `waypoint_interval` metre spacing. All waypoints are assigned
/// `flight_alt_m` as their elevation (flat-terrain assumption; terrain-
/// following adjustment is applied separately by the caller).
///
/// `datum` tags every output `FlightLine` with the correct vertical datum.
/// Use `AltitudeDatum::Egm2008` when `flight_alt_m` is an explicit MSL
/// altitude, or `AltitudeDatum::Agl` when it is a computed AGL from sensor
/// geometry (no explicit `--altitude-msl`).
///
/// # Errors
/// Returns `PhotogrammetryError::InvalidInput` if the centerline has fewer
/// than 2 vertices, buffer_width is non-positive, num_passes is not 1/3/5,
/// or waypoint_interval is non-positive.
pub fn generate_corridor_lines(
    params: &CorridorParams,
    waypoint_interval: f64,
    flight_alt_m: f64,
    datum: AltitudeDatum,
) -> Result<Vec<FlightLine>, PhotogrammetryError> {
    validate_params(params, waypoint_interval, flight_alt_m)?;

    /*
     * Determine a single UTM zone from the centerline centroid so that all
     * vertices share one coordinate system regardless of zone boundaries.
     */
    let centroid_lon =
        params.centerline.iter().map(|&(_, lon)| lon).sum::<f64>() / params.centerline.len() as f64;
    let centroid_lat =
        params.centerline.iter().map(|&(lat, _)| lat).sum::<f64>() / params.centerline.len() as f64;
    let zone = utm_zone(centroid_lon);
    let hemisphere = if centroid_lat >= 0.0 {
        Hemisphere::North
    } else {
        Hemisphere::South
    };

    // Phase 1: project centerline to UTM (sequential I/O-free step).
    let utm_centerline = {
        let raw = project_to_utm(&params.centerline, zone, hemisphere)?;
        // Collapse consecutive duplicate vertices to prevent zero-length
        // segments that produce degenerate normals in the offset algorithm.
        let mut deduped = Vec::with_capacity(raw.len());
        for pt in &raw {
            if let Some(prev) = deduped.last() {
                let (px, py): &(f64, f64) = prev;
                if (pt.0 - px).abs() < 1e-6 && (pt.1 - py).abs() < 1e-6 {
                    continue;
                }
            }
            deduped.push(*pt);
        }
        if deduped.len() < 2 {
            return Err(PhotogrammetryError::InvalidInput(
                "corridor centerline collapses to a single point in UTM".into(),
            ));
        }
        deduped
    };

    // Phase 2: compute offset distances for each pass.
    let offsets = compute_offsets(params.num_passes, params.buffer_width);

    /*
     * Miter limit: the spec mandates bevel joins when the miter spike
     * exceeds 2x buffer_width from the vertex.
     */
    let miter_limit = 2.0 * params.buffer_width;

    // Phase 3: generate offset polylines (parallel via rayon).
    let offset_polylines: Vec<Vec<(f64, f64)>> = offsets
        .par_iter()
        .map(|&d| {
            let mut poly = offset_polyline(&utm_centerline, d, miter_limit);
            remove_self_intersections(&mut poly);
            poly
        })
        .collect();

    // Phase 4: resample and convert to FlightLines (parallel via rayon).
    let lines: Result<Vec<FlightLine>, PhotogrammetryError> = offset_polylines
        .par_iter()
        .map(|poly| {
            let resampled = resample_polyline(poly, waypoint_interval);
            utm_to_flight_line(&resampled, zone, hemisphere, flight_alt_m, datum)
        })
        .collect();

    lines
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_params(
    params: &CorridorParams,
    waypoint_interval: f64,
    flight_alt_m: f64,
) -> Result<(), PhotogrammetryError> {
    if params.centerline.len() < 2 {
        return Err(PhotogrammetryError::InvalidInput(
            "corridor centerline must have at least 2 vertices".into(),
        ));
    }
    if !params.buffer_width.is_finite() || params.buffer_width <= 0.0 {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "corridor buffer_width must be positive, got {}",
            params.buffer_width
        )));
    }
    if !matches!(params.num_passes, 1 | 3 | 5) {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "corridor num_passes must be 1, 3, or 5, got {}",
            params.num_passes
        )));
    }
    if !waypoint_interval.is_finite() || waypoint_interval <= 0.0 {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "waypoint_interval must be positive, got {waypoint_interval}"
        )));
    }
    if !flight_alt_m.is_finite() {
        return Err(PhotogrammetryError::InvalidInput(format!(
            "flight_alt_m must be finite, got {flight_alt_m}"
        )));
    }

    // Reject equator-crossing centerlines (mixed hemispheres break UTM).
    let has_north = params.centerline.iter().any(|&(lat, _)| lat >= 0.0);
    let has_south = params.centerline.iter().any(|&(lat, _)| lat < 0.0);
    if has_north && has_south {
        return Err(PhotogrammetryError::InvalidInput(
            "corridor centerline crosses the equator; split into separate \
             northern and southern segments for correct UTM projection"
                .into(),
        ));
    }

    // Reject antimeridian-spanning centerlines (naive lon mean picks wrong zone).
    let lons: Vec<f64> = params.centerline.iter().map(|&(_, lon)| lon).collect();
    let lon_range = lons.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - lons.iter().cloned().fold(f64::INFINITY, f64::min);
    if lon_range > 180.0 {
        return Err(PhotogrammetryError::InvalidInput(
            "corridor centerline appears to span the antimeridian; \
             split into separate segments"
                .into(),
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// UTM projection helpers
// ---------------------------------------------------------------------------

fn project_to_utm(
    vertices: &[(f64, f64)],
    zone: u8,
    hemisphere: Hemisphere,
) -> Result<Vec<(f64, f64)>, PhotogrammetryError> {
    vertices
        .iter()
        .map(|&(lat, lon)| {
            let coord = GeoCoord::from_degrees(lat, lon, 0.0).map_err(|e| {
                PhotogrammetryError::InvalidInput(format!("invalid coordinate: {e}"))
            })?;
            let utm = wgs84_to_utm_in_zone(coord, zone, hemisphere).map_err(|e| {
                PhotogrammetryError::InvalidInput(format!("UTM projection failed: {e}"))
            })?;
            Ok((utm.easting, utm.northing))
        })
        .collect()
}

fn utm_to_flight_line(
    utm_points: &[(f64, f64)],
    zone: u8,
    hemisphere: Hemisphere,
    flight_alt_m: f64,
    datum: AltitudeDatum,
) -> Result<FlightLine, PhotogrammetryError> {
    let mut line = FlightLine::new().with_datum(datum);
    for &(easting, northing) in utm_points {
        let utm = UtmCoord {
            easting,
            northing,
            zone,
            hemisphere,
        };
        let (lat_deg, lon_deg) = utm_to_wgs84(&utm)
            .map_err(|e| PhotogrammetryError::InvalidInput(format!("inverse UTM failed: {e}")))?;
        let coord = GeoCoord::from_degrees(lat_deg, lon_deg, 0.0)
            .map_err(|e| PhotogrammetryError::InvalidInput(format!("invalid coordinate: {e}")))?;
        line.push(coord.lat_rad(), coord.lon_rad(), flight_alt_m);
    }
    Ok(line)
}

// ---------------------------------------------------------------------------
// Offset computation
// ---------------------------------------------------------------------------

/// Compute the lateral offset distances for each pass.
///
/// For num_passes=1: [0.0]  (centerline only)
/// For num_passes=3: [-w/2, 0.0, +w/2]
/// For num_passes=5: [-w/2, -w/4, 0.0, +w/4, +w/2]
fn compute_offsets(num_passes: u8, buffer_width: f64) -> Vec<f64> {
    if num_passes == 1 {
        return vec![0.0];
    }
    let half = buffer_width / 2.0;
    let step = buffer_width / (num_passes as f64 - 1.0);
    (0..num_passes as usize)
        .map(|i| -half + i as f64 * step)
        .collect()
}

// ---------------------------------------------------------------------------
// Offset polyline generation with miter/bevel joins
// ---------------------------------------------------------------------------

/// Generate a polyline offset by `d` metres from the input polyline.
///
/// Positive d = left offset (looking along the direction of travel).
/// Negative d = right offset.
///
/// At each interior vertex, the offset uses a miter join (extend offset
/// edges to their intersection). When the miter spike exceeds `miter_limit`
/// metres from the vertex, a bevel join is used instead (two points where
/// the offset edges meet the vertex normals).
fn offset_polyline(points: &[(f64, f64)], d: f64, miter_limit: f64) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    if d.abs() < 1e-10 {
        return points.to_vec();
    }

    let seg_count = n - 1;

    /*
     * Precompute unit normals for each segment.
     * Normal = left-perpendicular of the segment direction.
     */
    let normals: Vec<(f64, f64)> = (0..seg_count)
        .map(|i| {
            let dx = points[i + 1].0 - points[i].0;
            let dy = points[i + 1].1 - points[i].1;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-12 {
                (0.0, 0.0)
            } else {
                (-dy / len, dx / len)
            }
        })
        .collect();

    let mut result = Vec::with_capacity(n + seg_count);

    // First endpoint: offset along first segment normal.
    result.push((
        points[0].0 + d * normals[0].0,
        points[0].1 + d * normals[0].1,
    ));

    // Interior vertices: miter or bevel join.
    for i in 1..n - 1 {
        let (nx1, ny1) = normals[i - 1];
        let (nx2, ny2) = normals[i];

        // Bisector of the two normals.
        let bx = nx1 + nx2;
        let by = ny1 + ny2;
        let bl = (bx * bx + by * by).sqrt();

        if bl < 1e-10 {
            /*
             * Normals point in opposite directions (180-degree reversal).
             * Use the first segment's normal for both bevel points.
             */
            result.push((points[i].0 + d * nx1, points[i].1 + d * ny1));
            result.push((points[i].0 + d * nx2, points[i].1 + d * ny2));
            continue;
        }

        let bx_n = bx / bl;
        let by_n = by / bl;

        /*
         * Miter length = d / dot(bisector_unit, normal_unit).
         * This is the distance along the bisector needed to achieve
         * offset distance d from both adjacent segment lines.
         */
        let dot = bx_n * nx1 + by_n * ny1;
        if dot.abs() < 1e-10 {
            result.push((points[i].0 + d * nx1, points[i].1 + d * ny1));
            continue;
        }

        let miter_length = d / dot;

        if miter_length.abs() > miter_limit {
            // Bevel join: two offset points, one per adjacent segment normal.
            result.push((points[i].0 + d * nx1, points[i].1 + d * ny1));
            result.push((points[i].0 + d * nx2, points[i].1 + d * ny2));
        } else {
            // Miter join: single point along the bisector.
            result.push((
                points[i].0 + bx_n * miter_length,
                points[i].1 + by_n * miter_length,
            ));
        }
    }

    // Last endpoint: offset along last segment normal.
    let (nx_last, ny_last) = normals[seg_count - 1];
    result.push((points[n - 1].0 + d * nx_last, points[n - 1].1 + d * ny_last));

    result
}

// ---------------------------------------------------------------------------
// Self-intersection removal
// ---------------------------------------------------------------------------

/// Remove self-intersections from an offset polyline.
///
/// When an offset polyline crosses itself (common at tight curves), the
/// enclosed loop is removed by replacing the crossing segments with the
/// intersection point. This is a simple iterative scan — sufficient for
/// corridor polylines which are typically short (< 1000 vertices).
fn remove_self_intersections(points: &mut Vec<(f64, f64)>) {
    let mut changed = true;
    while changed {
        changed = false;
        'outer: for i in 0..points.len().saturating_sub(3) {
            for j in (i + 2)..points.len().saturating_sub(1) {
                if let Some(pt) =
                    segment_intersection(points[i], points[i + 1], points[j], points[j + 1])
                {
                    // Remove the loop between i+1 and j inclusive,
                    // replacing with the intersection point.
                    points.splice((i + 1)..=j, std::iter::once(pt));
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
}

/// Compute the intersection point of two line segments, if any.
///
/// Returns `Some((x, y))` if the segments cross at interior points
/// (exclusive of endpoints to avoid false positives at shared vertices).
fn segment_intersection(
    a1: (f64, f64),
    a2: (f64, f64),
    b1: (f64, f64),
    b2: (f64, f64),
) -> Option<(f64, f64)> {
    let d1x = a2.0 - a1.0;
    let d1y = a2.1 - a1.1;
    let d2x = b2.0 - b1.0;
    let d2y = b2.1 - b1.1;

    let denom = d1x * d2y - d1y * d2x;
    if denom.abs() < 1e-10 {
        return None; // parallel or collinear
    }

    let t = ((b1.0 - a1.0) * d2y - (b1.1 - a1.1) * d2x) / denom;
    let u = ((b1.0 - a1.0) * d1y - (b1.1 - a1.1) * d1x) / denom;

    // Strict interior intersection (exclude endpoints).
    let eps = 1e-9;
    if t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps {
        Some((a1.0 + t * d1x, a1.1 + t * d1y))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Polyline resampling
// ---------------------------------------------------------------------------

/// Resample a polyline at regular intervals.
///
/// The first and last vertices are always included. Intermediate vertices
/// are placed at exactly `interval` metres along the polyline's arc length.
fn resample_polyline(points: &[(f64, f64)], interval: f64) -> Vec<(f64, f64)> {
    if points.len() < 2 || interval <= 0.0 {
        return points.to_vec();
    }

    let mut result = vec![points[0]];
    let mut accumulated = 0.0;
    let mut seg_idx = 0;
    let mut t_in_seg = 0.0;

    while seg_idx < points.len() - 1 {
        let p1 = points[seg_idx];
        let p2 = points[seg_idx + 1];
        let seg_len = ((p2.0 - p1.0).powi(2) + (p2.1 - p1.1).powi(2)).sqrt();

        if seg_len < 1e-12 {
            seg_idx += 1;
            t_in_seg = 0.0;
            continue;
        }

        let remaining_in_seg = seg_len * (1.0 - t_in_seg);
        let remaining_to_next = interval - accumulated;

        if remaining_in_seg >= remaining_to_next {
            // Next waypoint falls within this segment.
            let t_advance = remaining_to_next / seg_len;
            t_in_seg += t_advance;
            let x = p1.0 + t_in_seg * (p2.0 - p1.0);
            let y = p1.1 + t_in_seg * (p2.1 - p1.1);
            result.push((x, y));
            accumulated = 0.0;
        } else {
            // Advance to next segment, accumulating partial distance.
            accumulated += remaining_in_seg;
            seg_idx += 1;
            t_in_seg = 0.0;
        }
    }

    // Always include the last vertex to guarantee full corridor coverage.
    let last = *points.last().unwrap();
    result.push(last);

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    /// Straight centerline: offsets must be exactly +/-buffer_width/2 apart
    /// measured perpendicular to the line direction.
    #[test]
    fn straight_line_offsets_are_exact() {
        // East-west line at northing=5_000_000, 1 km long.
        let center: Vec<(f64, f64)> = vec![(500_000.0, 5_000_000.0), (501_000.0, 5_000_000.0)];
        let buffer_width = 100.0;
        let miter_limit = 200.0;
        let half = buffer_width / 2.0;

        let left = offset_polyline(&center, half, miter_limit);
        let right = offset_polyline(&center, -half, miter_limit);

        // For an east-west line, left normal points north (+y).
        for pt in &left {
            assert_abs_diff_eq!(pt.1, 5_000_050.0, epsilon = 1e-6);
        }
        for pt in &right {
            assert_abs_diff_eq!(pt.1, 4_999_950.0, epsilon = 1e-6);
        }

        // Separation between left and right = buffer_width.
        assert_abs_diff_eq!(left[0].1 - right[0].1, buffer_width, epsilon = 1e-6);
    }

    /// 90-degree bend: miter join must not spike beyond 2x buffer_width.
    #[test]
    fn ninety_degree_bend_miter_within_limit() {
        let miter_limit = 200.0;

        // L-shaped path: east 500m then north 500m.
        let center = vec![
            (500_000.0, 5_000_000.0),
            (500_500.0, 5_000_000.0),
            (500_500.0, 5_000_500.0),
        ];

        let left = offset_polyline(&center, 50.0, miter_limit);

        /*
         * At a 90-degree bend, the miter length = d / sin(45deg) = d * sqrt(2).
         * For d=50, miter_length ~= 70.7 m, well under 200 m limit.
         * So this should produce a single miter point at the bend vertex.
         */
        // The offset polyline should have exactly 3 points (no bevel split).
        assert_eq!(left.len(), 3, "90-degree bend should use miter join");

        // The miter point should be within 2*buffer_width of the original vertex.
        let (vx, vy) = center[1];
        let (ox, oy) = left[1];
        let dist = ((ox - vx).powi(2) + (oy - vy).powi(2)).sqrt();
        assert!(
            dist <= miter_limit,
            "miter point {dist:.1}m from vertex exceeds limit {miter_limit}m"
        );
    }

    /// U-turn (180-degree bend): self-intersection must be clipped.
    #[test]
    fn u_turn_self_intersection_removed() {
        // Hairpin: east, then reverse back west with a tiny vertical offset.
        // At large offsets this creates a self-intersecting offset polyline.
        let center = vec![
            (500_000.0, 5_000_000.0),
            (500_200.0, 5_000_000.0),
            (500_200.0, 5_000_010.0), // tiny step north
            (500_000.0, 5_000_010.0),
        ];
        let d = 50.0;
        let miter_limit = 200.0;

        let mut poly = offset_polyline(&center, d, miter_limit);
        let len_before = poly.len();
        remove_self_intersections(&mut poly);

        // After removal, the polyline should have fewer or equal vertices
        // and no self-intersections.
        assert!(
            poly.len() <= len_before,
            "self-intersection removal should not add vertices"
        );

        // Verify no remaining self-intersections.
        for i in 0..poly.len().saturating_sub(3) {
            for j in (i + 2)..poly.len().saturating_sub(1) {
                assert!(
                    segment_intersection(poly[i], poly[i + 1], poly[j], poly[j + 1]).is_none(),
                    "residual self-intersection between segments {i} and {j}"
                );
            }
        }
    }

    /// Resample produces evenly spaced waypoints.
    #[test]
    fn resample_straight_line() {
        let points = vec![(0.0, 0.0), (1000.0, 0.0)];
        let resampled = resample_polyline(&points, 100.0);

        // 1000m / 100m = 10 intervals + start = 11, + forced last = 12.
        // The forced last vertex duplicates the 11th point (both at 1000m).
        assert_eq!(resampled.len(), 12);

        // First 11 points are evenly spaced.
        for i in 0..11 {
            assert_abs_diff_eq!(resampled[i].0, i as f64 * 100.0, epsilon = 1e-6);
            assert_abs_diff_eq!(resampled[i].1, 0.0, epsilon = 1e-6);
        }
        // Last point is the forced endpoint (same as point 10).
        assert_abs_diff_eq!(resampled[11].0, 1000.0, epsilon = 1e-6);
    }

    /// Offset distances are correctly computed for 1, 3, and 5 passes.
    #[test]
    fn offset_distances() {
        let offsets_1 = compute_offsets(1, 100.0);
        assert_eq!(offsets_1, vec![0.0]);

        let offsets_3 = compute_offsets(3, 100.0);
        assert_eq!(offsets_3.len(), 3);
        assert_abs_diff_eq!(offsets_3[0], -50.0, epsilon = 1e-9);
        assert_abs_diff_eq!(offsets_3[1], 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(offsets_3[2], 50.0, epsilon = 1e-9);

        let offsets_5 = compute_offsets(5, 100.0);
        assert_eq!(offsets_5.len(), 5);
        assert_abs_diff_eq!(offsets_5[0], -50.0, epsilon = 1e-9);
        assert_abs_diff_eq!(offsets_5[1], -25.0, epsilon = 1e-9);
        assert_abs_diff_eq!(offsets_5[2], 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(offsets_5[3], 25.0, epsilon = 1e-9);
        assert_abs_diff_eq!(offsets_5[4], 50.0, epsilon = 1e-9);
    }

    /// Validation rejects invalid parameters.
    #[test]
    fn validation_rejects_bad_params() {
        let good = CorridorParams {
            centerline: vec![(51.0, -0.1), (51.1, -0.1)],
            buffer_width: 100.0,
            num_passes: 3,
        };
        assert!(validate_params(&good, 50.0, 120.0).is_ok());

        // Too few vertices.
        let bad_cl = CorridorParams {
            centerline: vec![(51.0, -0.1)],
            buffer_width: 100.0,
            num_passes: 3,
        };
        assert!(validate_params(&bad_cl, 50.0, 120.0).is_err());

        // Invalid num_passes.
        let bad_np = CorridorParams {
            centerline: vec![(51.0, -0.1), (51.1, -0.1)],
            buffer_width: 100.0,
            num_passes: 2,
        };
        assert!(validate_params(&bad_np, 50.0, 120.0).is_err());

        // Non-positive buffer.
        let bad_bw = CorridorParams {
            centerline: vec![(51.0, -0.1), (51.1, -0.1)],
            buffer_width: -10.0,
            num_passes: 3,
        };
        assert!(validate_params(&bad_bw, 50.0, 120.0).is_err());
    }

    /// Segment intersection detects a known crossing.
    #[test]
    fn segment_intersection_basic() {
        // X-shaped crossing at (5, 5).
        let pt = segment_intersection((0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0));
        assert!(pt.is_some());
        let (x, y) = pt.unwrap();
        assert_abs_diff_eq!(x, 5.0, epsilon = 1e-6);
        assert_abs_diff_eq!(y, 5.0, epsilon = 1e-6);
    }

    /// Parallel segments have no intersection.
    #[test]
    fn segment_intersection_parallel() {
        let pt = segment_intersection((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), (10.0, 5.0));
        assert!(pt.is_none());
    }

    /// End-to-end: generate corridor lines from WGS84 centerline.
    #[test]
    fn generate_corridor_lines_end_to_end() {
        // Straight north-south centerline near London (~0.5 deg latitude).
        let params = CorridorParams {
            centerline: vec![(51.0, -0.1), (51.5, -0.1)],
            buffer_width: 200.0,
            num_passes: 3,
        };
        let lines = generate_corridor_lines(&params, 100.0, 120.0, AltitudeDatum::Egm2008).unwrap();
        assert_eq!(lines.len(), 3, "3-pass corridor should produce 3 lines");

        for line in &lines {
            assert!(
                line.len() >= 2,
                "each corridor line should have at least 2 waypoints"
            );
        }
    }
}
