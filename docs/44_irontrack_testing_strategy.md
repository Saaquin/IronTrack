# Comprehensive Test Strategy for the IronTrack Flight Management System

> **Source document:** 44_IronTrack-Testing-Strategy.docx
>
> **Scope:** Complete extraction — all content is unique. Covers property-based testing, authoritative geodetic test vectors, Kahan summation validation, Axum integration testing, WebSocket load testing, end-to-end pipeline with golden-file regression, embedded SIL/HIL firmware testing, mock telemetry closed-loop verification, CI cross-compilation matrix, headless Tauri testing, and Eagle Mapping field acceptance protocol.

---

## 1. Unit Testing the Core Engine

### 1.1 Property-Based Testing (proptest)
Traditional hardcoded input-output pairs are inadequate for spatial transformations with polar singularities and antimeridian discontinuities. The `proptest` framework generates pseudo-random f64 coordinates spanning the full global domain (lat ±90°, lon ±180°) and verifies invariant properties hold. On violation, the framework automatically "shrinks" to the minimal reproducible case.

**Critical round-trip invariants:**
- `from_utm(to_utm(coord))` must recover input within ε < 1 mm (Karney-Krüger 6th-order series).
- `inverse_helmert(helmert(coord, epoch))` must reverse tectonic drift and geocentric offset exactly.
- CI pipeline evaluates millions of spatial permutations per run.

### 1.2 Authoritative Known-Answer Test Vectors

| Authority | Dataset | Scope / Precision Target |
|---|---|---|
| Charles Karney | GeodTest.dat (25 MB, 500K geodesics) | Direct + inverse geodesic on WGS84 ellipsoid, 15 nm accuracy, antipodal solutions |
| NGA | Gold Data v6.3 | MGRS, UTM projections, 3/7-parameter datum transforms, extreme central meridian offsets |
| NGS | NCAT / VERTCON validation data | NAVD88→NAD83 via GEOID18 biquadratic interpolation, U.S. domestic geodetic compliance |

**GeodTest.dat format:** 7 space-delimited fields per line (lat1, lon1, azi1, lat2, lon2, azi2, distance). Parse start parameters → compute direct geodesic via geographiclib-rs → assert destination matches published coordinates.

### 1.3 Kahan Summation Drift Validation
Simulates 80 m/s fixed-wing aircraft for 100 seconds without GNSS fix → 100,000 consecutive 1000 Hz dead-reckoning integrations. Computes final position three ways:
1. **Naive summation** (accumulated f64 drift).
2. **IronTrack Kahan summation** (compensation variable captures truncated low-order bits).
3. **Arbitrary-precision oracle** (e.g., `rug` crate for exact arithmetic).

Assert: Kahan error remains effectively independent of operation count (O(1) error growth, not O(n)).

---

## 2. Integration Testing the Network Daemon

### 2.1 Axum In-Process Testing
Uses `axum-test` crate or `reqwest` against a `TestServer` — passes HTTP requests directly to handlers in-memory (no TCP port binding, no port collisions).

**Critical test scenarios:**
- **Auth rejection:** Request protected endpoints without ephemeral bearer token → assert 401 Unauthorized.
- **Structured errors:** Request DEM terrain outside cached GeoPackage bbox → assert exact `ERR_GEOID_MISSING` code (not generic 500).
- **State contention:** `tokio::spawn` hundreds of concurrent reads while a separate task writes (e.g., updating line completion status). Assert no deadlocks, no writer starvation, immediate state reflection in subsequent reads.

### 2.2 WebSocket Telemetry Verification
Using `tokio-tungstenite`:
1. Establish connection → assert 101 Switching Protocols.
2. Assert first frame is full state snapshot from `watch` channel (mid-mission join synchronization).
3. Inject mock NMEA bytes into daemon → await telemetry frames → verify computed XTE/TAE match spatial geometry.

### 2.3 Concurrent Load Testing
Spin up thousands of concurrent WebSocket clients. Measure propagation delay (NMEA injection → telemetry frame receipt at all clients). Monitor `RecvError::Lagged` behavior on `broadcast` channel — slow clients (degraded WiFi) must drop stale frames without blocking high-performance clients. Validates memory isolation and Tokio executor throughput under severe contention.

---

## 3. End-to-End Pipeline Testing

### 3.1 Full Pipeline
1. **Inject:** Complex GeoJSON boundary + sensor profile + target GSD.
2. **Terrain ingestion:** Mock 3DEP AWS S3 COG endpoint → HTTP 206 range requests for 512×512 microtiles.
3. **Trajectory routing:** Elastic Band → adaptive-knot B-spline. Assert: projected flight line **never penetrates raw DEM** (convex hull safety guarantee).
4. **Database assembly:** Query output GeoPackage → assert irontrack_metadata (schema_version, epoch, Copernicus attribution), R-tree spatial index triggers.

### 3.2 Export Format Verification
- **DJI .kmz:** Unzip, parse XML. Assert WGS84 Ellipsoidal → EGM96 conversion applied. Altitude values differ from internal EGM2008 by expected geoid separation.
- **Trackair .mdb:** Snapshot compatibility check — Flights, Flight_Leg, Waypoints tables match legacy schema for Applanix POSView.
- **QGC .plan:** Validate JSON against MAVLink mission schema. Verify camera trigger distance command enums.

### 3.3 Golden-File Regression (insta crate)
Generate flight plan for topographically complex boundary (steep cliffs, aggressive adaptive knot clustering). Serialize deterministic 3D coordinate array to `.snap` file in version control. On subsequent CI runs, regenerate and deep-diff. If B-spline logic changes the output by even a fraction of a millimeter → test fails → forces explicit engineering review of the geometric deviation.

---

## 4. Embedded Firmware Testing

### 4.1 Software-in-the-Loop (SIL)
Compile no_std intervalometer logic for x86_64 host (Rust's cross-compilation). Use `embedded-hal-mock` to mock MCU peripherals:

- **UART simulation:** Inject synthetic NMEA $GPGGA/$GPRMC byte arrays into mocked UART. Evaluate the firmware's finite state machine parser.
- **Trigger timing assertion:** Run 1000 Hz dead-reckoning. Monitor mocked GPIO pin state. Assert camera trigger transitions at the exact microsecond the dead-reckoned trajectory crosses the derivative zero-crossing of the target waypoint.

SIL validates RTIC scheduling, NMEA parsing, and spatial triggering math are logically flawless before flashing to hardware.

### 4.2 Hardware-in-the-Loop (HIL)
Physical RP2040 or STM32F407 evaluation board connected via FTDI USB-to-Serial. Three critical tests:

1. **Memory pagination:** Transmit 120 KB payload (15,000 LiDAR waypoints) via COBS-framed binary protocol. Assert heapless static buffers paginate without OOM panic. Verify CRC-32 acknowledgments.
2. **Baud rate / DMA saturation:** Bombard UART at 460,800 baud with 100 Hz UBX navigation data. Assert DMA circular buffer processes without ISR starvation — zero dropped frames.
3. **Electrical jitter:** Inject PPS signal + NMEA stream. Physical logic analyzer monitors GPIO trigger output. **Pass criterion: worst-case GPIO jitter < 100 ns** — confirms RTIC preemptive scheduling remains deterministic under load.

---

## 5. Mock Telemetry and Closed-Loop Testing

### 5.1 Daemon Mock Mode
Bypasses physical serial port enumeration. Internal kinematic simulator "flies" a virtual aircraft along the loaded B-spline trajectory with randomized crosswind, computed crab angle, and synthetic NMEA output (GGA, RMC, HDT for dual-antenna heading) at 10 Hz. Piped into daemon state manager + WebSocket broadcast → fully self-contained virtual environment.

### 5.2 Closed-Loop Verification
- **Glass cockpit:** Automated frontend tools connect Tauri to mocked daemon. As virtual aircraft deviates under crosswind → assert Track Vector CDI dynamically reflects accumulating XTE/TAE without frame drops.
- **SIL trigger sync:** Synthetic NMEA routed to SIL trigger controller. Monitor serial TX for TRIGGER_EVENT acknowledgments. Assert triggers fire at correct Cartesian coordinates despite lateral cross-track deviation → proves closest-approach derivative algorithm decouples lateral error from along-track synchronization.

---

## 6. Continuous Integration Pipeline

### 6.1 Static Analysis (Every PR)
- `cargo fmt` — idiomatic formatting.
- `cargo clippy` — anti-patterns, memory safety, performance.

### 6.2 Cross-Compilation Build Matrix

| Target | Architecture | Purpose |
|---|---|---|
| x86_64-unknown-linux-gnu | x86 | Standard host (office laptops, CI runners) |
| aarch64-unknown-linux-gnu | ARM64 | Raspberry Pi, edge devices (Dockerized qemu) |
| thumbv7em-none-eabihf | Cortex-M4F | STM32 trigger controller firmware |
| thumbv6m-none-eabi | Cortex-M0+ | RP2040 trigger controller firmware |

All four targets must compile successfully on every PR.

### 6.3 Headless Frontend Testing
Linux CI runners lack graphical displays. Solution: `xvfb-run` (X virtual framebuffer) + **Playwright** browser automation.
- Launch compiled Tauri application headlessly.
- Playwright scripts interact with Canvas 2D map, click 20mm touch targets, manipulate flight line selections.
- Assert DOM elements render correctly, WebSocket connection established.

### 6.4 Code Coverage
`cargo-llvm-cov` or `tarpaulin` integrated into PR workflow. Reviewers verify that new algorithmic branches (e.g., 3DEP/Copernicus terrain transition feathering) are fully exercised by tests before merge.

---

## 7. Field Validation: Eagle Mapping Acceptance Protocol

### 7.1 Trackair Parallel Execution
Identical mission planned in both Trackair and IronTrack (same boundary, sensor profile). Generate MDB exports from both. Deep spatial delta analysis of waypoint geometry.

**IronTrack must demonstrate:**
- Superior terrain-following (C² continuous B-spline vs Trackair's discontinuous altitude steps).
- Absolute MDB schema compatibility with downstream tools (Applanix POSView, Riegl RiACQUIRE).

### 7.2 Live Flight Mission
Deploy IronTrack daemon + Tauri glass cockpit on ruggedized field laptop. Trigger controller integrated into avionics rack (GNSS NMEA intercept, opto-isolated GPIO to Phase One / Riegl payload).

**Evaluate under real-world conditions:**
- **Coverage analysis:** Actual photographic/LiDAR footprint vs planned footprint. Overlap percentages must meet spec despite wind and attitude variation.
- **RTK degradation handling:** Monitor when fix drops from 4 (Fixed) to 5 (Float). Daemon must flash amber/red CDI warnings. Trigger controller must widen spatial firing radius and flag degraded events in telemetry.
- **Event logging accuracy:** Post-flight CSV trigger log (PPS-derived microsecond UTC timestamps + coordinates) compared against camera EXIF and GNSS RINEX trajectory.

### 7.3 ASPRS 2024 Compliance

| Metric | ASPRS 2024 Requirement | IronTrack Validation Target |
|---|---|---|
| Statistical measure | RMSE only (no 95% confidence multiplier) | All PPK evaluations reported in absolute RMSE |
| 3D checkpoints | Minimum 30 independent | Ground survey arrays verify RMSE_x, RMSE_y, RMSE_z |
| Vertical accuracy | NVA and VVA differentiated | Terrain-following profiles achieve target NVA threshold |
| Swath precision | Relative interswath precision documented | Trigger controller spatial consistency under crosswind |

**Acceptance gate:** Orthomosaics and point clouds meet targeted ASPRS RMSE accuracy class without manual waypoint intervention, AND flight crew validates glass cockpit ergonomic usability. Upon passing → authorized for production adoption at Eagle Mapping and wider open-source distribution.
