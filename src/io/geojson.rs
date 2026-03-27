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

// RFC 7946 GeoJSON FeatureCollection serializer.
//
// RFC 7946 §3.1.1 mandates WGS84 coordinates in (longitude, latitude) order;
// §3.1.1 also permits a third altitude element (metres). The vertical datum is
// identified by the `altitude_datum` property — consumers must read that field
// before interpreting the Z coordinate.
// `FlightLine::iter_lonlat_deg()` enforces the lon/lat order at the serialisation
// boundary. Elevation is added as the Z element so terrain-following plans are
// represented in full 3D — essential for autopilot consumption.
//
// The output is a FeatureCollection where each FlightLine becomes a Feature
// with a LineString geometry. Properties carry per-line metadata drawn from
// FlightPlanParams.

use std::io::Write;
use std::path::Path;

use serde_json::{json, Value};

use crate::error::IoError;
use crate::photogrammetry::FlightPlan;

/// Serialize `plan` to a RFC 7946 GeoJSON FeatureCollection and write it to
/// `path`. Truncates and overwrites any existing file at that path.
///
/// Each FlightLine becomes one Feature. Properties on each Feature:
/// - `line_index`: zero-based integer index within the plan
/// - `min_altitude_m`: minimum altitude across waypoints on this line (metres, in the plan's datum)
/// - `max_altitude_m`: maximum altitude across waypoints on this line (metres, in the plan's datum)
/// - `target_gsd_m`: target ground sample distance in metres/pixel
/// - `side_lap_pct`: required side overlap percentage
/// - `end_lap_pct`: required along-track (end) overlap percentage
pub fn write_geojson(
    path: &Path,
    plan: &FlightPlan,
    safety_warning: Option<&str>,
    attribution: Option<&str>,
    disclaimer: Option<&str>,
) -> Result<(), IoError> {
    let collection = flight_plan_to_geojson(plan, safety_warning, attribution, disclaimer);
    let json_bytes = serde_json::to_vec_pretty(&collection)
        .map_err(|e| IoError::Serialization(e.to_string()))?;
    let mut file = std::fs::File::create(path)?;
    file.write_all(&json_bytes)?;
    Ok(())
}

/// Convert a FlightPlan to a serde_json Value representing a GeoJSON
/// FeatureCollection. Separated from `write_geojson` so callers can embed
/// the collection in larger JSON documents or write to arbitrary writers.
pub fn flight_plan_to_geojson(
    plan: &FlightPlan,
    safety_warning: Option<&str>,
    attribution: Option<&str>,
    disclaimer: Option<&str>,
) -> Value {
    /*
     * Derive the datum tag from the first line. All lines in a plan are
     * expected to share the same datum (they all come from the same DEM pass),
     * so sampling the first is sufficient. Fall back to "EGM2008" for empty
     * plans to keep the output valid.
     */
    let datum_str = plan
        .lines
        .first()
        .map(|l| l.altitude_datum.as_str())
        .unwrap_or("EGM2008");

    let features: Vec<Value> = plan
        .lines
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            /*
             * iter_lonlat_deg() enforces (longitude, latitude) order and
             * radians→degrees conversion at the single serialisation boundary.
             * RFC 7946 §3.1.1: position = [longitude, latitude, elevation_msl].
             * The third element (elevation) preserves terrain-following altitude
             * data so the file represents the actual computed 3D flight path.
             */
            let elevs = line.elevations();
            let coords: Vec<Value> = line
                .iter_lonlat_deg()
                .zip(elevs.iter())
                .map(|((lon, lat), &elev)| json!([lon, lat, elev]))
                .collect();

            /*
             * Report actual per-line min/max MSL altitude derived from the
             * terrain-adjusted waypoints, not the nominal planning altitude.
             * After a DEM pass the two values will differ; over flat terrain
             * they are equal. This replaces the misleading plan-level scalar
             * that did not reflect the actual flown profile.
             */
            let min_alt = elevs.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_alt = elevs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

            json!({
                "type": "Feature",
                "geometry": {
                    "type": "LineString",
                    "coordinates": coords
                },
                "properties": {
                    "line_index": idx,
                    "altitude_datum": line.altitude_datum.as_str(),
                    "min_altitude_m": min_alt,
                    "max_altitude_m": max_alt,
                    "target_gsd_m": plan.params.target_gsd_m,
                    "side_lap_pct": plan.params.side_lap_percent,
                    "end_lap_pct": plan.params.end_lap_percent
                }
            })
        })
        .collect();

    let mut collection = json!({
        "type": "FeatureCollection",
        "altitude_datum": datum_str,
        "features": features
    });

    /*
     * Inject top-level metadata fields when present. All three are absent
     * when the corresponding source data was not used (no terrain, DTM source).
     * Consumers must surface attribution and disclaimer to satisfy the
     * Copernicus DEM open-access license requirements.
     */
    if let Some(warning) = safety_warning {
        collection["safety_warning"] = Value::String(warning.to_owned());
    }
    if let Some(attr) = attribution {
        collection["attribution"] = Value::String(attr.to_owned());
    }
    if let Some(disc) = disclaimer {
        collection["disclaimer"] = Value::String(disc.to_owned());
    }

    collection
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
    fn feature_collection_structure() {
        let plan = minimal_plan(3, 4);
        let fc = flight_plan_to_geojson(&plan, None, None, None);
        assert_eq!(fc["type"], "FeatureCollection");
        let features = fc["features"].as_array().unwrap();
        assert_eq!(features.len(), 3, "one Feature per FlightLine");
        for f in features {
            assert_eq!(f["type"], "Feature");
            assert_eq!(f["geometry"]["type"], "LineString");
            let coords = f["geometry"]["coordinates"].as_array().unwrap();
            assert_eq!(coords.len(), 4, "four waypoints per line");
        }
    }

    #[test]
    fn coordinate_precision_roundtrip() {
        // Coordinates must survive JSON serialization within 1e-9 degrees / 1e-9 m.
        let plan = minimal_plan(1, 2);
        let fc = flight_plan_to_geojson(&plan, None, None, None);
        let coords = fc["features"][0]["geometry"]["coordinates"]
            .as_array()
            .unwrap();
        // First line, first point: lon = 0.0 deg (base), lat = 51.0 deg, elev = 100.0 m
        let lon0: f64 = coords[0][0].as_f64().unwrap();
        let lat0: f64 = coords[0][1].as_f64().unwrap();
        let ele0: f64 = coords[0][2].as_f64().unwrap();
        assert_abs_diff_eq!(lon0, 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(lat0, 51.0, epsilon = 1e-9);
        assert_abs_diff_eq!(ele0, 100.0, epsilon = 1e-9);
    }

    #[test]
    fn coordinate_order_is_lon_lat() {
        // RFC 7946: coordinates are [longitude, latitude, elevation], not [latitude, longitude].
        // We test with expected values rather than magnitude comparison to catch edge cases
        // near the equator or antimeridian.
        let plan = minimal_plan(1, 2);
        let fc = flight_plan_to_geojson(&plan, None, None, None);
        let first_coord = &fc["features"][0]["geometry"]["coordinates"][0];
        let lon = first_coord[0].as_f64().unwrap();
        let lat = first_coord[1].as_f64().unwrap();
        let ele = first_coord[2].as_f64().unwrap();
        // First waypoint: lon_base=0, pi=0 → lon=0.0°, lat=51.0°, ele=100.0m
        assert_abs_diff_eq!(lon, 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(lat, 51.0, epsilon = 1e-9);
        assert_abs_diff_eq!(ele, 100.0, epsilon = 1e-9);
    }

    #[test]
    fn properties_carry_plan_metadata() {
        let plan = minimal_plan(2, 3);
        let fc = flight_plan_to_geojson(&plan, None, None, None);
        let props = &fc["features"][0]["properties"];
        assert_eq!(props["line_index"], 0);
        assert_abs_diff_eq!(
            props["target_gsd_m"].as_f64().unwrap(),
            0.05,
            epsilon = 1e-12
        );
        assert_abs_diff_eq!(
            props["side_lap_pct"].as_f64().unwrap(),
            60.0,
            epsilon = 1e-12
        );
        assert_abs_diff_eq!(
            props["end_lap_pct"].as_f64().unwrap(),
            80.0,
            epsilon = 1e-12
        );
        // minimal_plan pushes elevation=100.0 for every waypoint, so both min and max are 100.0
        assert_abs_diff_eq!(
            props["min_altitude_m"].as_f64().unwrap(),
            100.0,
            epsilon = 1e-12
        );
        assert_abs_diff_eq!(
            props["max_altitude_m"].as_f64().unwrap(),
            100.0,
            epsilon = 1e-12
        );
    }

    #[test]
    fn write_geojson_creates_valid_file() {
        let plan = minimal_plan(2, 3);
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.geojson");
        write_geojson(&path, &plan, None, None, None).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["type"], "FeatureCollection");
        assert_eq!(parsed["features"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn line_index_property_increments() {
        let plan = minimal_plan(3, 2);
        let fc = flight_plan_to_geojson(&plan, None, None, None);
        for (i, feature) in fc["features"].as_array().unwrap().iter().enumerate() {
            assert_eq!(feature["properties"]["line_index"], i);
        }
    }

    #[test]
    fn safety_warning_absent_when_none() {
        let plan = minimal_plan(1, 2);
        let fc = flight_plan_to_geojson(&plan, None, None, None);
        assert!(
            fc.get("safety_warning").is_none(),
            "safety_warning must be absent when no warning is provided"
        );
    }

    #[test]
    fn safety_warning_present_when_some() {
        let plan = minimal_plan(1, 2);
        let warning = "test warning text";
        let fc = flight_plan_to_geojson(&plan, Some(warning), None, None);
        assert_eq!(
            fc["safety_warning"].as_str().unwrap(),
            warning,
            "safety_warning must match the provided text"
        );
    }

    #[test]
    fn attribution_absent_when_none() {
        let plan = minimal_plan(1, 2);
        let fc = flight_plan_to_geojson(&plan, None, None, None);
        assert!(fc.get("attribution").is_none());
        assert!(fc.get("disclaimer").is_none());
    }

    #[test]
    fn attribution_present_when_some() {
        let plan = minimal_plan(1, 2);
        let fc = flight_plan_to_geojson(
            &plan,
            None,
            Some("test attribution"),
            Some("test disclaimer"),
        );
        assert_eq!(fc["attribution"].as_str().unwrap(), "test attribution");
        assert_eq!(fc["disclaimer"].as_str().unwrap(), "test disclaimer");
    }
}
