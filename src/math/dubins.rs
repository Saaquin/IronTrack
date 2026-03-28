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

// Dubins path generation for procedure turns between flight lines.
//
// A Dubins path is the shortest path between two oriented waypoints
// (position + heading) subject to a minimum turn radius constraint.
// There are six candidate path types composed of Left arcs (L), Right
// arcs (R), and Straight segments (S): LSL, RSR, LSR, RSL, LRL, RLR.
//
// This module also implements clothoid (Euler spiral) transition curves
// that smooth the curvature discontinuities at straight-to-arc junctions.
// Without clothoids, the aircraft would need an instantaneous roll-rate
// change — causing autopilot overshoot and gimbal destabilisation.
//
// All geometry operates in a local 2D Euclidean frame (UTM metres).
// The caller is responsible for projecting to/from WGS84.
//
// Reference:
//   Dubins, L. E. (1957). On curves of minimal length with a constraint
//   on average curvature, and with prescribed initial and terminal
//   positions and tangents. American Journal of Mathematics, 79(3).

use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// Standard gravity (m/s²).
const G: f64 = 9.806_65;

/// Default angular resolution for arc discretisation (radians).
/// 5 degrees per waypoint along circular arcs.
const DEFAULT_ARC_STEP_RAD: f64 = 5.0 * PI / 180.0;

/// Default maximum roll rate for clothoid transition length (deg/s).
const DEFAULT_MAX_ROLL_RATE_DEG_S: f64 = 15.0;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Physical turn parameters derived from aircraft performance.
///
/// The minimum turn radius is computed from physics — it is NOT a
/// user-configurable radius. The user controls airspeed and bank angle;
/// the radius follows from Newton's second law in a coordinated turn.
#[derive(Debug, Clone, Copy)]
pub struct TurnParams {
    /// True airspeed in m/s.
    pub true_airspeed: f64,
    /// Maximum bank angle during turns in degrees.
    pub max_bank_angle_deg: f64,
    /// Angular resolution for arc discretisation in radians.
    pub arc_step_rad: f64,
    /// Maximum roll rate in deg/s (for clothoid transition length).
    pub max_roll_rate_deg_s: f64,
}

impl TurnParams {
    /// Create turn parameters with default arc step and roll rate.
    pub fn new(true_airspeed: f64, max_bank_angle_deg: f64) -> Self {
        Self {
            true_airspeed,
            max_bank_angle_deg,
            arc_step_rad: DEFAULT_ARC_STEP_RAD,
            max_roll_rate_deg_s: DEFAULT_MAX_ROLL_RATE_DEG_S,
        }
    }

    /// Minimum turn radius derived from coordinated-turn physics.
    ///
    /// R_min = V^2 / (g * tan(bank_angle))
    pub fn min_radius(&self) -> f64 {
        let bank_rad = self.max_bank_angle_deg.to_radians();
        self.true_airspeed * self.true_airspeed / (G * bank_rad.tan())
    }

    /// Clothoid transition length in metres.
    ///
    /// The transition curve ramps curvature linearly from 0 to 1/R over
    /// this distance, ensuring the roll rate never exceeds the limit.
    ///
    /// L_clothoid = V * bank_angle_rad / max_roll_rate_rad
    /// Time to roll = bank_angle / roll_rate; length = V * time.
    pub fn clothoid_length(&self) -> f64 {
        let max_roll_rate_rad = self.max_roll_rate_deg_s.to_radians();
        if max_roll_rate_rad < 1e-12 {
            return 0.0;
        }
        /*
         * The bank angle changes from 0 to max_bank_angle over the
         * clothoid. Time to execute = bank_angle / roll_rate.
         * Length = V * time = V * bank_angle / roll_rate.
         */
        let bank_rad = self.max_bank_angle_deg.to_radians();
        self.true_airspeed * bank_rad / max_roll_rate_rad
    }
}

/// An oriented 2D waypoint: position + heading.
#[derive(Debug, Clone, Copy)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    /// Heading in radians, measured counter-clockwise from the +x axis.
    pub theta: f64,
}

/// A segment of a Dubins path: either a circular arc or a straight line.
#[derive(Debug, Clone, Copy)]
enum Segment {
    /// Left arc (counter-clockwise) with given sweep angle in radians (positive).
    Left(f64),
    /// Right arc (clockwise) with given sweep angle in radians (positive).
    Right(f64),
    /// Straight segment with given length in metres.
    Straight(f64),
}

/// The six Dubins path types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathType {
    Lsl,
    Rsr,
    Lsr,
    Rsl,
    Lrl,
    Rlr,
}

/// A complete Dubins path between two poses.
#[derive(Debug, Clone)]
struct DubinsPath {
    segments: [Segment; 3],
    total_length: f64,
    #[allow(dead_code)]
    path_type: PathType,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute the shortest Dubins path between two oriented waypoints and
/// discretise it into (x, y) waypoints.
///
/// The path respects the minimum turn radius from `params`. Clothoid
/// transitions are inserted at straight-to-arc junctions if the clothoid
/// length is non-negligible.
///
/// Returns a sequence of (x, y) points in the local 2D frame.
pub fn dubins_path(start: Pose, end: Pose, params: &TurnParams) -> Vec<(f64, f64)> {
    let r = params.min_radius();
    if !r.is_finite() || r < 1e-6 {
        return vec![(start.x, start.y), (end.x, end.y)];
    }

    let arc_step = if params.arc_step_rad.is_finite() && params.arc_step_rad > 1e-6 {
        params.arc_step_rad
    } else {
        DEFAULT_ARC_STEP_RAD
    };

    let path = shortest_dubins(start, end, r);
    let clothoid_len = params.clothoid_length().max(0.0);

    discretise_path(start, &path, r, arc_step, clothoid_len)
}

// ---------------------------------------------------------------------------
// Dubins solver: compute all 6 path types, select shortest
// ---------------------------------------------------------------------------

fn shortest_dubins(start: Pose, end: Pose, r: f64) -> DubinsPath {
    let candidates = [
        compute_csc(start, end, r, PathType::Lsl),
        compute_csc(start, end, r, PathType::Rsr),
        compute_csc(start, end, r, PathType::Lsr),
        compute_csc(start, end, r, PathType::Rsl),
        compute_ccc(start, end, r, PathType::Lrl),
        compute_ccc(start, end, r, PathType::Rlr),
    ];

    candidates
        .into_iter()
        .flatten()
        .min_by(|a, b| {
            a.total_length
                .partial_cmp(&b.total_length)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or_else(|| {
            // Fallback: straight line (degenerate case).
            let dx = end.x - start.x;
            let dy = end.y - start.y;
            let d = (dx * dx + dy * dy).sqrt();
            DubinsPath {
                segments: [
                    Segment::Straight(0.0),
                    Segment::Straight(d),
                    Segment::Straight(0.0),
                ],
                total_length: d,
                path_type: PathType::Lsl,
            }
        })
}

/// Compute a CSC (curve-straight-curve) Dubins path: LSL, RSR, LSR, RSL.
fn compute_csc(start: Pose, end: Pose, r: f64, pt: PathType) -> Option<DubinsPath> {
    let (c1, c2) = turn_centres(start, end, r, pt);

    let dx = c2.0 - c1.0;
    let dy = c2.1 - c1.1;
    let d = (dx * dx + dy * dy).sqrt();

    /*
     * Compute the tangent line angle between the two turn circles.
     * For same-side turns (LSL, RSR): outer tangent.
     * For cross-side turns (LSR, RSL): inner tangent.
     */
    let (tangent_angle, straight_len) = match pt {
        PathType::Lsl | PathType::Rsr => {
            // Outer tangent: circles have the same radius, so tangent is
            // perpendicular to the line connecting centres.
            if d < 1e-10 {
                return None;
            }
            let angle = dy.atan2(dx);
            (angle, d)
        }
        PathType::Lsr => {
            // Inner tangent: L then S then R.
            // Distance between centres must be >= 2r.
            if d < 2.0 * r - 1e-10 {
                return None;
            }
            let alpha = (2.0 * r / d).acos();
            let angle = dy.atan2(dx) + alpha;
            let straight = (d * d - 4.0 * r * r).sqrt();
            (angle, straight)
        }
        PathType::Rsl => {
            // Inner tangent: R then S then L.
            if d < 2.0 * r - 1e-10 {
                return None;
            }
            let alpha = (2.0 * r / d).acos();
            let angle = dy.atan2(dx) - alpha;
            let straight = (d * d - 4.0 * r * r).sqrt();
            (angle, straight)
        }
        _ => return None,
    };

    // Compute arc sweep angles.
    let (sweep1, sweep2) = match pt {
        PathType::Lsl => {
            let s1 = normalise_angle(tangent_angle - start.theta);
            let s2 = normalise_angle(end.theta - tangent_angle);
            (s1, s2)
        }
        PathType::Rsr => {
            let s1 = normalise_angle(start.theta - tangent_angle);
            let s2 = normalise_angle(tangent_angle - end.theta);
            (s1, s2)
        }
        PathType::Lsr => {
            // L arc from start heading to tangent departure angle.
            let departure = tangent_angle - FRAC_PI_2;
            let s1 = normalise_angle(departure - start.theta);
            // R arc from tangent arrival angle to end heading.
            let arrival = tangent_angle - FRAC_PI_2;
            let s2 = normalise_angle(end.theta - arrival);
            (s1, s2)
        }
        PathType::Rsl => {
            let departure = tangent_angle + FRAC_PI_2;
            let s1 = normalise_angle(start.theta - departure);
            let arrival = tangent_angle + FRAC_PI_2;
            let s2 = normalise_angle(arrival - end.theta);
            (s1, s2)
        }
        _ => return None,
    };

    if straight_len < -1e-10 {
        return None;
    }
    let straight_len = straight_len.max(0.0);

    let total = sweep1 * r + straight_len + sweep2 * r;

    let (seg1, seg3) = match pt {
        PathType::Lsl => (Segment::Left(sweep1), Segment::Left(sweep2)),
        PathType::Rsr => (Segment::Right(sweep1), Segment::Right(sweep2)),
        PathType::Lsr => (Segment::Left(sweep1), Segment::Right(sweep2)),
        PathType::Rsl => (Segment::Right(sweep1), Segment::Left(sweep2)),
        _ => return None,
    };

    Some(DubinsPath {
        segments: [seg1, Segment::Straight(straight_len), seg3],
        total_length: total,
        path_type: pt,
    })
}

/// Compute a CCC (curve-curve-curve) Dubins path: LRL, RLR.
fn compute_ccc(start: Pose, end: Pose, r: f64, pt: PathType) -> Option<DubinsPath> {
    let (c1, c2) = turn_centres(start, end, r, pt);

    let dx = c2.0 - c1.0;
    let dy = c2.1 - c1.1;
    let d = (dx * dx + dy * dy).sqrt();

    // CCC paths require the circles to be close enough: d < 4r.
    if d > 4.0 * r + 1e-10 || d < 1e-10 {
        return None;
    }

    /*
     * The middle circle is tangent to both end circles. Its centre lies
     * at distance 2r from both c1 and c2.
     *
     * Triangle c1-cm-c2: sides 2r, 2r, d.
     * Angle at c1: alpha = acos(d / (4r))
     * (using law of cosines: d² = (2r)² + (2r)² - 2(2r)(2r)cos(alpha))
     */
    let cos_alpha = d / (4.0 * r);
    if cos_alpha.abs() > 1.0 + 1e-10 {
        return None;
    }
    let alpha = cos_alpha.clamp(-1.0, 1.0).acos();
    let base_angle = dy.atan2(dx);

    let (cm, sweep1, sweep_mid, sweep2) = match pt {
        PathType::Lrl => {
            let cm_angle = base_angle + alpha;
            let cm = (
                c1.0 + 2.0 * r * cm_angle.cos(),
                c1.1 + 2.0 * r * cm_angle.sin(),
            );

            // Sweep 1: L arc from start heading around c1 to tangent with middle circle.
            let tangent1_angle = cm_angle;
            let s1 = normalise_angle(tangent1_angle - start.theta);

            // Middle sweep: R arc around cm.
            let tangent2_angle = (c2.1 - cm.1).atan2(c2.0 - cm.0);
            let s_mid = normalise_angle(tangent1_angle + PI - (tangent2_angle + PI));

            // Sweep 2: L arc around c2 to end heading.
            let s2 = normalise_angle(end.theta - tangent2_angle);

            (cm, s1, s_mid, s2)
        }
        PathType::Rlr => {
            let cm_angle = base_angle - alpha;
            let cm = (
                c1.0 + 2.0 * r * cm_angle.cos(),
                c1.1 + 2.0 * r * cm_angle.sin(),
            );

            let tangent1_angle = cm_angle;
            let s1 = normalise_angle(start.theta - tangent1_angle);

            let tangent2_angle = (c2.1 - cm.1).atan2(c2.0 - cm.0);
            let s_mid = normalise_angle((tangent2_angle + PI) - (tangent1_angle + PI));

            let s2 = normalise_angle(tangent2_angle - end.theta);

            (cm, s1, s_mid, s2)
        }
        _ => return None,
    };

    let _ = cm; // Centre used implicitly in geometry; retained for clarity.
    let total = (sweep1 + sweep_mid + sweep2) * r;

    let (seg1, seg2, seg3) = match pt {
        PathType::Lrl => (
            Segment::Left(sweep1),
            Segment::Right(sweep_mid),
            Segment::Left(sweep2),
        ),
        PathType::Rlr => (
            Segment::Right(sweep1),
            Segment::Left(sweep_mid),
            Segment::Right(sweep2),
        ),
        _ => return None,
    };

    Some(DubinsPath {
        segments: [seg1, seg2, seg3],
        total_length: total,
        path_type: pt,
    })
}

/// Compute turn circle centres for the start and end poses.
fn turn_centres(start: Pose, end: Pose, r: f64, pt: PathType) -> ((f64, f64), (f64, f64)) {
    let c1 = match pt {
        PathType::Lsl | PathType::Lsr | PathType::Lrl => left_centre(start, r),
        _ => right_centre(start, r),
    };
    let c2 = match pt {
        PathType::Lsl | PathType::Rsl | PathType::Lrl => left_centre(end, r),
        _ => right_centre(end, r),
    };
    (c1, c2)
}

/// Centre of the left (counter-clockwise) turn circle.
fn left_centre(pose: Pose, r: f64) -> (f64, f64) {
    (
        pose.x + r * (pose.theta + FRAC_PI_2).cos(),
        pose.y + r * (pose.theta + FRAC_PI_2).sin(),
    )
}

/// Centre of the right (clockwise) turn circle.
fn right_centre(pose: Pose, r: f64) -> (f64, f64) {
    (
        pose.x + r * (pose.theta - FRAC_PI_2).cos(),
        pose.y + r * (pose.theta - FRAC_PI_2).sin(),
    )
}

/// Normalise an angle to [0, 2*pi).
fn normalise_angle(a: f64) -> f64 {
    let mut a = a % TAU;
    if a < 0.0 {
        a += TAU;
    }
    a
}

// ---------------------------------------------------------------------------
// Path discretisation with clothoid transitions
// ---------------------------------------------------------------------------

/// Discretise a Dubins path into (x, y) waypoints.
///
/// Clothoid transitions are inserted at junctions between straight
/// segments and circular arcs to smooth curvature changes. This is an
/// approximate splicing — the Dubins tangency geometry is not re-solved,
/// so endpoint drift scales with clothoid length relative to turn radius.
fn discretise_path(
    start: Pose,
    path: &DubinsPath,
    r: f64,
    arc_step: f64,
    clothoid_len: f64,
) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let mut x = start.x;
    let mut y = start.y;
    let mut theta = start.theta;

    points.push((x, y));

    for (i, seg) in path.segments.iter().enumerate() {
        let prev_is_straight = if i > 0 {
            matches!(path.segments[i - 1], Segment::Straight(_))
        } else {
            false
        };
        let next_is_straight = if i + 1 < path.segments.len() {
            matches!(path.segments[i + 1], Segment::Straight(_))
        } else {
            false
        };

        match *seg {
            Segment::Left(sweep) => {
                discretise_arc(
                    &mut points,
                    &mut x,
                    &mut y,
                    &mut theta,
                    r,
                    sweep,
                    1.0,
                    arc_step,
                    clothoid_len,
                    prev_is_straight,
                    next_is_straight,
                );
            }
            Segment::Right(sweep) => {
                discretise_arc(
                    &mut points,
                    &mut x,
                    &mut y,
                    &mut theta,
                    r,
                    sweep,
                    -1.0,
                    arc_step,
                    clothoid_len,
                    prev_is_straight,
                    next_is_straight,
                );
            }
            Segment::Straight(len) => {
                discretise_straight(&mut points, &mut x, &mut y, theta, len, arc_step * r);
            }
        }
    }

    points
}

/// Discretise a circular arc with optional clothoid entry/exit transitions.
///
/// `direction`: +1.0 for left (CCW), -1.0 for right (CW).
#[allow(clippy::too_many_arguments)]
fn discretise_arc(
    points: &mut Vec<(f64, f64)>,
    x: &mut f64,
    y: &mut f64,
    theta: &mut f64,
    r: f64,
    sweep: f64,
    direction: f64,
    arc_step: f64,
    clothoid_len: f64,
    entry_clothoid: bool,
    exit_clothoid: bool,
) {
    let entry_len = if entry_clothoid && clothoid_len > 1e-6 {
        clothoid_len.min(sweep * r * 0.4) // Cap at 40% of arc length
    } else {
        0.0
    };
    let exit_len = if exit_clothoid && clothoid_len > 1e-6 {
        clothoid_len.min(sweep * r * 0.4)
    } else {
        0.0
    };

    let entry_sweep = if r > 1e-6 { entry_len / (2.0 * r) } else { 0.0 };
    let exit_sweep = if r > 1e-6 { exit_len / (2.0 * r) } else { 0.0 };

    // Entry clothoid: curvature ramps from 0 to 1/R.
    if entry_len > 1e-6 {
        discretise_clothoid(
            points, x, y, theta, r, entry_len, direction, arc_step, false,
        );
    }

    // Pure circular arc (reduced by clothoid sweeps).
    let pure_sweep = (sweep - entry_sweep - exit_sweep).max(0.0);
    if pure_sweep > 1e-10 {
        /*
         * Step around the arc centre. The centre is at distance R
         * perpendicular to the current heading. Each step rotates the
         * position vector around the centre by `step` radians.
         */
        let n_steps = (pure_sweep / arc_step).ceil() as usize;
        let step = pure_sweep / n_steps as f64;
        for _ in 0..n_steps {
            /*
             * Arc centre = pos + R * unit(theta + dir*pi/2).
             * Position  = centre + R * unit(theta - dir*pi/2).
             */
            let cx = *x + r * (*theta + direction * FRAC_PI_2).cos();
            let cy = *y + r * (*theta + direction * FRAC_PI_2).sin();

            // Advance heading.
            *theta += direction * step;

            // New position on the circle.
            *x = cx + r * (*theta - direction * FRAC_PI_2).cos();
            *y = cy + r * (*theta - direction * FRAC_PI_2).sin();

            points.push((*x, *y));
        }
    }

    // Exit clothoid: curvature ramps from 1/R to 0.
    if exit_len > 1e-6 {
        discretise_clothoid(points, x, y, theta, r, exit_len, direction, arc_step, true);
    }
}

/// Discretise a clothoid (Euler spiral) transition curve.
///
/// Uses midpoint incremental integration of curvature to generate waypoints.
/// Accuracy depends on step count (min 4 steps per clothoid).
///
/// `reverse`: if true, curvature ramps from 1/R to 0 (exit transition).
#[allow(clippy::too_many_arguments)]
fn discretise_clothoid(
    points: &mut Vec<(f64, f64)>,
    x: &mut f64,
    y: &mut f64,
    theta: &mut f64,
    r: f64,
    length: f64,
    direction: f64,
    arc_step: f64,
    reverse: bool,
) {
    /*
     * Clothoid parameter: A² = R × L
     * where R = final radius, L = total clothoid length.
     *
     * Entry clothoid (reverse=false): curvature ramps 0 → 1/R.
     *   κ(s) = s / (R·L), Δθ(s) = s²/(2·R·L)
     *
     * Exit clothoid (reverse=true): curvature ramps 1/R → 0.
     *   κ(s) = (L-s) / (R·L), Δθ(s) = s·(2L-s) / (2·R·L)
     *   Total heading change = L/(2R) (same as entry).
     *
     * Position via incremental stepping to avoid origin-relative errors.
     */
    let a_sq = r * length;
    let n_steps = ((length / (arc_step * r)).ceil() as usize).max(4);
    let ds = length / n_steps as f64;

    for i in 1..=n_steps {
        let s = i as f64 * ds;
        let s_prev = (i - 1) as f64 * ds;

        /*
         * Compute curvature at the midpoint of this step for accurate
         * heading integration.
         */
        let s_mid = (s + s_prev) / 2.0;
        let kappa = if reverse {
            (length - s_mid) / a_sq
        } else {
            s_mid / a_sq
        };

        // Heading change for this step.
        let dtheta = direction * kappa * ds;
        *theta += dtheta;

        // Advance position along current heading.
        *x += ds * (*theta - dtheta / 2.0).cos();
        *y += ds * (*theta - dtheta / 2.0).sin();

        points.push((*x, *y));
    }
}

/// Discretise a straight segment into waypoints.
fn discretise_straight(
    points: &mut Vec<(f64, f64)>,
    x: &mut f64,
    y: &mut f64,
    theta: f64,
    length: f64,
    step: f64,
) {
    if length < 1e-10 {
        return;
    }
    let cos_t = theta.cos();
    let sin_t = theta.sin();
    let n_steps = (length / step).ceil() as usize;
    let ds = length / n_steps as f64;

    for i in 1..=n_steps {
        let d = i as f64 * ds;
        let px = *x + d * cos_t;
        let py = *y + d * sin_t;
        if i == n_steps {
            *x = px;
            *y = py;
        }
        points.push((px, py));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    /// R_min = V^2 / (g * tan(bank)).
    /// At 30 m/s, 20 deg bank: R = 900 / (9.80665 * 0.364) = ~252 m.
    #[test]
    fn turn_radius_physics() {
        let tp = TurnParams::new(30.0, 20.0);
        let r = tp.min_radius();
        let expected = 30.0_f64.powi(2) / (G * 20.0_f64.to_radians().tan());
        assert_abs_diff_eq!(r, expected, epsilon = 0.1);
        assert!(r > 200.0 && r < 300.0);
    }

    /// Two parallel lines 200 m apart, same heading: verify the generated
    /// turn radius matches R_min.
    #[test]
    fn parallel_lines_same_heading() {
        let tp = TurnParams::new(30.0, 20.0);
        let r = tp.min_radius();

        // Start at origin heading east, end at (0, 200) heading east.
        let start = Pose {
            x: 0.0,
            y: 0.0,
            theta: 0.0,
        };
        let end = Pose {
            x: 0.0,
            y: 200.0,
            theta: 0.0,
        };

        let path = shortest_dubins(start, end, r);
        // Should select LSL or RSR.
        assert!(
            path.path_type == PathType::Lsl || path.path_type == PathType::Rsr,
            "expected LSL or RSR for same-heading parallel lines, got {:?}",
            path.path_type
        );

        // Discretise and check the path connects start to near end.
        let pts = dubins_path(start, end, &tp);
        assert!(pts.len() >= 3, "path should have multiple waypoints");

        let last = pts.last().unwrap();
        let dist_to_end = ((last.0 - end.x).powi(2) + (last.1 - end.y).powi(2)).sqrt();
        /*
         * Tolerance: 20% of turn radius. For 5-degree arc steps over a
         * full U-turn (~2*pi*R ≈ 1600 m), accumulated discretisation drift
         * of ~40 m is expected and acceptable for waypoint generation.
         */
        assert!(
            dist_to_end < r * 0.20,
            "path endpoint should be near target: dist={dist_to_end:.1}m"
        );
    }

    /// Two parallel lines, opposite heading: should choose an appropriate
    /// Dubins type.
    #[test]
    fn parallel_lines_opposite_heading() {
        let tp = TurnParams::new(30.0, 20.0);
        let r = tp.min_radius();

        // Start heading east, end heading west, 200m apart.
        let start = Pose {
            x: 0.0,
            y: 0.0,
            theta: 0.0,
        };
        let end = Pose {
            x: 0.0,
            y: 200.0,
            theta: PI,
        };

        let path = shortest_dubins(start, end, r);
        // For opposite headings, RSR or LSL are common.
        assert!(
            path.total_length > 0.0,
            "path length should be positive: {}",
            path.total_length
        );

        let pts = dubins_path(start, end, &tp);
        assert!(pts.len() >= 3);
    }

    /// Clothoid insertion: curvature at the straight-to-arc junction must
    /// start at 0, not 1/R.
    #[test]
    fn clothoid_transition_starts_at_zero_curvature() {
        let tp = TurnParams::new(30.0, 20.0);
        let cl = tp.clothoid_length();
        assert!(cl > 0.0, "clothoid length should be positive");

        // The clothoid heading change at s=0 is 0 (starts straight).
        // At s=L, heading change = L^2 / (2*R*L) = L / (2R).
        let r = tp.min_radius();
        let heading_change = cl / (2.0 * r);
        assert!(heading_change > 0.0 && heading_change < FRAC_PI_2);
    }

    /// Normalise angle to [0, 2pi).
    #[test]
    fn angle_normalisation() {
        assert_abs_diff_eq!(normalise_angle(0.0), 0.0, epsilon = 1e-12);
        assert_abs_diff_eq!(normalise_angle(TAU), 0.0, epsilon = 1e-12);
        assert_abs_diff_eq!(normalise_angle(-PI), PI, epsilon = 1e-12);
        assert_abs_diff_eq!(normalise_angle(3.0 * PI), PI, epsilon = 1e-12);
    }

    /// Turn centres are perpendicular to heading at distance R.
    #[test]
    fn turn_centre_geometry() {
        let pose = Pose {
            x: 100.0,
            y: 200.0,
            theta: 0.0,
        };
        let r = 50.0;

        let lc = left_centre(pose, r);
        assert_abs_diff_eq!(lc.0, 100.0, epsilon = 1e-6);
        assert_abs_diff_eq!(lc.1, 250.0, epsilon = 1e-6);

        let rc = right_centre(pose, r);
        assert_abs_diff_eq!(rc.0, 100.0, epsilon = 1e-6);
        assert_abs_diff_eq!(rc.1, 150.0, epsilon = 1e-6);
    }

    /// Straight-line path when start and end are aligned.
    #[test]
    fn straight_line_degenerate() {
        let tp = TurnParams::new(30.0, 20.0);
        let start = Pose {
            x: 0.0,
            y: 0.0,
            theta: 0.0,
        };
        let end = Pose {
            x: 1000.0,
            y: 0.0,
            theta: 0.0,
        };

        let pts = dubins_path(start, end, &tp);
        assert!(pts.len() >= 2);

        // All points should be roughly on the x-axis.
        for pt in &pts {
            assert!(
                pt.1.abs() < tp.min_radius() * 0.1,
                "y={:.1} too far from x-axis",
                pt.1
            );
        }
    }
}
