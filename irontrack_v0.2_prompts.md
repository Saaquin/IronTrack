# IronTrack — Post-v0.1 Prompt Playbook (Revised)

> **Release strategy:**
>
> | Release | Theme | Phases |
> |---------|-------|--------|
> | **v0.2** | Correctness, Safety, and Autopilot Integration | 4A – 4J |
> | **v0.3** | Kinematics and Route Optimization | 5A – 5C |
> | **v0.4** | Networked FMS Daemon | 6A – 6B |
> | **v0.5** | Atmospheric Kinematics and Energy Economy | 7A – 7B, 8A – 8B |
>
> Each release is independently shippable and testable.
> Prerequisites: Phases 0–3 complete. EGM2008 and Copernicus ingestion implemented.

---

# v0.2 — Correctness, Safety, and Autopilot Integration

---

## PHASE 4A: Verify & Harden — Karney-Krüger UTM

```
Read docs/02_geodesy_foundations.md and docs/formula_reference.md §3.2.

`src/geodesy/utm.rs` already exists from v0.1. This prompt is a hardening
pass, NOT a greenfield rewrite. Do not delete or rewrite the module from
scratch.

Tasks:

1. AUDIT the existing `to_utm()` and `from_utm()` implementations.
   Verify they use the full Karney-Krüger 6th-order series for ξ' and η'
   (Doc 02 §3.2), NOT a legacy Redfearn truncation. If they use fewer than
   6 series terms, upgrade them.

2. Verify the WGS84 constants sourced from types.rs match the values in
   Doc 02 (a = 6378137.0, f = 1/298.257223563). Verify n, A, α₁–α₆, β₁–β₆
   coefficients against the Karney reference.

3. Ensure scale factor k₀ = 0.9996 and False Easting = 500,000 m are
   correctly applied. Verify False Northing = 10,000,000 m for southern
   hemisphere.

4. Add or expand tests in `tests/geodesy.rs`:
   - Karney's published test vectors (at minimum: equator, mid-latitude,
     near-pole, zone boundary).
   - Round-trip: to_utm → from_utm must recover input to < 1 mm.
   - Zone-edge behavior: verify correct zone selection at ±180°, Norway/
     Svalbard exceptions are NOT required for v0.2 (document as TODO).

5. Verify the module is registered in `src/geodesy/mod.rs`.

Acceptance: All test vectors pass with sub-millimetre residuals. No
functional changes if the existing implementation is already correct.
```

---

## PHASE 4B: Safety — Datum-Tagged Altitudes

```
Read docs/05_vertical_datum_engineering.md and
docs/11_aerial_survey_autopilot_formats.md.

Our FlightLine struct stores raw altitudes with no datum tag. This is
dangerous: different autopilots expect different datums (WGS84 Ellipsoidal
vs EGM96 vs EGM2008 vs AGL). A silent datum mismatch produces flight
plans that are tens of meters wrong — a collision hazard.

CRITICAL DEPENDENCY: The vendored `egm96` crate at `vendor/egm96/` provides
EGM96 geoid undulation lookups. The existing `src/geodesy/geoid.rs` provides
EGM2008 lookups. Both must be wired into the datum conversion pipeline.

Tasks:

1. In `src/types.rs`, add:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   pub enum AltitudeDatum {
       Egm2008,
       Egm96,
       Wgs84Ellipsoidal,
       Agl,
   }
   ```

2. Add `altitude_datum: AltitudeDatum` field to the `FlightLine` struct.
   Default to `AltitudeDatum::Egm2008` (matches Copernicus DEM source).

3. Implement `FlightLine::to_datum(&self, target: AltitudeDatum, engine: &TerrainEngine) -> Result<FlightLine>`:
   - Egm2008 → Wgs84Ellipsoidal: For each waypoint, query EGM2008 geoid
     undulation N via `src/geodesy/geoid.rs`. Compute h = H + N.
   - Egm2008 → Egm96: First convert to Wgs84Ellipsoidal (add EGM2008 N),
     then convert to Egm96 orthometric (subtract EGM96 N from vendored crate).
     This is the ONLY correct two-step path. Do NOT assume EGM2008 ≈ EGM96.
   - Egm2008 → Agl: Subtract terrain surface height from waypoint elevation.
   - Wgs84Ellipsoidal → Egm2008: Subtract EGM2008 N.
   - All other conversions: Chain through Wgs84Ellipsoidal as the pivot datum.

4. Wire the vendored `vendor/egm96/` crate into `Cargo.toml` as a path
   dependency:
   ```toml
   egm96 = { path = "vendor/egm96" }
   ```

5. Implement a conversion validation test: Take a known lat/lon with
   published EGM2008 and EGM96 undulations. Verify the difference
   (typically 0.5–2.0 m globally, but up to 10+ m in some regions)
   is correctly applied. This is NOT a rounding error; it is the
   difference between safe flight and terrain collision on DJI platforms.

6. Update the CLI `plan` subcommand: Default output datum = Egm2008.

7. Update GeoPackage exporter: Write the datum name string into
   `gpkg_contents.description` alongside existing metadata.

8. Update GeoJSON exporter: Write `"altitude_datum": "EGM2008"` into
   the top-level `properties` object.

Architecture constraint: All geoid lookups are synchronous CPU work.
Use `rayon` if batch-querying thousands of waypoints. Do NOT use tokio
for this.
```

---

## PHASE 4C: Safety — GeoPackage Schema Versioning

```
Future releases will add tables, columns, and metadata to the GeoPackage
output. Without a schema version, opening a v0.2 file with v0.3 code
(or vice versa) will silently produce corrupt or incomplete results.

Tasks:

1. Create a new GeoPackage table `irontrack_metadata` with columns:
   - `key` TEXT PRIMARY KEY
   - `value` TEXT NOT NULL

2. On GeoPackage creation, insert:
   - key="schema_version", value="2" (v0.1 files implicitly = version 1)
   - key="engine_version", value=env!("CARGO_PKG_VERSION")
   - key="created_utc", value=<ISO 8601 timestamp>

3. On GeoPackage open/load (if we add that in future), check
   `schema_version`. If the file version is higher than the engine
   understands, return a clear error:
   "This GeoPackage was created by IronTrack vX.Y and requires
   schema version N. This engine supports schema version M. Please
   upgrade IronTrack."

4. If the file has no `irontrack_metadata` table at all (v0.1 file),
   treat it as schema_version=1 and print a warning suggesting re-export.

This is a small change but prevents painful data corruption later.
```

---

## PHASE 4D: Safety — DSM Penetration Warnings

```
Read docs/09_dsm_vs_dtm.md.

Copernicus GLO-30 is an X-band SAR Digital Surface Model. X-band radar
penetrates 2–8 m into forest canopy depending on density and moisture.
This means AGL clearance over forests is UNDERESTIMATED. We must
disclose this collision risk loudly and persistently.

Tasks:

1. In `src/main.rs` summary output: If terrain data source is Copernicus
   (or any DSM), print to stderr:

   ╔══════════════════════════════════════════════════════════════╗
   ║  ⚠ SAFETY WARNING: COPERNICUS DEM IS A DSM                 ║
   ║  Altitudes include canopy/structure tops. AGL clearance     ║
   ║  over forests may be UNDERESTIMATED by 2–8 m.               ║
   ║  Verify obstacle clearance manually before flight.          ║
   ╚══════════════════════════════════════════════════════════════╝

   Use ANSI bold/yellow if stdout is a TTY. Plain text otherwise.

2. Inject this warning text into:
   - GeoPackage `irontrack_metadata` table: key="safety_dsm_warning"
   - GeoPackage `gpkg_contents.description` (append to existing text)
   - GeoJSON top-level: `"safety_warning": "<warning text>"`

3. If the user supplies their own DTM in future (not yet implemented),
   this warning must NOT appear. Gate it on a `dem_type: DemType` enum
   (Dsm vs Dtm) attached to the terrain source config.
```

---

## PHASE 4E: Legal — Copernicus Attribution

```
Read docs/08_copernicus_dem.md.

The Copernicus DEM license requires mandatory attribution in any derived
product. Failure to include it is a license violation.

Tasks:

1. Create `src/legal.rs`. Define:
   ```rust
   pub const COPERNICUS_ATTRIBUTION: &str =
       "Contains modified Copernicus Sentinel data [YEAR], \
        processed by ESA.";
   pub const COPERNICUS_DISCLAIMER: &str =
       "The Copernicus DEM is provided under the Copernicus \
        Data Space Ecosystem License. See \
        https://dataspace.copernicus.eu/terms-and-conditions";
   ```

2. Register the module in `src/lib.rs`.

3. Print both strings in the CLI summary output (after the flight plan
   summary, before the DSM warning).

4. Write both strings to `irontrack_metadata` table:
   key="copernicus_attribution" and key="copernicus_disclaimer".

5. Write to GeoJSON top-level: `"attribution"` and `"disclaimer"` fields.
```

---

## PHASE 4F: Integration — QGroundControl Mission Export

```
Read docs/11_aerial_survey_autopilot_formats.md (§ QGroundControl / MAVLink).

Implement `src/io/qgc.rs`:

1. Export `FlightPlan` to QGroundControl `.plan` format (JSON).

2. Structure:
   - Top-level: `{ "fileType": "Plan", "version": 1, "groundStation": "IronTrack" }`
   - `mission.items[]`: Array of `MissionItem` objects.

3. Map each waypoint to `MAV_CMD_NAV_WAYPOINT` (command ID 16).
   - Use `frame: 3` (MAV_FRAME_GLOBAL_RELATIVE_ALT) for AGL, or
     `frame: 0` (MAV_FRAME_GLOBAL) for AMSL.
   - Provide altitudes in the datum the autopilot expects. ArduPilot
     AMSL mode uses WGS84 ellipsoidal heights. Call
     `flight_plan.to_datum(AltitudeDatum::Wgs84Ellipsoidal, engine)`
     before export.

4. Insert camera trigger commands:
   - `MAV_CMD_DO_SET_CAM_TRIGG_DIST` (command 206) at the start of each
     flight line with the calculated along-track trigger interval.
   - `MAV_CMD_DO_SET_CAM_TRIGG_DIST` with distance=0 at line end to stop.

5. Add `--qgc-plan <PATH>` flag to the CLI `plan` subcommand.

6. Integration test: Generate a plan, parse the JSON back, verify
   waypoint count, altitude values, and command IDs are correct.
```

---

## PHASE 4G: Integration — DJI WPML/KMZ Export

```
Read docs/11_aerial_survey_autopilot_formats.md (§ DJI WPML).

CRITICAL SAFETY CONTEXT: DJI autopilots do NOT calculate terrain offsets
in flight. They execute the exact static altitude values pre-compiled by
the planning software. If we export the wrong datum, the aircraft flies
at the wrong altitude. Period.

Implement `src/io/dji.rs`:

1. Export `FlightPlan` to DJI `.kmz` format.
   A .kmz is a ZIP archive containing:
   - `wpmz/template.kml` — mission config (flyToWaylineMode, finishAction,
     exitOnRCLost, global transitional speed, payload config).
   - `wpmz/waylines.wpml` — waypoint data with action groups.

2. DATUM CONVERSION (non-negotiable):
   Before writing any altitude value, call:
   `flight_plan.to_datum(AltitudeDatum::Egm96, engine)?`
   DJI's `wpml:executeHeightMode` must be set to "EGM96".
   This uses the Phase 4B conversion pipeline which chains through
   WGS84 Ellipsoidal and applies the vendored EGM96 geoid model.
   
   DO NOT skip this step. DO NOT assume EGM2008 ≈ EGM96.
   The difference ranges from 0.5 m to 10+ m depending on location.

3. XML generation: Use `quick-xml` crate (already available or add it).
   Write the `<kml>` and `<wpml:missionConfig>` elements per the DJI
   WPML specification in Doc 11.

4. ZIP packaging: Use the `zip` crate to bundle template.kml and
   waylines.wpml into the .kmz archive.

5. Camera triggers: Use `<wpml:actionGroup>` with
   `<wpml:actionTriggerType>reachPoint</wpml:actionTriggerType>` and
   `<wpml:action>` containing `wpml:takePhoto` or
   `wpml:startRecord` as appropriate.

6. Add `--dji-kmz <PATH>` flag to the CLI `plan` subcommand.

7. Test: Generate a .kmz, unzip it, parse the XML, verify all altitude
   values are in EGM96 and differ from the internal EGM2008 values by
   the expected geoid separation.

Dependencies to add to Cargo.toml:
```toml
quick-xml = { version = "0.36", features = ["serialize"] }
zip = { version = "2", default-features = false, features = ["deflate"] }
```
```

---

## PHASE 4H: Sensors — LiDAR Math & Lines

```
Read docs/13_lidar_flight_planning.md.

IronTrack v0.1 only plans photogrammetric (camera) missions. LiDAR flight
planning uses fundamentally different spacing logic: line spacing is driven
by swath overlap percentage (typically 50%), not by image side-lap and GSD.
Point density is the primary quality metric, not pixel resolution.

Tasks:

1. Create `src/photogrammetry/lidar.rs`.

2. Define `LidarSensorParams`:
   ```
   - prr: f64          // Pulse Repetition Rate (Hz), e.g. 240_000.0
   - scan_rate: f64     // Mirror oscillation frequency (Hz)
   - fov_deg: f64       // Total angular Field of View (degrees)
   - loss_fraction: f64 // Scanning point loss fraction (0.0–1.0)
   ```

3. Implement `swath_width(agl: f64, fov_deg: f64) -> f64`:
   W = 2 × AGL × tan(FOV/2)

4. Implement `point_density(params: &LidarSensorParams, agl: f64, ground_speed: f64) -> f64`:
   ρ = (PRR × (1 - LSP)) / (V × W)
   This is the global average density in pts/m².

5. Implement `required_speed_for_density(params: &LidarSensorParams, agl: f64, target_density: f64) -> f64`:
   Solve the density equation for V.

6. Implement `across_track_spacing(params: &LidarSensorParams, agl: f64) -> f64`
   and `along_track_spacing(ground_speed: f64, scan_rate: f64) -> f64`
   for uniformity analysis (Doc 13 §Decoupling Along/Across-Track).

7. Update `flightlines.rs` to accept an enum `MissionType { Camera, Lidar }`
   When MissionType::Lidar:
   - Line spacing = swath_width × (1 - sidelap_fraction).
     Default sidelap for LiDAR = 50% (vs 30% for camera).
   - No GSD or focal-length logic applies.

8. Add CLI flags:
   --mission-type lidar
   --lidar-prr <Hz>
   --lidar-scan-rate <Hz>
   --lidar-fov <degrees>
   --target-density <pts/m²>

9. Tests:
   - Verify density calculation against the worked example in Doc 13:
     AGL=100m, FOV=70°, V=5m/s, PRR=240,000 → ρ ≈ 342 pts/m².
   - Verify speed inversion: given target ρ=8 pts/m², solve for V.
   - Verify swath width: AGL=100, FOV=70° → W ≈ 140.04 m.

Register the module in `src/photogrammetry/mod.rs`.
```

---

## PHASE 4I: Boundary — KML/KMZ Import

```
Read docs/11_aerial_survey_autopilot_formats.md §KML.

Currently, the survey area is defined by --min-lat/--max-lat/--min-lon/
--max-lon (axis-aligned bounding box). Real survey boundaries are
irregular polygons, often with concave sections and interior exclusion
zones (no-fly areas, restricted airspace, buildings).

Tasks:

1. Add `quick-xml` to Cargo.toml if not already present (Phase 4G adds it).

2. Implement `src/io/kml.rs`:
   - Parse KML `<Polygon>` elements.
   - Extract exterior ring `<outerBoundaryIs><LinearRing><coordinates>`.
   - Extract interior rings `<innerBoundaryIs><LinearRing><coordinates>`
     (exclusion zones / holes). Store as Vec of coordinate rings.
   - Handle .kmz files: Unzip, find the doc.kml inside, parse it.

3. Implement point-in-polygon test in `src/math/geometry.rs`
   (create the module if it doesn't exist):
   - Ray-casting algorithm supporting:
     a) Concave exterior boundaries (ray-cast handles this natively).
     b) Interior holes: A point is "inside" if it's inside the exterior
        ring AND outside ALL interior rings.
   - All coordinate comparisons use f64.

4. Update `flightlines.rs`:
   - Generate lines over the polygon's bounding box (existing logic).
   - Clip each line: walk the waypoints, discard segments that fall
     outside the polygon (using the point-in-polygon test).
   - Split lines at polygon entry/exit points to avoid flying over
     excluded areas.

5. Support multi-polygon KML files: If the KML contains multiple
   `<Polygon>` elements (e.g., disjoint survey blocks), generate
   independent flight line sets for each polygon.

6. Add `--boundary <PATH.kml|.kmz>` to CLI as an alternative to
   `--min-lat` etc. The two input modes are mutually exclusive.

7. Tests:
   - Convex polygon: Verify all generated waypoints lie inside.
   - Concave "L-shaped" polygon: Verify clipping removes the
     correct interior corner.
   - Polygon with hole: Verify no waypoints fall inside the hole.

Known limitation for v0.2: Multi-polygon routing (which block to fly
first) is deferred to Phase 5C (TSP routing).
```

---

## PHASE 4J: Integration Test — End-to-End Pipeline Validation

```
This is not a feature prompt. It is a mandatory integration gate before
tagging the v0.2 release.

Create `tests/v02_integration.rs`:

1. Full pipeline test:
   - Input: A hardcoded polygon boundary (small area, ~1 km²).
   - Run the flight line generator with a camera sensor config.
   - Export to GeoPackage: Verify `irontrack_metadata` table exists
     with schema_version=2, copernicus_attribution, and safety_warning.
   - Export to GeoJSON: Verify altitude_datum, attribution, and
     safety_warning fields are present.
   - Export to QGC .plan: Parse JSON, verify waypoint altitudes are
     WGS84 ellipsoidal.
   - Export to DJI .kmz: Unzip, parse WPML XML, verify altitudes
     are EGM96 and executeHeightMode="EGM96".

2. Datum round-trip test:
   - Create a FlightLine in EGM2008.
   - Convert to Wgs84Ellipsoidal, then to Egm96, then back to
     Wgs84Ellipsoidal, then back to Egm2008.
   - Verify final altitudes match originals within < 1 mm (Kahan
     summation should prevent drift, but verify).

3. DSM warning test:
   - Run CLI with Copernicus DEM source.
   - Capture stderr, assert the safety warning string is present.

All v0.2 prompts (4A–4I) must pass before this test is written.
```

---

# v0.3 — Kinematics and Route Optimization

> Prerequisites: v0.2 complete. Datum tagging, KML boundary, and autopilot
> exports are all functional.

---

## PHASE 5A: Corridors — Linear Infrastructure Following

```
Read docs/07_corridor_mapping.md.

Corridor mapping (pipelines, powerlines, highways) requires the engine to
generate flight lines that follow a user-supplied linestring rather than
filling a polygon. This is geometrically different from area surveys.

Tasks:

1. Create `src/photogrammetry/corridor.rs`.

2. Define `CorridorParams`:
   - centerline: Vec<(f64, f64)>  // Lat/lon vertices of the infrastructure
   - buffer_width: f64             // Total corridor width in meters
   - num_passes: u8                // Number of parallel offset lines (1, 3, 5)

3. Implement parallel offset line generation:
   - For each segment of the centerline polyline, compute the left and right
     normal vectors.
   - Generate offset polylines at ±buffer_width/2, ±buffer_width, etc.
     depending on num_passes.
   - Handle MITER vs BEVEL joins at vertices:
     At acute interior angles (< 60°), miter joins produce spikes that
     extend far beyond the corridor. Detect these and switch to bevel joins
     (truncate the spike at 2× buffer_width from the vertex).
   - Handle self-intersections: When offset polylines cross themselves
     (common at tight curves), clip the self-intersection using the
     Weiler-Atherton or simpler loop-removal algorithm.

4. Project to UTM before computing offsets (distances must be in meters).
   Use `src/geodesy/utm.rs::to_utm()`. Convert results back to WGS84
   for storage.

5. Assign terrain-following altitudes to corridor waypoints using the
   existing TerrainEngine (query DEM under each offset waypoint).

6. Integrate with `flightlines.rs`: Add a `PlanMode::Corridor(CorridorParams)`
   variant alongside the existing `PlanMode::Area(...)`.

7. Add CLI flags:
   --mode corridor
   --centerline <PATH.kml>  (KML LineString, reuse Phase 4I parser)
   --corridor-width <meters>
   --corridor-passes <1|3|5>

8. Tests:
   - Straight line: Verify offsets are exactly ±buffer_width/2 apart.
   - 90° bend: Verify miter join doesn't spike beyond 2× buffer.
   - U-turn (180° bend): Verify self-intersection is clipped cleanly.

Architecture constraint: All offset geometry is synchronous CPU work
via rayon. No async.
```

---

## PHASE 5B: Turns — Dubins Path Generation

```
Read docs/07_corridor_mapping.md §Flight Dynamics and §Transition Curves.

When the aircraft reaches the end of a flight line and must reverse
direction to fly the adjacent line, it executes a procedure turn. The
turn geometry must respect the aircraft's physical minimum turn radius.
Feeding raw waypoints with instantaneous direction reversals to an
autopilot causes dangerous overshoot.

Tasks:

1. Create `src/math/dubins.rs`.

2. Define `TurnParams`:
   - true_airspeed: f64     // TAS in m/s
   - max_bank_angle_deg: f64 // Max bank angle during turns (default: 20°)
   - The minimum turn radius is derived:
     R_min = V² / (g × tan(bank_angle))
     where g = 9.80665 m/s².
   - This is NOT a user-configurable radius. It is computed from physics.

3. Implement Dubins path generation between two oriented waypoints
   (position + heading):
   - Compute all 6 Dubins path types: LSL, RSR, LSR, RSL, LRL, RLR.
   - Select the shortest feasible path.
   - Discretize the selected path into waypoints at a configurable
     angular resolution (default: 5° per waypoint along arcs).

4. Implement clothoid (Euler spiral) transition insertion:
   - Between each straight segment and circular arc, insert a short
     clothoid transition where curvature ramps linearly from 0 to 1/R.
   - This prevents instantaneous roll-rate demands on the autopilot.
   - Clothoid length = V / max_roll_rate (default max_roll_rate: 15°/s).
   - Doc 07 details this. The transition prevents control surface
     slamming and gimbal destabilization.

5. Integrate with `flightlines.rs`:
   - After generating parallel flight lines, connect adjacent line
     endpoints with Dubins turn paths.
   - The turn direction (teardrop vs bowtie) depends on which endpoint
     pair minimizes total path length.
   - Output: The complete mission is now a single continuous polyline
     (line → turn → line → turn → ...).

6. Add CLI flags:
   --max-bank-angle <degrees>  (default 20)
   --turn-airspeed <m/s>       (default: cruise speed)

7. Tests:
   - Two parallel lines 200 m apart, same heading: Verify the generated
     turn radius matches R_min within 1%.
   - Two parallel lines, opposite heading: Verify Dubins type selection
     (should choose RSR or LSL depending on geometry).
   - Verify clothoid insertion: Curvature at the straight→arc junction
     must be 0 (not 1/R).

8. Altitude during turns: Maintain the altitude of the lower of the two
   connected line endpoints. Do NOT interpolate terrain during turns —
   the aircraft is off the survey grid and terrain data may not be loaded
   for the turn area.
```

---

## PHASE 5C: TSP — Route Sorting to Minimize Distance

```
Read docs/07_corridor_mapping.md §Heuristic Segmentation.

After flight lines are generated (Phase 0–3) and turn paths are computed
(Phase 5B), the order in which lines are flown matters. The default
sequential order (line 1, 2, 3, ...) is rarely optimal: the aircraft
wastes fuel flying long repositioning legs between non-adjacent lines.

Tasks:

1. Create `src/math/routing.rs`.

2. Model the flight plan as a directed graph:
   - Each flight line has two endpoints (start and end). The line can be
     flown in either direction.
   - Each node in the graph is a (line_index, direction) pair.
   - Edge weight between node A-end and node B-start = Karney geodesic
     distance (use geographiclib-rs, NOT Haversine).

3. Implement a nearest-neighbor heuristic as the baseline:
   - Start from the line closest to the launch point.
   - Greedily pick the nearest unvisited line endpoint.
   - For each candidate line, evaluate BOTH directions and pick the one
     with the shorter repositioning leg.

4. Implement 2-opt local improvement:
   - After the greedy solution, iteratively swap pairs of edges to reduce
     total repositioning distance.
   - Terminate after no improvement found in a full pass, or after 1000
     iterations, whichever comes first.

5. Output: Reorder the `FlightPlan.lines` vector to match the optimized
   sequence. Update the Dubins turn connections (Phase 5B) accordingly.

6. Add CLI flags:
   --optimize-route          (enable TSP, default: off for v0.3)
   --launch-lat <deg>
   --launch-lon <deg>

7. Tests:
   - 4 parallel lines in a grid: Verify the optimized order is a
     serpentine (boustrophedon) pattern, not sequential.
   - Verify total repositioning distance is ≤ sequential ordering.

Note: In v0.5 (Phase 8B), the distance-based edge weights will be
replaced with energy-based weights that account for wind. The graph
structure and 2-opt logic built here will be reused directly.
```

---

# v0.4 — Networked FMS Daemon

> Prerequisites: v0.3 complete. The CLI engine can generate optimized,
> turn-connected flight plans with datum-safe autopilot exports.
>
> ARCHITECTURE SHIFT: This release transitions IronTrack from a one-shot
> CLI tool into a persistent, networked daemon. Update CLAUDE.md to reflect
> that Tokio is now permitted for the Axum server and WebSocket I/O, in
> addition to DEM downloads.

---

## PHASE 6A: Axum Daemon and REST API

```
Read the project CLAUDE.md architecture rules carefully.

We are adding a persistent HTTP daemon mode alongside the existing CLI.
The CLI remains fully functional — the daemon wraps the same engine.

CRITICAL ASYNC BOUNDARY RULE: The math engine (flight line generation,
geodesic calculations, DEM queries, datum conversions) runs synchronously
on OS threads via rayon. Axum handlers are async. You MUST bridge them
with `tokio::task::spawn_blocking`. Never call the engine directly inside
an async handler — it will block the Tokio runtime and starve all other
connections.

Tasks:

1. Add dependencies to Cargo.toml:
   ```toml
   axum = "0.7"
   tokio = { version = "1", features = ["full"] }
   tower-http = { version = "0.5", features = ["cors", "trace"] }
   ```
   Note: `tokio-websockets` is deferred to Phase 6B.

2. Create `src/network/mod.rs` and `src/network/server.rs`.

3. Define `FmsState`:
   ```rust
   pub struct FmsState {
       pub flight_plan: Option<FlightPlan>,
       pub terrain_engine: TerrainEngine,
       // Phase 6B will add: pub current_state: Option<CurrentState>,
   }
   ```
   Wrap in `Arc<RwLock<FmsState>>` and pass as Axum state.

4. Implement endpoint `POST /api/v1/mission/load`:
   - Request body (JSON): GeoJSON boundary polygon + configuration
     (sensor params, overlap, DEM source, output datum, etc.).
   - Handler: Deserialize the request, then call
     `tokio::task::spawn_blocking(move || { engine.generate(...) })`.
   - On success: Store the FlightPlan in FmsState. Return 200 with
     a summary JSON (line count, total distance, bounding box).
   - On error: Return 400/500 with the error message.

5. Implement endpoint `GET /api/v1/mission/plan`:
   - Returns the current FlightPlan as GeoJSON.
   - Returns 404 if no plan is loaded.

6. Implement endpoint `GET /api/v1/mission/export/qgc`:
   - Returns the current plan as a QGC .plan JSON file.
   - Calls `to_datum(Wgs84Ellipsoidal)` inside spawn_blocking.

7. Implement endpoint `GET /api/v1/mission/export/dji`:
   - Returns the current plan as a DJI .kmz binary.
   - Calls `to_datum(Egm96)` inside spawn_blocking.

8. Update `src/main.rs`:
   - Add a `daemon` subcommand: `irontrack daemon --port 8080`.
   - The existing `plan` subcommand is unchanged.
   - Use clap's subcommand derive pattern.

9. Enable CORS via tower-http (allow all origins for dev; the eventual
   frontend will be on a different port).

10. Tests:
    - Unit test: Verify FmsState round-trip (load plan, retrieve plan).
    - Integration test: Spawn the server in a test, POST a boundary,
      GET the plan, verify it matches CLI output for the same inputs.
```

---

## PHASE 6B: Live Telemetry — NMEA Parsing and WebSocket Broadcast

```
Read docs/11_aerial_survey_autopilot_formats.md (§ NMEA/MAVLink telemetry).

The daemon needs real-time aircraft position awareness to support future
features (camera triggering, geofence alerts, live progress tracking).

Tasks:

1. Add dependencies:
   ```toml
   nmea = "0.6"
   tokio-tungstenite = "0.23"
   ```

2. Create `src/network/telemetry.rs`.

3. Define `CurrentState`:
   ```rust
   pub struct CurrentState {
       pub lat_deg: f64,
       pub lon_deg: f64,
       pub alt_msl: f64,        // Meters above MSL (from GPGGA)
       pub ground_speed_kts: f64, // From GPRMC
       pub true_heading_deg: f64, // From GPRMC
       pub fix_quality: u8,     // 0=no fix, 1=GPS, 2=DGPS, 4=RTK
       pub timestamp_utc: String,
   }
   ```

4. Implement a UDP listener on a configurable port (default 5555):
   - Receive raw NMEA 0183 sentences.
   - Parse GPGGA (position, altitude, fix quality) and GPRMC
     (ground speed, true course).
   - Update `FmsState.current_state` behind the RwLock.
   - For v0.4 testing, also implement a mock mode that generates
     synthetic NMEA data walking along the loaded flight plan at
     a configurable speed.

5. Implement WebSocket endpoint `GET /api/v1/telemetry/stream`:
   - On connection, start broadcasting the CurrentState as JSON at 10 Hz.
   - Use `tokio::time::interval(Duration::from_millis(100))`.
   - Handle client disconnection gracefully (drop the sender).

6. Add CLI flags to the `daemon` subcommand:
   --nmea-port <UDP_PORT>     (default 5555)
   --mock-telemetry           (enable synthetic NMEA for testing)
   --mock-speed <m/s>         (ground speed for mock mode)

7. Tests:
   - Send a known GPGGA sentence to the UDP port, verify CurrentState
     updates correctly.
   - Connect a WebSocket client, verify JSON messages arrive at ~10 Hz.
   - Verify mock mode generates valid NMEA that parses correctly.
```

---

# v0.5 — Atmospheric Kinematics and Energy Economy

> Prerequisites: v0.4 complete. The daemon is running, telemetry is flowing.

---

## PHASE 7A: Wind Vector Ingestion and Wind Triangle

```
Read docs/16_aerial_wind_data.md and docs/03_photogrammetry_spec.md
§Crab Angle.

The engine needs atmospheric awareness. Wind affects ground speed, crab
angle, effective swath width, and energy consumption. All of these must
be modeled to produce accurate flight plans.

Tasks:

1. Create `src/kinematics/mod.rs` and `src/kinematics/atmosphere.rs`.

2. Define:
   ```rust
   pub struct WindVector {
       pub speed_ms: f64,        // Wind speed in m/s
       pub direction_deg: f64,   // Wind FROM direction, degrees true
   }
   ```

3. Implement the Wind Triangle solver:
   Given:
   - TAS (True Airspeed, m/s)
   - TRK (desired ground Track, degrees true)
   - WindVector
   Compute:
   - HDG (required aircraft Heading, degrees true)
   - GS (resulting Ground Speed, m/s)

   Use the law of sines:
   WCA = arcsin((wind_speed / TAS) × sin(wind_direction - TRK))
   HDG = TRK + WCA
   GS = TAS × (sin(TRK - wind_direction + WCA) / sin(WCA))
   (Handle the degenerate case where wind_speed ≈ 0 → WCA = 0, GS = TAS.)

4. For v0.5, wind is a STATIC input provided at plan time.
   Update the API endpoint `/api/v1/mission/load` to accept optional
   `wind_speed_ms` and `wind_direction_deg` parameters.
   Update the CLI: `--wind-speed <m/s>` and `--wind-dir <degrees>`.

5. Compute and store GS for each flight line segment. Headwind legs will
   have lower GS than tailwind legs. This directly affects:
   - Camera trigger interval timing (Phase 7A does not change triggers,
     but stores the data for later use).
   - Energy consumption (Phase 8A will use this).

6. Tests:
   - No wind: HDG = TRK, GS = TAS.
   - Pure headwind: HDG = TRK, GS = TAS - wind_speed.
   - Pure crosswind: Verify WCA matches the analytical solution.
   - Wind speed > TAS: Return an error (aircraft cannot make headway).

SCOPE NOTE: Real-time wind updates from the telemetry stream (Phase 6B)
feeding back into dynamic re-planning is a v0.6+ feature. v0.5 is
plan-time only.
```

---

## PHASE 7B: Crab Angle Compensation for Swath Width

```
Read docs/03_photogrammetry_spec.md §Crab Angle and
docs/07_corridor_mapping.md §Transition Curves.

When the aircraft crabs into a crosswind, the camera footprint rotates
relative to the ground track. This reduces the EFFECTIVE perpendicular
swath width, which means the default line spacing produces coverage gaps.

SCOPE: This is a PLAN-TIME adjustment. The wind vector from Phase 7A
(static input) is used to pre-compute adjusted line spacing. This is NOT
real-time re-planning.

Tasks:

1. Create `src/photogrammetry/crab_angle.rs`.

2. Implement crab angle calculation:
   crab_angle = HDG - TRK (from Phase 7A Wind Triangle output).

3. Implement effective swath width under crab:
   For a rectangular sensor footprint with dimensions W (cross-track)
   and L (along-track), rotated by crab_angle θ, the effective
   perpendicular swath width is:
   W_eff = W × cos(θ) - L × |sin(θ)|
   (The along-track dimension intrudes into the cross-track axis.)

4. Refactor `flightlines.rs`:
   - If wind parameters are provided, compute crab_angle for each
     flight line heading.
   - Use W_eff instead of W to compute line spacing.
   - Different headings have different crab angles (e.g., N-S lines vs
     E-W lines in the same wind). Each line's spacing adjustment is
     computed independently.
   - For LiDAR missions (Phase 4H), the same logic applies to the
     LiDAR swath width.

5. Store the crab angle per flight line in the FlightPlan output
   (GeoPackage attribute, GeoJSON property).

6. Tests:
   - Zero wind: crab_angle = 0, W_eff = W. Line spacing unchanged.
   - 10 m/s crosswind perpendicular to flight line, TAS 50 m/s:
     Verify crab angle ≈ 11.5° and W_eff < W.
   - Verify that the adjusted line spacing produces ≥ target sidelap
     under the crab condition (compute overlap percentage analytically).
```

---

## PHASE 8A: Aerodynamic Drag Model

```
Read docs/17_uav_survey_analysis.md §Kinematics.

To optimize route ordering by energy (not just distance), we need a
physics model that estimates power consumption as a function of airspeed,
wind, and flight segment geometry.

Tasks:

1. Create `src/kinematics/energy.rs`.

2. Define `PlatformDynamics`:
   ```rust
   pub struct PlatformDynamics {
       pub mass_kg: f64,
       pub drag_coefficient: f64,
       pub frontal_area_m2: f64,
       pub battery_capacity_wh: f64,
       pub propulsive_efficiency: f64,  // 0.0–1.0, typically 0.5–0.7
   }
   ```

3. Implement `power_required_w(platform: &PlatformDynamics, tas_ms: f64) -> f64`:
   For level flight (simplified model):
   P = (0.5 × ρ_air × Cd × A × V³) / η + (m × g)² / (0.5 × ρ_air × V × π × b²)
   
   Where the first term is parasite drag power and the second is
   induced drag power.
   
   For the v0.5 MVP, use a simplified quadratic model:
   P = k1 × V³ + k2 / V
   where k1 and k2 are derived from the platform parameters.
   Use ISA sea-level air density ρ = 1.225 kg/m³ (altitude correction
   is a future enhancement).

4. Implement `energy_cost_j(platform: &PlatformDynamics, segment: &Segment, wind: &WindVector) -> f64`:
   - Compute GS for the segment heading and wind (Phase 7A).
   - Time enroute = segment distance / GS.
   - Energy = power_required(TAS) × time.
   - Return energy in Joules.

5. Implement `total_mission_energy(plan: &FlightPlan, platform: &PlatformDynamics, wind: &WindVector) -> f64`:
   Sum energy for all segments (flight lines + repositioning legs + turns).

6. Implement `estimated_endurance_minutes(platform: &PlatformDynamics, mission_energy_j: f64) -> f64`:
   endurance = (battery_capacity_wh × 3600) / (mission_energy_j / time).
   Return whether the mission is feasible (total energy < battery capacity).

7. Print energy summary in CLI output:
   - Total energy (Wh)
   - Estimated endurance margin (%)
   - Warning if margin < 20%

8. Add CLI flags:
   --platform-mass <kg>
   --platform-drag-coeff <Cd>
   --platform-frontal-area <m²>
   --battery-capacity <Wh>

9. Tests:
   - Zero wind, constant speed: Verify energy = power × (distance/speed).
   - Headwind vs tailwind: Verify headwind segment costs MORE energy
     (same distance, lower GS, longer time).
   - Verify energy is asymmetric: A→B ≠ B→A when wind is present.
```

---

## PHASE 8B: Energy-Optimized TSP Routing

```
Read docs/17_uav_survey_analysis.md §2.2 and docs/07_corridor_mapping.md
§Heuristic Segmentation.

Phase 5C built a TSP solver using geodesic distance as the edge cost.
This phase replaces the cost function with the energy model from Phase 8A,
making route optimization wind-aware.

Tasks:

1. Open `src/math/routing.rs` (Phase 5C).

2. Replace the edge weight function:
   - OLD: weight(A, B) = karney_distance(A_end, B_start)
   - NEW: weight(A, B) = energy_cost_j(platform, segment(A_end→B_start), wind)

3. CRITICAL ASYMMETRY: With wind, flying line N-to-S costs different
   energy than S-to-N. The Phase 5C solver already evaluates both
   directions per line. Verify this still works correctly now that
   the cost function is asymmetric.

4. Additionally, the FLIGHT LINE ITSELF has asymmetric cost:
   - Flying a headwind line is more expensive than the same line with
     a tailwind.
   - The solver must now evaluate: For line L, is it cheaper to fly
     L forward or L reversed? Include the flight line energy cost
     in the total, not just the repositioning leg.

5. Update the solver's objective function:
   minimize Σ(flight_line_energy[i] + repositioning_energy[i→i+1])
   over all lines and their directions.

6. The 2-opt local improvement from Phase 5C applies unchanged — it just
   uses the new cost function.

7. Print the optimized route's total energy alongside the naive
   sequential route's energy so the user can see the savings.

8. Tests:
   - Strong headwind from the north: Verify the solver prefers flying
     N→S lines (tailwind) and repositions S→N (shorter ferry legs
     into headwind) rather than the reverse.
   - Zero wind: Verify results match Phase 5C distance-based output
     (energy is proportional to distance when wind=0).

Architecture constraint: The TSP solver and energy calculations are
synchronous CPU work (rayon). If called from an Axum endpoint, wrap
in spawn_blocking.
```

---

# Deferred / Future Phases (not yet specified)

| Phase | Feature | Notes |
|-------|---------|-------|
| 9A | 3DEP 1-meter DEM ingestion | US-domestic survey requires NAVD88/NAD83 → WGS84 datum pipeline. See doc 15. Eagle Mapping's primary market. |
| 9B | GEOID18 hybrid geoid support | Required for NAVD88 ↔ WGS84 conversion. See doc 05 §3. |
| 10A | Real-time wind re-planning | Telemetry wind updates feed back into dynamic line spacing adjustment during flight. |
| 10B | Oblique photogrammetry | See doc 18. Different GSD/overlap math for tilted sensors. |
| 10C | WGS84 epoch management | See doc 19. Plate tectonics shift coordinates ~5 cm/year. Matters for PPK. |
| 11A | React/Tauri frontend | Web-based map UI consuming the v0.4 REST API. |