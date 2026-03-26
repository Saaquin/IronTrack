  
**IRONTRACK**

Project Charter & System Manifest

**Version 2.0**

*Open-Source Flight Management System for Aerial Survey*

GPLv3 Licensed • Rust Core Engine • GeoPackage Native

March 2026

# **1\. Executive Summary**

**IronTrack** is an open-source Flight Management System (FMS) for aerial survey operations, built entirely in Rust. It replaces the fragmented legacy toolchain of proprietary flight planners, manual DXF conversions, and disconnected cockpit software with a single, unified system that flows from the planning desk to the aircraft cockpit and back.

The system is designed around a single universal data container — the GeoPackage — which carries the flight plan, terrain data, sensor configuration, safety metadata, and legal attribution through every stage of the mission lifecycle. An operator plans in the office, uploads the GeoPackage to an airborne edge device, and both the pilot and sensor operator see the same plan. No format conversions. No thumb drives with MDB files. No re-entry.

IronTrack operates across four distinct architectural layers: a headless CLI engine for batch planning and CI/CD integration, a persistent Axum-based network daemon that serves REST and WebSocket APIs from any laptop or field device, a dedicated real-time trigger controller that sits directly on the NMEA serial bus to fire camera and LiDAR pulses with sub-meter spatial precision, and two frontend layers — a Tauri desktop application for field operations and a React web interface for office collaboration.

**Target Adopter:** Eagle Mapping. The software must integrate into their existing aerial survey workflows as the primary proof-of-concept environment. Securing production adoption at Eagle Mapping is the gating requirement before wider community release.

# **2\. Licensing and IP Governance**

**License:** GNU General Public License v3 (GPLv3). All modifications, distributions, and derivative works must be distributed under the same open terms, preventing proprietary capture or vendor lock-in. Every Rust source file carries an SPDX-License-Identifier header.

**Intellectual Property:** IronTrack is an independent, founder-led FOSS initiative. All IP, copyright, and initial codebase authorship are retained by the founder. There is no corporate sponsorship, ownership, or exclusive rights granted to any employer or commercial entity.

**Corporate Boundary:** While designed to solve operational friction for commercial aerial survey providers, IronTrack is developed independently. The architecture remains strictly decoupled — headless engine with no GUI in the core — to maintain legal arm’s-length separation under GPLv3.

**Third-Party Attribution:** The Copernicus DEM license requires mandatory attribution in any derived product. IronTrack embeds attribution and disclaimer strings in every GeoPackage and GeoJSON output.

# **3\. System Architecture**

IronTrack is built as a layered system with strict separation between the compute engine, the network services, and the presentation layers. This decoupling ensures the mathematical core — which handles life-critical aviation data — is never compromised by UI concerns, network latency, or frontend framework churn.

## **3.1 Layer Model**

| Layer | Technology | Responsibility |
| :---- | :---- | :---- |
| Core Engine | Rust (synchronous, rayon) | Geodesy, photogrammetry, DEM processing, flight line generation, datum conversions, GeoPackage I/O. Pure math — no network, no hardware. |
| Network Daemon | Rust (Axum, Tokio) | REST API, WebSocket telemetry broadcast, NMEA GPS parsing, mission state management. Runs on any laptop, tablet, Pi, or NUC. Pre-computes trigger waypoints and uploads them to the trigger controller. |
| Trigger Controller | Embedded firmware (hardware TBD) | Real-time spatial intervalometer. Reads raw NMEA GGA sentences over UART, dead-reckons between GPS fixes, and fires GPIO voltage pulses to camera/LiDAR at precise spatial intervals. No TCP stack — bare-metal or RTOS only. |
| Glass Cockpit | Tauri (Rust \+ WebView) | Pilot-facing display: line navigation, heading/altitude/track guidance, line selection, progress tracking. Connects to the daemon via WebSocket. |
| Planning Web UI | React | Office-based mission planning, boundary import, sensor configuration, export management, team collaboration. Connects to the daemon via REST/WebSocket. |
| Desktop App | Tauri (Rust \+ WebView) | Field laptop: offline planning \+ live monitoring, combines planning and cockpit capabilities. Can also run the daemon locally. |

## **3.2 The Async Boundary Rule**

All geodesic calculations, CRS transforms, DEM queries, photogrammetric math, and datum conversions run synchronously on OS threads via rayon. Tokio is permitted only for network I/O (HTTP, WebSocket, DEM downloads, NMEA ingestion). Axum handlers must bridge to the synchronous engine via tokio::task::spawn\_blocking. This is a non-negotiable safety constraint: blocking the Tokio runtime with CPU-heavy math starves all network connections, including live telemetry.

## **3.3 The Real-Time Boundary**

The trigger controller operates below the operating system’s network stack entirely. Camera and LiDAR triggers require spatial precision of less than one meter at flight speed. Even localhost TCP introduces non-deterministic latency from kernel scheduling, buffer flushing, and Nagle’s algorithm. Therefore, the trigger controller reads raw NMEA sentences directly from the GPS receiver’s UART and fires GPIO voltage pulses in the same tight loop — no TCP, no OS networking, no daemon round-trip. The daemon’s role is limited to pre-computing the trigger waypoint list and uploading it to the controller before the line is flown. Once the line starts, the controller operates autonomously.

## **3.4 Precision Requirements**

All coordinates, distances, and angles use f64 (64-bit floating point). Never f32. An f32 mantissa provides only 7 significant digits, which yields roughly 1-meter precision at planetary scale. IronTrack requires sub-millimeter precision for RTK/PPK survey work. Kahan summation is mandatory for all trajectory and distance accumulations to prevent floating-point drift. The Structure of Arrays (SoA) memory layout is used for bulk coordinate data to maximize L1 cache prefetching and enable SIMD vectorization.

## **3.5 Geodetic Algorithms**

| Operation | Algorithm | Rationale |
| :---- | :---- | :---- |
| Geodesic distance | Karney (geographiclib-rs) | Haversine has 0.3% error. Vincenty has antipodal convergence failure causing thread starvation in rayon. |
| WGS84 → UTM projection | Karney-Krüger 6th-order series | Legacy Redfearn truncations are not survey-grade. 6th-order provides sub-mm within the UTM zone. |
| Geoid undulation (EGM2008) | Native Rust grid interpolation | Primary geoid model. Used for Copernicus DEM altitude reference. |
| Geoid undulation (EGM96) | Vendored egm96 crate | Required for DJI autopilot exports. DJI FMS expects EGM96 orthometric heights. |
| Distance accumulation | Kahan summation | Prevents floating-point drift over long flight trajectories. |

# **4\. The GeoPackage: Universal Data Container**

The GeoPackage is not merely an export format. It is the single artifact that flows through the entire IronTrack mission lifecycle: from planning desk, to edge device, to cockpit display, and back to the office for archival. It replaces the MDB files, DXF conversions, and proprietary flight plan formats that currently fragment the Eagle Mapping workflow.

## **4.1 What the GeoPackage Contains**

* Flight lines with full SoA coordinate data (lat, lon, elevation per waypoint)

* Altitude datum tag per flight line (EGM2008, EGM96, WGS84 Ellipsoidal, or AGL)

* Sensor configuration profile (focal length, sensor dimensions, pixel pitch, overlap settings)

* Cached DEM microtiles for the survey area (terrain-following without network access at altitude)

* R-tree spatial index with full trigger suite for fast spatial queries

* Schema version and engine version in irontrack\_metadata table

* Copernicus attribution and disclaimer (legal compliance)

* DSM penetration safety warning (if terrain source is a DSM)

* Mission summary statistics (line count, total distance, bounding box, estimated endurance)

## **4.2 Schema Versioning**

Every GeoPackage contains an irontrack\_metadata table with a schema\_version key. If a newer engine opens an older file, it prints a warning suggesting re-export. If an older engine opens a newer file, it returns a clear error with upgrade instructions. This prevents silent data corruption as the schema evolves across releases.

## **4.3 GeoPackage as SQLite**

GeoPackage is built on SQLite with WAL mode enabled. The Rust engine uses rusqlite with the bundled feature flag for zero-dependency deployment. StandardGeoPackageBinary serialization handles the GP magic bytes, flags byte, envelope, and WKB geometry encoding. The R-tree spatial index includes all seven mandatory insert/update/delete triggers per the OGC specification.

# **5\. Operational Workflow**

IronTrack replaces the current Eagle Mapping pipeline of client boundary → DXF conversion → Trackair line drawing → MDB export → thumb drive → cockpit computer. The new workflow is:

## **5.1 Office Planning Phase**

**1\. Boundary ingestion:** Client provides a KML, KMZ, or GeoJSON boundary file. IronTrack parses the polygon (including concave boundaries and interior exclusion zones) directly. No DXF conversion required.

**2\. Sensor selection:** Operator selects from the built-in sensor library, loads a saved profile, or enters parameters manually. Profiles are stored in the GeoPackage and can be shared across the team.

**3\. Flight plan generation:** The engine supports two modes. **Auto-generation:** the engine ingests the DEM (Copernicus GLO-30 or future 3DEP), projects to the appropriate UTM zone, and generates terrain-following flight lines with the correct overlap and GSD, applying wind-based crab angle compensation if wind data is provided. **Manual line drawing:** the operator draws, edits, repositions, and deletes individual flight lines directly on the map. Manual lines receive the same terrain-following altitude assignment and overlap validation as auto-generated lines. Both modes can be combined: auto-generate a block, then manually adjust individual lines to handle irregular features, obstacles, or client-specific requirements.

**4\. Export:** The operator exports the plan as a GeoPackage (primary), GeoJSON (lightweight web alternative), QGroundControl .plan (for ArduPilot), DJI .kmz (with mandatory EGM96 datum conversion), or **Snapshot-compatible MDB** (for immediate integration with the existing Eagle Mapping acquisition pipeline). The MDB export is critical for the initial rollout: it allows IronTrack to replace Trackair for planning while the existing Snapshot/cockpit system handles execution, enabling a clean transition without requiring the full FMS stack from day one.

## **5.2 Field Execution Phase**

**5\. Deploy to the field:** The operator uploads the GeoPackage to any device running the IronTrack daemon — an operator laptop, a Raspberry Pi, an Intel NUC, or a ruggedized tablet. The daemon serves the plan over the local network. Both the pilot’s glass cockpit display and the operator’s web interface connect to the same daemon instance. The trigger controller (a separate embedded device) receives its waypoint list from the daemon via serial.

**6\. Glass cockpit:** The pilot’s display shows a top-down or oblique view of the flight line block with optional basemap. The pilot or operator selects lines. When a line is picked, the display helps navigate to the start point, then provides real-time heading, altitude, and track guidance to keep the aircraft on the line.

**7\. Active mission execution:** During flight, the daemon ingests live NMEA GPS telemetry and tracks aircraft position against the plan. Before each line begins, the daemon pre-computes the trigger waypoint list and uploads it to the trigger controller. The trigger controller then operates autonomously — reading GGA sentences directly from the GPS UART, dead-reckoning between fixes, and firing camera/LiDAR pulses via GPIO at precise spatial intervals. Both pilot and sensor operator see live progress on their displays. Lines are marked complete as they are flown.

## **5.3 Post-Flight Phase (Future)**

Post-flight QC (comparing actual GPS track vs planned lines, flagging coverage gaps) is deferred to a future release. For the current roadmap, IronTrack’s job ends when the flight lines are flown and the data is handed off to processing software (Pix4D, Metashape, TerraSolid).

# **6\. Glass Cockpit Specification**

The glass cockpit is the pilot-facing display that replaces the current cockpit computer running legacy software. It connects to the IronTrack daemon running on the edge device and renders a purpose-built navigation interface.

## **6.1 Display Modes**

**Block Overview:** Top-down view of the entire line block. Lines are color-coded by status: planned (gray), selected/active (amber), in-progress (green), complete (blue), failed/flagged (red). The aircraft position is shown as a real-time icon with heading indicator. This is the transit and line-selection view.

**Line Navigation:** When a line is selected, the display transitions to a guidance view. The pilot sees the start point, the line vector, and dynamic indicators for cross-track error (lateral deviation from the line), altitude error (deviation from planned AGL/AMSL), and heading error (deviation from the line bearing). This is the primary instrument view during data acquisition.

**3D Perspective (Future):** A forward-looking 3D view where the pilot flies through rendered cubes or gates representing waypoints. This is a stretch goal that depends on the Tauri WebView’s GPU capabilities and will be prototyped after the 2D views are stable.

## **6.2 Interaction Model**

Both the pilot and sensor operator can select and reorder lines. The pilot interacts via the glass cockpit display (touch or physical controls). The sensor operator interacts via the web interface or a secondary Tauri instance connected to the same daemon. Line selection state is synchronized via WebSocket: when either user picks a line, both displays update in real-time.

## **6.3 Vector Graphics Rendering**

The cockpit display uses vector-based graphics (SVG or Canvas) rather than raster tile maps. This ensures clean rendering at any zoom level, works without an internet connection (no basemap tile server required), and keeps the rendering pipeline lightweight enough for constrained edge hardware. An optional offline basemap layer (raster tiles cached in the GeoPackage or a sidecar MBTiles file) can be enabled when available.

# **7\. Sensor Configuration System**

IronTrack supports three tiers of sensor configuration to accommodate operators who know their equipment intimately and new users who need guidance.

## **7.1 Built-In Sensor Library**

A bundled database of common aerial survey sensors with factory specifications. The operator selects from a dropdown. The library covers both photogrammetric cameras and LiDAR sensors, and includes specifications for focal length, sensor dimensions, pixel pitch, image resolution, and (for LiDAR) PRR, scan rate, and FOV.

## **7.2 Manual Parameter Entry**

For custom or unlisted sensors, the operator enters raw parameters directly. The engine validates the inputs (e.g., rejects a focal length of zero, warns if the resulting GSD exceeds reasonable bounds for the stated mission type).

## **7.3 Saved Profiles**

Sensor configurations are stored as named profiles within the GeoPackage. An operator who has tuned a profile for a specific aircraft/sensor combination can save it and share the GeoPackage with the team. Profiles are versioned alongside the schema.

## **7.4 Mission Types**

| Mission Type | Spacing Logic | Primary Metric | Default Sidelap |
| :---- | :---- | :---- | :---- |
| Camera (nadir) | GSD-driven swath with side-lap percentage | Ground Sample Distance (cm/px) | 30% |
| Camera (oblique) | Deferred — future phase | Mean GSD at principal point | TBD |
| LiDAR | Swath width × (1 \- sidelap) | Point density (pts/m²) | 50% |
| Hybrid (Camera \+ LiDAR) | Deferred — future phase | Conflicting constraints | TBD |
| Corridor | Buffer offset from centerline | Corridor width (m) | N/A |

# **8\. Safety Architecture**

IronTrack processes life-critical aviation data. Datum errors, terrain miscalculations, or silent unit mismatches can cause aircraft to fly at the wrong altitude. The safety architecture is designed to make dangerous states impossible to represent and dangerous operations impossible to perform silently.

## **8.1 Datum-Tagged Altitudes**

Every altitude value in the system carries an explicit datum tag (EGM2008, EGM96, WGS84 Ellipsoidal, or AGL). Untagged altitudes cannot exist in the type system. Conversion between datums is explicit and auditable: the engine chains through WGS84 Ellipsoidal as the pivot datum, applying the correct geoid model at each step. The critical conversion for DJI export (EGM2008 → WGS84 → EGM96) is handled by the vendored EGM96 crate and never approximated.

## **8.2 DSM Penetration Warnings**

Copernicus GLO-30 is an X-band SAR Digital Surface Model. X-band radar penetrates 2–8 meters into forest canopy, meaning AGL clearance over forests is underestimated. IronTrack prints a mandatory safety warning to stderr, embeds it in the GeoPackage metadata, and writes it to GeoJSON output. The warning cannot be suppressed. If a DTM source is provided instead of a DSM, the warning is automatically omitted.

## **8.3 Dynamic Overlap Buffering (Future)**

A future phase will implement dynamic overlap buffering based on the platform’s aerodynamic stability profile and IMU error budget. The algorithm chains crab angle reduction, roll instability margin, and IMU drift margin to compute the minimum safe flight line spacing. This replaces the brute-force 80/80% overlap that wastes acquisition time and bloats processing pipelines.

# **9\. Release Roadmap**

| Release | Theme | Key Deliverables |
| :---- | :---- | :---- |
| v0.1 (Complete) | Foundation | DEM ingestion (SRTM, Copernicus), flight line generation, GeoPackage/GeoJSON export, Karney geodesics, EGM2008, bilinear terrain interpolation, CLI interface |
| v0.2 | Correctness & Safety | Datum-tagged altitudes, EGM96 vendored crate integration, DSM safety warnings, Copernicus attribution, QGroundControl .plan export, DJI .kmz export (EGM96), Snapshot MDB export (Eagle Mapping pipeline compatibility), LiDAR flight planning, KML/KMZ boundary import, GeoPackage schema versioning |
| v0.3 | Kinematics & Routing | Corridor mapping (polyline offset with miter/bevel joins), Dubins path turn generation with clothoid transitions, TSP route optimization (nearest-neighbor \+ 2-opt), bank angle and turn radius physics |
| v0.4 | Networked FMS Daemon | Axum REST API, WebSocket telemetry broadcast, NMEA GPS parsing, mission state management, mock telemetry mode, daemon CLI subcommand |
| v0.5 | Atmospheric & Energy | Wind triangle solver, crab angle swath compensation, aerodynamic drag model, energy-optimized TSP routing (wind-aware asymmetric costs), endurance estimation |
| v0.6 | Glass Cockpit MVP | Tauri desktop app, block overview display, line navigation guidance (cross-track/altitude/heading error), vector graphics rendering, pilot/operator line selection sync via WebSocket |
| v0.7 | Web Planning UI | React web interface, boundary drawing/import, manual flight line drawing and editing tools, sensor library with saved profiles, plan generation via API, export management, team collaboration |
| v0.8 | Trigger Controller | Embedded firmware for real-time camera/LiDAR triggering, UART NMEA parsing, sub-GGA dead reckoning, GPIO pulse output, serial protocol for daemon → controller waypoint upload, hardware platform selection and testing with Eagle Mapping |
| v0.9 | 3DEP & US Domestic | 1-meter 3DEP DEM ingestion, NAVD88/NAD83 → WGS84 datum pipeline via GEOID18, US-domestic survey market support |
| v1.0 | Production Release | Integration testing across full workflow, Eagle Mapping production validation, stability hardening, documentation, community release |

# **10\. Research Foundation**

IronTrack’s design is backed by an extensive body of research documents that serve as the authoritative reference for each subsystem. These documents are maintained alongside the codebase and are referenced by the AI coding agents during implementation.

| Doc | Title | Covers |
| :---- | :---- | :---- |
| 00 | Project Charter | Scope boundaries, milestone definitions, licensing |
| 01 | Legal Framework | GPLv3 mechanics, copyright headers, CLA strategy |
| 02 | Geodesy Foundations | WGS84 ellipsoid, Karney algorithm, Karney-Krüger UTM, SoA layout, SIMD, Kahan summation |
| 03 | Photogrammetry Spec | GSD, FOV, swath width, dynamic AGL, collinearity, crab angle, sensor stabilization physics |
| 04 | GeoPackage Architecture | Binary header layout, flags bitfield, envelope codes, R-tree triggers, WAL pragmas |
| 05 | Vertical Datum Engineering | EGM96, EGM2008, NAVD88, GEOID18, CGVD2013, barometric ISA, terrain-following control theory |
| 06 | Rust Architecture | FFI boundaries, async traps, SIMD auto-vectorization, rayon vs tokio, zero-copy mmap |
| 07 | Corridor Mapping | Polyline offsets, self-intersection clipping, Dubins paths, bank angle, clothoid transitions |
| 08 | Copernicus DEM | TanDEM-X interferometry, X-band penetration, global coverage, licensing, ingestion architecture |
| 09 | DSM vs DTM | Canopy penetration, AGL safety, FABDEM analysis, pitch/roll dynamics on terrain transitions |
| 10 | COG Ingestion | Cloud-Optimized GeoTIFF architecture, HTTP range requests, tiled raster processing |
| 11 | Autopilot Formats | MAVLink, QGroundControl .plan, DJI WPML/KMZ, KML, ArduPilot mission items |
| 12 | EGM2008 Grid | Spherical harmonic coefficients, grid interpolation, undulation lookup architecture |
| 13 | LiDAR Flight Planning | PRR, scan rate, point density, across/along-track spacing, canopy penetration probability |
| 14 | GeoTIFF/COG Processing | TIFF structure, compression, tiling, overview levels, coordinate metadata |
| 15 | 3DEP Access | USGS API, NAVD88 vertical datum, 1-meter resolution, US-domestic coverage |
| 16 | Aerial Wind Data | Wind triangle, atmospheric models, wind shear, forecast ingestion |
| 17 | UAV Survey Analysis | Multirotor kinematics, acceleration dynamics, power modeling, endurance estimation |
| 18 | Oblique Photogrammetry | Multi-camera systems, trapezoidal footprints, Maltese cross BBA, facade capture |
| 19 | WGS84 Epoch Management | Plate tectonics, ITRF realization epochs, coordinate drift, PPK implications |
| 21 | Dynamic Footprint Geometry | 6DOF ray casting, polygon intersection overlap, IMU error propagation, dynamic overlap buffering algorithm |

# **11\. Multi-Aircraft Coordination (Long-Term)**

The initial releases target single-aircraft operations. Multi-aircraft coordination is on the long-term roadmap and will require deconflicted flight plans (ensuring aircraft maintain safe separation), synchronized line assignment (preventing two aircraft from flying the same line), and shared telemetry (each aircraft’s daemon broadcasting to a central coordinator). The GeoPackage schema and daemon API are designed with this future in mind: line assignment state is tracked per-line rather than globally, and the WebSocket telemetry protocol includes aircraft identification fields.

## **11.1 Legacy Avionics Integration (Post-v1.0)**

Eagle Mapping’s current acquisition pipeline includes several third-party software systems: Applanix GSM (GNSS/INS management), POSView (real-time trajectory monitoring), and Riegl RiAcquire (LiDAR data recording and control). Full IronTrack integration with these systems — sharing flight plans, streaming telemetry, or synchronizing triggers — would require cooperation from the respective vendors and is not guaranteed.

**Transition strategy:** The Snapshot-compatible MDB export (v0.2) enables IronTrack to replace Trackair for planning while the existing Snapshot cockpit system and its integrations with GSM, POSView, and RiAcquire continue to handle acquisition. This allows field testing of IronTrack’s planning engine without disrupting the proven execution pipeline. Direct integration with these avionics systems is deferred to post-v1.0 and contingent on stakeholder alignment and vendor cooperation.

**Scope boundary:** IronTrack will not reverse-engineer proprietary protocols. Integration with third-party avionics software will only proceed through documented APIs or with explicit vendor partnership. If vendor cooperation is not forthcoming, IronTrack’s own trigger controller and glass cockpit provide a fully independent execution path that does not depend on any third-party software.

# **12\. Trigger Controller Architecture**

The trigger controller is the most safety-critical and timing-sensitive component in IronTrack. It is a dedicated real-time embedded system that fires camera and LiDAR pulses at precise spatial intervals during active flight. It is architecturally separate from the daemon and operates below the OS network stack.

## **12.1 Why Not TCP?**

At typical survey speeds (50–120 m/s for fixed-wing, 5–15 m/s for multirotor), a camera trigger that needs sub-meter spatial accuracy must fire within milliseconds of the correct position. TCP — even on localhost — introduces non-deterministic latency from kernel scheduling, Nagle’s algorithm, socket buffer flushing, and OS context switching. A trigger routed through the daemon’s TCP stack could jitter by 10–50 ms, translating to 1–6 meters of positional error at fixed-wing speeds. This is unacceptable for photogrammetric overlap and PPK event marking. The trigger controller must read NMEA directly from the GPS receiver’s UART and fire GPIO voltage pulses in the same deterministic loop.

## **12.2 Hybrid Trigger Model**

**The daemon pre-computes trigger waypoints** for each flight line using the full geodesic math engine (Karney distance, terrain-following AGL, crab angle compensation). These waypoints are uploaded to the trigger controller before the line begins. **The controller fires on proximity** — it reads each GGA sentence, computes distance to the next trigger waypoint, and drops a voltage pulse on the GPIO pin when the threshold is crossed. The controller does not need to understand photogrammetry, datums, or flight planning. It only needs to answer one question: am I close enough to the next point to fire?

## **12.3 Sub-GGA Dead Reckoning**

Standard NMEA GPS receivers output GGA sentences at 10–20 Hz. At 10 Hz and a fixed-wing speed of 80 m/s, there is an 8-meter gap between successive position fixes. For camera work this may be acceptable, but for high-PRR LiDAR triggering or high-speed fixed-wing operations, the controller must interpolate between GPS fixes.

The dead-reckoning algorithm uses the ground speed and true heading from the most recent GGA/RMC sentence pair to linearly extrapolate position at the controller’s internal loop rate (typically 100–1000 Hz depending on hardware). When the next GGA arrives, the dead-reckoned position is snapped to the true fix and the residual error is used to refine the extrapolation model. This is a lightweight first-order prediction — not a full Kalman filter — but it provides the sub-meter inter-fix accuracy needed for precise trigger timing.

## **12.4 Hardware Candidates**

The specific hardware platform for the trigger controller is not yet determined and requires hands-on testing with Eagle Mapping’s aircraft and sensor configurations. Candidate platforms include bare-metal microcontrollers (STM32, ESP32) for sub-microsecond GPIO determinism, the RP2040 (Raspberry Pi Pico) which offers programmable I/O state machines for cycle-accurate timing, or a Raspberry Pi with a PREEMPT\_RT real-time kernel for more capable processing at the cost of some timing determinism.

The firmware may be written in embedded Rust (no\_std) to share geodesic distance functions with the core engine, or in C if the hardware ecosystem demands it. This is a distinct codebase from the daemon, with its own build toolchain, and will be resolved during v0.8 development.

## **12.5 Communication Protocol**

The daemon communicates with the trigger controller via a simple serial protocol (UART or USB-serial), not TCP. Before a line begins, the daemon sends a compact binary message containing the ordered list of trigger waypoint coordinates (lat, lon as scaled integers) and the proximity threshold in meters. The controller acknowledges receipt. During the line, the controller sends brief status messages back to the daemon (trigger fired at position X, Y) so the glass cockpit can display trigger events in real-time. If the serial link is lost mid-line, the controller continues firing autonomously using its pre-loaded waypoint list.

# **13\. Community Infrastructure**

**Repository:** GitHub with CI via GitHub Actions. Memory-safe builds verified on every push. Cargo clippy clean, cargo fmt enforced.

**Documentation:** Comprehensive README with build instructions, CONTRIBUTING.md, issue templates for bugs and features, and the full research document library in docs/.

**Workflow Agnosticism:** The operational workflows specific to Eagle Mapping are documented separately from the generalizable aerial survey workflows. This prevents the core engine from becoming bespoke internal software and ensures it remains valuable to the broader community.

**Governance Trajectory:** As the project matures past v1.0, the goal is to transition from a solo founder model to a meritocratic technical oversight committee, encouraging commercial survey outfits to dedicate engineering time upstream to maintain long-term support for industry-standard capabilities.

# **14\. Design Language**

IronTrack’s visual identity draws from industrial and steam-engine aesthetics: dark UI incorporating blacks and earth tones, with warm amber and saddlebrown accent colors. The glass cockpit display uses high-contrast vector graphics on a dark background for readability in bright cockpit environments. The web planning interface uses the same palette with a lighter background for extended office use. Typography is clean and sans-serif (Arial/Inter) for maximum legibility at small sizes on high-DPI displays.