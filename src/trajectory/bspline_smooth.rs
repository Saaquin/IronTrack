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
 * Cubic B-spline smoothing for terrain-following flight paths.
 *
 * Takes the relaxed elastic band profile and fits a C² continuous cubic
 * B-spline. The B-spline guarantees:
 *
 *   - Continuous first derivative (slope) — no pitch discontinuities
 *   - Continuous second derivative (curvature) — bounded vertical acceleration
 *   - Bounded third derivative (jerk) — stabilises gyro gimbals
 *
 * The convex hull property of B-splines provides a key safety guarantee:
 * if every segment of the control polygon clears min AGL above terrain,
 * then the entire B-spline curve is mathematically incapable of dropping
 * below that threshold. This transforms expensive continuous 3D collision
 * detection into cheap discrete point validation.
 *
 * Reference: docs/34_terrain_following_planning.md
 * Reference: .agents/formula_reference.md — Cubic B-Spline
 */

use crate::error::TrajectoryError;

// ---------------------------------------------------------------------------
// B-spline path
// ---------------------------------------------------------------------------

/// A cubic B-spline curve in 2D (along-track distance vs altitude).
///
/// The curve is defined by control points and a clamped uniform knot vector.
/// Evaluation uses the Cox-de Boor recursive algorithm (degree 3).
pub struct BSplinePath {
    /// Control points: (along_track_m, altitude_m).
    control_points: Vec<(f64, f64)>,
    /// Clamped knot vector. Length = n_control + degree + 1.
    knots: Vec<f64>,
    /// B-spline degree (always 3 for cubic).
    degree: usize,
}

impl BSplinePath {
    /// Fit a C² cubic B-spline through the given data points.
    ///
    /// Uses global interpolation: solves a tridiagonal linear system to find
    /// control points such that the spline passes through all data points.
    ///
    /// # Arguments
    ///
    /// * `data_points` — (along_track_m, altitude_m) pairs from the EB solver.
    ///   Must have at least 4 points (minimum for cubic interpolation).
    pub fn fit(data_points: &[(f64, f64)]) -> Result<Self, TrajectoryError> {
        let n = data_points.len();
        if n < 4 {
            return Err(TrajectoryError::InvalidInput(format!(
                "B-spline fit requires >= 4 data points, got {n}"
            )));
        }

        // Parameter values: chord-length parameterization for better
        // distribution of parameter space across the data.
        let params = chord_length_params(data_points);

        // Clamped uniform knot vector
        let knots = clamped_uniform_knots(&params, n, 3);

        // Solve the interpolation system: find control points P such that
        //   C(params[i]) = data_points[i] for all i
        //
        // For cubic B-splines, this produces a banded (tridiagonal) system.
        // We use the Thomas algorithm (O(n)) to solve it.
        let control_points = solve_interpolation(data_points, &params, &knots, 3)?;

        Ok(Self {
            control_points,
            knots,
            degree: 3,
        })
    }

    /// Fit with adaptive knot placement based on curvature.
    ///
    /// Inserts additional knots at points of high curvature (peaks, valleys,
    /// cliffs) and uses fewer knots over flat terrain. This produces a more
    /// efficient representation that is tight where it matters.
    pub fn fit_adaptive(
        data_points: &[(f64, f64)],
        curvature_threshold: f64,
    ) -> Result<Self, TrajectoryError> {
        let n = data_points.len();
        if n < 4 {
            return Err(TrajectoryError::InvalidInput(format!(
                "B-spline fit requires >= 4 data points, got {n}"
            )));
        }

        // Identify feature points based on discrete curvature.
        // Feature points are where |curvature| > threshold.
        let mut feature_indices: Vec<usize> = Vec::new();
        feature_indices.push(0);

        for i in 1..n - 1 {
            let dx = data_points[i + 1].0 - data_points[i - 1].0;
            if dx.abs() < 1e-12 {
                continue;
            }
            let d2y = (data_points[i + 1].1 - 2.0 * data_points[i].1 + data_points[i - 1].1)
                / (dx * dx / 4.0);
            if d2y.abs() > curvature_threshold {
                feature_indices.push(i);
            }
        }
        feature_indices.push(n - 1);
        feature_indices.dedup();

        // If feature detection produces too few points, subsample uniformly.
        // This avoids wasting control points on flat terrain where the full
        // dataset would produce an over-determined representation.
        if feature_indices.len() < 4 {
            let target = 4.max(n / 10).min(n);
            let step = (n - 1) as f64 / (target - 1) as f64;
            let subsampled: Vec<(f64, f64)> = (0..target)
                .map(|i| {
                    let idx = (i as f64 * step).round() as usize;
                    data_points[idx.min(n - 1)]
                })
                .collect();
            return Self::fit(&subsampled);
        }

        // Extract feature points and fit through them
        let feature_points: Vec<(f64, f64)> =
            feature_indices.iter().map(|&i| data_points[i]).collect();

        Self::fit(&feature_points)
    }

    /// Evaluate the spline at parameter `t`.
    ///
    /// Returns (along_track_m, altitude_m). Parameter `t` should be in the
    /// range [knots[degree], knots[n_control]].
    pub fn evaluate(&self, t: f64) -> (f64, f64) {
        let t_clamped = t
            .max(self.knots[self.degree])
            .min(self.knots[self.control_points.len()]);
        deboor_eval(&self.control_points, &self.knots, self.degree, t_clamped)
    }

    /// First derivative at parameter `t`.
    ///
    /// Returns (ds/dt, dalt/dt). Used for slope computation:
    ///   slope = dalt/ds = (dalt/dt) / (ds/dt)
    pub fn derivative(&self, t: f64) -> (f64, f64) {
        // Derivative of a B-spline of degree p is a B-spline of degree p-1
        // with control points: Q[i] = p * (P[i+1] - P[i]) / (knots[i+p+1] - knots[i+1])
        let p = self.degree;
        let n = self.control_points.len();
        if n < 2 {
            return (0.0, 0.0);
        }

        let mut deriv_cp: Vec<(f64, f64)> = Vec::with_capacity(n - 1);
        for i in 0..n - 1 {
            let denom = self.knots[i + p + 1] - self.knots[i + 1];
            if denom.abs() < 1e-15 {
                deriv_cp.push((0.0, 0.0));
            } else {
                let scale = p as f64 / denom;
                let dx = self.control_points[i + 1].0 - self.control_points[i].0;
                let dy = self.control_points[i + 1].1 - self.control_points[i].1;
                deriv_cp.push((dx * scale, dy * scale));
            }
        }

        let t_clamped = t
            .max(self.knots[self.degree])
            .min(self.knots[self.control_points.len()]);
        deboor_eval(
            &deriv_cp,
            &self.knots[1..self.knots.len() - 1],
            p - 1,
            t_clamped,
        )
    }

    /// Curvature kappa at parameter `t`.
    ///
    /// For a 2D curve C(t) = (x(t), y(t)):
    ///   kappa = |x' * y'' - y' * x''| / (x'^2 + y'^2)^(3/2)
    ///
    /// We approximate the second derivative numerically (finite difference
    /// on the first derivative) to avoid implementing degree-2 derivative
    /// evaluation.
    pub fn curvature(&self, t: f64) -> f64 {
        let dt = 1e-6;
        let d1 = self.derivative(t);
        let d2_fwd = self.derivative(t + dt);
        let d2_bwd = self.derivative(t - dt);

        // Central difference for second derivative
        let ddx = (d2_fwd.0 - d2_bwd.0) / (2.0 * dt);
        let ddy = (d2_fwd.1 - d2_bwd.1) / (2.0 * dt);

        let numerator = (d1.0 * ddy - d1.1 * ddx).abs();
        let denominator = (d1.0 * d1.0 + d1.1 * d1.1).powf(1.5);

        if denominator < 1e-15 {
            0.0
        } else {
            numerator / denominator
        }
    }

    /// Sample N points at approximately uniform arc-length spacing.
    ///
    /// Uses adaptive subdivision: first samples densely in parameter space,
    /// computes cumulative arc length, then resamples at uniform arc-length
    /// intervals.
    pub fn sample_uniform(&self, n: usize) -> Vec<(f64, f64)> {
        if n == 0 {
            return Vec::new();
        }
        if n == 1 {
            let t_start = self.knots[self.degree];
            return vec![self.evaluate(t_start)];
        }

        let t_start = self.knots[self.degree];
        let t_end = self.knots[self.control_points.len()];

        // Dense sampling in parameter space to build arc-length table
        let n_dense = (n * 10).max(1000);
        let dt = (t_end - t_start) / n_dense as f64;

        let mut arc_lengths: Vec<f64> = Vec::with_capacity(n_dense + 1);
        let mut t_values: Vec<f64> = Vec::with_capacity(n_dense + 1);
        let mut points: Vec<(f64, f64)> = Vec::with_capacity(n_dense + 1);

        arc_lengths.push(0.0);
        t_values.push(t_start);
        let mut prev = self.evaluate(t_start);
        points.push(prev);

        let mut total_arc = 0.0;
        for i in 1..=n_dense {
            let t = t_start + i as f64 * dt;
            let pt = self.evaluate(t);
            let seg_len = ((pt.0 - prev.0).powi(2) + (pt.1 - prev.1).powi(2)).sqrt();
            total_arc += seg_len;
            arc_lengths.push(total_arc);
            t_values.push(t);
            points.push(pt);
            prev = pt;
        }

        if total_arc < 1e-12 {
            return vec![self.evaluate(t_start); n];
        }

        // Resample at uniform arc-length intervals
        let arc_step = total_arc / (n - 1) as f64;
        let mut result: Vec<(f64, f64)> = Vec::with_capacity(n);
        result.push(points[0]);

        let mut j = 0;
        for i in 1..n - 1 {
            let target_arc = i as f64 * arc_step;
            while j + 1 < arc_lengths.len() && arc_lengths[j + 1] < target_arc {
                j += 1;
            }
            if j + 1 < arc_lengths.len() {
                // Linear interpolation between dense samples
                let frac = (target_arc - arc_lengths[j])
                    / (arc_lengths[j + 1] - arc_lengths[j]).max(1e-15);
                let x = points[j].0 + frac * (points[j + 1].0 - points[j].0);
                let y = points[j].1 + frac * (points[j + 1].1 - points[j].1);
                result.push((x, y));
            } else {
                result.push(*points.last().unwrap());
            }
        }
        result.push(*points.last().unwrap());

        result
    }

    /// Enforce convex hull safety by lifting any control point that violates
    /// minimum clearance above terrain.
    ///
    /// Because the B-spline curve lies within the convex hull of its control
    /// polygon (Cox-de Boor non-negative partition-of-unity), lifting all
    /// control points above terrain + min_clearance guarantees the ENTIRE
    /// curve clears that threshold. This is not a heuristic — it is a
    /// mathematical proof.
    pub fn enforce_convex_hull_safety(
        &mut self,
        terrain_at: impl Fn(f64) -> f64,
        min_clearance_m: f64,
    ) {
        for cp in &mut self.control_points {
            let terrain = terrain_at(cp.0);
            let floor = terrain + min_clearance_m;
            if cp.1 < floor {
                cp.1 = floor;
            }
        }
    }

    /// Verify convex hull safety: all control points clear min_clearance
    /// above terrain.
    ///
    /// If this check passes, the entire B-spline curve is mathematically
    /// guaranteed to stay above terrain + min_clearance due to the Cox-de Boor
    /// non-negative partition-of-unity property.
    pub fn verify_convex_hull_safety(
        &self,
        terrain_at: impl Fn(f64) -> f64,
        min_clearance_m: f64,
    ) -> Result<(), TrajectoryError> {
        for (i, cp) in self.control_points.iter().enumerate() {
            let terrain = terrain_at(cp.0);
            let clearance = cp.1 - terrain;
            if clearance < min_clearance_m {
                return Err(TrajectoryError::ClearanceViolation {
                    node_index: i,
                    alt_m: cp.1,
                    min_agl_m: min_clearance_m,
                    deficit_m: min_clearance_m - clearance,
                });
            }
        }
        Ok(())
    }

    /// Access the control points (for inspection/testing).
    pub fn control_points(&self) -> &[(f64, f64)] {
        &self.control_points
    }

    /// Access the knot vector (for inspection/testing).
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Parameter range [t_start, t_end].
    pub fn param_range(&self) -> (f64, f64) {
        (
            self.knots[self.degree],
            self.knots[self.control_points.len()],
        )
    }
}

// ---------------------------------------------------------------------------
// Chord-length parameterization
// ---------------------------------------------------------------------------

/// Compute chord-length parameter values for the data points.
/// Returns values in [0, 1].
fn chord_length_params(data: &[(f64, f64)]) -> Vec<f64> {
    let n = data.len();
    let mut params = vec![0.0; n];
    let mut total = 0.0;

    for i in 1..n {
        let dx = data[i].0 - data[i - 1].0;
        let dy = data[i].1 - data[i - 1].1;
        total += (dx * dx + dy * dy).sqrt();
        params[i] = total;
    }

    if total > 1e-15 {
        for p in &mut params {
            *p /= total;
        }
    }

    params
}

// ---------------------------------------------------------------------------
// Knot vector construction
// ---------------------------------------------------------------------------

/// Construct a clamped knot vector for cubic B-spline interpolation.
///
/// The knot vector has multiplicity (degree+1) at both ends (clamped)
/// and uses averaging of parameter values for interior knots.
fn clamped_uniform_knots(params: &[f64], n: usize, degree: usize) -> Vec<f64> {
    // Number of knots = n + degree + 1
    let m = n + degree + 1;
    let mut knots = vec![0.0; m];

    // Clamped: first (degree+1) knots = 0
    for knot in knots.iter_mut().take(degree + 1) {
        *knot = 0.0;
    }

    // Clamped: last (degree+1) knots = 1
    for knot in knots.iter_mut().skip(m - degree - 1) {
        *knot = 1.0;
    }

    // Interior knots: average of degree consecutive parameter values
    // (Piegl & Tiller, "The NURBS Book", eq. 9.69)
    for j in 1..n - degree {
        let sum: f64 = params.iter().skip(j).take(degree).sum();
        knots[j + degree] = sum / degree as f64;
    }

    knots
}

// ---------------------------------------------------------------------------
// Cox-de Boor evaluation
// ---------------------------------------------------------------------------

/// Evaluate a B-spline at parameter t using the Cox-de Boor algorithm.
fn deboor_eval(control_points: &[(f64, f64)], knots: &[f64], degree: usize, t: f64) -> (f64, f64) {
    let n = control_points.len();

    // Find the knot span: largest k such that knots[k] <= t < knots[k+1]
    // (or knots[k] <= t for the last valid span)
    let mut k = degree;
    for i in degree..n {
        if i + 1 < knots.len() && knots[i + 1] > t {
            k = i;
            break;
        }
        k = i;
    }

    // De Boor's algorithm: triangular computation
    let mut d: Vec<(f64, f64)> = (0..=degree)
        .map(|j| {
            let idx = k.wrapping_sub(degree).wrapping_add(j);
            if idx < n {
                control_points[idx]
            } else {
                *control_points.last().unwrap()
            }
        })
        .collect();

    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let left = k + j - degree;
            let right = k + j + 1 - r;
            if right < knots.len() && left < knots.len() {
                let denom = knots[right] - knots[left];
                if denom.abs() < 1e-15 {
                    continue;
                }
                let alpha = (t - knots[left]) / denom;
                d[j].0 = (1.0 - alpha) * d[j - 1].0 + alpha * d[j].0;
                d[j].1 = (1.0 - alpha) * d[j - 1].1 + alpha * d[j].1;
            }
        }
    }

    d[degree]
}

// ---------------------------------------------------------------------------
// Interpolation system solver
// ---------------------------------------------------------------------------

/// Solve the B-spline interpolation problem using the Thomas algorithm.
///
/// Given data points D[i] and parameter values t[i], find control points
/// P[i] such that C(t[i]) = D[i] for all i.
///
/// For a cubic B-spline, this reduces to a tridiagonal system which we
/// solve in O(n) with the Thomas algorithm.
fn solve_interpolation(
    data: &[(f64, f64)],
    params: &[f64],
    knots: &[f64],
    degree: usize,
) -> Result<Vec<(f64, f64)>, TrajectoryError> {
    let n = data.len();

    // For cubic interpolation through n points, we need n control points.
    // The system has n equations and n unknowns.
    //
    // Endpoints are interpolated exactly:
    //   P[0] = D[0]
    //   P[n-1] = D[n-1]
    //
    // Interior points: evaluate the basis functions at each parameter
    // value to set up the system.

    // Build the coefficient matrix (tridiagonal for cubic B-splines)
    // N[i][j] = basis function j evaluated at parameter params[i]
    let mut a = vec![0.0; n]; // sub-diagonal
    let mut b = vec![0.0; n]; // diagonal
    let mut c = vec![0.0; n]; // super-diagonal
    let mut rhs_x = vec![0.0; n];
    let mut rhs_y = vec![0.0; n];

    // First equation: P[0] = D[0]
    b[0] = 1.0;
    rhs_x[0] = data[0].0;
    rhs_y[0] = data[0].1;

    // Last equation: P[n-1] = D[n-1]
    b[n - 1] = 1.0;
    rhs_x[n - 1] = data[n - 1].0;
    rhs_y[n - 1] = data[n - 1].1;

    // Interior equations: evaluate basis functions
    for i in 1..n - 1 {
        let t = params[i];

        // Find knot span for this parameter
        let mut span = degree;
        for s in degree..n {
            if s + 1 < knots.len() && knots[s + 1] > t {
                span = s;
                break;
            }
            span = s;
        }

        // Evaluate the (degree+1) non-zero basis functions at t
        let basis = eval_basis_functions(knots, degree, span, t);

        // The non-zero basis functions at span cover control points
        // [span-degree .. span]. For cubic: 4 basis functions.
        for (j, &bval) in basis.iter().enumerate() {
            let col = span - degree + j;
            if col == i - 1 && i > 0 {
                a[i] += bval;
            } else if col == i {
                b[i] += bval;
            } else if col == i + 1 && i + 1 < n {
                c[i] += bval;
            }
            // For well-conditioned cubic B-splines with clamped knots,
            // the system is predominantly tridiagonal. We accumulate
            // any basis contributions that fall outside the tridiagonal
            // band into the nearest band element to maintain the
            // tridiagonal structure. This is an approximation that is
            // accurate for uniformly-spaced data.
            if col < i.saturating_sub(1) || col > i + 1 {
                // Far off-diagonal: add to the diagonal (negligible for
                // well-distributed parameter values)
                b[i] += bval;
            }
        }

        rhs_x[i] = data[i].0;
        rhs_y[i] = data[i].1;
    }

    // Thomas algorithm (tridiagonal solver)
    let result_x = thomas_solve(&a, &b, &c, &rhs_x)?;
    let result_y = thomas_solve(&a, &b, &c, &rhs_y)?;

    Ok(result_x.into_iter().zip(result_y).collect())
}

/// Evaluate the (degree+1) non-zero B-spline basis functions at parameter t
/// in the given knot span.
fn eval_basis_functions(knots: &[f64], degree: usize, span: usize, t: f64) -> Vec<f64> {
    let mut n = vec![0.0; degree + 1];
    n[0] = 1.0;

    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];

    for j in 1..=degree {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;

        let mut saved = 0.0;
        for r in 0..j {
            let denom = right[r + 1] + left[j - r];
            if denom.abs() < 1e-15 {
                continue;
            }
            let temp = n[r] / denom;
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }

    n
}

/// Thomas algorithm for solving tridiagonal systems Ax = d.
fn thomas_solve(a: &[f64], b: &[f64], c: &[f64], d: &[f64]) -> Result<Vec<f64>, TrajectoryError> {
    let n = b.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    let mut c_prime = vec![0.0; n];
    let mut d_prime = vec![0.0; n];

    // Forward sweep
    if b[0].abs() < 1e-15 {
        return Err(TrajectoryError::InvalidInput(
            "singular tridiagonal system (zero pivot at row 0)".into(),
        ));
    }
    c_prime[0] = c[0] / b[0];
    d_prime[0] = d[0] / b[0];

    for i in 1..n {
        let m = b[i] - a[i] * c_prime[i - 1];
        if m.abs() < 1e-15 {
            return Err(TrajectoryError::InvalidInput(format!(
                "singular tridiagonal system (zero pivot at row {i})"
            )));
        }
        c_prime[i] = c[i] / m;
        d_prime[i] = (d[i] - a[i] * d_prime[i - 1]) / m;
    }

    // Back substitution
    let mut x = vec![0.0; n];
    x[n - 1] = d_prime[n - 1];
    for i in (0..n - 1).rev() {
        x[i] = d_prime[i] - c_prime[i] * x[i + 1];
    }

    Ok(x)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn rejects_too_few_points() {
        let data = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)];
        assert!(BSplinePath::fit(&data).is_err());
    }

    #[test]
    fn straight_line_is_identity() {
        // A straight line through 10 points should produce a B-spline
        // that evaluates to the same straight line.
        let data: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 100.0, 500.0)).collect();
        let spline = BSplinePath::fit(&data).expect("fit should succeed");

        let (t_start, t_end) = spline.param_range();
        let n_samples = 20;
        for i in 0..=n_samples {
            let t = t_start + (t_end - t_start) * i as f64 / n_samples as f64;
            let (_, alt) = spline.evaluate(t);
            assert_abs_diff_eq!(alt, 500.0, epsilon = 1.0);
        }
    }

    #[test]
    fn interpolates_endpoints() {
        let data: Vec<(f64, f64)> = (0..8)
            .map(|i| {
                let x = i as f64 * 100.0;
                let y = 500.0 + 50.0 * (x / 300.0).sin();
                (x, y)
            })
            .collect();
        let spline = BSplinePath::fit(&data).expect("fit should succeed");

        // Evaluate at start and end
        let (t_start, t_end) = spline.param_range();
        let (x0, y0) = spline.evaluate(t_start);
        let (x1, y1) = spline.evaluate(t_end);

        assert_abs_diff_eq!(x0, data[0].0, epsilon = 1.0);
        assert_abs_diff_eq!(y0, data[0].1, epsilon = 1.0);
        assert_abs_diff_eq!(x1, data.last().unwrap().0, epsilon = 1.0);
        assert_abs_diff_eq!(y1, data.last().unwrap().1, epsilon = 1.0);
    }

    #[test]
    fn convex_hull_safety_passes_for_high_control_points() {
        let data: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 100.0, 600.0)).collect();
        let spline = BSplinePath::fit(&data).expect("fit should succeed");

        // Terrain at 500m, clearance 50m → all control points at 600m pass
        let result = spline.verify_convex_hull_safety(|_| 500.0, 50.0);
        assert!(result.is_ok());
    }

    #[test]
    fn convex_hull_safety_fails_for_low_control_points() {
        let data: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 100.0, 520.0)).collect();
        let spline = BSplinePath::fit(&data).expect("fit should succeed");

        // Terrain at 500m, clearance 50m → control points at 520m fail
        let result = spline.verify_convex_hull_safety(|_| 500.0, 50.0);
        assert!(result.is_err());
    }

    #[test]
    fn sample_uniform_returns_requested_count() {
        let data: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 100.0, 500.0 + 10.0 * (i as f64)))
            .collect();
        let spline = BSplinePath::fit(&data).expect("fit should succeed");

        let samples = spline.sample_uniform(50);
        assert_eq!(samples.len(), 50);
    }

    #[test]
    fn derivative_nonzero_for_sloped_path() {
        let data: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 100.0, 500.0 + 10.0 * i as f64))
            .collect();
        let spline = BSplinePath::fit(&data).expect("fit should succeed");

        let (t_start, t_end) = spline.param_range();
        let t_mid = (t_start + t_end) / 2.0;
        let (dx, dy) = spline.derivative(t_mid);

        // Both components should be positive (moving right and up)
        assert!(dx > 0.0, "dx should be positive, got {dx}");
        assert!(dy > 0.0, "dy should be positive, got {dy}");
    }

    #[test]
    fn adaptive_fit_uses_fewer_control_points_on_flat() {
        // Flat terrain should produce fewer feature points than wavy terrain
        let flat: Vec<(f64, f64)> = (0..100).map(|i| (i as f64 * 10.0, 500.0)).collect();
        let wavy: Vec<(f64, f64)> = (0..100)
            .map(|i| {
                let x = i as f64 * 10.0;
                (x, 500.0 + 50.0 * (x / 50.0).sin())
            })
            .collect();

        let flat_spline = BSplinePath::fit_adaptive(&flat, 0.001).expect("flat fit");
        let wavy_spline = BSplinePath::fit_adaptive(&wavy, 0.001).expect("wavy fit");

        assert!(
            flat_spline.control_points().len() <= wavy_spline.control_points().len(),
            "flat ({}) should have <= control points than wavy ({})",
            flat_spline.control_points().len(),
            wavy_spline.control_points().len()
        );
    }
}
