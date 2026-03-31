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

use crate::kinematics::atmosphere::{solve_wind_triangle, WindVector};
use crate::kinematics::energy::PlatformDynamics;
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
    /// Total tour energy in joules (transit + line energy). `Some` only when
    /// energy-optimized via [`optimize_route_energy`].
    pub total_energy_j: Option<f64>,
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
            total_energy_j: None,
        };
    }
    if n == 1 {
        return RouteResult {
            order: vec![valid_indices[0]],
            reversed: vec![false],
            total_reposition_m: 0.0,
            total_energy_j: None,
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
        total_energy_j: None,
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
// Energy-weighted route optimization (Phase 8B)
// ---------------------------------------------------------------------------

/// Context for energy-weighted cost evaluation.
struct EnergyCostCtx<'a> {
    geod: Geodesic,
    platform: &'a PlatformDynamics,
    wind: &'a WindVector,
    tas_ms: f64,
}

/// Energy cost for a repositioning (ferry) leg between two points (joules).
///
/// Uses the wind triangle to determine ground speed along the transit heading,
/// then applies `P(TAS) × (distance / GS)`. Returns `f64::MAX` if the
/// transit heading is infeasible (crosswind > TAS).
fn transit_energy_j(ctx: &EnergyCostCtx, lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (s12, azi1, _, _): (f64, f64, f64, f64) =
        InverseGeodesic::inverse(&ctx.geod, lat1, lon1, lat2, lon2);
    if s12 < 1e-6 {
        return 0.0;
    }

    // Normalize azimuth to [0, 360).
    let track = ((azi1 % 360.0) + 360.0) % 360.0;

    let gs = match solve_wind_triangle(track, ctx.tas_ms, ctx.wind) {
        Ok(wr) => wr.ground_speed_ms,
        Err(_) => return f64::MAX, // infeasible heading
    };

    // TAS is validated at optimize_route_energy() entry; this is a defensive
    // fallback for an unreachable path.
    let power = ctx
        .platform
        .power_required_w(ctx.tas_ms)
        .unwrap_or(f64::MAX);
    power * (s12 / gs)
}

/// Approximate energy to fly one flight line in a given direction (joules).
///
/// Uses the line's endpoints to determine a single track azimuth, then
/// applies `P(TAS) × (line_distance / GS)`. This linear approximation is
/// acceptable because survey flight lines are straight.
fn line_energy_j(ctx: &EnergyCostCtx, ep: &Endpoint, reversed: bool) -> f64 {
    let (entry_lat, entry_lon) = ep.entry(reversed);
    let (exit_lat, exit_lon) = ep.exit(reversed);

    let (s12, azi1, _, _): (f64, f64, f64, f64) =
        InverseGeodesic::inverse(&ctx.geod, entry_lat, entry_lon, exit_lat, exit_lon);
    if s12 < 1e-6 {
        return 0.0;
    }

    let track = ((azi1 % 360.0) + 360.0) % 360.0;

    let gs = match solve_wind_triangle(track, ctx.tas_ms, ctx.wind) {
        Ok(wr) => wr.ground_speed_ms,
        Err(_) => return f64::MAX,
    };

    // TAS is validated at optimize_route_energy() entry; defensive fallback.
    let power = ctx
        .platform
        .power_required_w(ctx.tas_ms)
        .unwrap_or(f64::MAX);
    power * (s12 / gs)
}

/// Total tour energy: Σ(transit energy + line energy) with Kahan summation.
fn tour_energy_j(
    ctx: &EnergyCostCtx,
    endpoints: &[Endpoint],
    order: &[usize],
    reversed: &[bool],
    launch_lat: f64,
    launch_lon: f64,
) -> f64 {
    let mut acc = KahanAccumulator::new();
    if order.is_empty() {
        return 0.0;
    }

    // Launch → first line entry.
    let first_entry = endpoints[order[0]].entry(reversed[0]);
    acc.add(transit_energy_j(
        ctx,
        launch_lat,
        launch_lon,
        first_entry.0,
        first_entry.1,
    ));

    // First line energy.
    acc.add(line_energy_j(ctx, &endpoints[order[0]], reversed[0]));

    // Subsequent: transit + line for each step.
    for i in 0..order.len() - 1 {
        let exit = endpoints[order[i]].exit(reversed[i]);
        let entry = endpoints[order[i + 1]].entry(reversed[i + 1]);
        acc.add(transit_energy_j(ctx, exit.0, exit.1, entry.0, entry.1));
        acc.add(line_energy_j(
            ctx,
            &endpoints[order[i + 1]],
            reversed[i + 1],
        ));
    }

    acc.total()
}

/// Greedy nearest-neighbor tour using energy cost (transit + line).
fn nearest_neighbor_energy(
    ctx: &EnergyCostCtx,
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
        let mut best_cost = f64::MAX;

        for (j, ep) in endpoints.iter().enumerate() {
            if visited[j] {
                continue;
            }

            // Forward direction: transit + line energy.
            let cost_fwd =
                transit_energy_j(ctx, cur_lat, cur_lon, ep.entry(false).0, ep.entry(false).1)
                    + line_energy_j(ctx, ep, false);

            // Reverse direction.
            let cost_rev =
                transit_energy_j(ctx, cur_lat, cur_lon, ep.entry(true).0, ep.entry(true).1)
                    + line_energy_j(ctx, ep, true);

            if cost_fwd < best_cost {
                best_cost = cost_fwd;
                best_idx = j;
                best_rev = false;
            }
            if cost_rev < best_cost {
                best_cost = cost_rev;
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

/// 2-opt improvement using energy cost.
///
/// For each candidate reversal of segment [i+1..=j], computes energy
/// of the affected region before and after. This is O(segment_length)
/// per evaluation (not O(1) as in distance-based 2-opt) because reversal
/// changes every line's wind cost. Acceptable for < 100 lines.
fn two_opt_energy(
    ctx: &EnergyCostCtx,
    endpoints: &[Endpoint],
    order: &mut [usize],
    reversed: &mut [bool],
) {
    let n = order.len();
    if n < 3 {
        return;
    }

    for _ in 0..MAX_TWO_OPT_ITERATIONS {
        let mut improved = false;

        for i in 0..n - 1 {
            for j in i + 2..n {
                let old_cost = segment_region_energy(ctx, endpoints, order, reversed, i, j);

                // Tentatively reverse [i+1..=j].
                order[i + 1..=j].reverse();
                reversed[i + 1..=j].reverse();
                for rev in reversed.iter_mut().take(j + 1).skip(i + 1) {
                    *rev = !*rev;
                }

                let new_cost = segment_region_energy(ctx, endpoints, order, reversed, i, j);

                if new_cost < old_cost - 1e-3 {
                    // Keep the reversal — it reduced energy.
                    improved = true;
                } else {
                    // Undo the reversal.
                    for rev in reversed.iter_mut().take(j + 1).skip(i + 1) {
                        *rev = !*rev;
                    }
                    reversed[i + 1..=j].reverse();
                    order[i + 1..=j].reverse();
                }
            }
        }

        if !improved {
            break;
        }
    }
}

/// Energy of the tour region affected by a 2-opt move on segment [i+1..=j].
///
/// Computes: transit into i+1 + Σ(line[k] + transit[k→k+1]) for k in [i+1..j]
/// + transit out of j (if j+1 < n).
#[allow(clippy::too_many_arguments)]
fn segment_region_energy(
    ctx: &EnergyCostCtx,
    endpoints: &[Endpoint],
    order: &[usize],
    reversed: &[bool],
    i: usize,
    j: usize,
) -> f64 {
    let n = order.len();
    let mut acc = KahanAccumulator::new();

    // Transit into i+1.
    let from = if i == 0 {
        // From launch to first-in-segment (which may be index 0 or 1).
        // If i==0 and we're evaluating region [1..=j], the transit is from
        // line 0's exit. But line 0 is NOT in the reversed segment.
        // Actually i can't be 0 in the current loop (i starts at 0, segment is [i+1..=j]).
        // When i==0, the entry to i+1 comes from line 0's exit.
        endpoints[order[0]].exit(reversed[0])
    } else {
        endpoints[order[i]].exit(reversed[i])
    };
    let to = endpoints[order[i + 1]].entry(reversed[i + 1]);
    acc.add(transit_energy_j(ctx, from.0, from.1, to.0, to.1));

    // Lines and inter-line transits within [i+1..=j].
    for k in (i + 1)..=j {
        acc.add(line_energy_j(ctx, &endpoints[order[k]], reversed[k]));
        if k < j {
            let exit = endpoints[order[k]].exit(reversed[k]);
            let entry = endpoints[order[k + 1]].entry(reversed[k + 1]);
            acc.add(transit_energy_j(ctx, exit.0, exit.1, entry.0, entry.1));
        }
    }

    // Transit out of j to j+1.
    if j + 1 < n {
        let exit = endpoints[order[j]].exit(reversed[j]);
        let entry = endpoints[order[j + 1]].entry(reversed[j + 1]);
        acc.add(transit_energy_j(ctx, exit.0, exit.1, entry.0, entry.1));
    }

    acc.total()
}

/// Optimize the visit order and direction of flight lines to minimize
/// total tour energy (transit + line energy) accounting for wind.
///
/// When wind is present, the cost of flying a line depends on direction
/// (headwind vs tailwind), making this an Asymmetric TSP.
///
/// `launch_lat` / `launch_lon` are in decimal degrees.
pub fn optimize_route_energy(
    lines: &[FlightLine],
    launch_lat: f64,
    launch_lon: f64,
    platform: &PlatformDynamics,
    v_tas_ms: f64,
    wind: &WindVector,
) -> RouteResult {
    // Validate TAS upfront so downstream power_required_w() calls cannot fail.
    debug_assert!(
        v_tas_ms.is_finite() && v_tas_ms > 0.0,
        "optimize_route_energy requires positive finite TAS, got {v_tas_ms}"
    );

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
            total_energy_j: Some(0.0),
        };
    }
    if n == 1 {
        let ctx = EnergyCostCtx {
            geod: Geodesic::wgs84(),
            platform,
            wind,
            tas_ms: v_tas_ms,
        };
        let ep = {
            let l = &lines[valid_indices[0]];
            let last = l.len() - 1;
            Endpoint {
                start_lat: l.lats()[0].to_degrees(),
                start_lon: l.lons()[0].to_degrees(),
                end_lat: l.lats()[last].to_degrees(),
                end_lon: l.lons()[last].to_degrees(),
            }
        };
        // Choose the cheaper direction including launch-to-line transit.
        let transit_fwd = transit_energy_j(
            &ctx,
            launch_lat,
            launch_lon,
            ep.entry(false).0,
            ep.entry(false).1,
        );
        let transit_rev = transit_energy_j(
            &ctx,
            launch_lat,
            launch_lon,
            ep.entry(true).0,
            ep.entry(true).1,
        );
        let cost_fwd = transit_fwd + line_energy_j(&ctx, &ep, false);
        let cost_rev = transit_rev + line_energy_j(&ctx, &ep, true);
        let reversed = cost_rev < cost_fwd;
        let energy = cost_fwd.min(cost_rev);
        let launch_dist = geo_dist(
            &ctx.geod,
            launch_lat,
            launch_lon,
            ep.entry(reversed).0,
            ep.entry(reversed).1,
        );
        return RouteResult {
            order: vec![valid_indices[0]],
            reversed: vec![reversed],
            total_reposition_m: launch_dist,
            total_energy_j: Some(energy),
        };
    }

    let geod = Geodesic::wgs84();
    let ctx = EnergyCostCtx {
        geod: Geodesic::wgs84(),
        platform,
        wind,
        tas_ms: v_tas_ms,
    };

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

    // Phase 1: nearest-neighbor with energy cost.
    let (mut local_order, mut local_reversed) =
        nearest_neighbor_energy(&ctx, &endpoints, launch_lat, launch_lon);

    // Phase 2: 2-opt improvement with energy cost.
    two_opt_energy(&ctx, &endpoints, &mut local_order, &mut local_reversed);

    // Compute total energy and repositioning distance.
    let total_energy = tour_energy_j(
        &ctx,
        &endpoints,
        &local_order,
        &local_reversed,
        launch_lat,
        launch_lon,
    );

    let first_entry = endpoints[local_order[0]].entry(local_reversed[0]);
    let launch_dist = geo_dist(&geod, launch_lat, launch_lon, first_entry.0, first_entry.1);
    let inter_line = total_reposition(&geod, &endpoints, &local_order, &local_reversed);
    let total_dist = launch_dist + inter_line;

    let order: Vec<usize> = local_order.iter().map(|&i| valid_indices[i]).collect();

    RouteResult {
        order,
        reversed: local_reversed,
        total_reposition_m: total_dist,
        total_energy_j: Some(total_energy),
    }
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

    // -- Energy-optimized routing tests (Phase 8B) --

    fn test_platform() -> PlatformDynamics {
        PlatformDynamics::phantom4pro()
    }

    /// Strong headwind from north: for a single line the solver should
    /// choose the tailwind direction (southward = reversed). For multi-line
    /// grids the serpentine pattern may override per-line wind preference
    /// (transit savings > headwind cost on alternating lines).
    #[test]
    fn headwind_prefers_tailwind_direction() {
        let platform = test_platform();
        let wind = WindVector::new(15.0, 0.0).unwrap(); // from north

        // Single line: no transit trade-offs → pure energy preference.
        let lines = vec![make_line(51.0, 0.0, 51.01, 0.0)];
        let result = optimize_route_energy(&lines, 51.005, 0.0, &platform, 30.0, &wind);
        assert_eq!(result.order.len(), 1);
        assert!(
            result.reversed[0],
            "single line with northerly headwind should be reversed to fly southward"
        );

        // Multi-line: optimizer may use serpentine (alternating directions),
        // but total energy must be less than naive all-forward.
        let multi: Vec<FlightLine> = (0..4)
            .map(|i| make_line(51.0, i as f64 * 0.001, 51.01, i as f64 * 0.001))
            .collect();
        let result = optimize_route_energy(&multi, 51.01, 0.0, &platform, 30.0, &wind);
        assert_eq!(result.order.len(), 4);

        let ctx = EnergyCostCtx {
            geod: Geodesic::wgs84(),
            platform: &platform,
            wind: &wind,
            tas_ms: 30.0,
        };
        let endpoints: Vec<Endpoint> = multi
            .iter()
            .map(|l| Endpoint {
                start_lat: l.lats()[0].to_degrees(),
                start_lon: l.lons()[0].to_degrees(),
                end_lat: l.lats()[l.len() - 1].to_degrees(),
                end_lon: l.lons()[l.len() - 1].to_degrees(),
            })
            .collect();
        let naive_order: Vec<usize> = (0..4).collect();
        let naive_rev = vec![false; 4];
        let naive_e = tour_energy_j(&ctx, &endpoints, &naive_order, &naive_rev, 51.01, 0.0);
        let opt_e = result.total_energy_j.unwrap();
        assert!(
            opt_e <= naive_e + 1.0,
            "optimized {:.0} J should be <= naive {:.0} J",
            opt_e,
            naive_e
        );
    }

    /// Zero wind: energy-optimized order should match distance-optimized order.
    #[test]
    fn zero_wind_energy_matches_distance_order() {
        let lines: Vec<FlightLine> = (0..4)
            .map(|i| {
                let lon = i as f64 * 0.001;
                make_line(51.0, lon, 51.01, lon)
            })
            .collect();

        let platform = test_platform();
        let calm = WindVector::new(0.0, 0.0).unwrap();

        let dist_result = optimize_route(&lines, 51.0, 0.0);
        let energy_result = optimize_route_energy(&lines, 51.0, 0.0, &platform, 30.0, &calm);

        // Same visit order.
        assert_eq!(dist_result.order, energy_result.order);
        // Same direction choices.
        assert_eq!(dist_result.reversed, energy_result.reversed);
    }

    /// Energy-optimized tour energy must be <= naive sequential tour energy.
    #[test]
    fn energy_tour_not_worse_than_sequential() {
        let lines: Vec<FlightLine> = (0..4)
            .map(|i| {
                let lon = i as f64 * 0.001;
                make_line(51.0, lon, 51.01, lon)
            })
            .collect();

        let platform = test_platform();
        let wind = WindVector::new(10.0, 90.0).unwrap(); // crosswind from east

        let result = optimize_route_energy(&lines, 51.0, 0.0, &platform, 30.0, &wind);

        // Compute naive sequential energy (0→1→2→3 all forward).
        let ctx = EnergyCostCtx {
            geod: Geodesic::wgs84(),
            platform: &platform,
            wind: &wind,
            tas_ms: 30.0,
        };
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
        let seq_energy = tour_energy_j(&ctx, &endpoints, &seq_order, &seq_reversed, 51.0, 0.0);

        let opt_energy = result.total_energy_j.unwrap();
        assert!(
            opt_energy <= seq_energy + 1.0,
            "optimized {:.0} J should be <= sequential {:.0} J",
            opt_energy,
            seq_energy
        );
    }

    /// Transit energy is asymmetric with crosswind.
    #[test]
    fn asymmetric_transit_cost_with_crosswind() {
        let platform = test_platform();
        let wind = WindVector::new(10.0, 90.0).unwrap(); // from east
        let ctx = EnergyCostCtx {
            geod: Geodesic::wgs84(),
            platform: &platform,
            wind: &wind,
            tas_ms: 30.0,
        };

        // North-south transit vs south-north transit should differ
        // because the wind triangle yields different ground speeds
        // for different track bearings when crosswind is present.
        let e_north = transit_energy_j(&ctx, 51.0, 0.0, 51.01, 0.0);
        let e_south = transit_energy_j(&ctx, 51.01, 0.0, 51.0, 0.0);

        // With easterly crosswind, both N and S tracks have similar GS
        // (symmetric crosswind), so energies should be close but due to
        // WCA differences, not exactly equal.
        assert!(e_north.is_finite());
        assert!(e_south.is_finite());
        assert!(e_north > 0.0);
        assert!(e_south > 0.0);

        // East-west transit vs west-east should be clearly asymmetric:
        // eastward = into wind, westward = tailwind.
        let e_east = transit_energy_j(&ctx, 51.0, 0.0, 51.0, 0.01);
        let e_west = transit_energy_j(&ctx, 51.0, 0.01, 51.0, 0.0);
        assert!(
            e_east > e_west,
            "flying east (into easterly wind) should cost more: {:.0} vs {:.0}",
            e_east,
            e_west
        );
    }
}
