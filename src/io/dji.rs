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

// DJI WPML/KMZ mission file exporter.
//
// Produces a DJI-compatible .kmz archive containing:
//   wpmz/template.kml  — mission config (missionConfig, coordinate system)
//   wpmz/waylines.wpml — executable waypoint data with action groups
//
// CRITICAL SAFETY CONTEXT: DJI autopilots do NOT calculate terrain offsets
// in flight. They execute the exact static altitude values pre-compiled by
// the planning software. All altitudes MUST be in EGM96 orthometric height.
// The caller is responsible for converting via FlightLine::to_datum(Egm96)
// before invoking this exporter.
//
// Reference: docs/11_aerial_survey_autopilot_formats.md §DJI WPML

use std::io::{Cursor, Write};
use std::path::Path;

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::error::IoError;
use crate::photogrammetry::FlightPlan;
use crate::types::AltitudeDatum;

/*
 * DJI WPML XML namespace. The wpml: prefix is used on all DJI-proprietary
 * elements within both template.kml and waylines.wpml.
 */
const WPML_NS: &str = "http://www.dji.com/wpmz/1.0.0";
const KML_NS: &str = "http://www.opengis.net/kml/2.2";

/*
 * Default mission parameters. These match conservative defaults for survey
 * operations — the operator should adjust via DJI Pilot 2 before flight.
 */
const DEFAULT_TAKEOFF_SECURITY_HEIGHT: f64 = 20.0;
const DEFAULT_TRANSITIONAL_SPEED: f64 = 15.0;
const DEFAULT_AUTO_FLIGHT_SPEED: f64 = 10.0;

/// Serialize `plan` to a DJI .kmz archive and write to `path`.
///
/// # Datum requirements
///
/// DJI autopilots execute static altitude values in EGM96 orthometric height.
/// The plan **must** be converted to `AltitudeDatum::Egm96` before calling
/// this function. Returns `IoError::Serialization` if the datum is wrong.
///
/// All lines must share the same altitude datum.
pub fn write_dji_kmz(path: &Path, plan: &FlightPlan) -> Result<(), IoError> {
    validate_datum(plan)?;

    let template_xml = build_template_kml(plan)?;
    let waylines_xml = build_waylines_wpml(plan)?;

    /*
     * Package the two XML files into a ZIP archive with the .kmz extension.
     * DJI requires the wpmz/ directory prefix inside the archive.
     */
    let file = std::fs::File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("wpmz/template.kml", options)
        .map_err(|e| IoError::Serialization(format!("ZIP error: {e}")))?;
    zip.write_all(&template_xml)?;

    zip.start_file("wpmz/waylines.wpml", options)
        .map_err(|e| IoError::Serialization(format!("ZIP error: {e}")))?;
    zip.write_all(&waylines_xml)?;

    zip.finish()
        .map_err(|e| IoError::Serialization(format!("ZIP finish error: {e}")))?;

    Ok(())
}

/// Validate that the plan's altitude datum is EGM96 — the only datum DJI accepts.
fn validate_datum(plan: &FlightPlan) -> Result<(), IoError> {
    if plan.lines.is_empty() {
        return Ok(());
    }

    let first_datum = plan.lines[0].altitude_datum;

    if plan.lines.iter().any(|l| l.altitude_datum != first_datum) {
        return Err(IoError::Serialization(
            "DJI export requires all flight lines to share the same altitude datum".into(),
        ));
    }

    if first_datum != AltitudeDatum::Egm96 {
        return Err(IoError::Serialization(format!(
            "DJI export requires EGM96 altitudes, but plan uses {}. \
             Convert with FlightLine::to_datum(AltitudeDatum::Egm96) before export.",
            first_datum.as_str()
        )));
    }

    Ok(())
}

/// Build the template.kml XML document.
///
/// Contains mission-level configuration: fly-to mode, finish action,
/// RC-lost behaviour, takeoff height, transitional speed, height mode.
fn build_template_kml(plan: &FlightPlan) -> Result<Vec<u8>, IoError> {
    let mut buf = Cursor::new(Vec::new());
    let mut w = Writer::new_with_indent(&mut buf, b' ', 2);

    // XML declaration
    w.write_event(Event::Decl(quick_xml::events::BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        None,
    )))
    .map_err(xml_err)?;

    // <kml xmlns="..." xmlns:wpml="...">
    let mut kml = BytesStart::new("kml");
    kml.push_attribute(("xmlns", KML_NS));
    kml.push_attribute(("xmlns:wpml", WPML_NS));
    w.write_event(Event::Start(kml)).map_err(xml_err)?;

    // <Document>
    w.write_event(Event::Start(BytesStart::new("Document")))
        .map_err(xml_err)?;

    // <wpml:missionConfig>
    w.write_event(Event::Start(BytesStart::new("wpml:missionConfig")))
        .map_err(xml_err)?;

    write_text_element(&mut w, "wpml:flyToWaylineMode", "safely")?;
    write_text_element(&mut w, "wpml:finishAction", "goHome")?;
    write_text_element(&mut w, "wpml:exitOnRCLost", "executeLostAction")?;
    write_text_element(
        &mut w,
        "wpml:takeOffSecurityHeight",
        &format!("{DEFAULT_TAKEOFF_SECURITY_HEIGHT:.1}"),
    )?;
    write_text_element(
        &mut w,
        "wpml:globalTransitionalSpeed",
        &format!("{DEFAULT_TRANSITIONAL_SPEED:.1}"),
    )?;
    write_text_element(&mut w, "wpml:executeHeightMode", "EGM96")?;

    // </wpml:missionConfig>
    w.write_event(Event::End(BytesEnd::new("wpml:missionConfig")))
        .map_err(xml_err)?;

    /*
     * One Folder per flight line, containing Placemarks for visual reference
     * in KML viewers. The template.kml is the "business logic" layer — the
     * autopilot ignores it, but operators use it for pre-flight review.
     */
    for (line_idx, line) in plan.lines.iter().enumerate() {
        w.write_event(Event::Start(BytesStart::new("Folder")))
            .map_err(xml_err)?;
        write_text_element(&mut w, "wpml:templateId", &line_idx.to_string())?;

        w.write_event(Event::Start(BytesStart::new(
            "wpml:waylineCoordinateSysParam",
        )))
        .map_err(xml_err)?;
        write_text_element(&mut w, "wpml:coordinateMode", "WGS84")?;
        write_text_element(&mut w, "wpml:heightMode", "EGM96")?;
        w.write_event(Event::End(BytesEnd::new("wpml:waylineCoordinateSysParam")))
            .map_err(xml_err)?;

        let lats = line.lats();
        let lons = line.lons();
        let elevs = line.elevations();
        for i in 0..line.len() {
            let lat_deg = lats[i].to_degrees();
            let lon_deg = lons[i].to_degrees();
            let alt = elevs[i];

            w.write_event(Event::Start(BytesStart::new("Placemark")))
                .map_err(xml_err)?;
            w.write_event(Event::Start(BytesStart::new("Point")))
                .map_err(xml_err)?;
            write_text_element(
                &mut w,
                "coordinates",
                &format!("{lon_deg:.9},{lat_deg:.9},{alt:.3}"),
            )?;
            w.write_event(Event::End(BytesEnd::new("Point")))
                .map_err(xml_err)?;
            write_text_element(&mut w, "wpml:index", &i.to_string())?;
            w.write_event(Event::End(BytesEnd::new("Placemark")))
                .map_err(xml_err)?;
        }

        w.write_event(Event::End(BytesEnd::new("Folder")))
            .map_err(xml_err)?;
    }

    // </Document>
    w.write_event(Event::End(BytesEnd::new("Document")))
        .map_err(xml_err)?;
    // </kml>
    w.write_event(Event::End(BytesEnd::new("kml")))
        .map_err(xml_err)?;

    Ok(buf.into_inner())
}

/// Build the waylines.wpml XML document.
///
/// Contains the executable waypoint data: per-wayline folders with Placemarks
/// for each waypoint, plus actionGroup elements for camera triggering.
fn build_waylines_wpml(plan: &FlightPlan) -> Result<Vec<u8>, IoError> {
    let mut buf = Cursor::new(Vec::new());
    let mut w = Writer::new_with_indent(&mut buf, b' ', 2);

    w.write_event(Event::Decl(quick_xml::events::BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        None,
    )))
    .map_err(xml_err)?;

    let mut kml = BytesStart::new("kml");
    kml.push_attribute(("xmlns", KML_NS));
    kml.push_attribute(("xmlns:wpml", WPML_NS));
    w.write_event(Event::Start(kml)).map_err(xml_err)?;

    w.write_event(Event::Start(BytesStart::new("Document")))
        .map_err(xml_err)?;

    /*
     * Global action group counter. DJI requires unique actionGroupId values
     * across the entire document, not per-wayline.
     */
    let mut action_group_id: u32 = 0;

    for (line_idx, line) in plan.lines.iter().enumerate() {
        let n = line.len();
        if n == 0 {
            continue;
        }

        w.write_event(Event::Start(BytesStart::new("Folder")))
            .map_err(xml_err)?;
        write_text_element(&mut w, "wpml:templateId", &line_idx.to_string())?;
        write_text_element(&mut w, "wpml:executeHeightMode", "EGM96")?;
        write_text_element(&mut w, "wpml:waylineId", &line_idx.to_string())?;
        write_text_element(
            &mut w,
            "wpml:autoFlightSpeed",
            &format!("{DEFAULT_AUTO_FLIGHT_SPEED:.1}"),
        )?;

        let lats = line.lats();
        let lons = line.lons();
        let elevs = line.elevations();

        for i in 0..n {
            let lat_deg = lats[i].to_degrees();
            let lon_deg = lons[i].to_degrees();
            let alt = elevs[i];

            w.write_event(Event::Start(BytesStart::new("Placemark")))
                .map_err(xml_err)?;

            w.write_event(Event::Start(BytesStart::new("Point")))
                .map_err(xml_err)?;
            write_text_element(&mut w, "coordinates", &format!("{lon_deg:.9},{lat_deg:.9}"))?;
            w.write_event(Event::End(BytesEnd::new("Point")))
                .map_err(xml_err)?;

            write_text_element(&mut w, "wpml:index", &i.to_string())?;
            write_text_element(&mut w, "wpml:executeHeight", &format!("{alt:.3}"))?;
            write_text_element(
                &mut w,
                "wpml:waypointSpeed",
                &format!("{DEFAULT_AUTO_FLIGHT_SPEED:.1}"),
            )?;

            // Heading: follow wayline direction
            w.write_event(Event::Start(BytesStart::new("wpml:waypointHeadingParam")))
                .map_err(xml_err)?;
            write_text_element(&mut w, "wpml:waypointHeadingMode", "followWayline")?;
            w.write_event(Event::End(BytesEnd::new("wpml:waypointHeadingParam")))
                .map_err(xml_err)?;

            // Turn mode: coordinate turn for smooth survey flight
            w.write_event(Event::Start(BytesStart::new("wpml:waypointTurnParam")))
                .map_err(xml_err)?;
            write_text_element(&mut w, "wpml:waypointTurnMode", "coordinateTurn")?;
            write_text_element(&mut w, "wpml:waypointTurnDampingDist", "0")?;
            w.write_event(Event::End(BytesEnd::new("wpml:waypointTurnParam")))
                .map_err(xml_err)?;

            w.write_event(Event::End(BytesEnd::new("Placemark")))
                .map_err(xml_err)?;
        }

        /*
         * Camera trigger action group. Spans all waypoints in this flight
         * line (index 0 to n-1). Uses reachPoint trigger — the camera fires
         * once at each waypoint. This matches the photogrammetric waypoint
         * spacing computed by FlightPlanParams::photo_interval().
         */
        w.write_event(Event::Start(BytesStart::new("wpml:actionGroup")))
            .map_err(xml_err)?;
        write_text_element(&mut w, "wpml:actionGroupId", &action_group_id.to_string())?;
        write_text_element(&mut w, "wpml:actionGroupStartIndex", "0")?;
        write_text_element(&mut w, "wpml:actionGroupEndIndex", &(n - 1).to_string())?;
        write_text_element(&mut w, "wpml:actionGroupMode", "sequence")?;

        // Trigger condition
        w.write_event(Event::Start(BytesStart::new("wpml:actionTrigger")))
            .map_err(xml_err)?;
        write_text_element(&mut w, "wpml:actionTriggerType", "reachPoint")?;
        w.write_event(Event::End(BytesEnd::new("wpml:actionTrigger")))
            .map_err(xml_err)?;

        // Action: take photo
        w.write_event(Event::Start(BytesStart::new("wpml:action")))
            .map_err(xml_err)?;
        write_text_element(&mut w, "wpml:actionId", "0")?;
        write_text_element(&mut w, "wpml:actionActuatorFunc", "takePhoto")?;
        w.write_event(Event::Start(BytesStart::new(
            "wpml:actionActuatorFuncParam",
        )))
        .map_err(xml_err)?;
        write_text_element(&mut w, "wpml:payloadPositionIndex", "0")?;
        w.write_event(Event::End(BytesEnd::new("wpml:actionActuatorFuncParam")))
            .map_err(xml_err)?;
        w.write_event(Event::End(BytesEnd::new("wpml:action")))
            .map_err(xml_err)?;

        w.write_event(Event::End(BytesEnd::new("wpml:actionGroup")))
            .map_err(xml_err)?;

        action_group_id += 1;

        w.write_event(Event::End(BytesEnd::new("Folder")))
            .map_err(xml_err)?;
    }

    w.write_event(Event::End(BytesEnd::new("Document")))
        .map_err(xml_err)?;
    w.write_event(Event::End(BytesEnd::new("kml")))
        .map_err(xml_err)?;

    Ok(buf.into_inner())
}

/// Write a simple XML element: `<tag>text</tag>`.
fn write_text_element(
    w: &mut Writer<&mut Cursor<Vec<u8>>>,
    tag: &str,
    text: &str,
) -> Result<(), IoError> {
    w.write_event(Event::Start(BytesStart::new(tag)))
        .map_err(xml_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(xml_err)?;
    w.write_event(Event::End(BytesEnd::new(tag)))
        .map_err(xml_err)?;
    Ok(())
}

/// Convert a quick_xml error to IoError.
fn xml_err(e: quick_xml::Error) -> IoError {
    IoError::Serialization(format!("XML error: {e}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::tempdir;

    use crate::photogrammetry::flightlines::FlightPlanParams;
    use crate::photogrammetry::FlightPlan;
    use crate::types::{FlightLine, SensorParams};

    /// Create a minimal flight plan with EGM96 datum — the datum required
    /// by DJI exports.
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
                    line.push(lat, lon, 100.0 + li as f64 * 10.0);
                }
                line.altitude_datum = AltitudeDatum::Egm96;
                line
            })
            .collect();
        FlightPlan {
            params,
            azimuth_deg: 0.0,
            lines,
        }
    }

    /// Helper: unzip a .kmz and return (template.kml, waylines.wpml) as strings.
    fn unzip_kmz(path: &Path) -> (String, String) {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();

        let mut template = String::new();
        archive
            .by_name("wpmz/template.kml")
            .unwrap()
            .read_to_string(&mut template)
            .unwrap();

        let mut waylines = String::new();
        archive
            .by_name("wpmz/waylines.wpml")
            .unwrap()
            .read_to_string(&mut waylines)
            .unwrap();

        (template, waylines)
    }

    #[test]
    fn kmz_contains_required_files() {
        let plan = minimal_plan(2, 3);
        let dir = tempdir().unwrap();
        let path = dir.path().join("mission.kmz");
        write_dji_kmz(&path, &plan).unwrap();

        let file = std::fs::File::open(&path).unwrap();
        let archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<&str> = archive.file_names().collect();
        assert!(names.contains(&"wpmz/template.kml"), "missing template.kml");
        assert!(
            names.contains(&"wpmz/waylines.wpml"),
            "missing waylines.wpml"
        );
    }

    #[test]
    fn template_kml_has_mission_config() {
        let plan = minimal_plan(1, 2);
        let dir = tempdir().unwrap();
        let path = dir.path().join("mission.kmz");
        write_dji_kmz(&path, &plan).unwrap();

        let (template, _) = unzip_kmz(&path);
        assert!(template.contains("<wpml:flyToWaylineMode>safely</wpml:flyToWaylineMode>"));
        assert!(template.contains("<wpml:finishAction>goHome</wpml:finishAction>"));
        assert!(template.contains("<wpml:exitOnRCLost>executeLostAction</wpml:exitOnRCLost>"));
        assert!(template.contains("<wpml:executeHeightMode>EGM96</wpml:executeHeightMode>"));
        assert!(template.contains("<wpml:heightMode>EGM96</wpml:heightMode>"));
    }

    #[test]
    fn waylines_wpml_has_waypoints_with_correct_altitude() {
        let plan = minimal_plan(1, 2);
        let dir = tempdir().unwrap();
        let path = dir.path().join("mission.kmz");
        write_dji_kmz(&path, &plan).unwrap();

        let (_, waylines) = unzip_kmz(&path);

        /*
         * Line 0 has elevation 100.0 for all waypoints.
         * Verify the executeHeight element carries this value.
         */
        assert!(
            waylines.contains("<wpml:executeHeight>100.000</wpml:executeHeight>"),
            "waylines must contain altitude 100.000"
        );
        assert!(waylines.contains("<wpml:executeHeightMode>EGM96</wpml:executeHeightMode>"));
    }

    #[test]
    fn waylines_wpml_has_action_group_with_take_photo() {
        let plan = minimal_plan(1, 3);
        let dir = tempdir().unwrap();
        let path = dir.path().join("mission.kmz");
        write_dji_kmz(&path, &plan).unwrap();

        let (_, waylines) = unzip_kmz(&path);
        assert!(waylines.contains("<wpml:actionTriggerType>reachPoint</wpml:actionTriggerType>"));
        assert!(waylines.contains("<wpml:actionActuatorFunc>takePhoto</wpml:actionActuatorFunc>"));
        assert!(waylines.contains("<wpml:actionGroupStartIndex>0</wpml:actionGroupStartIndex>"));
        assert!(waylines.contains("<wpml:actionGroupEndIndex>2</wpml:actionGroupEndIndex>"));
    }

    #[test]
    fn waylines_coordinates_are_lon_lat_degrees() {
        /*
         * DJI KML coordinates follow the KML spec: longitude,latitude
         * (decimal degrees, comma-separated). Verify the first waypoint
         * of line 0 (lon=0.0, lat=51.0).
         */
        let plan = minimal_plan(1, 2);
        let dir = tempdir().unwrap();
        let path = dir.path().join("mission.kmz");
        write_dji_kmz(&path, &plan).unwrap();

        let (_, waylines) = unzip_kmz(&path);
        assert!(
            waylines.contains("0.000000000,51.000000000"),
            "coordinates must be lon,lat in degrees"
        );
    }

    #[test]
    fn multi_line_plan_produces_multiple_folders() {
        let plan = minimal_plan(3, 2);
        let dir = tempdir().unwrap();
        let path = dir.path().join("mission.kmz");
        write_dji_kmz(&path, &plan).unwrap();

        let (_, waylines) = unzip_kmz(&path);
        let folder_count = waylines.matches("<wpml:waylineId>").count();
        assert_eq!(folder_count, 3, "one wayline folder per flight line");
    }

    #[test]
    fn egm2008_datum_rejected() {
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
        // Default datum is EGM2008 — must be rejected.
        let plan = FlightPlan {
            params,
            azimuth_deg: 0.0,
            lines: vec![line],
        };

        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.kmz");
        let result = write_dji_kmz(&path, &plan);
        assert!(
            result.is_err(),
            "EGM2008 plan must be rejected for DJI export"
        );
    }

    #[test]
    fn wgs84_datum_rejected() {
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
        line.altitude_datum = AltitudeDatum::Wgs84Ellipsoidal;
        let plan = FlightPlan {
            params,
            azimuth_deg: 0.0,
            lines: vec![line],
        };

        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.kmz");
        let result = write_dji_kmz(&path, &plan);
        assert!(
            result.is_err(),
            "WGS84 plan must be rejected for DJI export"
        );
    }

    #[test]
    fn empty_plan_creates_valid_kmz() {
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

        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.kmz");
        write_dji_kmz(&path, &plan).unwrap();

        let (template, waylines) = unzip_kmz(&path);
        assert!(template.contains("<wpml:missionConfig>"));
        assert!(waylines.contains("<Document"));
    }

    #[test]
    fn different_altitudes_per_line_preserved() {
        /*
         * minimal_plan sets elevation = 100 + li*10 per line.
         * Line 0 = 100.0, Line 1 = 110.0. Verify both appear in WPML.
         */
        let plan = minimal_plan(2, 1);
        let dir = tempdir().unwrap();
        let path = dir.path().join("mission.kmz");
        write_dji_kmz(&path, &plan).unwrap();

        let (_, waylines) = unzip_kmz(&path);
        assert!(
            waylines.contains("<wpml:executeHeight>100.000</wpml:executeHeight>"),
            "line 0 altitude"
        );
        assert!(
            waylines.contains("<wpml:executeHeight>110.000</wpml:executeHeight>"),
            "line 1 altitude"
        );
    }
}
