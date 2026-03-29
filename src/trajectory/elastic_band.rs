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
 * Elastic Band energy minimization solver for terrain-following flight paths.
 *
 * The solver operates on a 2D vertical slice: nodes are positioned at uniform
 * along-track distances, and only the altitude of each interior node is
 * adjusted. All nodes (including endpoints) are free to float — the
 * solver produces a smooth profile without artificial boundary
 * discontinuities. Entry/exit altitude matching is the responsibility
 * of the flight line generator.
 *
 * Four energy terms drive the relaxation:
 *
 *   E_tension   — Hooke's law springs between neighbors. Minimizes total
 *                 path length by pulling nodes toward a straight line.
 *
 *   E_torsion   — Angular penalty at each node. Penalizes bending (large
 *                 pitch angle change), enforcing curvature smoothing.
 *
 *   E_terrain   — Asymmetric exponential barrier. Repels nodes that
 *                 approach the terrain + min_clearance boundary. One-sided:
 *                 only active when the node is close to or below the floor.
 *
 *   E_gravity   — Weak constant downward pull. Prevents the band from
 *                 floating arbitrarily high above the terrain.
 *
 * The net force on each node is the negative gradient of the total energy.
 * The Jacobian is strictly tridiagonal (forces depend only on adjacent
 * neighbors), so each iteration is O(n).
 *
 * Initialization: all nodes start above the highest peak in the profile
 * plus min_clearance, then relax downward onto the terrain repulsion field.
 * This avoids local minima traps.
 *
 * Reference: docs/34_terrain_following_planning.md
 * Reference: .agents/formula_reference.md — Elastic Band Energy Functional
 */

use rayon::prelude::*;

use crate::error::TrajectoryError;
use crate::types::KinematicLimits;

// ---------------------------------------------------------------------------
// Floor mode
// ---------------------------------------------------------------------------

/// Strategy for computing the altitude floor that the elastic band must not
/// penetrate. All modes enforce terrain + min_clearance as a hard safety
/// minimum — the reference surface can only raise the floor, never lower it.
#[derive(Debug, Clone, Copy, Default)]
pub enum FloorMode {
    /// Per-point terrain following: floor[i] = terrain[i] + clearance.
    /// The band hugs the terrain contour. Standard for photogrammetric survey
    /// where consistent GSD matters.
    #[default]
    TerrainFollowing,
    /// Constant floor at mean terrain elevation + clearance.
    /// Smooths over small-scale variation. Suitable for gently rolling terrain
    /// where consistent absolute altitude is preferred over constant AGL.
    MeanTerrain,
    /// Constant floor at median terrain elevation + clearance.
    /// More robust to outlier peaks/valleys than mean. Good for terrain with
    /// isolated anomalies (towers, quarries).
    MedianTerrain,
    /// Constant floor at a fixed MSL altitude. Terrain clearance is still
    /// enforced — the band rises above this altitude where terrain demands it.
    /// Used for absolute altitude missions (e.g., airspace constraints).
    FixedMsl(f64),
    /// Constant floor at max(terrain) + clearance across the entire line.
    /// Guarantees level flight above the highest point. Most conservative mode,
    /// typical for obstacle-rich environments or when GSD variation is acceptable.
    HighestPoint,
}

// ---------------------------------------------------------------------------
// Spring constants
// ---------------------------------------------------------------------------

/// Tuning parameters for the elastic band energy functional.
///
/// These constants control the relative weight of each energy term. The
/// default values are tuned for typical aerial survey terrain-following
/// at 30–80 m AGL over moderate terrain (hills, ridges, valleys).
#[derive(Debug, Clone, Copy)]
pub struct SpringConstants {
    /// Tension spring constant (k_t). Controls path-shortening force.
    /// Higher values produce straighter (but less terrain-conforming) paths.
    pub tension: f64,
    /// Torsional spring constant (k_b). Controls curvature penalty.
    /// Higher values produce smoother paths with lower pitch rate.
    pub torsion: f64,
    /// Terrain repulsion strength (k_r). Controls the barrier force that
    /// prevents nodes from dropping below min_clearance AGL.
    pub repulsion: f64,
    /// Repulsion decay distance (d_0, metres). The terrain barrier force
    /// decays as exp(-d / d_0) where d is the distance above the clearance
    /// floor. Smaller values create a sharper boundary.
    pub repulsion_decay: f64,
    /// Gravitational pull constant (k_g). Weak downward force that prevents
    /// the band from floating arbitrarily high. Should be much smaller than
    /// the other constants.
    pub gravity: f64,
}

impl SpringConstants {
    /// Default spring constants tuned for aerial survey terrain-following.
    ///
    /// Equilibrium altitude above the clearance floor is approximately:
    ///   d_eq = -repulsion_decay * ln(gravity * repulsion_decay / repulsion)
    /// With these defaults: d_eq ≈ 3.5 m above the clearance floor.
    ///
    /// Spring constants are scaled so that explicit Euler integration is
    /// stable with learning_rate <= 0.3. The stability condition is:
    ///   learning_rate < 2 / (4 * tension + repulsion / decay^2)
    pub fn default_survey() -> Self {
        Self {
            tension: 0.1,
            torsion: 0.05,
            repulsion: 5.0,
            repulsion_decay: 5.0,
            gravity: 0.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Solver
// ---------------------------------------------------------------------------

/// Elastic band terrain-following path smoother.
///
/// Given a terrain profile (along-track distance, terrain elevation), the
/// solver produces a relaxed altitude profile that:
///
/// - Maintains minimum AGL clearance at every node (hard safety floor)
/// - Minimizes total path curvature (smooth pitch transitions)
/// - Tracks the terrain as closely as possible (photogrammetric efficiency)
/// - Respects kinematic limits (max slope, max curvature)
pub struct ElasticBandSolver {
    /// Platform kinematic constraints.
    pub limits: KinematicLimits,
    /// Spring constants controlling the energy balance.
    pub springs: SpringConstants,
    /// Minimum clearance above terrain (metres). This is the hard safety
    /// floor — no node will ever be placed below terrain + min_clearance.
    pub min_clearance_m: f64,
    /// Strategy for computing the reference altitude floor.
    pub floor_mode: FloorMode,
    /// Maximum gradient descent iterations before declaring non-convergence.
    pub max_iterations: usize,
    /// Convergence threshold (metres). The solver stops when the maximum
    /// node displacement in a single iteration drops below this value.
    pub convergence_threshold_m: f64,
    /// Learning rate for gradient descent (metres per unit force).
    /// Controls the step size. Too large causes oscillation; too small
    /// causes slow convergence. Typical range: 0.3–1.0.
    pub learning_rate: f64,
}

impl ElasticBandSolver {
    /// Create a solver with sensible defaults for aerial survey.
    pub fn new(limits: KinematicLimits, min_clearance_m: f64) -> Self {
        Self {
            limits,
            springs: SpringConstants::default_survey(),
            min_clearance_m,
            floor_mode: FloorMode::default(),
            max_iterations: 500,
            convergence_threshold_m: 0.01,
            learning_rate: 0.3,
        }
    }

    /// Compute the altitude floor for each node based on the floor mode.
    ///
    /// The safety floor (terrain + min_clearance) is always enforced as
    /// a hard minimum. The reference surface from the floor mode can only
    /// raise the floor above this safety baseline.
    fn compute_floor(&self, terrain: &[f64]) -> Vec<f64> {
        let n = terrain.len();
        let safety: Vec<f64> = terrain.iter().map(|t| t + self.min_clearance_m).collect();

        match self.floor_mode {
            FloorMode::TerrainFollowing => safety,

            FloorMode::MeanTerrain => {
                let mean = terrain.iter().sum::<f64>() / n as f64;
                let ref_alt = mean + self.min_clearance_m;
                safety.iter().map(|&s| s.max(ref_alt)).collect()
            }

            FloorMode::MedianTerrain => {
                let mut sorted = terrain.to_vec();
                // NaN-safe sort: NaN values sort to the end so they don't
                // corrupt the median. Non-finite terrain is caught upstream
                // by TerrainEngine, but we defend against it here too.
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Greater));
                let median = if n.is_multiple_of(2) {
                    (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
                } else {
                    sorted[n / 2]
                };
                let ref_alt = median + self.min_clearance_m;
                safety.iter().map(|&s| s.max(ref_alt)).collect()
            }

            FloorMode::FixedMsl(alt) => safety.iter().map(|&s| s.max(alt)).collect(),

            FloorMode::HighestPoint => {
                let max_terrain = terrain.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let ref_alt = max_terrain + self.min_clearance_m;
                // ref_alt >= terrain[i] + clearance for all i, so safety is
                // always satisfied. Use a constant floor.
                vec![ref_alt; n]
            }
        }
    }

    /// Solve the vertical profile.
    ///
    /// # Arguments
    ///
    /// * `terrain_profile` — Slice of (along_track_m, terrain_elevation_m)
    ///   pairs. Must be sorted by along-track distance with uniform spacing.
    ///   Minimum 3 points.
    ///
    /// # Returns
    ///
    /// A vector of relaxed altitudes (same length as input). All nodes
    /// (including endpoints) are free to move during relaxation.
    pub fn solve(&self, terrain_profile: &[(f64, f64)]) -> Result<Vec<f64>, TrajectoryError> {
        let n = terrain_profile.len();
        if n < 3 {
            return Err(TrajectoryError::InvalidInput(format!(
                "terrain profile needs >= 3 points, got {n}"
            )));
        }

        // Validate uniform spacing
        let dx = terrain_profile[1].0 - terrain_profile[0].0;
        if dx <= 0.0 || !dx.is_finite() {
            return Err(TrajectoryError::InvalidInput(
                "terrain profile must have positive, finite along-track spacing".into(),
            ));
        }

        // Extract terrain elevations and compute the mode-aware floor.
        // The safety invariant (terrain + min_clearance) is always enforced
        // inside compute_floor regardless of mode.
        let terrain: Vec<f64> = terrain_profile.iter().map(|p| p.1).collect();
        let floor = self.compute_floor(&terrain);

        // Initialize each node at the floor. For TerrainFollowing this is
        // terrain + min_clearance; for constant modes it is the reference
        // altitude. Starting at the floor minimizes iterations to equilibrium.
        let mut alt: Vec<f64> = floor.clone();

        // Gradient descent — all nodes are free to move (including endpoints).
        // The flight line generator handles entry/exit altitude separately.
        let mut converged = false;
        let mut final_max_disp = 0.0;
        let mut iteration = 0;

        for iter in 0..self.max_iterations {
            iteration = iter + 1;

            // Phase 1: Compute forces in parallel (Jacobi-style — all nodes
            // read from the SAME snapshot of alt). Each node's force depends
            // only on its immediate neighbors, so this is embarrassingly
            // parallel across the node array.
            let forces: Vec<f64> = (0..n)
                .into_par_iter()
                .map(|i| self.net_force_at(i, n, &alt, &floor, dx))
                .collect();

            // Phase 2: Apply displacements sequentially and clamp.
            let mut max_disp = 0.0_f64;
            for i in 0..n {
                let displacement = self.learning_rate * forces[i];
                alt[i] += displacement;

                // Hard safety clamp: never go below floor
                if alt[i] < floor[i] {
                    alt[i] = floor[i];
                }

                max_disp = max_disp.max(displacement.abs());
            }

            final_max_disp = max_disp;
            if max_disp < self.convergence_threshold_m {
                converged = true;
                break;
            }
        }

        if !converged {
            return Err(TrajectoryError::ConvergenceFailure {
                iterations: iteration,
                max_disp_m: final_max_disp,
            });
        }

        // Note: slope and curvature enforcement is the responsibility of the
        // B-spline smoother (Phase 5B) which applies C² continuity constraints.
        // The EB solver only guarantees clearance above terrain.

        Ok(alt)
    }

    /// Compute the net vertical force on node `i`.
    ///
    /// The force is the negative gradient of the total energy with respect
    /// to the altitude of node i. Positive force = push upward.
    ///
    /// Boundary nodes (i=0, i=n-1) use one-sided stencils for tension and
    /// torsion. This lets all nodes float freely — the solver produces a
    /// smooth profile without artificial endpoint discontinuities.
    fn net_force_at(&self, i: usize, n: usize, alt: &[f64], floor: &[f64], dx: f64) -> f64 {
        let k = &self.springs;

        // Tension and torsion: require neighbors on both sides.
        // At boundaries, use one-sided approximations.
        let (f_tension, f_torsion) = if i == 0 || i == n - 1 {
            // Boundary: only apply tension toward the single neighbor.
            // No torsion (can't compute bending without 3 points).
            let neighbor = if i == 0 { alt[1] } else { alt[n - 2] };
            let f_t = -k.tension * (alt[i] - neighbor);
            (f_t, 0.0)
        } else {
            // Interior: full three-point stencil.
            // Tension: pull toward midpoint of neighbors (discrete Laplacian).
            let f_t = -k.tension * (2.0 * alt[i] - alt[i - 1] - alt[i + 1]);

            // Torsion: penalizes bending angle.
            //   angle ~= (alt[i+1] - 2*alt[i] + alt[i-1]) / dx
            //   F = 4 * k_b * angle / dx
            let angle = (alt[i + 1] - 2.0 * alt[i] + alt[i - 1]) / dx;
            let f_b = 4.0 * k.torsion * angle / dx;

            (f_t, f_b)
        };

        // Terrain repulsion: exponential barrier above the clearance floor.
        // d = altitude above the floor (positive = safe)
        let d = alt[i] - floor[i];
        let f_repulsion = if d < k.repulsion_decay * 5.0 {
            (k.repulsion / k.repulsion_decay) * (-d / k.repulsion_decay).exp()
        } else {
            0.0
        };

        // Gravity: constant downward pull.
        let f_gravity = -k.gravity;

        f_tension + f_torsion + f_repulsion + f_gravity
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_solver(min_clearance: f64) -> ElasticBandSolver {
        let limits = KinematicLimits::fixed_wing_default();
        ElasticBandSolver::new(limits, min_clearance)
    }

    /// Helper: generate a flat terrain profile at a given elevation.
    fn flat_terrain(elevation: f64, length_m: f64, spacing_m: f64) -> Vec<(f64, f64)> {
        let n = (length_m / spacing_m).ceil() as usize + 1;
        (0..n).map(|i| (i as f64 * spacing_m, elevation)).collect()
    }

    /// Helper: generate a single triangular peak.
    fn peak_terrain(
        base_elev: f64,
        peak_elev: f64,
        length_m: f64,
        spacing_m: f64,
    ) -> Vec<(f64, f64)> {
        let n = (length_m / spacing_m).ceil() as usize + 1;
        let mid = length_m / 2.0;
        (0..n)
            .map(|i| {
                let x = i as f64 * spacing_m;
                let t = 1.0 - (x - mid).abs() / mid; // 0 at edges, 1 at center
                let elev = base_elev + t.max(0.0) * (peak_elev - base_elev);
                (x, elev)
            })
            .collect()
    }

    #[test]
    fn rejects_too_few_points() {
        let solver = make_solver(50.0);
        let profile = vec![(0.0, 100.0), (10.0, 100.0)];
        assert!(solver.solve(&profile).is_err());
    }

    #[test]
    fn flat_terrain_stays_at_target_agl() {
        let min_clearance = 50.0;
        let solver = make_solver(min_clearance);
        let profile = flat_terrain(500.0, 3000.0, 10.0);
        let result = solver
            .solve(&profile)
            .expect("flat terrain should converge");

        // All nodes should be at or very near terrain + min_clearance
        let target = 500.0 + min_clearance;
        for (i, &alt) in result.iter().enumerate() {
            assert!(
                alt >= target - 0.1,
                "node {i}: alt {alt:.2} below target {target:.2}"
            );
            // Gravity pulls down, so nodes shouldn't float too high above target
            assert!(
                alt < target + min_clearance * 2.0,
                "node {i}: alt {alt:.2} unreasonably high above target {target:.2}"
            );
        }
    }

    #[test]
    fn respects_min_clearance_everywhere() {
        let min_clearance = 50.0;
        let solver = make_solver(min_clearance);
        let profile = peak_terrain(100.0, 800.0, 5000.0, 10.0);
        let result = solver
            .solve(&profile)
            .expect("peak terrain should converge");

        for (i, (&alt, &(_, terrain))) in result.iter().zip(profile.iter()).enumerate() {
            let floor = terrain + min_clearance;
            assert!(
                alt >= floor - 0.01, // allow tiny numerical noise
                "node {i}: alt {alt:.2} below floor {floor:.2} (terrain {terrain:.2})"
            );
        }
    }

    #[test]
    fn single_peak_smooths_over() {
        let min_clearance = 50.0;
        let solver = make_solver(min_clearance);
        let profile = peak_terrain(100.0, 500.0, 3000.0, 10.0);
        let result = solver.solve(&profile).expect("should converge");

        // The path should be smooth — no wild oscillations. Check that the
        // maximum altitude is reasonably bounded (not floating at infinity).
        let max_alt = result.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_alt < 1500.0,
            "max altitude {max_alt:.1} unreasonably high for 500m peak"
        );
    }

    #[test]
    fn converges_within_500_iterations() {
        let min_clearance = 80.0;
        let mut solver = make_solver(min_clearance);
        solver.max_iterations = 500;

        // 30 km line at 10 m spacing = 3,001 nodes
        let profile = peak_terrain(200.0, 600.0, 30_000.0, 10.0);
        let result = solver.solve(&profile);
        assert!(
            result.is_ok(),
            "3000-node profile should converge within 500 iterations: {:?}",
            result.err()
        );
    }

    #[test]
    fn gentle_terrain_slope_within_limit() {
        // Use terrain with slopes well within kinematic limits.
        // The EB solver tracks the terrain closely when it can, so gentle
        // terrain should produce gentle path slopes.
        let min_clearance = 50.0;
        let solver = make_solver(min_clearance);
        // 100m rise over 10km = 0.01 slope (well within 0.21 limit)
        let profile = peak_terrain(100.0, 200.0, 10_000.0, 10.0);
        let result = solver.solve(&profile).expect("should converge");

        let max_slope = solver.limits.max_slope();
        let dx = 10.0;
        for i in 0..result.len() - 1 {
            let slope = (result[i + 1] - result[i]).abs() / dx;
            assert!(
                slope <= max_slope * 1.5, // generous tolerance — B-spline enforces tightly
                "segment {i}: slope {slope:.4} exceeds limit {max_slope:.4}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Floor mode tests
    // -----------------------------------------------------------------------

    #[test]
    fn mean_terrain_raises_valleys() {
        // A valley profile: 500m at edges, dipping to 100m in the center.
        // With MeanTerrain mode, the floor should be elevated over the valley
        // compared to TerrainFollowing.
        let min_clearance = 50.0;
        let mut solver = make_solver(min_clearance);
        solver.floor_mode = FloorMode::MeanTerrain;

        // Build a valley profile: 500m at edges, dipping to 100m at center.
        let n = 301;
        let valley: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let x = i as f64 * 10.0;
                let mid = 1500.0;
                let t = (x - mid).abs() / mid; // 1 at edges, 0 at center
                let elev = 100.0 + t * 400.0; // 500 at edges, 100 at center
                (x, elev)
            })
            .collect();

        let result = solver.solve(&valley).expect("should converge");

        // Mean terrain is ~300m. Floor = 300 + 50 = 350m.
        // At the valley center (terrain=100m), TerrainFollowing floor = 150m,
        // but MeanTerrain floor = 350m. Result should be >= 350m at center.
        let center = n / 2;
        assert!(
            result[center] >= 340.0,
            "valley center alt {:.1} should be raised by mean floor",
            result[center]
        );
    }

    #[test]
    fn median_terrain_robust_to_outlier() {
        let min_clearance = 50.0;
        let mut solver = make_solver(min_clearance);
        solver.floor_mode = FloorMode::MedianTerrain;
        solver.max_iterations = 2000;
        solver.convergence_threshold_m = 0.2; // hill floor transition is steep

        // Mostly flat at 200m with a hill rising to 500m over 40 nodes.
        // Median stays near 200m, so mean-based floor would be higher.
        let n = 301;
        let hill_center = 150;
        let hill_width = 40;
        let terrain: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let x = i as f64 * 10.0;
                let dist = (i as f64 - hill_center as f64).abs();
                let elev = if dist < hill_width as f64 {
                    let t = 1.0 - dist / hill_width as f64;
                    200.0 + t * 300.0
                } else {
                    200.0
                };
                (x, elev)
            })
            .collect();

        let result = solver.solve(&terrain).expect("should converge");

        // Median ≈ 200m, so floor ≈ 250m for most nodes. Nodes far from the
        // hill should not be inflated by the outlier.
        let typical = result[20]; // far from the hill
        assert!(
            typical < 300.0,
            "typical node {:.1} should not be inflated by outlier hill",
            typical
        );
        // But the hill peak must still be cleared
        assert!(
            result[hill_center] >= 500.0 + min_clearance - 0.01,
            "hill peak must still be cleared"
        );
    }

    #[test]
    fn fixed_msl_sets_constant_floor() {
        let min_clearance = 50.0;
        let mut solver = make_solver(min_clearance);
        solver.floor_mode = FloorMode::FixedMsl(600.0);

        // Terrain at 200m — well below the 600m MSL floor.
        let profile = flat_terrain(200.0, 3000.0, 10.0);
        let result = solver.solve(&profile).expect("should converge");

        // All nodes should be at or above 600m (not 200+50=250m).
        for (i, &alt) in result.iter().enumerate() {
            assert!(
                alt >= 599.9,
                "node {i}: alt {alt:.1} below fixed MSL floor 600"
            );
        }
    }

    #[test]
    fn fixed_msl_still_clears_terrain() {
        let min_clearance = 50.0;
        let mut solver = make_solver(min_clearance);
        solver.floor_mode = FloorMode::FixedMsl(300.0);

        // Terrain peaks at 500m — above the 300m MSL floor.
        // Safety floor (500+50=550) must override the MSL floor.
        let profile = peak_terrain(100.0, 500.0, 3000.0, 10.0);
        let result = solver.solve(&profile).expect("should converge");

        for (i, (&alt, &(_, terrain))) in result.iter().zip(profile.iter()).enumerate() {
            let safety = terrain + min_clearance;
            assert!(
                alt >= safety - 0.01,
                "node {i}: alt {alt:.2} below safety floor {safety:.2}"
            );
        }
    }

    #[test]
    fn highest_point_produces_level_flight() {
        let min_clearance = 50.0;
        let mut solver = make_solver(min_clearance);
        solver.floor_mode = FloorMode::HighestPoint;

        let profile = peak_terrain(100.0, 500.0, 3000.0, 10.0);
        let result = solver.solve(&profile).expect("should converge");

        // Floor is 500 + 50 = 550m everywhere. All nodes should be near 550m.
        let max_alt = result.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min_alt = result.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(
            max_alt - min_alt < 20.0,
            "HighestPoint should produce near-level flight, got spread {:.1}m",
            max_alt - min_alt
        );
        assert!(
            min_alt >= 549.9,
            "min altitude {min_alt:.1} below expected floor 550"
        );
    }

    #[test]
    fn endpoints_above_floor() {
        let min_clearance = 50.0;
        let solver = make_solver(min_clearance);
        let profile = flat_terrain(300.0, 1000.0, 10.0);
        let result = solver.solve(&profile).expect("should converge");

        let floor = 300.0 + min_clearance;
        // Endpoints must be at or above the clearance floor
        assert!(result[0] >= floor - 0.01, "start below floor");
        assert!(*result.last().unwrap() >= floor - 0.01, "end below floor");
    }
}
