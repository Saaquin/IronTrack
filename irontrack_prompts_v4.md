# IronTrack — Post-v0.2 Prompt Playbook (v3.5 Revision)

> **Release strategy (aligned to Manifest v3.5, informed by 52 research documents):**
>
> | Release | Theme | Phases | Key Source Docs |
> |---------|-------|--------|-----------------|
> | ~~**v0.2**~~ | ~~Correctness, Safety, Autopilot~~ | ~~4A–4J~~ **COMPLETE** | 05, 07, 09, 11-13 |
> | **v0.3** | Terrain-Following Trajectory Generation | 5A – 5D | 34, 39, 50 |
> | **v0.4** | Networked FMS Daemon | 6A – 6D | 28, 29, 42 |
> | **v0.5** | Atmospheric Kinematics and Energy | 7A – 7B, 8A – 8B | 16, 38, 50 |
> | **v0.6** | Glass Cockpit MVP | 9A – 9D | 25, 26 |
> | **v0.7** | Web Planning UI and Airspace | 10A – 10F | 35, 42, 43, 45, 49 |
> | **v0.8** | Embedded Trigger Controller | 11A – 11E | 22, 29, 40, 41 |
> | **v0.9** | 3DEP and US-Domestic Geodesy | 12A – 12E | 30, 33, 37, 42, 52 |
> | **v1.0** | Production Release | 13A – 13E | 36, 44, 46, 51 |
>
> Each release is independently shippable and testable.
> Prerequisites: v0.2 complete. Datum-tagged altitudes, EGM96, DSM warnings,
> Copernicus attribution, QGC/DJI exports, LiDAR lines, KML import,
> schema versioning, corridors, Dubins paths, TSP routing all implemented.
>
> **Gemini Structural Review (3 mitigations applied):**
> 1. ~~Geodetic Type Erasure~~ → PhantomData `Coordinate<D>` deferred to v0.9
>    design note (not a v0.3 scope bomb against 33 existing GeoCoord call sites).
> 2. GeoPackage Concurrency → WAL pragmas + `deadpool-sqlite` connection pool
>    mandated in Phase 6A.
> 3. Crate Contamination → Trigger controller is an isolated `#![no_std]` crate
>    within a Cargo workspace, mandated in Phase 11A.

---

# v0.3 — Terrain-Following Trajectory Generation

> Prerequisites: v0.2 complete. Corridors, Dubins turns, and TSP routing
> implemented. The engine can generate flat-plane and stepped flight lines.
> This release adds dynamically feasible, jerk-free terrain-following
> trajectories via the two-stage hybrid algorithm from Doc 34.

---

## PHASE 5A: Elastic Band Energy Minimization

```
Read docs/34_terrain_following_planning.md §3 (Elastic Band) and
docs/09_dsm_vs_dtm.md.

The current terrain-following logic steps altitudes per waypoint from
the DEM. This produces a jagged trajectory that causes autopilot pitch
chatter, destroys sensor stability, and violates load factor limits.
This phase replaces it with a physically-modeled energy minimization.

Tasks:

1. Create `src/trajectory/mod.rs` and `src/trajectory/elastic_band.rs`.

2. Define the discrete node array:
   - Input: the raw DEM-sampled terrain profile along a flight line
     (Vec<f64> elevations at fixed along-track intervals, e.g., 10 m).
   - Add target AGL offset to get the raw "ideal" path.
   - Initialize the elastic band ABOVE the highest peak in the segment
     (prevents local minima traps from ravines/overhangs).

3. Implement the energy functional with three force components:
   a. TENSION springs (Hooke's law between adjacent nodes):
      F_tension[i] = k_t × (dist(node[i-1], node[i+1]) - 2×spacing)
      Drives the path to be short and straight.
   b. TORSIONAL springs (bending penalty on 3 consecutive nodes):
      F_torsion[i] = k_b × (angle(node[i-1], node[i], node[i+1]))
      Enforces the curvature limit κ_max = a_max / V².
   c. TERRAIN REPULSION (asymmetric one-sided barrier):
      If node altitude < (terrain + min_AGL): massive upward force.
      Use exponential decay: F_repulse = k_r × exp(-d/d_0) where
      d = node_alt - (terrain_alt + min_AGL).
   d. GRAVITY (weak constant downward pull):
      Prevents the band from floating infinitely high. Magnitude
      tuned so the band settles near the target survey AGL.

4. Implement the gradient descent solver:
   - Per iteration: compute net force on every node, displace by
     η × F_net (learning rate η ≈ 0.5–1.0 m per iteration).
   - Convergence: stop when max displacement < 0.01 m or after
     max_iterations (default 500).
   - The Jacobian is tridiagonal — only adjacent neighbors matter.

5. Translate aircraft dynamic limits to geometric parameters:
   Accept a `KinematicLimits` struct:
   ```rust
   pub struct KinematicLimits {
       pub max_pitch_deg: f64,      // 10–15° typical
       pub max_vert_accel_g: f64,   // 0.5–1.0 g typical
       pub max_pitch_rate_dps: f64, // 3–5°/s typical
       pub survey_speed_ms: f64,    // Ground speed
   }
   ```
   Derive: max_slope = tan(max_pitch_deg.to_radians()),
   κ_max = (max_vert_accel_g × 9.81) / survey_speed_ms².
   Use these to set k_b (torsional coefficient).

6. Use rayon par_iter for the force computation (each node's forces
   depend only on neighbors — embarrassingly parallel).
   SoA memory layout for node positions.

7. Tests:
   - Flat terrain: band converges to exact target AGL, zero curvature.
   - Single step function (cliff): band projects a climb ramp starting
     well before the cliff base, governed by max_slope.
   - Verify convergence in < 500 iterations for a 3,000-node line.

Architecture: This is synchronous rayon work. Never call from async.
```

---

## PHASE 5B: Cubic B-Spline Fitting with Convex Hull Safety

```
Read docs/34_terrain_following_planning.md §4 (B-Spline) and §Rust Crates.

The Elastic Band output is a discrete sequence of nodes. Its higher-order
derivatives are noisy finite differences. This phase wraps the EB output
in a C² continuous analytical curve.

Tasks:

1. Create `src/trajectory/bspline_smooth.rs`.

2. Add dependencies:
   ```toml
   bspline = "2"     # or nalgebra-based custom Cox-de Boor
   nalgebra = "0.33"
   ```
   Do NOT use `spliny` (FFI to Fortran FITPACK — violates memory safety,
   ARM cross-compilation issues).

3. Implement adaptive knot placement:
   a. Compute discrete curvature at each EB node (finite difference
      second derivative).
   b. Identify dominant feature points: local curvature maxima
      (peaks, valleys, cliff edges) above a threshold.
   c. Place knots densely at feature points (e.g., every 20 m).
   d. Place knots sparsely over flat terrain (e.g., every 200 m).
   e. Return a non-uniform knot vector.

4. Fit a degree-3 (cubic) B-spline through the EB nodes using
   least-squares with the adaptive knot vector. The control points
   are solved via the normal equations.

5. CRITICAL — Convex Hull AGL Enforcement:
   After fitting, iterate over every control point. For each, query
   the DEM at the control point's horizontal position. If the control
   point altitude is less than (terrain + min_AGL), translate it
   vertically upward until it clears. Because the B-spline curve lies
   strictly within the convex hull of its control polygon, this
   guarantees the ENTIRE curve clears minimum AGL.
   This is not a heuristic — it is a mathematical proof.

6. Implement B-spline evaluation:
   - `evaluate(t: f64) -> (along_track_m: f64, altitude_m: f64)`
   - Use the Cox-de Boor recursion formula.
   - Support arc-length parameterization via Gaussian quadrature
     (needed for Phase 5C look-ahead).

7. Validate C² continuity: sample the curve at high density (every 1 m).
   Compute slope (1st derivative) and curvature (2nd derivative) via
   finite differences on the samples. Assert:
   - |slope| ≤ max_slope at all points.
   - |curvature| ≤ κ_max at all points.
   - No discontinuous jumps in slope between adjacent samples.

8. Output the smoothed trajectory as a new FlightLine with waypoints
   sampled from the B-spline at the desired trigger interval.

9. Tests:
   - Sinusoidal terrain: verify curvature is clipped to κ_max.
   - Sawtooth terrain: verify NO sampled point drops below min_AGL.
   - Flat terrain: verify minimal knots and zero curvature.
```

---

## PHASE 5C: Dynamic Look-Ahead Pure Pursuit

```
Read docs/34_terrain_following_planning.md §Look-Ahead.

The B-spline produces a perfect analytical path. The autopilot must
track it using a pure pursuit algorithm with a dynamically varying
look-ahead distance.

Tasks:

1. Create `src/trajectory/pursuit.rs`.

2. Implement kinematic minimum look-ahead:
   L_a_min = (Δh × V) / V_z_max
   where Δh = upcoming terrain rise, V = ground speed,
   V_z_max = max sustained climb rate.

3. Implement dynamic look-ahead adaptation:
   a. CURVATURE-BASED: Compute the B-spline curvature at the
      aircraft's current along-track position.
      - Low curvature (flat): L_a = L_a_max (smooth, efficient).
      - High curvature (ridge apex): L_a = L_a_min (tight tracking).
      - Interpolate: L_a = L_a_min + (L_a_max - L_a_min) ×
        (1 - κ_current / κ_max).
   b. ERROR-BASED: If |altitude_error| > threshold:
      L_a *= exp(-gain × |altitude_error|).
      This aggressively tightens tracking when wind shear pushes
      the aircraft off the planned profile.

4. Implement the pursuit target resolver:
   Given the aircraft's current along-track position s_current,
   find the point on the B-spline at arc-length s_current + L_a.
   Use Gaussian quadrature on the B-spline's arc-length integral.
   Return the target (lat, lon, altitude) for the autopilot.

5. This module is used by the glass cockpit (v0.6) to display the
   target altitude bug on the VDI tape. For v0.3, expose it as
   a library function — the daemon integration comes in v0.4/v0.6.

6. Tests:
   - Step function terrain: verify the pursuit target begins climbing
     well BEFORE the cliff base (anticipatory behavior).
   - Verify L_a contracts at high curvature and expands on flat terrain.
```

---

## PHASE 5D: Synthetic Validation Suite

```
Read docs/34_terrain_following_planning.md §Validation.

Before any terrain-following code is merged, it must pass a deterministic
synthetic test suite. These tests are the safety gate.

Tasks:

1. Create `tests/terrain_following_validation.rs`.

2. Implement four synthetic DEM profiles as test fixtures:
   a. FLAT: constant elevation (e.g., 500 m). Zero slope.
   b. SINUSOIDAL: y = A × sin(2π × x / λ) + baseline.
      Parametric: vary A and λ to test progressively harder terrain.
   c. STEP FUNCTION: flat at 500 m for 10 km, instantaneous rise to
      800 m, flat at 800 m for 10 km.
   d. SAWTOOTH: high-frequency triangle waves (e.g., 100 m amplitude,
      500 m period).

3. For each profile, run the full pipeline:
   raw DEM → Elastic Band → B-spline fit → sample at 1 m intervals.

4. Assert the following on every sampled point:
   a. altitude ≥ terrain + min_AGL (NEVER violated — convex hull).
   b. |slope| ≤ tan(max_pitch_deg) (pitch limit).
   c. |curvature| ≤ κ_max (g-load limit).
   d. No discontinuous jumps > 0.01 in slope between adjacent samples
      (C² continuity proxy).

5. Compute and print photogrammetric efficiency metrics:
   - RMS deviation from ideal AGL (lower = tighter terrain-following).
   - Max deviation from ideal AGL (identifies worst-case GSD error).
   - Percentage of path within ±5% of ideal AGL.

6. The sinusoidal test should be parametric: progressively increase
   amplitude and decrease wavelength until the terrain EXCEEDS the
   aircraft's kinematic envelope. Verify the B-spline gracefully
   clips curvature to κ_max and increases AGL deviation rather than
   violating the load factor.

Acceptance: All four profiles pass all assertions. This test suite runs
in CI on every push and must never be disabled.
```

---

# v0.4 — Networked FMS Daemon

> Prerequisites: v0.3 complete. The engine generates dynamically feasible,
> C²-continuous terrain-following trajectories.
>
> ARCHITECTURE SHIFT: This release transitions IronTrack from a CLI tool
> into a persistent, networked daemon. Tokio is now permitted for the
> Axum server, WebSocket I/O, serial port polling, and NMEA ingestion.
> Read Doc 28 (Daemon Design) thoroughly before starting.

---

## PHASE 6A: Axum Daemon, REST API, and Shared State

```
Read docs/28_irontrack_daemon_design.md §1 (REST) and §3 (State).
Read docs/42_geopackage_extensions.md §WAL and §Concurrency.

Tasks:

1. Add dependencies:
   axum = "0.7", tokio = { version = "1", features = ["full"] },
   tower-http = { version = "0.5", features = ["cors", "trace"] },
   serde_json = "1", deadpool-sqlite = "0.8"

2. Create `src/network/mod.rs`, `src/network/server.rs`,
   `src/network/errors.rs`.

3. Define `FmsState`:
   ```rust
   pub struct FmsState {
       pub flight_plan: Option<FlightPlan>,
       pub terrain_engine: TerrainEngine,
       pub current_gps: Option<CurrentState>,
       pub active_line: Option<usize>,
       pub line_status: Vec<LineStatus>,  // per-line progress/flags
       pub trigger_log: Vec<TriggerEvent>,
   }
   ```
   Wrap in `Arc<tokio::sync::RwLock<FmsState>>`.
   RwLock, NOT Mutex — the read-to-write ratio is heavily skewed
   (many WebSocket readers vs one NMEA writer). tokio::sync::RwLock
   specifically to avoid deadlocks across .await points.

   CONCURRENCY MANDATE [Doc 42 — Gemini Criticism 2]:
   ─────────────────────────────────────────────────────
   a) Execute these PRAGMAs on EVERY new connection:
      PRAGMA journal_mode = WAL;
      PRAGMA synchronous = NORMAL;
      PRAGMA busy_timeout = 0;  -- we WANT immediate failure, not silent retry
   b) Use `deadpool-sqlite` as a connection pool. Never share a single
      rusqlite::Connection across async tasks — Tokio's cooperative
      scheduling means a connection held across an .await point blocks
      ALL other tasks trying to write.
   c) SQLITE_BUSY is NOT a transient retry condition — it is a FATAL
      architectural violation. The WAL-reset bug can cause permanent,
      unrecoverable database corruption. If you receive SQLITE_BUSY,
      log the error with full context and trigger ERR_DB_LOCKED
      fail-safe shutdown of the offending process.
   d) WAL fails on network filesystems (NFS/SMB). The GeoPackage MUST
      reside on local storage.
   ─────────────────────────────────────────────────────

4. Implement REST resource model (path-versioned /api/v1/):
   - POST /api/v1/missions — ingest boundary + config, generate plan
     via spawn_blocking. Return summary JSON.
   - GET /api/v1/missions/{id} — metadata, bounding box, stats.
   - GET /api/v1/missions/{id}/lines — flight line SoA data.
   - PUT /api/v1/missions/{id}/lines/{line_id} — modify line params.
   - DELETE /api/v1/missions/{id}/lines/{line_id} — remove line,
     recalculate block stats.
   - GET /api/v1/missions/{id}/export — stream GeoPackage binary.
     Content-Type: application/geopackage+sqlite3.
     Content-Disposition: attachment; filename="mission.gpkg".
     Use StreamBody wrapping tokio::fs::File — low memory footprint.

5. Implement structured error handling:
   ```rust
   pub struct ApiError {
       pub status: StatusCode,
       pub code: String,     // e.g., "ERR_GEOID_MISSING"
       pub message: String,
   }
   impl IntoResponse for ApiError { ... }
   ```
   Use thiserror for the error enum. Frontend triggers specific
   recovery flows based on the programmatic `code` field.

6. Implement bearer token middleware via tower:
   - On startup, generate a cryptographic random token (32 bytes hex).
   - Print to stdout: "Session token: <token>"
   - All routes wrapped in middleware::from_fn that extracts the
     Authorization header, verifies Bearer <token> via constant-time
     comparison. Returns 401 if invalid.

7. Enable CORS via tower-http (allow all origins for dev).

8. Add CLI subcommand: `irontrack daemon --port 3000`.
   The existing `plan` subcommand is unchanged.

9. Tests:
   - POST a boundary, GET the plan, verify it matches CLI output.
   - Verify 401 on missing/wrong token.
   - Verify structured error JSON on invalid input.
```

---

## PHASE 6B: WebSocket Telemetry and Hybrid Channels

```
Read docs/28_irontrack_daemon_design.md §2 (WebSocket Architecture).

Tasks:

1. Add dependencies:
   tokio-tungstenite or use axum's built-in WebSocket support.

2. Create `src/network/telemetry.rs`.

3. Implement the hybrid channel architecture:
   a. tokio::sync::broadcast channel (capacity ~256):
      For 10 Hz transient telemetry (CurrentState payloads).
      Slow receivers get RecvError::Lagged — oldest frames dropped.
      This is intentional: stale position data is dangerous.
   b. tokio::sync::watch channel:
      Holds the LATEST full system state snapshot (all line statuses,
      active line, trigger counts). Always readable.

4. Implement WebSocket endpoint GET /ws?token=<token>:
   - Verify token from query parameter (browser WS APIs don't support
     custom headers during upgrade).
   - On upgrade, IMMEDIATELY read the watch channel and send the full
     state as the first initialization frame.
   - Then enter the broadcast loop: forward 10 Hz telemetry frames.
   - Handle bidirectional messages: line selection from the client
     writes to FmsState, then broadcasts STATE_CHANGED to all clients.

5. Implement WAL-mode SQLite persistence:
   - Spawn a low-priority background task that periodically (every 5s):
     a. Acquires a READ lock on FmsState.
     b. Clones the line_status vector.
     c. Writes it to the GeoPackage via spawn_blocking.
   - Configure PRAGMA journal_mode=WAL on the SQLite connection.
   - This survives power losses without blocking concurrent REST reads.

6. Implement mock telemetry mode:
   --mock-telemetry flag: generates synthetic NMEA walking along the
   loaded flight plan at --mock-speed <m/s>. Essential for testing
   without hardware.

7. Tests:
   - Connect WS, verify init frame contains full state.
   - Verify 10 Hz message rate.
   - Kill and reconnect: verify resync from watch channel.
```

---

## PHASE 6C: NMEA Parsing and Serial Integration

```
Read docs/23_nmea_data_requirements.md and
docs/29_avionics_serial_communication.md §4 (Rust Serial).

Tasks:

1. Add dependencies:
   serialport = "4", tokio-serial = "5"

2. Create `src/network/nmea.rs` and `src/network/serial_manager.rs`.

3. Implement NMEA sentence parser for:
   - GGA (position, fix quality, HDOP, MSL altitude, geoid separation)
   - RMC (ground speed, true course, date/time)
   - Optional: PTNL,GGK (Trimble 8-decimal precision, native EHT)

4. Implement VID/PID auto-detection:
   - Maintain a config mapping logical roles to USB hardware signatures:
     ```toml
     [serial.gnss_receiver]
     vid = 0x1546
     pid = 0x01A9
     ```
   - On startup, iterate serialport::available_ports(), match VID/PID.
   - NEVER hardcode /dev/ttyUSB0 or COM3.

5. Implement hot-plug resilience (5-step recovery):
   a. On ConnectionReset/BrokenPipe: drop the SerialStream.
   b. Async backoff: tokio::time::sleep(500ms).
   c. Re-enumerate via available_ports(), match VID/PID for new path.
   d. Instantiate fresh tokio_serial::SerialStream.
   e. Resume the async read loop.

6. Implement RTK degradation detection:
   - Monitor GGA fix quality field.
   - If quality drops from 4 (RTK Fixed) to 5 (Float) or lower:
     broadcast an amber/red warning via the telemetry WebSocket.
   - Log the degradation event with timestamp.

7. Implement mDNS service discovery:
   - On daemon startup, advertise _irontrack._tcp.local. via mdns-sd.
   - Include port number and host IP in the advertisement.
   - Document the fallback chain: mDNS → cached IP → manual entry.

8. Tests:
   - Feed known NMEA sentences, verify CurrentState updates.
   - Simulate serial disconnect/reconnect, verify recovery.
   - Verify RTK degradation triggers the correct warning.
```

---

## PHASE 6D: Integration Test Gate

```
Comprehensive integration test spanning 6A–6C.

Tasks:

1. Spawn the daemon in a test with mock telemetry enabled.
2. POST a boundary polygon via REST, verify plan generation.
3. Connect a WebSocket client, verify init frame + 10 Hz stream.
4. PUT to modify a line, verify all WS clients receive the update.
5. Simulate RTK degradation in mock telemetry, verify warning appears.
6. Kill the daemon mid-flight, restart, verify WAL recovery restores
   line completion status.
7. Verify the /export endpoint streams a valid GeoPackage.

Acceptance: All integration tests pass. This suite runs in CI.
```

---

# v0.5 — Atmospheric Kinematics and Energy Economy

> Read Doc 16 (Wind Data), Doc 38 (UAV Endurance Modeling),
> Doc 50 (Wind-Corrected Dubins Paths).

> Prerequisites: v0.4 complete. The daemon is running, telemetry is flowing.

---

## PHASE 7A: Wind Vector Ingestion and Wind Triangle

```
Read docs/16_aerial_wind_data.md, docs/03_photogrammetry_spec.md
§Crab Angle, and docs/50_dubins_mathematical_extension.md §6 (Wind Correction).

Wind Correction Angle: WCA = arcsin(V_crosswind / V_TAS) [Doc 50]
Wind-corrected turn radius: r_safe = (V_TAS + V_wind)² / (g × tan(φ_max)) [Doc 50]
This expanded radius MUST become the baseline for all Dubins geometric derivations
when wind data is available.

Tasks:

1. Create `src/kinematics/mod.rs` and `src/kinematics/atmosphere.rs`.

2. Define WindVector { speed_ms: f64, direction_deg: f64 }.

3. Implement the Wind Triangle solver:
   Given TAS, desired ground Track, and WindVector, compute:
   - WCA = arcsin((wind_speed / TAS) × sin(wind_direction - TRK))
   - HDG = TRK + WCA
   - GS = resulting ground speed
   Handle degenerate case (wind ≈ 0) and impossible case (wind > TAS).

4. Wind is a STATIC input for v0.5 (plan-time only).
   Update POST /api/v1/missions to accept optional wind params.
   Update CLI: --wind-speed <m/s> --wind-dir <degrees>.

5. Compute and store GS per flight line segment. Headwind legs have
   lower GS. This data feeds into energy calculations (Phase 8A)
   and trigger interval timing (future).

6. Tests:
   - No wind: HDG = TRK, GS = TAS.
   - Pure headwind/tailwind: verify analytically.
   - Wind > TAS: return error.
```

---

## PHASE 7B: Crab Angle Compensation for Swath Width

```
Read docs/03_photogrammetry_spec.md §Crab Angle and
docs/21_dynamic_footprint_geometry.md.

Tasks:

1. Create `src/photogrammetry/crab_angle.rs`.

2. Compute crab_angle = HDG - TRK (from Phase 7A).

3. Compute effective swath width under crab:
   W_eff = W × cos(θ) - L × |sin(θ)|

4. Refactor line spacing: use W_eff instead of W when wind is provided.
   Different headings have different crab angles — each line's spacing
   is computed independently.

5. Store crab_angle per flight line in GeoPackage and GeoJSON output.

6. Tests:
   - Zero wind: W_eff = W, spacing unchanged.
   - 10 m/s crosswind: verify W_eff < W and adjusted spacing
     produces ≥ target sidelap.
```

---

## PHASE 8A: Aerodynamic Drag Model

```
Read docs/17_uav_survey_analysis.md §Kinematics and
docs/38_uav_endurance_modeling.md (complete document — power models,
battery electrochemistry, per-segment energy costing).

Key equations from Doc 38:
- Multirotor hover: P = (mg)^1.5 / √(2ρA)
- Fixed-wing drag polar: C_D = C_D0 + C_L²/(πeAR)
- Peukert battery effect: k ≈ 1.05 for LiPo
- Per-segment: E_line = P(V_TAS) × d/V_GS
- Turn energy: 60° bank = load factor 2.0 = 4× induced drag
- 20% battery reserve is a BLOCKING enforcement, not a warning —
  the LiPo discharge cliff in the final 15-20% SoC combined with
  current spikes virtually guarantees LVC breach [Doc 38].

Tasks:

1. Create `src/kinematics/energy.rs`.

2. Define PlatformDynamics { mass_kg, drag_coefficient,
   frontal_area_m2, battery_capacity_wh, propulsive_efficiency }.

3. Implement simplified quadratic power model:
   P = k1 × V³ + k2 / V (parasite + induced drag).
   ISA sea-level ρ = 1.225 kg/m³.

4. Implement energy_cost_j per segment (using GS from Phase 7A).

5. Implement total_mission_energy and estimated_endurance_minutes.
   Warning if margin < 20%.

6. CLI flags: --platform-mass, --platform-drag-coeff, etc.

7. Tests:
   - Zero wind: energy = power × (distance / speed).
   - Headwind costs MORE than tailwind (asymmetric).
```

---

## PHASE 8B: Energy-Optimized TSP Routing

```
Read docs/17_uav_survey_analysis.md §2.2.

Replace the TSP solver's edge weight from geodesic distance to energy
cost (Phase 8A). The cost function is now asymmetric with wind.

Tasks:

1. Replace weight(A, B) = karney_distance → energy_cost_j.

2. Verify the solver handles asymmetric costs correctly (A→B ≠ B→A).

3. Include flight line energy in the total (headwind vs tailwind
   direction choice per line).

4. Print optimized vs naive energy comparison.

5. Tests:
   - Strong headwind from north: solver prefers N→S (tailwind) lines.
   - Zero wind: results match distance-based output.

Architecture: synchronous rayon work. spawn_blocking from Axum.
```

---

# v0.6 — Glass Cockpit MVP

> Prerequisites: v0.5 complete. The daemon runs with REST, WebSocket,
> and NMEA telemetry.
>
> Read Doc 25 (Cockpit Display Design) and Doc 26 (Tauri Integration)
> thoroughly before starting.

---

## PHASE 9A: Tauri Application Shell and Daemon Embedding

```
Read docs/26_tauri_integration.md §1–§3.

Tasks:

1. Initialize a Tauri 2.0 project with React frontend.

2. Implement the auto-detecting hybrid daemon mode:
   - On startup, probe localhost:3000 via HTTP handshake.
   - Success → thin client mode (connect to external daemon).
   - Failure → spawn the Axum daemon in an isolated OS thread:
     ```rust
     std::thread::spawn(move || {
         let rt = tokio::runtime::Builder::new_multi_thread()
             .enable_all().build().unwrap();
         rt.block_on(run_daemon(state));
     });
     ```
     This prevents the "Cannot start a runtime from within a runtime"
     panic. The daemon's tokio runtime is completely independent of
     Tauri's event loop.

3. Register the irontrack:// custom URI protocol:
   - Intercept tile requests at the OS level.
   - Query the GeoPackage SQLite for the binary tile blob.
   - Return as HTTP response with correct MIME headers.
   - The frontend map library treats this identically to a remote server.

4. Implement multi-window support:
   - planning_view and cockpit_view labels.
   - Revision-ordered invalidation-pull: Rust backend = single source
     of truth with monotonically increasing revision counter.
     On mutation → broadcast lightweight invalidation event →
     each window independently pulls fresh data.

5. Configure IPC:
   - Commands (tauri::command) via spawn_blocking for engine calls.
   - Events (app.emit) for 10 Hz telemetry.
   - Binary channels (tauri::ipc::Response) for SoA coordinate
     arrays — bypass JSON serialization.

6. Disable Tauri's auto-updater in tauri.conf.json.

7. Tests:
   - Verify daemon spawns in embedded mode when no external daemon.
   - Verify thin-client mode connects to running daemon.
   - Verify irontrack:// serves tiles from GeoPackage.
```

---

## PHASE 9B: Track Vector CDI and Block Overview

```
Read docs/25_aircraft_cockpit_design.md §2 (CDI) and §4 (Colors).

Tasks:

1. Implement the Canvas 2D block overview:
   - Render all flight lines color-coded per ARINC 661 standards:
     gray (planned), amber (selected/active), green (in-progress),
     cyan (complete), red (failed).
   - Aircraft icon at current position/heading (10 Hz update).
   - Touch-to-select lines via R-tree spatial index hit-testing.

2. Implement the Track Vector CDI:
   - Horizontal position = cross-track error (XTE).
   - Tilt angle = track angle error (TAE).
   - CDI scale auto-locks on line entry:
     photogrammetry ±100 m, LiDAR ±50 m, corridor ±25 m.
   - Scale changes during active acquisition are PROHIBITED.

3. Apply B612 font (Airbus cockpit-optimized):
   - Minimum 3.8 mm physical character height at 760 mm viewing distance.
   - Load as a web font in the Tauri WebView.

4. Enforce 20 mm × 20 mm minimum invisible touch hit-boxes for all
   interactive elements. Visual icons can be smaller (12 mm) but the
   active hit area must be 20 mm.

5. High-DPI rendering: multiply canvas pixel buffer by devicePixelRatio,
   scale context, CSS shrink to 100%.

6. Dark background mandatory. Never Red or Amber for non-alerting elements.
```

---

## PHASE 9C: Altitude Guidance and Terrain Warnings

```
Read docs/25_aircraft_cockpit_design.md §3 (Altitude) and §6 (Basemaps).

Tasks:

1. Implement the altitude tape (right side of attitude area):
   - Primary barometric AMSL altitude on a vertical tape.
   - Adjacent color-coded AGL ERROR deviation indicator:
     Green zone = ±15 m acceptable deviation.
   - DSM penetration amber hazard zone at the bottom of the VDI tape
     when flying over Copernicus-derived terrain.

2. Implement offline basemap rendering:
   - Default: vector-only (flight lines, boundaries, exclusion zones).
   - Optional: embedded raster tiles from GeoPackage (shaded relief).
   - Served via the irontrack:// protocol from Phase 9A.

3. Subscribe to the daemon's WebSocket for 10 Hz telemetry.
   Update all instruments via requestAnimationFrame.
```

---

## PHASE 9D: Airspace and Obstacle Cockpit Overlay

```
Read docs/35_airspace_obstacle_data.md §Cockpit Alerting.

Tasks:

1. Render airspace boundaries from the GeoPackage R-tree on the
   block overview map (Class B blue, C magenta, D dashed blue).

2. Render DOF obstacles with semi-transparent hazard perimeter rings
   (10× AGL buffer). Verified = solid red, unverified = hollow amber.

3. Implement Time-to-CPA proximity alerting:
   - Project the aircraft's forward vector (speed + COG + vertical rate).
   - Evaluate temporal intersection with R-tree airspace boundaries.
   - Threshold: 3 minutes + 500 ft vertical buffer.

4. Implement the AC 25-11B alert state machine:
   - Advisory (green/white) → Caution (amber, chime) → Warning (red,
     continuous tone, flashing).
   - 20 mm acknowledgment hit-box for caution alerts. Tap silences
     audio, amber banner stays until divergence.
   - Escalation to Warning if actual penetration occurs.

5. All alerts are advisory only. Never restrict pilot control authority.
```

---

# v0.7 — Web Planning UI and Airspace Integration

> Prerequisites: v0.6 complete. The glass cockpit is functional.
>
> Read Doc 27 (Web UI Plan) and Doc 35 (Airspace/Obstacles).

---

## PHASE 10A: React + MapLibre Foundation

```
Read docs/27_web_ui_plan.md §1 (Map Library).

Tasks:

1. Initialize a React project. Add react-map-gl + MapLibre GL JS.
   Leaflet rejected (DOM bottleneck). Mapbox rejected (proprietary).

2. Implement the map canvas with satellite/topo/blank layer switching.

3. Implement client-side boundary parsing:
   - GeoJSON: JSON.parse → MapLibre GeoJSONSource.
   - KML/KMZ: jszip + @tmcw/togeojson.
   - Shapefile: shapefile.js + proj4js for CRS reprojection.

4. Implement polygon editing via maplibre-gl-geo-editor:
   - Vertex snapping (15–18 px tolerance).
   - Exclusion zones via turf.difference (interior rings).
   - 50-operation undo stack (Ctrl+Z / Ctrl+Y).

5. Connect to the daemon REST API with the bearer token.
   Gateway screen prompts for the token on first connection.
```

---

## PHASE 10B: Real-Time Streaming Plan Preview

```
Read docs/27_web_ui_plan.md §3 (Streaming Preview).

Tasks:

1. Implement WebSocket connection to the daemon.

2. As the operator adjusts parameters, the daemon streams flight lines
   one by one via tokio::sync::broadcast as they're computed.

3. Intercept messages with TanStack Query:
   queryClient.setQueryData(['flightPlan', planId], ...) for partial
   cache updates. Do NOT use raw useState — causes 20+ re-renders/sec.

4. Update MapLibre directly:
   map.getSource('flight-lines').setData(updatedFC) inside
   requestAnimationFrame. Bypasses React virtual DOM for map ops.

5. Implement the sensor library (TanStack Table, headless, ~15 KB):
   - Searchable, sortable grid of factory specs.
   - Side-by-side comparison matrix.
   - Profile save/load to GeoPackage via REST POST.
```

---

## PHASE 10C: Export and Batch Download

```
Read docs/27_web_ui_plan.md §5 (Export).

Tasks:

1. Implement the pre-flight metadata verification modal:
   - Schema version.
   - Copernicus attribution.
   - DSM penetration safety warning (non-dismissable if DSM source).
   - Altitude datum tag for the export format.

2. Implement batch export:
   - POST to daemon with export parameters.
   - Daemon generates all formats (GeoPackage, GeoJSON, QGC, DJI, MDB).
   - Returns ZIP archive as application/octet-stream.
   - React creates Blob → URL.createObjectURL → programmatic anchor click.
```

---

## PHASE 10D: Multi-User Collaboration

```
Read docs/27_web_ui_plan.md §6 (Collaboration).

Tasks:

1. Implement granular optimistic locking via WebSocket:
   - When an operator opens an editing subsystem, emit a lock request.
   - Daemon registers the lock, broadcasts STATE_LOCKED.
   - Other clients gray out the locked subsystem.
   - On save/close, release the lock.

2. Last-Write-Wins at the database level as the race-condition backstop.

3. CRDTs are explicitly rejected for v1.0 — silent merging of flight
   parameters could produce semantically unsafe combinations.
```

---

## PHASE 10E: Airspace and Obstacle Planning Overlay

```
Read docs/35_airspace_obstacle_data.md §1–§5.

Tasks:

1. Implement FAA airspace boundary ingestion:
   - Query ADDS ArcGIS Hub REST API for the project bounding box.
   - Recursive offset pagination for complete feature collection.
   - Render on MapLibre: Class B (blue), C (magenta), D (dashed blue).

2. Implement TFR/NOTAM ingestion:
   - Query NMS/SWIM APIs (or fallback to tfr.faa.gov).
   - Compute 4D temporal-spatial intersection per flight line.
   - Flag lines only if spatial AND temporal overlap.

3. Implement DOF obstacle overlay:
   - Ingest Daily DOF CSV.
   - Render with 10× buffer circles.
   - Verified (solid red) vs unverified (hollow amber).

4. Serialize all airspace/obstacle data into the GeoPackage R-tree.

5. Implement Part 107 structural exception calculator:
   - Fuse DEM + DOF for altitude ceiling expansion near structures.

6. All planning-phase alerts are visual warnings only.
```

---

## PHASE 10F: Sensor Library and Boundary Import Enhancements

```
Read docs/45_aerial_sensor_library.md, docs/43_boundary_import_formats.md,
and docs/49_computational_geometry_primitives.md.

Tasks:

1. Implement `irontrack_sensors` table in GeoPackage [Doc 45]:
   - Normalized columns: name, manufacturer, type (CAMERA/LIDAR),
     sensor_width_mm, pixel_pitch_um, resolution_x/y,
     focal_lengths_mm (JSON array), prr_hz, fov_deg, weight_kg,
     trigger_interface, event_output, notes (JSON overrides).
   - Pre-populate with 10 cameras + 9 LiDAR from Doc 45.
   - Rolling shutter flag drives non-dismissable warning.

2. Implement `irontrack_mission_profiles` table [Doc 45]:
   - FK to sensors. Core params (overlap_fwd, overlap_side, mission_type)
     as relational columns. Custom overrides as JSON column.
   - NOT a JSON blob in irontrack_metadata.

3. Wire sensor library into TanStack Table headless grid in React UI.

4. Implement CSV-WKT boundary import [Doc 43]:
   - WKT geometry column parsing via wkt crate (TryFromWkt trait).
   - Supports holes and multi-polygons natively.

5. Implement DXF boundary import (transitional) [Doc 43]:
   - LwPolyline extraction with bulge-to-arc discretization.
   - Mandatory non-dismissable EPSG modal for missing SRS.
   - CRITICAL: v1.0 CRS = WGS84 + UTM only. Halt import on
     unsupported datums with clear reproject instructions.

6. Implement computational geometry primitives [Doc 49]:
   - Point-in-polygon: Sunday winding number (NOT ray casting).
   - Clipping: Weiler-Atherton for concave boundaries.
     Greiner-Hormann DISQUALIFIED (epsilon perturbation).
   - Buffer: straight skeleton via geo-buffer crate.
   - Area: Karney ellipsoidal (NOT Shoelace on UTM).
   - Triangulation: spade CDT (NOT delaunator).
```

---

# v0.8 — Embedded Trigger Controller

> Prerequisites: v0.7 complete. The full office→cockpit pipeline works.
>
> Read Doc 22 (Trigger Controller), Doc 23 (NMEA Requirements),
> Doc 29 (Serial Communication), Doc 40 (Sensor Trigger Library),
> Doc 41 (LiDAR System Control).

---

## PHASE 11A: Firmware Foundation — NMEA Parser and Dead Reckoning

```
Read docs/22_embedded_trigger_controller.md §2–§4,
docs/23_nmea_data_requirements.md, docs/40_sensor_trigger_library.md,
and docs/41_lidar_system_control.md.

WORKSPACE ISOLATION MANDATE [Doc 51 — Gemini Criticism 3]:
───────────────────────────────────────────────────────────
The trigger controller MUST be a completely isolated #![no_std] crate
within a Cargo workspace. It is legally a separate work (MIT-licensed,
not GPLv3) and architecturally a different target (thumbv7em-none-eabihf
or thumbv6m-none-eabi). If it shares a crate with the daemon, Claude
WILL inevitably use String, Vec, or other std types that require a heap
allocator — completely destroying the microsecond determinism required
for the camera intervalometer.

Create the workspace structure:
```toml
# Root Cargo.toml
[workspace]
members = [
    "irontrack-engine",    # CLI + daemon (std, x86/aarch64)
    "irontrack-trigger",   # Embedded firmware (no_std, thumbv*)
    "irontrack-protocol",  # Shared wire types (no_std compatible)
]
```
The `irontrack-protocol` crate defines the COBS frame types, CRC-32
constants, and scaled i32 waypoint encoding — shared by both the
daemon and firmware without either importing the other's dependencies.
It MUST be `#![no_std]` with `#![no_alloc]`.
───────────────────────────────────────────────────────────

Tasks:

1. Create the `irontrack-trigger/` crate as a no_std embedded Rust
   project targeting STM32 or RP2040. RTIC framework for the
   trigger-critical path. MIT/Apache-2.0 license (not GPLv3).

2. Implement the NMEA finite state machine parser:
   - Parse GGA (position, fix quality) and RMC (speed, course).
   - Auto-detect UBX binary (0xB5 0x62) vs NMEA ASCII ($).
   - DMA circular buffer ingestion at 230400+ baud.

3. Implement 1000 Hz linear dead reckoning:
   - On each RMC: latch ground speed and heading.
   - At 1000 Hz (hardware timer ISR): extrapolate position
     using speed × dt and heading.
   - On next GGA: snap to the true position (no smoothing).
   - Error bound: ~7 cm over 100 ms at 80 m/s.

4. Implement PPS integration:
   - Dedicate a GPIO pin to the receiver's PPS output.
   - On PPS rising edge: hardware interrupt latches the microsecond timer.
   - Camera fire timestamps = PPS_time + timer_delta.

5. Use heapless crate for all buffers (no heap, no OOM panics).
   Use defmt for zero-overhead diagnostic telemetry via RTT.
```

---

## PHASE 11B: Spatial Firing Algorithm

```
Read docs/22_embedded_trigger_controller.md §5 (Firing Algorithm).

Tasks:

1. Implement closest-approach derivative zero-crossing firing:
   - Monitor dd/dt (rate of change of distance to next waypoint).
   - As aircraft approaches: dd/dt < 0.
   - At the moment of passing: dd/dt transitions through zero.
   - Fire on the zero-crossing, regardless of lateral offset.

2. Implement 4-state spatial debounce machine:
   ARMED → READY → FIRED → LOCKOUT.
   Prevents double-firing from GPS jitter.

3. Implement opto-isolated GPIO trigger output:
   - Active-low, 10–15 ms pulse (Phase One/Hasselblad requirement).
   - Non-blocking hardware timer manages pulse duration.
   - The NMEA parsing loop is never stalled by the trigger.

4. Pre-loaded waypoints stored in a static heapless::Vec<Waypoint>.
   i32 scaled coordinates (lat×10⁷, lon×10⁷) for bandwidth efficiency.
```

---

## PHASE 11C: Serial Protocol — Daemon Communication

```
Read docs/29_avionics_serial_communication.md §2 (Binary Protocol).

Tasks:

1. Implement magic-byte framed binary protocol:
   Header (0xAA 0x55), MSG_TYPE (u8), PAYLOAD_LEN (u16),
   PAYLOAD ([u8]), CRC-32 (u32).

2. Implement the 7 message types:
   UPLOAD_WAYPOINTS, ACK, START_LINE, STOP_LINE,
   TRIGGER_EVENT, STATUS, ERROR.

3. Implement chunked waypoint upload:
   - 512-byte chunks with per-chunk ACK and CRC-32 validation.
   - Timeout and retransmit if no ACK.
   - RTS/CTS hardware flow control mandatory.

4. On START_LINE: arm GPIO, begin dead-reckoning loop.
   On STOP_LINE: disarm, flush buffer.
   On serial link loss: continue firing autonomously.

5. TRIGGER_EVENT packets sent back to daemon:
   sequence number, approximate lat/lon, internal timestamp.
```

---

## PHASE 11D: MAVLink Hybrid Path for UAVs

```
Read docs/29_avionics_serial_communication.md §3 (MAVLink).

Tasks:

1. Implement MAVLink MISSION_ITEM_INT export:
   - i32 coordinates scaled by 10⁷ (matches IronTrack encoding).
   - MAV_CMD_DO_SET_CAM_TRIGG_DIST for spatial triggering.

2. This path is for slow UAVs only (5–15 m/s).
   The jitter from the autopilot's 400 Hz polling loop is acceptable
   at these speeds (~3.75 cm at 15 m/s).

3. For fast fixed-wing (>15 m/s): the dedicated trigger controller
   from Phases 11A–11C is mandatory.

4. The operator selects trigger mode in the sensor configuration.
```

---

## PHASE 11E: Hardware Integration Test

```
Tasks:

1. Bench test with a u-blox F9P or Trimble receiver:
   - Verify NMEA parsing at 230400 baud.
   - Verify PPS timestamp accuracy.
   - Verify dead reckoning convergence.

2. Bench test trigger output:
   - Oscilloscope verification of pulse width (10–15 ms).
   - Opto-isolation verification.

3. End-to-end: daemon uploads waypoints → controller fires at
   correct spatial positions along a simulated (or real) trajectory.

4. Test with Eagle Mapping's aircraft hardware.
```

---

# v0.9 — 3DEP and US-Domestic Geodesy

> Prerequisites: v0.8 complete. The full embedded pipeline works.
>
> Read Doc 30 (Geodetic Datum Transformation), Doc 31 (NAD83 Epochs),
> Doc 32 (Geoid Interpolation), Doc 33 (3DEP Integration).

---

## PHASE 12A: GEOID18 Binary Grid Parser

```
Read docs/30_geodetic_datum_transformation.md §3 (Binary Grid) and
docs/32_geoid_interpolation_survey.md §GEOID18.

Tasks:

1. Create `src/geodesy/geoid18.rs`.

2. Parse the NGS Big-Endian binary grid (g2018u0.bin, 34.3 MB):
   - 44-byte header: 4×f64 bounds + 3×i32 descriptors.
   - Data: row-major S→N, W→E packed f32 values.
   - USE f32::from_be_bytes(). Little-Endian versions are CORRUPTED
     (2019 NGS compilation error — decimeter-level errors).

3. Memory-map via memmap2. The Mmap instance is Send + Sync —
   share across rayon threads without locks.

4. Implement biquadratic interpolation (3×3 quadrant-aware window):
   - Determine which quadrant the query point occupies.
   - Shift the 3×3 window accordingly.
   - Row-wise quadratic fit (3 rows) → evaluate at query longitude.
   - Column-wise quadratic through 3 intermediate values → evaluate
     at query latitude.
   - Cast f32 grid values to f64 BEFORE polynomial math.

5. Implement the GeopotentialModel trait:
   ```rust
   pub trait GeopotentialModel: Send + Sync {
       fn undulation(&self, lat_deg: f64, lon_deg: f64) -> Result<f64>;
       fn grid_resolution_arcmin(&self) -> f64;
   }
   ```
   GEOID18 implements this. So does the existing EGM2008 module.
   This enables hot-swap to GEOID2022 when published.

6. Deployment: sidecar asset, NOT include_bytes! (34 MB binary bloat).

7. Tests:
   - Validate against known NGS INTG output for several CONUS points.
   - Verify biquadratic produces different (smoother) results than
     bilinear for the same query points.
```

---

## PHASE 12B: 14-Parameter Helmert Transformation

```
Read docs/30_geodetic_datum_transformation.md §2 (Helmert) and
docs/31_nad83_epoch_management.md §Helmert Coefficients.

Tasks:

1. Create `src/geodesy/helmert.rs`.

2. Implement the 14-parameter kinematic Helmert:
   - Published IGS08→NAD83(2011) coefficients (base epoch 1997.0).
   - Time-dependent parameter computation:
     T(t) = T(t₀) + Ṫ × (t - t₀) for all 7 parameters.
   - Geographic → ECEF → apply rotation matrix → ECEF → geographic.

3. Add epoch field to all spatial structs:
   ```rust
   pub struct GeoPoint {
       pub lat_deg: f64,
       pub lon_deg: f64,
       pub alt_m: f64,
       pub datum: AltitudeDatum,
       pub epoch: f64,  // e.g., 2010.0
   }
   ```

4. Record observation_epoch in GeoPackage irontrack_metadata.

5. Document the navigational vs analytical boundary:
   - For flight execution: 40 cm epoch drift is imperceptible.
   - For PPK post-processing: operator must resolve via HTDP/NCAT.

6. Tests:
   - Transform a known NAD83(2011) point to WGS84.
   - Verify the result differs from the input by ~1–2.2 m horizontally.
   - Round-trip: WGS84 → NAD83 → WGS84 recovers input to < 1 mm.
```

---

## PHASE 12C: 3DEP COG Ingestion Pipeline

```
Read docs/33_3dep_data_integration.md §2–§4.

Tasks:

1. Create `src/terrain/threedep.rs`.

2. Implement TNM Access API metadata query:
   GET https://tnmaccess.nationalmap.gov/api/v1/products
   with bbox, datasets="1 meter DEM", prodFormats="GeoTIFF".

3. Implement resolution tier fallback:
   a. Query 1-meter. Compute geometric coverage (geo crate intersection).
   b. If < 100% coverage: reject, fall back to 1/3 arc-second seamless.
   c. Alaska/transborder: escalate to 1 arc-second.
   d. NEVER generate flight lines over a DEM void.

4. Implement COG HTTP range request pipeline:
   a. Fetch TIFF header + IFD (first ~16 KB).
   b. Decode IFD, project flight bbox into COG's UTM CRS.
   c. Calculate which 512×512 tiles intersect, map to byte offsets.
   d. Spawn parallel tokio tasks issuing HTTP 206 requests.
   e. Decompress (LZW/DEFLATE) in memory.

5. Transform every pixel to WGS84 Ellipsoidal:
   H_NAVD88 + N_GEOID18 → h_NAD83 → Helmert → h_WGS84.
   Execute on rayon threads. Cache permanently in GeoPackage microtiles
   (256×256, dual-encoded: visual PNG + raw f64 SoA).

6. Tests:
   - Ingest a known 3DEP COG, verify pixel values match the source.
   - Verify datum transformation matches NGS NCAT output for test points.
   - Verify fallback triggers when 1-meter coverage has voids.
```

---

## PHASE 12D: Multi-Source Terrain Fusion

```
Read docs/33_3dep_data_integration.md §6 (Fusion) and
docs/30_geodetic_datum_transformation.md §4 (Canonical Pivot).

Tasks:

1. Implement datum homogenization:
   - 3DEP → GEOID18 + Helmert → WGS84 Ellipsoidal.
   - Copernicus → EGM2008 bicubic → WGS84 Ellipsoidal.

2. Implement transition zone feathering (MBlend-style):
   - Detect where the bounding box intersects both datasets.
   - Construct a spatial corridor (e.g., 500 m wide) at the boundary.
   - Compute vertical delta between 3DEP and Copernicus.
   - Bilinear blend: 100% 3DEP → gradual → 100% Copernicus.
   - Result: C⁰ continuous gradient, no datum-cliff pitch chatter.

3. Implement precedence rules:
   - 3DEP always takes absolute precedence where both exist.
   - Resolution superiority (1 m vs 30 m).
   - DTM vs DSM safety (bare-earth vs canopy-top).

4. Tests:
   - Verify no vertical discontinuity at the fusion boundary.
   - Verify DSM warning is embedded when Copernicus is used.
```

---

## PHASE 12E: PhantomData Datum Type Safety — Design Note and Performance Architecture

```
Read docs/37_aviation_safety_principles.md §PhantomData,
docs/52_geospatial_performance_rust.md, and the working Rust compiler
proof in docs/design/phantom_datum_types.md.

Tasks:

1. Write `docs/design/phantom_datum_types.md`:
   - Document the Coordinate<D> pattern with PhantomData zero-sized types.
   - Include the working Rust proof (Wgs84, Nad83_2011, Egm2008, Egm96,
     Navd88 marker structs + transformation functions).
   - Evaluate migration cost: ~33 GeoCoord constructor sites, FlightLine
     in every module. The existing runtime AltitudeDatum tagging works
     but does NOT prevent cross-datum operations at compile time.
   - Decision: implement during v0.9 epoch-aware struct refactoring.
     When epoch: f64 is added to every spatial struct, introduce
     PhantomData datum markers simultaneously — one coordinated
     migration, not two.
   - NOT a v0.3 scope bomb. This is a DESIGN NOTE for future phases.

2. Implement performance architecture [Doc 52]:
   - Add Criterion benchmark suite: Karney inverse (single + 1M batch),
     DEM bicubic interpolation, B-spline generation, GeoPackage bulk
     insertion, WebSocket broadcast latency.
   - Enforce with_min_len(1024) on all rayon DEM evaluation pipelines.
   - Verify SoA memory layout for all bulk coordinate arrays.
   - Add cargo-pgo CI step (instrumented build → profile → optimized build).
   - CRITICAL: fast_math is REJECTED. Do not enable -ffast-math or use
     the fast_math crate. It destroys Kahan summation error compensation.
```

---

# v1.0 — Production Release

> Prerequisites: v0.9 complete. The full US-domestic pipeline works.
>
> Read Doc 24 (PPK Integration), Doc 31 (NAD83 Epoch Management),
> Doc 36 (ASPRS Standards), Doc 44 (Testing Strategy),
> Doc 46 (Post-Flight QC), Doc 51 (GPLv3 Compliance).

---

## PHASE 13A: PPK Pulse Routing and Event Logging

```
Read docs/24_survey_photogrammetry_ppk.md.

Tasks:

1. Implement closed-loop PPK support:
   - IronTrack fires the camera.
   - Camera hot-shoe feedback → GNSS event pin (not IronTrack's concern).
   - IronTrack logs the trigger command timestamp.

2. Implement open-loop PPK support:
   - Trigger controller splits the pulse: one to camera, one to
     GNSS event pin simultaneously (opto-isolated).

3. Export the trigger event CSV:
   event_number, UTC_timestamp, lat, lon, alt, fix_quality.
   Compatible with Trimble Business Center, NovAtel Inertial Explorer,
   and Emlid Studio import formats.

4. Implement the rolling shutter non-dismissable warning (if not
   already present from v0.2).
```

---

## PHASE 13B: Eagle Mapping Production Validation

```
Read docs/44_irontrack_testing_strategy.md §7 (Field Acceptance) and
docs/36_asprs_data_standards.md.

Tasks:

1. Full workflow integration test with Eagle Mapping's hardware [Doc 44]:
   - Phase 1: Trackair parallel execution. Generate identical flight plans
     in both IronTrack and Trackair. Spatial delta analysis must demonstrate
     C² terrain-following vs Trackair's discontinuous stepped altitudes.
   - Phase 2: Live flight with physical trigger controller + sensor payload.
     Verify trigger events, coverage completeness, PPK compatibility.
   - Phase 3: ASPRS 2024 compliance [Doc 36]:
     * RMSE only (95% confidence multiplier eliminated in 2023 Ed. 2).
     * 30 minimum checkpoints per CLT (scaling to 120 cap).
     * NVA/VVA vertical accuracy differentiation.
     * GSD heuristics: AT ≈ 0.5×pixel, raw GSD ≤ 95% of deliverable.
   - Gate: orthomosaics meet ASPRS class without manual waypoint
     intervention AND flight crew validates cockpit ergonomics.

2. Verify datum transformation pipeline (GEOID18 + Helmert) produces
   results matching NGS OPUS output to < 3 cm.

3. Stress test: 500-line block over mountainous terrain.
   Verify streaming preview performance, GeoPackage size, and
   terrain-following trajectory quality.

4. Run the full Criterion benchmark suite [Doc 52] and establish
   baseline performance numbers for the production hardware
   (field laptop + Raspberry Pi 5).
```

---

## PHASE 13C: Stability Hardening

```
Tasks:

1. NSRS 2022 preparation:
   - Implement GEOID2022 trait stub (pending NGS grid publication).
   - Verify epoch-aware structs propagate correctly through the pipeline.
   - Monitor FGCS vote (expected mid-2026).

2. CI/CD hardening [Doc 44]:
   - cargo fmt + cargo clippy -- -D warnings on every push.
   - cargo-deny for license scanning (SSPL/BSL exclusion) [Doc 51].
   - Cross-compilation build matrix: x86_64 + aarch64 + thumbv7em + thumbv6m.
   - Synthetic terrain-following validation suite (SAFETY GATE — never disable).
   - Criterion benchmark regression detection (>5% slowdown = flag PR).
   - cargo-llvm-cov code coverage.
   - Dockerized ARM cross-compilation for Pi edge binaries.
```

---

## PHASE 13D: Post-Flight QC and Automated Re-Fly Planning

```
Read docs/46_post_flight_qc.md and docs/49_computational_geometry_primitives.md.

Tasks:

1. Implement Cross-Track Error (XTE) computation [Doc 46]:
   - Transform to local UTM (Karney-Krüger), compute perpendicular vector
     distance with directional sign (left/right drift).
   - Low-pass filter to reject GNSS multipath spikes.
   - Dynamic threshold from mission parameters (footprint width, sidelap).
   - Flag segments exceeding threshold for distance > footprint length.

2. Implement trigger event analysis [Doc 46]:
   - Terrain-adjusted GSD per exposure (AGL from DEM interpolation).
   - Forward overlap = 1 - (trigger_spacing / footprint_length).
   - RTK Float events flagged with widened error margins.
   - Gap markers in GeoPackage where overlap < minimum threshold.

3. Implement 6DOF image footprint reconstruction [Docs 21, 46]:
   - Quaternion attitude → DCM rotation matrix → boresight correction
     → collinearity equation inversion → DEM ray casting → irregular
     convex quadrilateral on topography.
   - Polygon intersection: Sutherland-Hodgman O(n×m) for standard
     clipping, O'Rourke O(n+m) for batch convex footprint processing.
   - Crab angle "sawtooth" detection: rotated footprints can have
     dangerously low side-lap even with perfect line spacing.

4. Implement coverage heat map [Doc 46]:
   - Explicit intersection meshing (NOT additive blending): Rust backend
     shatters the survey boundary into mutually exclusive sub-polygons
     with exact overlap depth.
   - Stream as raw binary via WebSocket to MapLibre.
   - Red/yellow/green aviation color schema per AC 25-11B.
   - Offline rendering via irontrack:// tile protocol.

5. Implement automated re-fly generation [Doc 46]:
   - Trapezoidal cellular decomposition of non-convex gap polygons.
   - Boustrophedon sweep within each convex cell.
   - TSP routing between gaps (NN + Or-opt).
   - Dubins/clothoid transitions. EB + B-spline terrain-following.
   - Append to active GeoPackage via WAL (non-blocking).
   - Instant WebSocket propagation to cockpit — no landing required.
```

---

## PHASE 13E: GPLv3 Compliance and Community Release

```
Read docs/51_gplv3_compliance.md.

Tasks:

1. License audit [Doc 51]:
   - Verify cargo-deny CI catches SSPL/BSL/proprietary contamination.
   - Verify serialport crate MPL-2.0 §3.3 Secondary License compliance
     (no Exhibit B exclusion).
   - Verify embedded firmware (irontrack-trigger crate) is MIT-licensed
     as a separate work — NOT GPLv3.
   - If shared geodesic code exists between daemon and firmware,
     isolate it in irontrack-protocol crate under MIT license.

2. Proprietary SDK integration [Doc 51]:
   - Document IPC Bridge Pattern for Phase One / DJI SDKs.
   - Plugin daemon template: separate binary, proprietary license,
     communicates with core via TCP/gRPC.

3. Geospatial attribution [Doc 51]:
   - Verify Copernicus attribution string in every GeoPackage/GeoJSON.
   - Verify 3DEP/EGM voluntary source citations in irontrack_metadata.

4. Documentation:
   - User guide (office planning → field execution → post-processing).
   - Developer guide (architecture, async boundary, trait system).
   - All 52 research documents in docs/ directory.
   - Operator manual documenting v1.0 CRS limitations.

5. Community release:
   - README, CONTRIBUTING.md, issue templates.
   - GPLv3 license headers on all .rs files (SPDX-License-Identifier).
   - CLA for initial contributor phase [Doc 51].
   - GitHub repository public.
```
