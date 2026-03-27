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

//! v0.2 integration tests — mandatory gate before tagging the release.
//!
//! These tests exercise the full end-to-end pipeline including all v0.2
//! features: GeoPackage metadata, GeoJSON attribution, QGC .plan export,
//! DJI .kmz export, DSM safety warning, KML boundary import, and datum
//! round-trip accuracy.

use std::io::Read;

use approx::assert_abs_diff_eq;
use tempfile::tempdir;

use irontrack::gpkg::GeoPackage;
use irontrack::io::{parse_boundary, write_dji_kmz, write_geojson, write_qgc_plan};
use irontrack::math::geometry::Polygon;
use irontrack::photogrammetry::flightlines::{
    clip_to_polygon, generate_flight_lines, BoundingBox, FlightPlan, FlightPlanParams,
};
use irontrack::types::{AltitudeDatum, FlightLine, SensorParams};

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

fn phantom4pro() -> SensorParams {
    SensorParams {
        focal_length_mm: 8.8,
        sensor_width_mm: 13.2,
        sensor_height_mm: 8.8,
        image_width_px: 5472,
        image_height_px: 3648,
    }
}

fn england_bbox() -> BoundingBox {
    BoundingBox {
        min_lat: 51.500,
        min_lon: -0.100,
        max_lat: 51.510,
        max_lon: -0.080,
    }
}

fn standard_params() -> FlightPlanParams {
    FlightPlanParams::new(phantom4pro(), 0.05)
        .unwrap()
        .with_side_lap(60.0)
        .unwrap()
        .with_end_lap(80.0)
        .unwrap()
}

// ---------------------------------------------------------------------------
// Phase 4J Test 1: Full pipeline — GeoPackage + GeoJSON + QGC + DJI
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_all_exports() {
    let dir = tempdir().unwrap();
    let gpkg_path = dir.path().join("plan.gpkg");
    let geojson_path = dir.path().join("plan.geojson");
    let qgc_path = dir.path().join("plan.plan");
    let dji_path = dir.path().join("plan.kmz");

    let plan = generate_flight_lines(&england_bbox(), 0.0, &standard_params()).unwrap();
    assert!(!plan.lines.is_empty());

    // --- GeoPackage --------------------------------------------------------

    let gpkg = GeoPackage::new(&gpkg_path).unwrap();
    gpkg.create_feature_table("flight_lines").unwrap();
    gpkg.insert_flight_plan("flight_lines", &plan).unwrap();

    /*
     * Verify irontrack_metadata table exists with schema_version=2.
     */
    let schema_ver: String = gpkg
        .query_metadata("schema_version")
        .unwrap()
        .expect("schema_version must exist");
    assert_eq!(schema_ver, "2");

    // --- GeoJSON -----------------------------------------------------------

    let warning = "test DSM warning";
    let attribution = "test attribution";
    let disclaimer = "test disclaimer";
    write_geojson(
        &geojson_path,
        &plan,
        Some(warning),
        Some(attribution),
        Some(disclaimer),
    )
    .unwrap();

    let fc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&geojson_path).unwrap()).unwrap();
    assert_eq!(fc["type"], "FeatureCollection");
    assert!(fc["altitude_datum"].is_string(), "altitude_datum present");
    assert_eq!(fc["safety_warning"].as_str().unwrap(), warning);
    assert_eq!(fc["attribution"].as_str().unwrap(), attribution);
    assert_eq!(fc["disclaimer"].as_str().unwrap(), disclaimer);

    // --- QGC .plan ---------------------------------------------------------

    /*
     * QGC export requires WGS84 ellipsoidal. Stamp the plan.
     */
    let mut qgc_lines = plan.lines.clone();
    for line in &mut qgc_lines {
        line.altitude_datum = AltitudeDatum::Wgs84Ellipsoidal;
    }
    let qgc_plan = FlightPlan {
        params: plan.params.clone(),
        azimuth_deg: plan.azimuth_deg,
        lines: qgc_lines,
    };
    let trigger_dist = standard_params().photo_interval();
    write_qgc_plan(&qgc_path, &qgc_plan, trigger_dist).unwrap();

    let qgc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&qgc_path).unwrap()).unwrap();
    assert_eq!(qgc["fileType"], "Plan");

    /*
     * Verify waypoint altitudes are present (WGS84 ellipsoidal via frame 0).
     */
    let items = qgc["mission"]["items"].as_array().unwrap();
    let wp = items.iter().find(|i| i["command"].as_u64() == Some(16)).unwrap();
    assert_eq!(wp["frame"], 0, "WGS84 ellipsoidal must use MAV_FRAME_GLOBAL");

    // --- DJI .kmz ----------------------------------------------------------

    /*
     * DJI export requires EGM96. Stamp the plan.
     */
    let mut dji_lines = plan.lines.clone();
    for line in &mut dji_lines {
        line.altitude_datum = AltitudeDatum::Egm96;
    }
    let dji_plan = FlightPlan {
        params: plan.params.clone(),
        azimuth_deg: plan.azimuth_deg,
        lines: dji_lines,
    };
    write_dji_kmz(&dji_path, &dji_plan).unwrap();

    let file = std::fs::File::open(&dji_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();

    let mut waylines = String::new();
    archive
        .by_name("wpmz/waylines.wpml")
        .unwrap()
        .read_to_string(&mut waylines)
        .unwrap();
    assert!(
        waylines.contains("<wpml:executeHeightMode>EGM96</wpml:executeHeightMode>"),
        "DJI altitudes must be EGM96"
    );
}

// ---------------------------------------------------------------------------
// Phase 4J Test 2: Datum round-trip
// ---------------------------------------------------------------------------

#[test]
fn datum_round_trip_egm2008_ellipsoidal_egm96_and_back() {
    /*
     * Create a FlightLine in EGM2008. Convert through the datum chain:
     *   EGM2008 → WGS84 Ellipsoidal → EGM96 → WGS84 Ellipsoidal → EGM2008
     * Verify final altitudes match originals within < 1 mm.
     *
     * This requires a TerrainEngine for AGL paths but our chain avoids AGL,
     * so we only need the geoid models (no DEM tiles).
     */
    let engine = irontrack::dem::TerrainEngine::new().unwrap();

    let mut line = FlightLine::new();
    line.push(51.5_f64.to_radians(), (-0.1_f64).to_radians(), 100.0);
    line.push(51.5_f64.to_radians(), (-0.09_f64).to_radians(), 150.0);
    line.push(51.5_f64.to_radians(), (-0.08_f64).to_radians(), 200.0);
    assert_eq!(line.altitude_datum, AltitudeDatum::Egm2008);

    let original_elevs: Vec<f64> = line.elevations().to_vec();

    // EGM2008 → WGS84 Ellipsoidal
    let line_e = line.to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine).unwrap();
    assert_eq!(line_e.altitude_datum, AltitudeDatum::Wgs84Ellipsoidal);

    /*
     * WGS84 ellipsoidal heights should differ from EGM2008 by the geoid
     * undulation (~47 m at London). Verify they're different.
     */
    for (i, &elev) in line_e.elevations().iter().enumerate() {
        assert!(
            (elev - original_elevs[i]).abs() > 1.0,
            "ellipsoidal should differ from EGM2008 by the undulation"
        );
    }

    // WGS84 Ellipsoidal → EGM96
    let line_96 = line_e.to_datum(AltitudeDatum::Egm96, &engine).unwrap();
    assert_eq!(line_96.altitude_datum, AltitudeDatum::Egm96);

    // EGM96 → WGS84 Ellipsoidal
    let line_e2 = line_96.to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine).unwrap();

    // WGS84 Ellipsoidal → EGM2008
    let line_back = line_e2.to_datum(AltitudeDatum::Egm2008, &engine).unwrap();
    assert_eq!(line_back.altitude_datum, AltitudeDatum::Egm2008);

    for (i, &elev) in line_back.elevations().iter().enumerate() {
        assert_abs_diff_eq!(
            elev,
            original_elevs[i],
            epsilon = 0.001,
        );
    }
}

// ---------------------------------------------------------------------------
// Phase 4J Test 3: KML boundary import + polygon clipping
// ---------------------------------------------------------------------------

#[test]
fn kml_boundary_import_and_polygon_clipping() {
    let dir = tempdir().unwrap();

    /*
     * Create a KML file with a polygon covering the western half of the
     * England bbox. Generate lines over the full bbox, clip to the polygon,
     * verify all surviving waypoints are inside the polygon.
     */
    let kml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Placemark>
    <Polygon>
      <outerBoundaryIs>
        <LinearRing>
          <coordinates>
            -0.100,51.499 -0.090,51.499 -0.090,51.511 -0.100,51.511 -0.100,51.499
          </coordinates>
        </LinearRing>
      </outerBoundaryIs>
    </Polygon>
  </Placemark>
</kml>"#;
    let kml_path = dir.path().join("boundary.kml");
    std::fs::write(&kml_path, kml_content).unwrap();

    let polygons = parse_boundary(&kml_path).unwrap();
    assert_eq!(polygons.len(), 1);

    let plan = generate_flight_lines(&england_bbox(), 0.0, &standard_params()).unwrap();
    let clipped = clip_to_polygon(plan, &polygons[0]);

    assert!(!clipped.lines.is_empty(), "some lines should survive");

    for line in &clipped.lines {
        for i in 0..line.len() {
            let lat = line.lats()[i].to_degrees();
            let lon = line.lons()[i].to_degrees();
            assert!(
                polygons[0].contains(lat, lon),
                "waypoint ({lat:.6}, {lon:.6}) must be inside the boundary polygon"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 4J Test 4: Concave polygon clipping removes notch
// ---------------------------------------------------------------------------

#[test]
fn concave_polygon_clipping_removes_correct_region() {
    /*
     * L-shaped polygon covering the England bbox area. The "notch" is in
     * the north-east corner. Lines passing through the notch should be
     * split or trimmed.
     */
    let poly = Polygon::new(vec![
        (51.499, -0.101),
        (51.499, -0.079),
        (51.505, -0.079),
        (51.505, -0.090),
        (51.511, -0.090),
        (51.511, -0.101),
    ]);

    let plan = generate_flight_lines(&england_bbox(), 0.0, &standard_params()).unwrap();
    let original_total: usize = plan.lines.iter().map(|l| l.len()).sum();
    let clipped = clip_to_polygon(plan, &poly);
    let clipped_total: usize = clipped.lines.iter().map(|l| l.len()).sum();

    assert!(
        clipped_total < original_total,
        "concave clipping should remove some waypoints"
    );

    for line in &clipped.lines {
        for i in 0..line.len() {
            let lat = line.lats()[i].to_degrees();
            let lon = line.lons()[i].to_degrees();
            assert!(
                poly.contains(lat, lon),
                "waypoint ({lat:.6}, {lon:.6}) must be inside the L-shaped polygon"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 4J Test 5: Polygon with hole — no waypoints inside hole
// ---------------------------------------------------------------------------

#[test]
fn polygon_with_hole_no_waypoints_in_exclusion_zone() {
    let outer = vec![
        (51.499, -0.101),
        (51.499, -0.079),
        (51.511, -0.079),
        (51.511, -0.101),
    ];
    let hole = vec![
        (51.503, -0.095),
        (51.503, -0.085),
        (51.507, -0.085),
        (51.507, -0.095),
    ];
    let poly = Polygon::with_holes(outer, vec![hole]);

    let plan = generate_flight_lines(&england_bbox(), 0.0, &standard_params()).unwrap();
    let clipped = clip_to_polygon(plan, &poly);

    for line in &clipped.lines {
        for i in 0..line.len() {
            let lat = line.lats()[i].to_degrees();
            let lon = line.lons()[i].to_degrees();
            let in_hole = lat > 51.503 && lat < 51.507 && lon > -0.095 && lon < -0.085;
            assert!(
                !in_hole,
                "waypoint ({lat:.6}, {lon:.6}) must not be inside the exclusion zone"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 4J Test 6: GeoPackage metadata fields
// ---------------------------------------------------------------------------

#[test]
fn geopackage_metadata_fields_present() {
    let dir = tempdir().unwrap();
    let gpkg_path = dir.path().join("metadata.gpkg");

    let plan = generate_flight_lines(&england_bbox(), 0.0, &standard_params()).unwrap();

    let gpkg = GeoPackage::new(&gpkg_path).unwrap();
    gpkg.create_feature_table("flight_lines").unwrap();
    gpkg.insert_flight_plan("flight_lines", &plan).unwrap();

    /*
     * Inject the DSM warning and attribution (simulating the CLI path).
     */
    gpkg.upsert_metadata("safety_dsm_warning", "test warning")
        .unwrap();
    gpkg.upsert_metadata("copernicus_attribution", "test attribution")
        .unwrap();
    gpkg.upsert_metadata("copernicus_disclaimer", "test disclaimer")
        .unwrap();

    assert_eq!(
        gpkg.query_metadata("schema_version").unwrap().unwrap(),
        "2"
    );
    assert_eq!(
        gpkg.query_metadata("safety_dsm_warning").unwrap().unwrap(),
        "test warning"
    );
    assert_eq!(
        gpkg.query_metadata("copernicus_attribution")
            .unwrap()
            .unwrap(),
        "test attribution"
    );
    assert_eq!(
        gpkg.query_metadata("copernicus_disclaimer")
            .unwrap()
            .unwrap(),
        "test disclaimer"
    );
    assert!(gpkg.query_metadata("engine_version").unwrap().is_some());
    assert!(gpkg.query_metadata("created_utc").unwrap().is_some());
}
