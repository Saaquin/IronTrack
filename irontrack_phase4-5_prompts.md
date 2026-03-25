# IronTrack — Phase 4 Prompt Playbook (Post-Research)

> Phase 4 focus: Correctness, Safety, and Autopilot Integration.
>
> Prerequisites: Phases 0–3 complete. EGM2008 and Copernicus ingestion are already implemented.

---

## PHASE 4A: High-Precision Projections — Karney-Krüger UTM

```
Read docs/02_geodesy_foundations.md and docs/formula_reference.md §3.2.

We currently work in geographic coordinates (radians). For local engineering and
survey-grade precision, we need to support UTM projections using the 
Karney-Krüger 6th-order series. This replaces legacy Redfearn truncations.

Implement src/geodesy/utm.rs:

1. Use the WGS84 constants from types.rs (n, a, f, etc.).
2. Implement `to_utm(lat_deg, lon_deg) -> Result<UtmCoord>`:
   - Calculate zone and central meridian.
   - Apply the 6th-order series for ξ' and η' (Doc 02 §3.2).
   - Apply scale factor k0 = 0.9996 and False Easting 500,000.
3. Implement `from_utm(utm: &UtmCoord) -> Result<(f64, f64)>` (Inverse).
4. Register the module in src/geodesy/mod.rs.
5. Tests: Verify against the Karney-Krüger test set (Doc 02 reference values).
   Ensure sub-millimetre precision within the UTM zone.
```

---

## PHASE 4B: Safety — Datum-Tagged Altitudes

```
Read docs/11_aerial_survey_autopilot_formats.md.

Our FlightLine struct stores raw altitudes. This is dangerous; different 
autopilots expect different datums (WGS84 Ellipsoidal vs EGM96 vs AGL).

Modify src/types.rs and src/photogrammetry/flightlines.rs:

1. Add `enum AltitudeDatum { Egm2008, Egm96, Wgs84Ellipsoidal, Agl }`.
2. Add `altitude_datum: AltitudeDatum` to `FlightLine`.
3. Implement `FlightLine::to_datum(&self, target: AltitudeDatum, engine: &TerrainEngine) -> Result<FlightLine>`.
   - Egm2008 -> WGS84: Add N from EGM2008 model.
   - Egm2008 -> Agl: Subtract terrain height H from waypoints.
4. Update CLI `plan` to default to Egm2008 (matches Copernicus).
5. Update GeoPackage/GeoJSON exporters to write the datum name into metadata.
```

---

## PHASE 4C: Safety — DSM Penetration Warnings

```
Read docs/09_dsm_vs_dtm.md.

Copernicus is a DSM. X-band radar penetrates 2-8m into canopy. 
We MUST disclose this collision risk to users.

1. Update `src/main.rs` summary output:
   If `terrain` is used, print a bold "[SAFETY WARNING]: Copernicus DEM is a DSM.
   Altitudes include canopy top. AGL clearance over forests may be 
   UNDERESTIMATED by 2-8m. Verify clearance manually."
2. Inject this warning into GeoPackage `gpkg_contents` description and 
   GeoJSON `properties.safety_note`.
```

---

## PHASE 4D: Legal — Copernicus Attribution

```
Read docs/08_copernicus_dem.md.

The Copernicus license requires mandatory attribution.

1. Add `COPERNICUS_ATTRIBUTION` and `COPERNICUS_DISCLAIMER` strings to 
   a new `src/legal.rs`.
2. Print these in the CLI summary.
3. Write them to a new GeoPackage table `irontrack_metadata` and 
   GeoJSON top-level `attribution` field.
```

---

## PHASE 4E: Integration — QGroundControl Mission Export

```
Read docs/11_aerial_survey_autopilot_formats.md.

Implement src/io/qgc.rs:
1. Export `FlightPlan` to `.plan` (JSON).
2. Map waypoints to `MAV_CMD_NAV_WAYPOINT` (16).
3. Use `AltitudeMode: 2` (AMSL) and provide ellipsoidal heights if possible.
4. Add camera trigger: `MAV_CMD_DO_SET_CAM_TRIGG_DIST` at line start/end.
5. Add `--qgc-plan <PATH>` to CLI.
```

---

## PHASE 4F: Integration — DJI WPML/KMZ Export

```
Read docs/11_aerial_survey_autopilot_formats.md.

Implement src/io/dji.rs:
1. Export `FlightPlan` to DJI `.kmz` (ZIP containing `template.kml` and `waylines.wpml`).
2. CRITICAL: DJI expects EGM96. Use `to_datum(AltitudeDatum::Egm96)` 
   before export.
3. Add `--dji-kmz <PATH>` to CLI.
```

---

## PHASE 4G: Sensors — LiDAR Math & Lines

```
Read docs/13_lidar_flight_planning.md.

1. Create `src/photogrammetry/lidar.rs`.
2. Implement `LidarSensorParams` (PRR, Scan Rate, FOV).
3. Implement `point_density(agl, speed)` and `required_speed_for_density`.
4. Update `flightlines.rs` to support LiDAR-based spacing (typically 50% sidelap).
5. Add CLI flags for `--lidar-prr`, `--target-density`, etc.
```

---

## PHASE 4H: Boundary — KML/KMZ Import

```
Read docs/11_aerial_survey_autopilot_formats.md §KML.

1. Implement `src/io/kml.rs` using `quick-xml`.
2. Parse `<Polygon>` boundaries.
3. Update `flightlines.rs`: generate lines over bbox, then clip to the Polygon 
   using a point-in-polygon (ray casting) test.
4. Add `--boundary <PATH.kml>` to CLI as an alternative to `--min-lat` etc.
```

---

## PHASE 5: Kinematics & Optimization

5A: **Corridors** — Implement linear infrastructure following (Linestrings).
5B: **Turns** — Dubins Path generation (Teardrops/Bowties) based on `--max-bank-angle`.
5C: **TSP** — Route sorting to minimize battery consumption.
