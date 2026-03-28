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

// QGroundControl .plan JSON exporter.
//
// Produces a QGC-compatible mission file containing MAVLink SimpleItems.
// Each flight line becomes a sequence of MAV_CMD_NAV_WAYPOINT commands
// bracketed by MAV_CMD_DO_SET_CAM_TRIGG_DIST (start/stop) to activate
// distance-based camera triggering.
//
// Frame selection:
//   - AGL altitudes     -> MAV_FRAME_GLOBAL_RELATIVE_ALT (3)
//   - All other datums  -> MAV_FRAME_GLOBAL (0)
//
// ArduPilot AMSL mode expects WGS84 ellipsoidal heights. The caller is
// responsible for converting the FlightPlan to the appropriate datum
// (typically AltitudeDatum::Wgs84Ellipsoidal) before invoking this exporter.
//
// Reference: docs/11_aerial_survey_autopilot_formats.md §QGroundControl

use std::io::Write;
use std::path::Path;

use serde_json::{json, Value};

use crate::error::IoError;
use crate::photogrammetry::FlightPlan;
use crate::types::AltitudeDatum;

/*
 * MAVLink command IDs used in the generated mission.
 * See https://mavlink.io/en/messages/common.html for the full enum.
 */
const MAV_CMD_NAV_WAYPOINT: u16 = 16;
const MAV_CMD_DO_SET_CAM_TRIGG_DIST: u16 = 206;

/*
 * MAV_FRAME values. Frame 0 = altitudes relative to WGS84 ellipsoid (AMSL).
 * Frame 3 = altitudes relative to the home position (AGL).
 * Frame 2 = MISSION — used for DO_* commands that have no spatial position.
 */
const MAV_FRAME_GLOBAL: u8 = 0;
const MAV_FRAME_GLOBAL_RELATIVE_ALT: u8 = 3;
const MAV_FRAME_MISSION: u8 = 2;

/*
 * Default values for the mission-level fields. These match QGC's own defaults
 * for a generic multirotor vehicle. The caller can override via the builder
 * if needed in the future.
 */
const DEFAULT_FIRMWARE_TYPE: u8 = 3; // MAV_AUTOPILOT_ARDUPILOTMEGA
const DEFAULT_VEHICLE_TYPE: u8 = 2; // MAV_TYPE_QUADROTOR
const DEFAULT_CRUISE_SPEED: f64 = 15.0;
const DEFAULT_HOVER_SPEED: f64 = 5.0;

/// Serialize `plan` to QGroundControl .plan JSON and write to `path`.
///
/// `trigger_distance_m` is the along-track camera trigger interval in metres,
/// typically obtained from `FlightPlanParams::photo_interval()`.
///
/// The home position is set to the first waypoint of the first flight line.
///
/// # Datum requirements
///
/// ArduPilot interprets `MAV_FRAME_GLOBAL` altitudes as WGS84 ellipsoidal.
/// Non-AGL plans **must** be converted to `AltitudeDatum::Wgs84Ellipsoidal`
/// before calling this function. Returns `IoError::Serialization` if the
/// plan's datum is neither AGL nor WGS84 ellipsoidal.
///
/// All lines in the plan must share the same altitude datum. Mixed-datum
/// plans are rejected with `IoError::Serialization`.
pub fn write_qgc_plan(
    path: &Path,
    plan: &FlightPlan,
    trigger_distance_m: f64,
) -> Result<(), IoError> {
    validate_datum(plan)?;
    let plan_json = flight_plan_to_qgc(plan, trigger_distance_m);
    let json_bytes =
        serde_json::to_vec_pretty(&plan_json).map_err(|e| IoError::Serialization(e.to_string()))?;
    let mut file = std::fs::File::create(path)?;
    file.write_all(&json_bytes)?;
    Ok(())
}

/// Validate that the plan's altitude datum is suitable for QGC export.
fn validate_datum(plan: &FlightPlan) -> Result<(), IoError> {
    if plan.lines.is_empty() {
        return Ok(());
    }

    let first_datum = plan.lines[0].altitude_datum;

    /*
     * All lines must share the same datum. QGC applies a single MAV_FRAME
     * to all waypoints, so mixed datums would silently corrupt altitudes.
     */
    if plan.lines.iter().any(|l| l.altitude_datum != first_datum) {
        return Err(IoError::Serialization(
            "QGC export requires all flight lines to share the same altitude datum".into(),
        ));
    }

    /*
     * ArduPilot MAV_FRAME_GLOBAL = WGS84 ellipsoidal. EGM2008/EGM96
     * altitudes would be misinterpreted by the autopilot, causing the
     * aircraft to fly at the wrong height — a flight-safety issue.
     */
    if first_datum != AltitudeDatum::Agl && first_datum != AltitudeDatum::Wgs84Ellipsoidal {
        return Err(IoError::Serialization(format!(
            "QGC export requires WGS84 ellipsoidal or AGL altitudes, but plan uses {}. \
             Convert with FlightLine::to_datum() before export.",
            first_datum.as_str()
        )));
    }

    Ok(())
}

/// Convert a FlightPlan to a serde_json Value representing a QGC .plan file.
///
/// Separated from `write_qgc_plan` so callers can inspect or embed the JSON
/// without writing to disk.
pub fn flight_plan_to_qgc(plan: &FlightPlan, trigger_distance_m: f64) -> Value {
    /*
     * Determine the MAV_FRAME based on the altitude datum of the plan.
     * AGL plans use GLOBAL_RELATIVE_ALT (frame 3); everything else uses
     * GLOBAL (frame 0) which ArduPilot interprets as WGS84 ellipsoidal.
     */
    let waypoint_frame = if plan
        .lines
        .first()
        .is_some_and(|l| l.altitude_datum == AltitudeDatum::Agl)
    {
        MAV_FRAME_GLOBAL_RELATIVE_ALT
    } else {
        MAV_FRAME_GLOBAL
    };

    /*
     * Build the planned home position from the first waypoint of the first
     * line. If the plan is empty, default to (0, 0, 0).
     */
    let (home_lat, home_lon, home_alt) = plan
        .lines
        .first()
        .filter(|l| !l.is_empty())
        .map(|l| {
            (
                l.lats()[0].to_degrees(),
                l.lons()[0].to_degrees(),
                l.elevations()[0],
            )
        })
        .unwrap_or((0.0, 0.0, 0.0));

    /*
     * Sequence counter. QGC expects a monotonically increasing `doJumpId`
     * across all items in the mission (1-based).
     */
    let mut seq: u32 = 1;
    let mut items: Vec<Value> = Vec::new();

    for line in &plan.lines {
        let n = line.len();
        if n == 0 {
            continue;
        }

        let is_survey = !line.is_transit;

        /*
         * Start-of-line: enable distance-based camera triggering.
         * Only for survey legs — transit/turn segments fly without
         * camera activation to avoid wasting exposures off-grid.
         */
        if is_survey {
            items.push(simple_item(
                seq,
                MAV_CMD_DO_SET_CAM_TRIGG_DIST,
                MAV_FRAME_MISSION,
                [trigger_distance_m, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            ));
            seq += 1;
        }

        /*
         * Waypoints along the flight line.
         * Param4 = NaN → autopilot calculates yaw to face the next waypoint.
         * Params 5/6 carry lat/lon in degrees (QGC JSON format, not int32).
         * Param 7 carries altitude in metres in the selected frame.
         */
        let lats = line.lats();
        let lons = line.lons();
        let elevs = line.elevations();

        for i in 0..n {
            let lat_deg = lats[i].to_degrees();
            let lon_deg = lons[i].to_degrees();
            let alt_m = elevs[i];

            items.push(simple_item(
                seq,
                MAV_CMD_NAV_WAYPOINT,
                waypoint_frame,
                [0.0, 0.0, 0.0, f64::NAN, lat_deg, lon_deg, alt_m],
            ));
            seq += 1;
        }

        /*
         * End-of-line: disable camera triggering.
         */
        if is_survey {
            items.push(simple_item(
                seq,
                MAV_CMD_DO_SET_CAM_TRIGG_DIST,
                MAV_FRAME_MISSION,
                [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ));
            seq += 1;
        }
    }

    json!({
        "fileType": "Plan",
        "version": 1,
        "groundStation": "IronTrack",
        "mission": {
            "firmwareType": DEFAULT_FIRMWARE_TYPE,
            "vehicleType": DEFAULT_VEHICLE_TYPE,
            "cruiseSpeed": DEFAULT_CRUISE_SPEED,
            "hoverSpeed": DEFAULT_HOVER_SPEED,
            "plannedHomePosition": [home_lat, home_lon, home_alt],
            "items": items
        },
        "geoFence": {
            "circles": [],
            "polygons": [],
            "version": 2
        },
        "rallyPoints": {
            "points": [],
            "version": 2
        }
    })
}

/// Build a single QGC SimpleItem JSON object.
fn simple_item(seq: u32, command: u16, frame: u8, params: [f64; 7]) -> Value {
    json!({
        "autoContinue": true,
        "command": command,
        "doJumpId": seq,
        "frame": frame,
        "params": params,
        "type": "SimpleItem"
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use tempfile::tempdir;

    use crate::photogrammetry::flightlines::FlightPlanParams;
    use crate::photogrammetry::FlightPlan;
    use crate::types::{FlightLine, SensorParams};

    /// Create a minimal flight plan with WGS84 ellipsoidal datum — the
    /// datum required by QGC for MAV_FRAME_GLOBAL exports.
    fn minimal_plan(n_lines: usize, pts_per_line: usize) -> FlightPlan {
        let sensor = SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        };
        let params = FlightPlanParams {
            sensor,
            target_gsd_m: 0.05,
            side_lap_percent: 60.0,
            end_lap_percent: 80.0,
            flight_altitude_msl: Some(600.0),
        };
        let lines: Vec<FlightLine> = (0..n_lines)
            .map(|li| {
                let lon_base = li as f64 * 0.01;
                let mut line = FlightLine::new();
                for pi in 0..pts_per_line {
                    let lon = (lon_base + pi as f64 * 0.001_f64).to_radians();
                    let lat = 51.0_f64.to_radians();
                    line.push(lat, lon, 100.0);
                }
                line.altitude_datum = AltitudeDatum::Wgs84Ellipsoidal;
                line
            })
            .collect();
        FlightPlan {
            params,
            azimuth_deg: 0.0,
            lines,
        }
    }

    #[test]
    fn plan_root_structure() {
        let plan = minimal_plan(2, 3);
        let qgc = flight_plan_to_qgc(&plan, 25.0);

        assert_eq!(qgc["fileType"], "Plan");
        assert_eq!(qgc["version"], 1);
        assert_eq!(qgc["groundStation"], "IronTrack");
        assert!(qgc["mission"].is_object());
        assert!(qgc["geoFence"].is_object());
        assert!(qgc["rallyPoints"].is_object());
    }

    #[test]
    fn mission_items_count() {
        /*
         * For N lines with P points each, the mission should contain:
         *   N * (1 trigger-start + P waypoints + 1 trigger-stop) items.
         */
        let plan = minimal_plan(2, 3);
        let qgc = flight_plan_to_qgc(&plan, 25.0);
        let items = qgc["mission"]["items"].as_array().unwrap();

        let expected = 2 * (1 + 3 + 1);
        assert_eq!(items.len(), expected);
    }

    #[test]
    fn waypoint_command_id_and_frame() {
        let plan = minimal_plan(1, 2);
        let qgc = flight_plan_to_qgc(&plan, 25.0);
        let items = qgc["mission"]["items"].as_array().unwrap();

        /*
         * Items: [trigger-start, wp0, wp1, trigger-stop]
         * Waypoints should be command 16 with frame 0 (GLOBAL, since
         * the default FlightLine datum is EGM2008, not AGL).
         */
        let wp0 = &items[1];
        assert_eq!(wp0["command"], MAV_CMD_NAV_WAYPOINT as u64);
        assert_eq!(wp0["frame"], MAV_FRAME_GLOBAL as u64);
        assert_eq!(wp0["type"], "SimpleItem");
        assert_eq!(wp0["autoContinue"], true);
    }

    #[test]
    fn camera_trigger_commands() {
        let plan = minimal_plan(1, 2);
        let trigger_dist = 30.0;
        let qgc = flight_plan_to_qgc(&plan, trigger_dist);
        let items = qgc["mission"]["items"].as_array().unwrap();

        /*
         * First item: trigger start (command 206, distance = trigger_dist).
         * Last item: trigger stop (command 206, distance = 0).
         */
        let start = &items[0];
        assert_eq!(start["command"], MAV_CMD_DO_SET_CAM_TRIGG_DIST as u64);
        assert_eq!(start["frame"], MAV_FRAME_MISSION as u64);
        assert_abs_diff_eq!(
            start["params"][0].as_f64().unwrap(),
            trigger_dist,
            epsilon = 1e-9
        );
        assert_abs_diff_eq!(start["params"][2].as_f64().unwrap(), 1.0, epsilon = 1e-9);

        let stop = items.last().unwrap();
        assert_eq!(stop["command"], MAV_CMD_DO_SET_CAM_TRIGG_DIST as u64);
        assert_abs_diff_eq!(stop["params"][0].as_f64().unwrap(), 0.0, epsilon = 1e-9);
    }

    #[test]
    fn waypoint_coordinates_are_degrees() {
        /*
         * QGC .plan JSON carries lat/lon as decimal degrees (not radians,
         * not scaled integers — the int32 scaling is only for the MAVLink
         * binary protocol, not the JSON file).
         */
        let plan = minimal_plan(1, 2);
        let qgc = flight_plan_to_qgc(&plan, 25.0);
        let items = qgc["mission"]["items"].as_array().unwrap();

        let wp0 = &items[1];
        let lat = wp0["params"][4].as_f64().unwrap();
        let lon = wp0["params"][5].as_f64().unwrap();
        let alt = wp0["params"][6].as_f64().unwrap();

        assert_abs_diff_eq!(lat, 51.0, epsilon = 1e-6);
        assert_abs_diff_eq!(lon, 0.0, epsilon = 1e-6);
        assert_abs_diff_eq!(alt, 100.0, epsilon = 1e-9);
    }

    #[test]
    fn do_jump_ids_are_sequential() {
        let plan = minimal_plan(2, 3);
        let qgc = flight_plan_to_qgc(&plan, 25.0);
        let items = qgc["mission"]["items"].as_array().unwrap();

        for (i, item) in items.iter().enumerate() {
            assert_eq!(
                item["doJumpId"].as_u64().unwrap(),
                (i + 1) as u64,
                "doJumpId must be 1-based sequential"
            );
        }
    }

    #[test]
    fn agl_datum_selects_relative_alt_frame() {
        let sensor = SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        };
        let params = FlightPlanParams {
            sensor,
            target_gsd_m: 0.05,
            side_lap_percent: 60.0,
            end_lap_percent: 80.0,
            flight_altitude_msl: Some(600.0),
        };
        let mut line = FlightLine::new();
        line.push(51.0_f64.to_radians(), 0.0_f64.to_radians(), 50.0);
        line.altitude_datum = AltitudeDatum::Agl;

        let plan = FlightPlan {
            params,
            azimuth_deg: 0.0,
            lines: vec![line],
        };
        let qgc = flight_plan_to_qgc(&plan, 25.0);
        let items = qgc["mission"]["items"].as_array().unwrap();

        let wp = &items[1];
        assert_eq!(
            wp["frame"], MAV_FRAME_GLOBAL_RELATIVE_ALT as u64,
            "AGL datum must use MAV_FRAME_GLOBAL_RELATIVE_ALT"
        );
    }

    #[test]
    fn home_position_matches_first_waypoint() {
        let plan = minimal_plan(2, 3);
        let qgc = flight_plan_to_qgc(&plan, 25.0);

        let home = qgc["mission"]["plannedHomePosition"].as_array().unwrap();
        let home_lat = home[0].as_f64().unwrap();
        let home_lon = home[1].as_f64().unwrap();
        let home_alt = home[2].as_f64().unwrap();

        assert_abs_diff_eq!(home_lat, 51.0, epsilon = 1e-6);
        assert_abs_diff_eq!(home_lon, 0.0, epsilon = 1e-6);
        assert_abs_diff_eq!(home_alt, 100.0, epsilon = 1e-9);
    }

    #[test]
    fn write_qgc_plan_creates_valid_file() {
        let plan = minimal_plan(2, 3);
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.plan");
        write_qgc_plan(&path, &plan, 25.0).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["fileType"], "Plan");
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["groundStation"], "IronTrack");
    }

    #[test]
    fn empty_plan_produces_empty_items() {
        let sensor = SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        };
        let params = FlightPlanParams {
            sensor,
            target_gsd_m: 0.05,
            side_lap_percent: 60.0,
            end_lap_percent: 80.0,
            flight_altitude_msl: Some(600.0),
        };
        let plan = FlightPlan {
            params,
            azimuth_deg: 0.0,
            lines: vec![],
        };
        let qgc = flight_plan_to_qgc(&plan, 25.0);
        let items = qgc["mission"]["items"].as_array().unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn multi_line_trigger_pattern() {
        /*
         * With 2 lines of 2 waypoints, the item sequence must be:
         *   [trigger-start, wp, wp, trigger-stop, trigger-start, wp, wp, trigger-stop]
         * Verify the command pattern is correct across line boundaries.
         */
        let plan = minimal_plan(2, 2);
        let qgc = flight_plan_to_qgc(&plan, 25.0);
        let items = qgc["mission"]["items"].as_array().unwrap();

        let expected_commands: Vec<u64> = vec![206, 16, 16, 206, 206, 16, 16, 206];
        let actual_commands: Vec<u64> = items
            .iter()
            .map(|item| item["command"].as_u64().unwrap())
            .collect();
        assert_eq!(actual_commands, expected_commands);
    }

    #[test]
    fn egm2008_datum_rejected_by_write() {
        /*
         * EGM2008 altitudes are not valid for QGC export — ArduPilot would
         * misinterpret them as WGS84 ellipsoidal, causing altitude errors
         * equal to the local geoid undulation (up to ~100 m).
         */
        let sensor = SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        };
        let params = FlightPlanParams {
            sensor,
            target_gsd_m: 0.05,
            side_lap_percent: 60.0,
            end_lap_percent: 80.0,
            flight_altitude_msl: Some(600.0),
        };
        let mut line = FlightLine::new();
        line.push(51.0_f64.to_radians(), 0.0_f64.to_radians(), 100.0);
        // Default datum is EGM2008 — should be rejected.
        let plan = FlightPlan {
            params,
            azimuth_deg: 0.0,
            lines: vec![line],
        };

        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.plan");
        let result = write_qgc_plan(&path, &plan, 25.0);
        assert!(
            result.is_err(),
            "EGM2008 plan must be rejected for QGC export"
        );
    }

    #[test]
    fn mixed_datum_rejected_by_write() {
        let sensor = SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        };
        let params = FlightPlanParams {
            sensor,
            target_gsd_m: 0.05,
            side_lap_percent: 60.0,
            end_lap_percent: 80.0,
            flight_altitude_msl: Some(600.0),
        };
        let mut line_a = FlightLine::new();
        line_a.push(51.0_f64.to_radians(), 0.0_f64.to_radians(), 100.0);
        line_a.altitude_datum = AltitudeDatum::Wgs84Ellipsoidal;

        let mut line_b = FlightLine::new();
        line_b.push(51.0_f64.to_radians(), 0.001_f64.to_radians(), 100.0);
        line_b.altitude_datum = AltitudeDatum::Agl;

        let plan = FlightPlan {
            params,
            azimuth_deg: 0.0,
            lines: vec![line_a, line_b],
        };

        let dir = tempdir().unwrap();
        let path = dir.path().join("mixed.plan");
        let result = write_qgc_plan(&path, &plan, 25.0);
        assert!(
            result.is_err(),
            "mixed-datum plan must be rejected for QGC export"
        );
    }
}
