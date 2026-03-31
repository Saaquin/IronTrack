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

// TSP route optimizer for flight line ordering.
//
// After flight lines are generated, the order in which they are flown
// matters. The default left-to-right sequential order is rarely optimal:
// the aircraft wastes fuel flying long repositioning legs between
// non-adjacent lines.
//
// This module models the flight plan as a directed graph where each line
// can be flown in either direction, then applies:
//   1. Nearest-neighbor heuristic from the launch point (baseline).
//   2. 2-opt local improvement (iterative edge swaps).
//
// Edge weights are Karney geodesic distances (NOT Haversine).
//
// In v0.5 (Phase 8B) the distance-based weights will be replaced with
// energy-based weights that account for wind. The graph structure and
// 2-opt logic built here will be reused directly.

use geographiclib_rs::{Geodesic, InverseGeodesic};

use crate::math::numerics::KahanAccumulator;
use crate::types::FlightLine;

/// Maximum number of 2-opt improvement iterations.
const MAX_TWO_OPT_ITERATIONS: usize = 1000;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of route optimization.
pub struct RouteResult {
    /// Indices into the original lines array, in optimized visit order.
    pub order: Vec<usize>,
    /// Whether each line (in visit order) should be flown in reverse.
    pub reversed: Vec<bool>,
    /// Total repositioning distance in metres (sum of ferry legs).
    pub total_reposition_m: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Optimize the visit order and direction of flight lines to minimize
/// total repositioning (ferry) distance.
///
/// Each flight line can be flown in either direction. The optimizer
/// considers both orientations when building the tour.
///
/// `launch_lat` / `launch_lon` are in decimal degrees — the aircraft's
/// starting position (e.g. takeoff point or base).
pub fn optimize_route(lines: &[FlightLine], launch_lat: f64, launch_lon: f64) -> RouteResult {
    // Filter out empty lines that would cause index-out-of-bounds.
    let valid_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.len() >= 2)
        .map(|(i, _)| i)
        .collect();

    let n = valid_indices.len();
    if n == 0 {
        return RouteResult {
            order: vec![],
            reversed: vec![],
            total_reposition_m: 0.0,
        };
    }
    if n == 1 {
        return RouteResult {
            order: vec![valid_indices[0]],
            reversed: vec![false],
            total_reposition_m: 0.0,
        };
    }

    let geod = Geodesic::wgs84();

    let endpoints: Vec<Endpoint> = valid_indices
        .iter()
        .map(|&i| {
            let l = &lines[i];
            let last = l.len() - 1;
            Endpoint {
                start_lat: l.lats()[0].to_degrees(),
                start_lon: l.lons()[0].to_degrees(),
                end_lat: l.lats()[last].to_degrees(),
                end_lon: l.lons()[last].to_degrees(),
            }
        })
        .collect();

    // Phase 1: nearest-neighbor heuristic (produces indices into endpoints[]).
    let (mut local_order, mut reversed) =
        nearest_neighbor(&geod, &endpoints, launch_lat, launch_lon);

    // Phase 2: 2-opt local improvement.
    two_opt(&geod, &endpoints, &mut local_order, &mut reversed);

    // Include launch-to-first-leg distance in total.
    let first_entry = endpoints[local_order[0]].entry(reversed[0]);
    let launch_dist = geo_dist(&geod, launch_lat, launch_lon, first_entry.0, first_entry.1);
    let inter_line = total_reposition(&geod, &endpoints, &local_order, &reversed);
    let total = launch_dist + inter_line;

    // Map local indices back to original line indices.
    let order: Vec<usize> = local_order.iter().map(|&i| valid_indices[i]).collect();

    RouteResult {
        order,
        reversed,
        total_reposition_m: total,
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct Endpoint {
    start_lat: f64,
    start_lon: f64,
    end_lat: f64,
    end_lon: f64,
}

impl Endpoint {
    /// Entry point when flown in the given direction.
    fn entry(&self, reversed: bool) -> (f64, f64) {
        if reversed {
            (self.end_lat, self.end_lon)
        } else {
            (self.start_lat, self.start_lon)
        }
    }

    /// Exit point when flown in the given direction.
    fn exit(&self, reversed: bool) -> (f64, f64) {
        if reversed {
            (self.start_lat, self.start_lon)
        } else {
            (self.end_lat, self.end_lon)
        }
    }
}

// ---------------------------------------------------------------------------
// Geodesic distance helper
// ---------------------------------------------------------------------------

fn geo_dist(geod: &Geodesic, lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    // InverseGeodesic<f64> returns s12 (distance in metres) directly.
    // The 3-tuple form returns (azi1, azi2, a12) — NOT (s12, azi1, azi2).
    let s12: f64 = InverseGeodesic::inverse(geod, lat1, lon1, lat2, lon2);
    s12
}

// ---------------------------------------------------------------------------
// Nearest-neighbor heuristic
// ---------------------------------------------------------------------------

/// Greedy nearest-neighbor tour from the launch point.
///
/// For each unvisited line, evaluates both directions and picks the one
/// whose entry point is closest to the current position (exit of the
/// previously visited line).
fn nearest_neighbor(
    geod: &Geodesic,
    endpoints: &[Endpoint],
    launch_lat: f64,
    launch_lon: f64,
) -> (Vec<usize>, Vec<bool>) {
    let n = endpoints.len();
    let mut visited = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut reversed = Vec::with_capacity(n);

    let mut cur_lat = launch_lat;
    let mut cur_lon = launch_lon;

    for _ in 0..n {
        let mut best_idx = 0;
        let mut best_rev = false;
        let mut best_dist = f64::MAX;

        for (j, ep) in endpoints.iter().enumerate() {
            if visited[j] {
                continue;
            }

            // Forward: enter at start.
            let (elat, elon) = ep.entry(false);
            let d_fwd = geo_dist(geod, cur_lat, cur_lon, elat, elon);

            // Backward: enter at end.
            let (elat_r, elon_r) = ep.entry(true);
            let d_rev = geo_dist(geod, cur_lat, cur_lon, elat_r, elon_r);

            if d_fwd < best_dist {
                best_dist = d_fwd;
                best_idx = j;
                best_rev = false;
            }
            if d_rev < best_dist {
                best_dist = d_rev;
                best_idx = j;
                best_rev = true;
            }
        }

        visited[best_idx] = true;
        order.push(best_idx);
        reversed.push(best_rev);

        let (exit_lat, exit_lon) = endpoints[best_idx].exit(best_rev);
        cur_lat = exit_lat;
        cur_lon = exit_lon;
    }

    (order, reversed)
}

// ---------------------------------------------------------------------------
// 2-opt local improvement
// ---------------------------------------------------------------------------

/// Improve the tour by iteratively reversing sub-segments.
///
/// When a segment [i+1..=j] is reversed, the visit order within that
/// segment is flipped and each line's direction is toggled. The
/// repositioning legs at the segment boundaries are recomputed.
///
/// Terminates after no improvement in a full pass, or after
/// `MAX_TWO_OPT_ITERATIONS` iterations.
fn two_opt(geod: &Geodesic, endpoints: &[Endpoint], order: &mut [usize], reversed: &mut [bool]) {
    let n = order.len();
    if n < 3 {
        return;
    }

    for _ in 0..MAX_TWO_OPT_ITERATIONS {
        let mut improved = false;

        for i in 0..n - 1 {
            for j in i + 2..n {
                let gain = two_opt_gain(geod, endpoints, order, reversed, i, j);
                if gain > 1e-3 {
                    // Reverse the segment [i+1..=j].
                    order[i + 1..=j].reverse();
                    reversed[i + 1..=j].reverse();
                    for rev in reversed.iter_mut().take(j + 1).skip(i + 1) {
                        *rev = !*rev;
                    }
                    improved = true;
                }
            }
        }

        if !improved {
            break;
        }
    }
}

/// Compute the distance reduction from reversing segment [i+1..=j].
///
/// Returns a positive value if the reversal improves the tour.
fn two_opt_gain(
    geod: &Geodesic,
    endpoints: &[Endpoint],
    order: &[usize],
    reversed: &[bool],
    i: usize,
    j: usize,
) -> f64 {
    let n = order.len();

    // Current edges: (i_exit → (i+1)_entry) and (j_exit → (j+1)_entry).
    let i_exit = endpoints[order[i]].exit(reversed[i]);
    let ip1_entry = endpoints[order[i + 1]].entry(reversed[i + 1]);
    let j_exit = endpoints[order[j]].exit(reversed[j]);

    let old_d1 = geo_dist(geod, i_exit.0, i_exit.1, ip1_entry.0, ip1_entry.1);

    let old_d2 = if j + 1 < n {
        let jp1_entry = endpoints[order[j + 1]].entry(reversed[j + 1]);
        geo_dist(geod, j_exit.0, j_exit.1, jp1_entry.0, jp1_entry.1)
    } else {
        0.0
    };

    /*
     * After reversal of [i+1..=j]:
     *   - What was position i+1 is now at position j (reversed direction).
     *   - What was position j is now at position i+1 (reversed direction).
     * New edges:
     *   (i_exit → j_entry_reversed) and ((i+1)_exit_reversed → (j+1)_entry)
     */
    let j_entry_rev = endpoints[order[j]].entry(!reversed[j]);
    let new_d1 = geo_dist(geod, i_exit.0, i_exit.1, j_entry_rev.0, j_entry_rev.1);

    let new_d2 = if j + 1 < n {
        let ip1_exit_rev = endpoints[order[i + 1]].exit(!reversed[i + 1]);
        let jp1_entry = endpoints[order[j + 1]].entry(reversed[j + 1]);
        geo_dist(
            geod,
            ip1_exit_rev.0,
            ip1_exit_rev.1,
            jp1_entry.0,
            jp1_entry.1,
        )
    } else {
        0.0
    };

    (old_d1 + old_d2) - (new_d1 + new_d2)
}

// ---------------------------------------------------------------------------
// Total repositioning distance
// ---------------------------------------------------------------------------

/// Total inter-line repositioning distance using Kahan compensated summation.
fn total_reposition(
    geod: &Geodesic,
    endpoints: &[Endpoint],
    order: &[usize],
    reversed: &[bool],
) -> f64 {
    let mut acc = KahanAccumulator::new();
    for i in 0..order.len() - 1 {
        let exit = endpoints[order[i]].exit(reversed[i]);
        let entry = endpoints[order[i + 1]].entry(reversed[i + 1]);
        let d = geo_dist(geod, exit.0, exit.1, entry.0, entry.1);
        acc.add(d);
    }
    acc.total()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;
    use crate::types::FlightLine;

    /// Build a simple flight line from two lat/lon degree pairs.
    fn make_line(start_lat: f64, start_lon: f64, end_lat: f64, end_lon: f64) -> FlightLine {
        let mut line = FlightLine::new();
        line.push(start_lat.to_radians(), start_lon.to_radians(), 100.0);
        line.push(end_lat.to_radians(), end_lon.to_radians(), 100.0);
        line
    }

    /// 4 parallel N-S lines at 0.001 degree spacing.
    /// Sequential order: 0, 1, 2, 3 (left-to-right).
    /// Optimal: boustrophedon (serpentine) pattern: 0, 1, 2, 3 alternating directions.
    #[test]
    fn four_parallel_lines_serpentine() {
        let lines: Vec<FlightLine> = (0..4)
            .map(|i| {
                let lon = i as f64 * 0.001;
                make_line(51.0, lon, 51.01, lon)
            })
            .collect();

        // Launch from the start of line 0.
        let result = optimize_route(&lines, 51.0, 0.0);

        assert_eq!(result.order.len(), 4);

        // All lines should be visited exactly once.
        let mut visited = result.order.clone();
        visited.sort();
        assert_eq!(visited, vec![0, 1, 2, 3]);

        /*
         * Verify serpentine: adjacent lines in the tour should be
         * geometrically adjacent (not jumping across the grid).
         * The nearest-neighbor heuristic should produce a tour that
         * visits spatially adjacent lines consecutively.
         */
        for i in 0..result.order.len() - 1 {
            let diff = (result.order[i] as i32 - result.order[i + 1] as i32).unsigned_abs();
            assert!(
                diff <= 1,
                "tour should visit adjacent lines: {} → {}",
                result.order[i],
                result.order[i + 1]
            );
        }

        /*
         * Verify the total repositioning distance is short — a serpentine
         * pattern over 4 closely spaced lines should have 3 short ferry
         * legs of ~64m each (0.001 deg longitude at 51°N).
         */
        assert!(
            result.total_reposition_m < 500.0,
            "serpentine reposition distance should be short: {:.0}m",
            result.total_reposition_m
        );
    }

    /// Total repositioning distance must be <= sequential ordering.
    #[test]
    fn optimized_not_worse_than_sequential() {
        let lines: Vec<FlightLine> = (0..4)
            .map(|i| {
                let lon = i as f64 * 0.001;
                make_line(51.0, lon, 51.01, lon)
            })
            .collect();

        let result = optimize_route(&lines, 51.0, 0.0);

        // Compute sequential distance (0→1→2→3, all forward).
        let geod = Geodesic::wgs84();
        let endpoints: Vec<Endpoint> = lines
            .iter()
            .map(|l| {
                let n = l.len();
                Endpoint {
                    start_lat: l.lats()[0].to_degrees(),
                    start_lon: l.lons()[0].to_degrees(),
                    end_lat: l.lats()[n - 1].to_degrees(),
                    end_lon: l.lons()[n - 1].to_degrees(),
                }
            })
            .collect();

        let seq_order: Vec<usize> = (0..4).collect();
        let seq_reversed = vec![false; 4];
        let seq_dist = total_reposition(&geod, &endpoints, &seq_order, &seq_reversed);

        assert!(
            result.total_reposition_m <= seq_dist + 1.0,
            "optimized {:.0}m should be <= sequential {:.0}m",
            result.total_reposition_m,
            seq_dist
        );
    }

    /// Single line: no optimization needed.
    #[test]
    fn single_line_passthrough() {
        let lines = vec![make_line(51.0, 0.0, 51.01, 0.0)];
        let result = optimize_route(&lines, 51.0, 0.0);
        assert_eq!(result.order, vec![0]);
        assert_eq!(result.reversed, vec![false]);
        assert_abs_diff_eq!(result.total_reposition_m, 0.0, epsilon = 1e-6);
    }

    /// Empty plan: no-op.
    #[test]
    fn empty_plan() {
        let result = optimize_route(&[], 51.0, 0.0);
        assert!(result.order.is_empty());
    }

    /// Lines scattered far apart: optimizer should find a short tour.
    #[test]
    fn scattered_lines_shorter_than_worst_case() {
        // 4 lines placed in a diamond pattern.
        let lines = vec![
            make_line(51.0, 0.0, 51.01, 0.0),     // North
            make_line(50.99, 0.01, 51.0, 0.01),   // East
            make_line(50.98, 0.0, 50.99, 0.0),    // South
            make_line(50.99, -0.01, 51.0, -0.01), // West
        ];

        let result = optimize_route(&lines, 51.0, 0.0);
        assert_eq!(result.order.len(), 4);

        // Verify it found a tour (all lines visited).
        let mut visited = result.order.clone();
        visited.sort();
        assert_eq!(visited, vec![0, 1, 2, 3]);

        // Verify total distance is finite.
        assert!(
            result.total_reposition_m.is_finite(),
            "total should be finite: {}",
            result.total_reposition_m
        );
    }
}
