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

//! Integration tests for the kinematics subsystem (wind triangle solver).

use approx::assert_abs_diff_eq;

use irontrack::kinematics::atmosphere::{
    compute_segment_ground_speeds, solve_wind_triangle, WindVector,
};

/// Verify that per-segment ground speeds from `compute_segment_ground_speeds`
/// match individual `solve_wind_triangle` calls for a simple northbound line.
#[test]
fn segment_ground_speeds_match_individual_solves() {
    let lats = vec![51.0_f64.to_radians(), 51.01_f64.to_radians()];
    let lons = vec![0.0_f64.to_radians(), 0.0_f64.to_radians()];
    let wind = WindVector::new(5.0, 270.0).unwrap();
    let tas = 30.0;

    let gs = compute_segment_ground_speeds(&lats, &lons, tas, &wind).unwrap();
    assert_eq!(gs.len(), 1);

    // Segment azimuth is ~0° (north). Wind from 270° (west) is a crosswind.
    // Verify against direct solve with track ≈ 0°.
    let direct = solve_wind_triangle(0.0, tas, &wind).unwrap();
    assert_abs_diff_eq!(gs[0], direct.ground_speed_ms, epsilon = 0.5);
}

/// Empty or single-point lines must produce empty ground speed vectors.
#[test]
fn empty_and_single_point_lines() {
    let wind = WindVector::new(5.0, 0.0).unwrap();

    let gs_empty = compute_segment_ground_speeds(&[], &[], 30.0, &wind).unwrap();
    assert!(gs_empty.is_empty());

    let gs_single = compute_segment_ground_speeds(
        &[51.0_f64.to_radians()],
        &[0.0_f64.to_radians()],
        30.0,
        &wind,
    )
    .unwrap();
    assert!(gs_single.is_empty());
}

/// `apply_wind_to_plan` sets ground_speeds on each line.
#[test]
fn apply_wind_to_plan_sets_ground_speeds() {
    use irontrack::kinematics::atmosphere::apply_wind_to_plan;
    use irontrack::photogrammetry::flightlines::{FlightPlan, FlightPlanParams};
    use irontrack::types::{FlightLine, SensorParams};

    let sensor = SensorParams::new(8.8, 13.2, 8.8, 5472, 3648).unwrap();
    let params = FlightPlanParams {
        sensor,
        target_gsd_m: 0.05,
        side_lap_percent: 60.0,
        end_lap_percent: 80.0,
        flight_altitude_msl: Some(600.0),
    };

    let mut line = FlightLine::new();
    line.push(51.0_f64.to_radians(), 0.0_f64.to_radians(), 100.0);
    line.push(51.01_f64.to_radians(), 0.0_f64.to_radians(), 100.0);
    line.push(51.02_f64.to_radians(), 0.0_f64.to_radians(), 100.0);

    let mut plan = FlightPlan {
        params,
        azimuth_deg: 0.0,
        lines: vec![line],
    };

    let wind = WindVector::new(5.0, 180.0).unwrap(); // tailwind
    apply_wind_to_plan(&mut plan, 30.0, &wind).unwrap();

    let gs = plan.lines[0]
        .ground_speeds()
        .expect("ground_speeds should be set");
    assert_eq!(gs.len(), 2); // 3 waypoints → 2 segments
    for &speed in gs {
        assert!(speed > 30.0, "tailwind should increase GS above TAS");
    }
}

/// `apply_wind_to_plan` with calm wind is a no-op.
#[test]
fn apply_wind_calm_is_noop() {
    use irontrack::kinematics::atmosphere::apply_wind_to_plan;
    use irontrack::photogrammetry::flightlines::{FlightPlan, FlightPlanParams};
    use irontrack::types::{FlightLine, SensorParams};

    let sensor = SensorParams::new(8.8, 13.2, 8.8, 5472, 3648).unwrap();
    let params = FlightPlanParams {
        sensor,
        target_gsd_m: 0.05,
        side_lap_percent: 60.0,
        end_lap_percent: 80.0,
        flight_altitude_msl: Some(600.0),
    };

    let mut line = FlightLine::new();
    line.push(51.0_f64.to_radians(), 0.0_f64.to_radians(), 100.0);
    line.push(51.01_f64.to_radians(), 0.0_f64.to_radians(), 100.0);

    let mut plan = FlightPlan {
        params,
        azimuth_deg: 0.0,
        lines: vec![line],
    };

    let wind = WindVector::new(0.0, 0.0).unwrap();
    apply_wind_to_plan(&mut plan, 30.0, &wind).unwrap();

    // Calm wind → no ground speeds set
    assert!(plan.lines[0].ground_speeds().is_none());
}

/// `apply_wind_to_plan` populates crab_angles on each line (Phase 7B).
#[test]
fn apply_wind_stores_crab_angles_integration() {
    use irontrack::kinematics::atmosphere::apply_wind_to_plan;
    use irontrack::photogrammetry::flightlines::BoundingBox;
    use irontrack::photogrammetry::flightlines::{generate_flight_lines, FlightPlanParams};
    use irontrack::types::SensorParams;

    let sensor = SensorParams::new(50.0, 53.4, 40.0, 11664, 8750).unwrap();
    let params = FlightPlanParams::new(sensor, 0.05).unwrap();
    let bbox = BoundingBox {
        min_lat: 0.0,
        min_lon: 0.0,
        max_lat: 0.01,
        max_lon: 0.01,
    };
    let mut plan = generate_flight_lines(&bbox, 0.0, &params, None).unwrap();

    let wind = WindVector::new(10.0, 90.0).unwrap(); // crosswind from east
    apply_wind_to_plan(&mut plan, 50.0, &wind).unwrap();

    for line in &plan.lines {
        if line.len() >= 2 {
            let crab = line.crab_angles().expect("crab angles should be set");
            assert_eq!(crab.len(), line.len() - 1);
            // With crosswind, WCA should be non-zero
            assert!(crab.iter().any(|a| a.abs() > 0.1));
            // Representative should be the max absolute
            let max_abs = crab.iter().map(|a| a.abs()).fold(0.0_f64, f64::max);
            assert_abs_diff_eq!(
                line.representative_crab_angle().unwrap(),
                max_abs,
                epsilon = 1e-10
            );
        }
    }
}

/// Wind-compensated spacing produces tighter lines than nominal (Phase 7B).
#[test]
fn wind_compensation_produces_tighter_spacing() {
    use irontrack::photogrammetry::flightlines::BoundingBox;
    use irontrack::photogrammetry::flightlines::{generate_flight_lines, FlightPlanParams};
    use irontrack::types::SensorParams;

    let sensor = SensorParams::new(50.0, 53.4, 40.0, 11664, 8750).unwrap();
    let params = FlightPlanParams::new(sensor, 0.05).unwrap();
    let bbox = BoundingBox {
        min_lat: 0.0,
        min_lon: 0.0,
        max_lat: 0.05,
        max_lon: 0.05,
    };

    let wind = WindVector::new(10.0, 90.0).unwrap(); // 10 m/s crosswind
    let adjusted = params
        .wind_adjusted_flight_line_spacing(0.0, 50.0, &wind)
        .unwrap();
    let nominal = params.flight_line_spacing();

    assert!(
        adjusted < nominal,
        "adjusted {adjusted:.1} m should be < nominal {nominal:.1} m"
    );

    let plan_no_wind = generate_flight_lines(&bbox, 0.0, &params, None).unwrap();
    let plan_wind = generate_flight_lines(&bbox, 0.0, &params, Some(adjusted)).unwrap();

    assert!(
        plan_wind.lines.len() >= plan_no_wind.lines.len(),
        "wind plan should have >= lines: {} vs {}",
        plan_wind.lines.len(),
        plan_no_wind.lines.len()
    );
}

/// Multi-segment line with wind should produce correct count of GS values.
#[test]
fn multi_segment_line_produces_n_minus_one_speeds() {
    let lats: Vec<f64> = (0..5)
        .map(|i| (51.0 + i as f64 * 0.01).to_radians())
        .collect();
    let lons: Vec<f64> = vec![0.0_f64.to_radians(); 5];
    let wind = WindVector::new(8.0, 180.0).unwrap(); // tailwind for northbound track

    let gs = compute_segment_ground_speeds(&lats, &lons, 30.0, &wind).unwrap();
    assert_eq!(gs.len(), 4); // 5 points → 4 segments

    // All segments are northbound with a tailwind, so GS > TAS
    for &speed in &gs {
        assert!(speed > 30.0, "expected GS > TAS for tailwind, got {speed}");
    }
}
