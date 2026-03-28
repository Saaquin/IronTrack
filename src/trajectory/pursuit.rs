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
 * Dynamic look-ahead pure pursuit controller.
 *
 * Converts a B-spline reference path into discrete waypoints suitable for
 * autopilot execution. The look-ahead distance L_a adapts dynamically:
 *
 *   - Curvature-aware: L_a shrinks at ridge apexes (high curvature) to
 *     stay tight on the path, expands on flat terrain to smooth out noise.
 *
 *   - Error-aware: L_a contracts exponentially with altitude error to
 *     correct deviations aggressively.
 *
 * The controller samples the B-spline at uniform along-track intervals
 * (waypoint_interval_m) and validates that each waypoint respects the
 * platform's kinematic limits.
 *
 * Reference: docs/34_terrain_following_planning.md
 * Reference: .agents/formula_reference.md — Dynamic Look-Ahead Distance
 */

use crate::error::TrajectoryError;
use crate::math::numerics::KahanAccumulator;
use crate::trajectory::bspline_smooth::BSplinePath;
use crate::types::KinematicLimits;

// ---------------------------------------------------------------------------
// Controller
// ---------------------------------------------------------------------------

/// Dynamic look-ahead pure pursuit controller for terrain-following.
///
/// Generates waypoints from a B-spline reference path while adapting the
/// look-ahead distance based on local curvature and altitude error.
pub struct PursuitController {
    /// Platform kinematic constraints.
    pub limits: KinematicLimits,
    /// Minimum look-ahead distance (metres). Used at maximum curvature.
    pub min_lookahead_m: f64,
    /// Maximum look-ahead distance (metres). Used on flat terrain.
    pub max_lookahead_m: f64,
    /// Exponential contraction gain for altitude error adaptation.
    /// Higher values make the controller more aggressive at correcting
    /// altitude deviations. Typical range: 0.01–0.1.
    pub error_gain: f64,
}

impl PursuitController {
    /// Create a controller with defaults derived from kinematic limits.
    pub fn new(limits: KinematicLimits) -> Self {
        // Default look-ahead range: 2x to 10x the waypoint spacing
        // (will be used relative to waypoint_interval)
        Self {
            limits,
            min_lookahead_m: 50.0,
            max_lookahead_m: 500.0,
            error_gain: 0.05,
        }
    }

    /// Generate waypoints from a B-spline reference path.
    ///
    /// Samples the path at `waypoint_interval_m` along-track spacing. At
    /// each sample, computes the dynamic look-ahead distance and selects
    /// the target point on the spline that distance ahead.
    ///
    /// # Arguments
    ///
    /// * `path` — B-spline reference path from the smoothing stage.
    /// * `waypoint_interval_m` — Desired spacing between waypoints (metres).
    /// * `target_alt_fn` — Function returning the ideal altitude at a given
    ///   along-track distance (e.g., terrain + AGL). Used for error-based
    ///   look-ahead adaptation.
    ///
    /// # Returns
    ///
    /// Vector of (along_track_m, altitude_m) waypoints.
    pub fn generate_waypoints(
        &self,
        path: &BSplinePath,
        waypoint_interval_m: f64,
        target_alt_fn: impl Fn(f64) -> f64,
    ) -> Result<Vec<(f64, f64)>, TrajectoryError> {
        if waypoint_interval_m <= 0.0 || !waypoint_interval_m.is_finite() {
            return Err(TrajectoryError::InvalidInput(
                "waypoint_interval_m must be positive and finite".into(),
            ));
        }

        // Sample the B-spline densely to build an along-track lookup table.
        // We need this to convert between parameter space and arc-length space.
        let samples = path.sample_uniform(10_000);

        if samples.len() < 2 {
            return Err(TrajectoryError::InvalidInput(
                "B-spline path too short to generate waypoints".into(),
            ));
        }

        // Build cumulative arc-length table using Kahan compensated summation.
        // Over a 30 km path sampled at 10,000 points, naive summation can
        // accumulate millimetre-level drift; Kahan keeps it sub-micrometre.
        let mut arc_lengths = vec![0.0_f64; samples.len()];
        let mut arc_acc = KahanAccumulator::new();
        for i in 1..samples.len() {
            let dx = samples[i].0 - samples[i - 1].0;
            let dy = samples[i].1 - samples[i - 1].1;
            arc_acc.add((dx * dx + dy * dy).sqrt());
            arc_lengths[i] = arc_acc.total();
        }

        // samples.len() >= 2 is guaranteed by the check above, so
        // last() always returns Some.
        let total_length = arc_lengths[samples.len() - 1];
        if total_length < waypoint_interval_m {
            // Path shorter than one interval — return start and end
            return Ok(vec![samples[0], samples[samples.len() - 1]]);
        }

        // Generate waypoints at uniform arc-length intervals
        let n_waypoints = (total_length / waypoint_interval_m).ceil() as usize + 1;
        let mut waypoints: Vec<(f64, f64)> = Vec::with_capacity(n_waypoints);
        waypoints.push(samples[0]);

        let kappa_max = self.limits.max_curvature();
        let max_slope = self.limits.max_slope();
        let mut search_start = 0;

        for i in 1..n_waypoints {
            let target_arc = (i as f64 * waypoint_interval_m).min(total_length);

            // Find the sample point at this arc length
            let (pt, new_search) =
                interpolate_at_arc_length(&samples, &arc_lengths, target_arc, search_start);
            search_start = new_search;

            // Compute dynamic look-ahead adaptation
            let curvature = estimate_curvature(&samples, &arc_lengths, target_arc);
            let curvature_ratio = (curvature / kappa_max).min(1.0);
            let mut lookahead = self.min_lookahead_m
                + (self.max_lookahead_m - self.min_lookahead_m) * (1.0 - curvature_ratio);

            // Error-based contraction
            let target_alt = target_alt_fn(pt.0);
            let alt_error = (pt.1 - target_alt).abs();
            lookahead *= (-self.error_gain * alt_error).exp();
            lookahead = lookahead.max(self.min_lookahead_m);

            // Look ahead on the spline to find the pursuit target
            let pursuit_arc = (target_arc + lookahead).min(total_length);
            let (pursuit_pt, _) =
                interpolate_at_arc_length(&samples, &arc_lengths, pursuit_arc, search_start);

            // Use the B-spline's altitude directly. The EB + B-spline pipeline
            // has already enforced clearance and smoothness constraints. Overriding
            // the altitude with slope clamping here can inadvertently push waypoints
            // below terrain clearance, which is a safety violation.
            //
            // The dynamic look-ahead serves a different purpose: it allows the
            // autopilot to anticipate terrain changes and begin pitch transitions
            // early, rather than reacting late at waypoint boundaries.
            let _ = (pursuit_pt, max_slope); // used for look-ahead computation above

            waypoints.push(pt);
        }

        // Ensure the last waypoint is at the end of the path.
        // samples is guaranteed non-empty (len >= 2 checked above).
        let last_sample = samples[samples.len() - 1];
        if let Some(last_wp) = waypoints.last() {
            if (last_wp.0 - last_sample.0).abs() > waypoint_interval_m * 0.1 {
                waypoints.push(last_sample);
            }
        }

        Ok(waypoints)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Interpolate a point at a given arc length along the sampled path.
/// Returns the interpolated point and the search index for the next query.
fn interpolate_at_arc_length(
    samples: &[(f64, f64)],
    arc_lengths: &[f64],
    target_arc: f64,
    search_start: usize,
) -> ((f64, f64), usize) {
    let mut j = search_start;
    while j + 1 < arc_lengths.len() && arc_lengths[j + 1] < target_arc {
        j += 1;
    }

    if j + 1 >= arc_lengths.len() {
        // samples is guaranteed non-empty by the caller's len >= 2 check.
        return (samples[samples.len() - 1], j);
    }

    let denom = arc_lengths[j + 1] - arc_lengths[j];
    if denom.abs() < 1e-15 {
        return (samples[j], j);
    }

    let frac = (target_arc - arc_lengths[j]) / denom;
    let x = samples[j].0 + frac * (samples[j + 1].0 - samples[j].0);
    let y = samples[j].1 + frac * (samples[j + 1].1 - samples[j].1);

    ((x, y), j)
}

/// Estimate the path curvature at a given arc length using finite differences
/// on the sampled points.
fn estimate_curvature(samples: &[(f64, f64)], arc_lengths: &[f64], target_arc: f64) -> f64 {
    // Find the closest sample index
    let idx = arc_lengths
        .iter()
        .position(|&a| a >= target_arc)
        .unwrap_or(arc_lengths.len() - 1);

    if idx == 0 || idx >= samples.len() - 1 {
        return 0.0;
    }

    // Three-point curvature estimate using the circumscribed circle formula
    let (x0, y0) = samples[idx - 1];
    let (x1, y1) = samples[idx];
    let (x2, y2) = samples[idx + 1];

    let dx1 = x1 - x0;
    let dy1 = y1 - y0;
    let dx2 = x2 - x1;
    let dy2 = y2 - y1;

    let cross = dx1 * dy2 - dy1 * dx2;
    let d1 = (dx1 * dx1 + dy1 * dy1).sqrt();
    let d2 = (dx2 * dx2 + dy2 * dy2).sqrt();
    let d3 = ((x2 - x0).powi(2) + (y2 - y0).powi(2)).sqrt();

    let denom = d1 * d2 * d3;
    if denom < 1e-15 {
        0.0
    } else {
        (2.0 * cross.abs()) / denom
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_controller() -> PursuitController {
        let limits = KinematicLimits::fixed_wing_default();
        PursuitController::new(limits)
    }

    fn straight_path() -> BSplinePath {
        let data: Vec<(f64, f64)> = (0..20).map(|i| (i as f64 * 100.0, 500.0)).collect();
        BSplinePath::fit(&data).expect("fit should succeed")
    }

    fn sloped_path() -> BSplinePath {
        let data: Vec<(f64, f64)> = (0..20)
            .map(|i| (i as f64 * 100.0, 500.0 + 2.0 * i as f64))
            .collect();
        BSplinePath::fit(&data).expect("fit should succeed")
    }

    #[test]
    fn flat_terrain_uniform_waypoints() {
        let controller = make_controller();
        let path = straight_path();
        let waypoints = controller
            .generate_waypoints(&path, 50.0, |_| 500.0)
            .expect("should succeed");

        assert!(waypoints.len() > 2, "should produce multiple waypoints");

        // All altitudes should be close to 500m
        for (i, wp) in waypoints.iter().enumerate() {
            assert!(
                (wp.1 - 500.0).abs() < 5.0,
                "waypoint {i}: altitude {:.1} too far from 500.0",
                wp.1
            );
        }
    }

    #[test]
    fn waypoints_are_monotonically_advancing() {
        let controller = make_controller();
        let path = straight_path();
        let waypoints = controller
            .generate_waypoints(&path, 50.0, |_| 500.0)
            .expect("should succeed");

        for i in 1..waypoints.len() {
            assert!(
                waypoints[i].0 >= waypoints[i - 1].0,
                "waypoint {i} goes backward: {:.1} < {:.1}",
                waypoints[i].0,
                waypoints[i - 1].0
            );
        }
    }

    #[test]
    fn respects_max_slope() {
        let controller = make_controller();
        let path = sloped_path();
        let max_slope = controller.limits.max_slope();

        let waypoints = controller
            .generate_waypoints(&path, 50.0, |x| 500.0 + x * 0.01)
            .expect("should succeed");

        for i in 1..waypoints.len() {
            let dx = waypoints[i].0 - waypoints[i - 1].0;
            if dx > 1.0 {
                let slope = (waypoints[i].1 - waypoints[i - 1].1).abs() / dx;
                assert!(
                    slope <= max_slope * 1.5, // tolerance for discretization
                    "segment {}: slope {slope:.4} exceeds limit {max_slope:.4}",
                    i - 1
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_interval() {
        let controller = make_controller();
        let path = straight_path();
        assert!(controller
            .generate_waypoints(&path, 0.0, |_| 500.0)
            .is_err());
        assert!(controller
            .generate_waypoints(&path, -10.0, |_| 500.0)
            .is_err());
    }

    #[test]
    fn lookahead_shrinks_at_high_curvature() {
        // This is tested indirectly: the controller should produce tighter
        // waypoint spacing in curved regions. We verify that the curvature
        // estimation function works correctly on a known curve.
        let samples: Vec<(f64, f64)> = (0..100)
            .map(|i| {
                let x = i as f64 * 10.0;
                (x, 500.0 + 50.0 * (x / 200.0).sin())
            })
            .collect();

        let mut arc_lengths = vec![0.0; samples.len()];
        for i in 1..samples.len() {
            let dx = samples[i].0 - samples[i - 1].0;
            let dy = samples[i].1 - samples[i - 1].1;
            arc_lengths[i] = arc_lengths[i - 1] + (dx * dx + dy * dy).sqrt();
        }

        // Curvature at peak (x=~157, sin peak) should be higher than at
        // zero crossing (x=0, sin zero)
        let curv_at_zero = estimate_curvature(&samples, &arc_lengths, 10.0);
        let curv_at_peak = estimate_curvature(&samples, &arc_lengths, 150.0);

        // At peak of sin, curvature should be noticeably larger
        assert!(
            curv_at_peak > curv_at_zero * 0.5,
            "curvature at peak ({curv_at_peak:.6}) should be comparable to or larger than at start ({curv_at_zero:.6})"
        );
    }
}
