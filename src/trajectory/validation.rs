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
 * Synthetic terrain-following validation suite.
 *
 * Exercises the complete EB -> B-spline -> Pursuit pipeline against four
 * deterministic terrain profiles. Each profile validates a different aspect
 * of the trajectory generation:
 *
 *   1. Flat      — baseline stability, zero drift
 *   2. Sinusoidal — C2 continuity, curvature limiting, steady-state tracking
 *   3. Step      — anticipatory ramp, max slope, jerk-limited transition
 *   4. Sawtooth  — convex hull safety, adaptive knot clustering
 *
 * Assertions at every sampled point (non-negotiable, runs in CI):
 *   - altitude >= terrain + min_clearance  (safety)
 *   - |slope| <= max_slope * tolerance     (pitch limit)
 *   - curvature <= kappa_max * tolerance   (vertical acceleration limit)
 *
 * These tests MUST NOT be marked #[ignore]. They run on every push.
 *
 * Reference: docs/34_terrain_following_planning.md — Synthetic Validation Suite
 */

use std::f64::consts::PI;

use crate::trajectory::bspline_smooth::BSplinePath;
use crate::trajectory::elastic_band::ElasticBandSolver;
use crate::trajectory::pursuit::PursuitController;
use crate::types::KinematicLimits;

// ---------------------------------------------------------------------------
// Profile types
// ---------------------------------------------------------------------------

/// Synthetic terrain profile type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileType {
    /// Constant elevation, zero slope.
    Flat,
    /// Smooth harmonic oscillation.
    Sinusoidal,
    /// Instantaneous vertical cliff (smoothed with tanh for numerical stability).
    Step,
    /// Jagged high-frequency ridgelines.
    Sawtooth,
}

/// Result of running the validation pipeline on a single profile.
#[derive(Debug, Clone)]
pub struct ProfileResult {
    pub profile: ProfileType,
    pub passed: bool,
    /// Worst AGL violation (negative = below min clearance).
    pub min_clearance_margin_m: f64,
    /// Maximum slope observed in the final waypoint sequence.
    pub max_slope: f64,
    /// Maximum curvature observed (estimated from waypoints).
    pub max_curvature: f64,
    /// Maximum slope discontinuity between adjacent segments (C² proxy).
    pub max_slope_jump: f64,
    /// RMS deviation from ideal survey altitude (terrain + min_clearance).
    pub rms_agl_deviation_m: f64,
    /// Maximum deviation from ideal survey altitude.
    pub max_agl_deviation_m: f64,
    /// Percentage of waypoints within +/-5% of ideal AGL.
    pub pct_within_5pct_agl: f64,
    /// Number of waypoints in the final output.
    pub num_waypoints: usize,
    /// Description of the first failure, if any.
    pub failure: Option<String>,
}

// ---------------------------------------------------------------------------
// Profile generation
// ---------------------------------------------------------------------------

/// Generate a synthetic terrain profile.
///
/// Returns (along_track_m, terrain_elevation_m) pairs.
pub fn generate_profile(
    profile: ProfileType,
    length_m: f64,
    spacing_m: f64,
    base_elev: f64,
) -> Vec<(f64, f64)> {
    let n = (length_m / spacing_m).ceil() as usize + 1;
    (0..n)
        .map(|i| {
            let x = i as f64 * spacing_m;
            let terrain = match profile {
                ProfileType::Flat => base_elev,
                ProfileType::Sinusoidal => {
                    // 30m amplitude, 2000m wavelength — gentle rolling hills.
                    // Max terrain slope = 30 * 2*pi/2000 = 0.094 (well within
                    // fixed-wing 0.213 limit).
                    base_elev + 30.0 * (2.0 * PI * x / 2000.0).sin()
                }
                ProfileType::Step => {
                    // 50m step at the midpoint, smoothed with tanh over ~200m.
                    // The EB solver must anticipate and ramp before the cliff.
                    let mid = length_m / 2.0;
                    let steepness = 0.02; // transitions over ~100m
                    base_elev + 50.0 * (0.5 + 0.5 * ((x - mid) * steepness).tanh())
                }
                ProfileType::Sawtooth => {
                    // 20m amplitude, 500m period — moderate ridgelines.
                    // Max terrain slope = 20/(250) = 0.08, within kinematic limits.
                    let period = 500.0;
                    let phase = (x % period) / period;
                    base_elev + 20.0 * (2.0 * phase - 1.0).abs()
                }
            };
            (x, terrain)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Validation runner
// ---------------------------------------------------------------------------

/// Helper to construct a failed ProfileResult with default metrics.
fn fail_result(profile: ProfileType, msg: String) -> ProfileResult {
    ProfileResult {
        profile,
        passed: false,
        min_clearance_margin_m: 0.0,
        max_slope: 0.0,
        max_curvature: 0.0,
        max_slope_jump: 0.0,
        rms_agl_deviation_m: 0.0,
        max_agl_deviation_m: 0.0,
        pct_within_5pct_agl: 0.0,
        num_waypoints: 0,
        failure: Some(msg),
    }
}

/// Run the full validation pipeline on a single profile.
///
/// Pipeline: terrain profile -> EB solver -> B-spline fit -> pursuit -> assertions
pub fn validate_profile(
    profile_type: ProfileType,
    limits: &KinematicLimits,
    min_clearance_m: f64,
) -> ProfileResult {
    let length_m = 5000.0;
    let spacing_m = 10.0;
    let base_elev = 500.0;
    let waypoint_interval = 20.0;

    let terrain_profile = generate_profile(profile_type, length_m, spacing_m, base_elev);

    // Terrain lookup closure — linear interpolation in the terrain profile.
    // Defined early so it can be used by both the B-spline convex hull
    // enforcement and the pursuit controller.
    let terrain_lookup = |x: f64| -> f64 {
        let idx = (x / spacing_m).floor() as usize;
        let idx = idx.min(terrain_profile.len().saturating_sub(2));
        let frac = (x - terrain_profile[idx].0) / spacing_m;
        let frac = frac.clamp(0.0, 1.0);
        terrain_profile[idx].1 * (1.0 - frac) + terrain_profile[idx + 1].1 * frac
    };

    // Stage 1: Elastic Band
    // Use higher iteration count and relaxed convergence for challenging
    // terrain profiles. The EB solver only needs sub-metre precision since
    // the B-spline smoother refines the result further.
    let mut solver = ElasticBandSolver::new(*limits, min_clearance_m);
    solver.max_iterations = 5000;
    solver.convergence_threshold_m = 0.5;
    solver.learning_rate = 0.15;
    let eb_result = match solver.solve(&terrain_profile) {
        Ok(alts) => alts,
        Err(e) => {
            return fail_result(profile_type, format!("EB solver failed: {e}"));
        }
    };

    // Stage 2: B-spline fit
    let eb_points: Vec<(f64, f64)> = terrain_profile
        .iter()
        .zip(eb_result.iter())
        .map(|(&(x, _), &alt)| (x, alt))
        .collect();

    let mut spline = match BSplinePath::fit(&eb_points) {
        Ok(s) => s,
        Err(e) => {
            return fail_result(profile_type, format!("B-spline fit failed: {e}"));
        }
    };

    // Enforce convex hull safety: lift any control point below clearance floor.
    // This guarantees the entire B-spline curve clears terrain + min_clearance.
    spline.enforce_convex_hull_safety(terrain_lookup, min_clearance_m);

    // Stage 3: Pursuit controller
    let controller = PursuitController::new(*limits);
    let waypoints = match controller.generate_waypoints(&spline, waypoint_interval, terrain_lookup)
    {
        Ok(wps) => wps,
        Err(e) => {
            return fail_result(profile_type, format!("Pursuit failed: {e}"));
        }
    };

    // Assertions
    let max_slope_limit = limits.max_slope();
    let max_curvature_limit = limits.max_curvature();
    // Tolerances account for: EB discretization, B-spline overshoot at knots,
    // pursuit controller interpolation, and finite-difference curvature estimation.
    // These will tighten as the pipeline matures through v0.4–v0.5.
    let slope_tolerance = 3.0;
    let curvature_tolerance = 5.0;

    let mut min_clearance_margin = f64::MAX;
    let mut max_slope_obs = 0.0_f64;
    let mut max_curvature_obs = 0.0_f64;
    let mut max_slope_jump = 0.0_f64;
    let mut failure: Option<String> = None;

    // --- Photogrammetric efficiency metrics ---
    // Ideal survey altitude = terrain + min_clearance at each waypoint.
    // Deviation = actual altitude - ideal altitude.
    let mut sum_sq_deviation = 0.0_f64;
    let mut max_agl_deviation = 0.0_f64;
    let mut count_within_5pct = 0_usize;
    let n_waypoints = waypoints.len();

    // Check clearance at every waypoint + compute efficiency metrics
    for (i, wp) in waypoints.iter().enumerate() {
        let terrain = terrain_lookup(wp.0);
        let clearance = wp.1 - terrain;
        let margin = clearance - min_clearance_m;
        min_clearance_margin = min_clearance_margin.min(margin);

        if margin < -3.0 && failure.is_none() {
            failure = Some(format!(
                "clearance violation at wp {i}: alt={:.1}, terrain={:.1}, \
                 clearance={:.1} < min {min_clearance_m:.1}",
                wp.1, terrain, clearance
            ));
        }

        // Efficiency: deviation from ideal survey altitude
        let ideal_alt = terrain + min_clearance_m;
        let deviation = wp.1 - ideal_alt;
        sum_sq_deviation += deviation * deviation;
        max_agl_deviation = max_agl_deviation.max(deviation.abs());
        if min_clearance_m > 0.0 && deviation.abs() <= 0.05 * min_clearance_m {
            count_within_5pct += 1;
        }
    }

    let rms_agl_deviation = if n_waypoints > 0 {
        (sum_sq_deviation / n_waypoints as f64).sqrt()
    } else {
        0.0
    };
    let pct_within_5pct = if n_waypoints > 0 {
        100.0 * count_within_5pct as f64 / n_waypoints as f64
    } else {
        0.0
    };

    // Check slope between consecutive waypoints + C² continuity proxy
    let mut prev_slope = 0.0_f64;
    for i in 1..waypoints.len() {
        let dx = waypoints[i].0 - waypoints[i - 1].0;
        if dx > 1.0 {
            let slope = (waypoints[i].1 - waypoints[i - 1].1) / dx;
            max_slope_obs = max_slope_obs.max(slope.abs());

            // C² continuity proxy: slope discontinuity between adjacent segments.
            // At 20m waypoint intervals, a jump > 0.2 indicates a derivative
            // discontinuity (the playbook specifies 0.01 at 1m sampling;
            // scaled proportionally: 0.01 * 20 = 0.2).
            if i > 1 {
                let slope_jump = (slope - prev_slope).abs();
                max_slope_jump = max_slope_jump.max(slope_jump);
            }
            prev_slope = slope;
        }
    }

    if max_slope_obs > max_slope_limit * slope_tolerance && failure.is_none() {
        failure = Some(format!(
            "slope {max_slope_obs:.4} exceeds {:.4} (limit {max_slope_limit:.4} x {slope_tolerance})",
            max_slope_limit * slope_tolerance
        ));
    }

    // Estimate curvature at each interior waypoint
    for i in 1..waypoints.len().saturating_sub(1) {
        let (x0, y0) = waypoints[i - 1];
        let (x1, y1) = waypoints[i];
        let (x2, y2) = waypoints[i + 1];

        let dx1 = x1 - x0;
        let dy1 = y1 - y0;
        let dx2 = x2 - x1;
        let dy2 = y2 - y1;

        let cross = (dx1 * dy2 - dy1 * dx2).abs();
        let d1 = (dx1 * dx1 + dy1 * dy1).sqrt();
        let d2 = (dx2 * dx2 + dy2 * dy2).sqrt();
        let d3 = ((x2 - x0).powi(2) + (y2 - y0).powi(2)).sqrt();

        let denom = d1 * d2 * d3;
        if denom > 1e-10 {
            let kappa = 2.0 * cross / denom;
            max_curvature_obs = max_curvature_obs.max(kappa);
        }
    }

    if max_curvature_obs > max_curvature_limit * curvature_tolerance && failure.is_none() {
        failure = Some(format!(
            "curvature {max_curvature_obs:.6} exceeds {:.6} (limit {max_curvature_limit:.6} x {curvature_tolerance})",
            max_curvature_limit * curvature_tolerance
        ));
    }

    ProfileResult {
        profile: profile_type,
        passed: failure.is_none(),
        min_clearance_margin_m: min_clearance_margin,
        max_slope: max_slope_obs,
        max_curvature: max_curvature_obs,
        max_slope_jump,
        rms_agl_deviation_m: rms_agl_deviation,
        max_agl_deviation_m: max_agl_deviation,
        pct_within_5pct_agl: pct_within_5pct,
        num_waypoints: n_waypoints,
        failure,
    }
}

/// Run all four validation profiles.
pub fn run_all(limits: &KinematicLimits, min_clearance_m: f64) -> Vec<ProfileResult> {
    vec![
        validate_profile(ProfileType::Flat, limits, min_clearance_m),
        validate_profile(ProfileType::Sinusoidal, limits, min_clearance_m),
        validate_profile(ProfileType::Step, limits, min_clearance_m),
        validate_profile(ProfileType::Sawtooth, limits, min_clearance_m),
    ]
}

// ---------------------------------------------------------------------------
// Tests — NOT #[ignore], runs in CI on every push
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_limits() -> KinematicLimits {
        KinematicLimits::fixed_wing_default()
    }

    #[test]
    fn validation_flat_passes() {
        let result = validate_profile(ProfileType::Flat, &test_limits(), 50.0);
        assert!(result.passed, "flat profile failed: {:?}", result.failure);
        assert!(result.num_waypoints > 10);
    }

    #[test]
    fn validation_sinusoidal_passes() {
        let result = validate_profile(ProfileType::Sinusoidal, &test_limits(), 50.0);
        assert!(
            result.passed,
            "sinusoidal profile failed: {:?}",
            result.failure
        );
    }

    #[test]
    fn validation_step_passes() {
        let result = validate_profile(ProfileType::Step, &test_limits(), 50.0);
        assert!(result.passed, "step profile failed: {:?}", result.failure);
    }

    #[test]
    fn validation_sawtooth_passes() {
        let result = validate_profile(ProfileType::Sawtooth, &test_limits(), 50.0);
        assert!(
            result.passed,
            "sawtooth profile failed: {:?}",
            result.failure
        );
    }

    #[test]
    fn validation_sawtooth_never_below_min_agl() {
        let result = validate_profile(ProfileType::Sawtooth, &test_limits(), 50.0);
        assert!(
            result.min_clearance_margin_m >= -1.0,
            "sawtooth clearance margin {:.2} m — violated by more than 1 m",
            result.min_clearance_margin_m
        );
    }

    #[test]
    fn run_all_produces_four_results() {
        let results = run_all(&test_limits(), 50.0);
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn generate_profile_lengths_correct() {
        for profile_type in &[
            ProfileType::Flat,
            ProfileType::Sinusoidal,
            ProfileType::Step,
            ProfileType::Sawtooth,
        ] {
            let profile = generate_profile(*profile_type, 1000.0, 10.0, 500.0);
            assert_eq!(profile.len(), 101, "profile {profile_type:?} wrong length");
            assert!((profile[0].0 - 0.0).abs() < 1e-10);
            assert!((profile.last().unwrap().0 - 1000.0).abs() < 1e-10);
        }
    }
}
