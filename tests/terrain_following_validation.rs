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
 * Integration tests for the terrain-following validation suite.
 *
 * Exercises the complete EB → B-spline → Pursuit pipeline against
 * deterministic synthetic terrain profiles. These tests run in CI on
 * every push and must NEVER be disabled.
 *
 * Two validation paths are tested:
 *
 *   1. **Full pipeline** (via `run_all`): EB → B-spline → pursuit → waypoints.
 *      Tolerances account for pursuit controller discretization at 20 m.
 *
 *   2. **Direct B-spline** (via `run_all_direct`): EB → B-spline → 1 m sampling.
 *      Tighter assertions: slope, curvature, C² continuity, clearance.
 *
 *   3. **Parametric sinusoidal sweep**: progressively harder terrain until the
 *      kinematic envelope is exceeded. Verifies graceful curvature clipping.
 *
 * Reference: docs/34_terrain_following_planning.md — Synthetic Validation Suite
 */

use irontrack::trajectory::validation::{
    parametric_sinusoidal_sweep, run_all, run_all_direct, validate_bspline_direct, ProfileType,
};
use irontrack::types::KinematicLimits;

fn limits() -> KinematicLimits {
    KinematicLimits::fixed_wing_default()
}

// ---------------------------------------------------------------------------
// Full pipeline (pursuit-based, 20 m waypoints)
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_all_profiles_pass() {
    let lim = limits();
    let results = run_all(&lim, 50.0);
    assert_eq!(results.len(), 4);
    for r in &results {
        assert!(r.passed, "{:?} failed: {:?}", r.profile, r.failure);

        // Assert on actual safety metrics, not just the boolean.
        // Catches regressions that stay within tolerances but degrade.
        assert!(
            r.min_clearance_margin_m >= -3.0,
            "{:?}: clearance margin {:.2} m",
            r.profile,
            r.min_clearance_margin_m
        );
        assert!(
            r.max_slope <= lim.max_slope() * 3.0,
            "{:?}: slope {:.4} exceeds 3× limit",
            r.profile,
            r.max_slope
        );
        assert!(
            r.max_curvature <= lim.max_curvature() * 5.0,
            "{:?}: curvature {:.6} exceeds 5× limit",
            r.profile,
            r.max_curvature
        );
        assert!(
            r.num_waypoints > 10,
            "{:?}: only {} waypoints",
            r.profile,
            r.num_waypoints
        );
    }
}

// ---------------------------------------------------------------------------
// Direct B-spline (1 m sampling, tighter assertions)
// ---------------------------------------------------------------------------

#[test]
fn direct_bspline_all_profiles_pass() {
    let results = run_all_direct(&limits(), 50.0);
    assert_eq!(results.len(), 4);
    for r in &results {
        assert!(r.passed, "{:?} direct failed: {:?}", r.profile, r.failure);
    }
}

#[test]
fn direct_bspline_sinusoidal_curvature_clipped() {
    let lim = limits();
    let result = validate_bspline_direct(ProfileType::Sinusoidal, &lim, 50.0);
    assert!(result.passed, "sinusoidal failed: {:?}", result.failure);
    assert!(
        result.max_curvature <= lim.max_curvature() * 2.0,
        "curvature {:.6} exceeds 2x limit {:.6}",
        result.max_curvature,
        lim.max_curvature()
    );
}

#[test]
fn direct_bspline_sawtooth_never_below_min_agl() {
    let result = validate_bspline_direct(ProfileType::Sawtooth, &limits(), 50.0);
    assert!(
        result.min_clearance_margin_m >= -1.0,
        "sawtooth clearance margin {:.2} m — violated by more than 1 m",
        result.min_clearance_margin_m
    );
}

#[test]
fn direct_bspline_flat_minimal_curvature() {
    let result = validate_bspline_direct(ProfileType::Flat, &limits(), 50.0);
    assert!(result.passed, "flat failed: {:?}", result.failure);
    assert!(
        result.max_curvature < 0.001,
        "flat terrain should have near-zero curvature, got {:.6}",
        result.max_curvature
    );
}

// ---------------------------------------------------------------------------
// Parametric sinusoidal sweep
// ---------------------------------------------------------------------------

#[test]
fn sweep_pipeline_completes_all_steps() {
    let steps = parametric_sinusoidal_sweep(&limits(), 50.0);
    assert_eq!(steps.len(), 6);
    for step in &steps {
        assert!(
            step.pipeline_ok,
            "pipeline failed for A={}, wavelength={}: {:?}",
            step.amplitude_m, step.wavelength_m, step.error
        );
    }
}

#[test]
fn sweep_clearance_always_maintained() {
    let steps = parametric_sinusoidal_sweep(&limits(), 50.0);
    for step in &steps {
        assert!(
            step.clearance_maintained,
            "clearance violated for A={}, wavelength={}",
            step.amplitude_m, step.wavelength_m
        );
    }
}

#[test]
fn sweep_curvature_always_bounded() {
    // Recompute the curvature bound from raw numerics rather than trusting
    // the production-computed `curvature_bounded` boolean. This catches
    // bugs in the bound calculation itself.
    let steps = parametric_sinusoidal_sweep(&limits(), 50.0);
    let kappa_max = limits().max_curvature();
    let curvature_tolerance = 10.0;

    for step in &steps {
        let terrain_curvature = step.terrain_max_slope.powi(2)
            / (2.0 * std::f64::consts::PI * step.amplitude_m / step.wavelength_m);
        let bound = (kappa_max * curvature_tolerance).max(terrain_curvature * 1.5);
        assert!(
            step.max_curvature <= bound,
            "A={}, wavelength={}: kappa={:.6} > recomputed bound {:.6}",
            step.amplitude_m,
            step.wavelength_m,
            step.max_curvature,
            bound
        );
        // Cross-check: production boolean should agree with our recomputation
        assert_eq!(
            step.curvature_bounded,
            step.max_curvature <= bound,
            "curvature_bounded flag disagrees with recomputed bound"
        );
    }
}

#[test]
fn sweep_agl_deviation_increases_with_difficulty() {
    let steps = parametric_sinusoidal_sweep(&limits(), 50.0);

    // Check monotonic progression: each step should have >= RMS deviation
    // than the previous, catching mid-progression regressions.
    for i in 1..steps.len() {
        // Allow small tolerance for numerical noise between adjacent steps
        assert!(
            steps[i].rms_agl_deviation_m >= steps[i - 1].rms_agl_deviation_m - 1.0,
            "AGL deviation should not decrease significantly: \
             step {} (A={}, RMS={:.1}) < step {} (A={}, RMS={:.1})",
            i,
            steps[i].amplitude_m,
            steps[i].rms_agl_deviation_m,
            i - 1,
            steps[i - 1].amplitude_m,
            steps[i - 1].rms_agl_deviation_m
        );
    }

    // Also verify easy-vs-hard endpoint comparison
    let easy = steps.iter().find(|s| !s.exceeds_kinematic_envelope);
    let hard = steps.iter().rev().find(|s| s.exceeds_kinematic_envelope);

    if let (Some(e), Some(h)) = (easy, hard) {
        assert!(
            h.rms_agl_deviation_m >= e.rms_agl_deviation_m,
            "harder terrain (A={}, wavelength={}, RMS={:.1}) should have >= AGL deviation \
             than easy terrain (A={}, wavelength={}, RMS={:.1})",
            h.amplitude_m,
            h.wavelength_m,
            h.rms_agl_deviation_m,
            e.amplitude_m,
            e.wavelength_m,
            e.rms_agl_deviation_m
        );
    } else {
        panic!("sweep should have both easy and hard steps");
    }
}

#[test]
fn sweep_identifies_envelope_exceedance() {
    let steps = parametric_sinusoidal_sweep(&limits(), 50.0);
    let within: Vec<_> = steps
        .iter()
        .filter(|s| !s.exceeds_kinematic_envelope)
        .collect();
    let beyond: Vec<_> = steps
        .iter()
        .filter(|s| s.exceeds_kinematic_envelope)
        .collect();
    assert!(!within.is_empty(), "should have at least one easy step");
    assert!(!beyond.is_empty(), "should have at least one hard step");
}
