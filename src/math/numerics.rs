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
 * Numerical utilities: compensated summation and precision-critical arithmetic.
 *
 * Kahan compensated summation maintains O(eps) error independent of array
 * length, versus O(n * eps) for naive summation. This is critical for
 * distance/trajectory accumulations over hundreds of kilometres where
 * naive summation can drift by millimetres.
 *
 * Reference: docs/formula_reference.md -- Kahan Compensated Summation.
 */

/// Kahan compensated summation over a slice of f64 values.
///
/// Error bound: O(machine epsilon) independent of slice length.
/// Naive summation error grows as O(n * machine epsilon).
///
/// ```
/// # use irontrack::math::numerics::kahan_sum;
/// let values = vec![1e-8; 100_000_000];
/// let result = kahan_sum(&values);
/// assert!((result - 1.0).abs() < 1e-10);
/// ```
pub fn kahan_sum(values: &[f64]) -> f64 {
    let mut acc = KahanAccumulator::new();
    for &v in values {
        acc.add(v);
    }
    acc.total()
}

/// Stateful Kahan compensated accumulator for use in loops where values
/// arrive one at a time.
///
/// Preferred over `kahan_sum` when the values are computed inside the loop
/// body (e.g. geodesic distances, waypoint intervals) rather than being
/// available as a pre-existing slice.
///
/// ```
/// # use irontrack::math::numerics::KahanAccumulator;
/// let mut acc = KahanAccumulator::new();
/// for _ in 0..100_000_000 {
///     acc.add(1e-8);
/// }
/// assert!((acc.total() - 1.0).abs() < 1e-10);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct KahanAccumulator {
    sum: f64,
    compensation: f64,
}

impl KahanAccumulator {
    /// Create a new accumulator starting at zero.
    pub fn new() -> Self {
        Self {
            sum: 0.0,
            compensation: 0.0,
        }
    }

    /// Add a value with compensation for previously lost low-order bits.
    #[inline]
    pub fn add(&mut self, value: f64) {
        let y = value - self.compensation;
        let t = self.sum + y;
        self.compensation = (t - self.sum) - y;
        self.sum = t;
    }

    /// Return the accumulated sum.
    #[inline]
    pub fn total(&self) -> f64 {
        self.sum
    }
}

impl Default for KahanAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kahan_sum_empty_is_zero() {
        assert_eq!(kahan_sum(&[]), 0.0);
    }

    #[test]
    fn kahan_sum_single_value() {
        assert_eq!(kahan_sum(&[42.0]), 42.0);
    }

    #[test]
    fn kahan_sum_exact_for_small_arrays() {
        let values = vec![0.1, 0.2, 0.3];
        let result = kahan_sum(&values);
        // Kahan should give a result very close to 0.6
        assert!((result - 0.6).abs() < 1e-15);
    }

    #[test]
    fn kahan_accumulator_beats_naive_on_many_small_values() {
        let n = 10_000_000;
        let small = 1e-8;
        let expected = n as f64 * small; // 0.1

        // Naive summation
        let mut naive = 0.0_f64;
        for _ in 0..n {
            naive += small;
        }

        // Kahan summation
        let mut acc = KahanAccumulator::new();
        for _ in 0..n {
            acc.add(small);
        }

        let naive_err = (naive - expected).abs();
        let kahan_err = (acc.total() - expected).abs();

        // Kahan error should be orders of magnitude smaller than naive
        assert!(
            kahan_err < naive_err,
            "Kahan error ({kahan_err:.2e}) should be less than naive error ({naive_err:.2e})"
        );
        // Kahan should be within a few ULPs of the exact answer
        assert!(kahan_err < 1e-14, "Kahan error {kahan_err:.2e} too large");
    }

    #[test]
    fn kahan_accumulator_matches_kahan_sum() {
        let values: Vec<f64> = (0..1000).map(|i| (i as f64) * 0.001).collect();
        let sum_result = kahan_sum(&values);

        let mut acc = KahanAccumulator::new();
        for &v in &values {
            acc.add(v);
        }

        assert_eq!(sum_result, acc.total());
    }

    #[test]
    fn kahan_accumulator_default_is_zero() {
        let acc = KahanAccumulator::default();
        assert_eq!(acc.total(), 0.0);
    }
}
