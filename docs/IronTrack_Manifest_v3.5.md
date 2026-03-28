**IRONTRACK**  
Project Charter & System Manifest

**Version 3.5**

*Open-Source Flight Management System for Aerial Survey*

GPLv3 Licensed • Rust Core Engine • GeoPackage Native

March 2026

*Informed by 52 deep-research documents • 455+ architectural decisions • Zero hallucinated capabilities*

# **Table of Contents**

**1\. Executive Summary ..................................................................................... 3**

**2\. Licensing and IP Governance .................................................................... 3**

2.1 Dependency License Audit ................................................................... 3

2.2 Embedded Firmware Boundary ............................................................ 4

2.3 Proprietary SDK Isolation ...................................................................... 4

2.4 Geospatial Data Attribution .................................................................. 4

2.5 Contributor Strategy ............................................................................. 4

**3\. Architecture and Geodetic Foundation ..................................................... 5**

3.1 Four-Layer Architecture ........................................................................ 5

3.2 The Async Boundary Rule .................................................................... 5

3.3 The Real-Time Boundary ...................................................................... 5

3.4 Geodetic Computation Standards ......................................................... 5

3.5 Geoid Models and Interpolation ............................................................ 6

3.6 Canonical Pivot Datum: WGS84 Ellipsoidal ......................................... 6

3.7 Navigational vs Analytical Boundary .................................................... 6

3.8 Preprocessing Mandate ........................................................................ 6

**4\. The GeoPackage: Universal Data Container ............................................. 7**

4.1  Contents

4.2  Extension Registration

4.3  Tiled Gridded Coverage Extension

4.4  Offline Basemap: Embedded Tiles

4.5  Concurrency: WAL Mode

4.6  Performance and Maintenance

**5\. Boundary Import and CRS Handling ......................................................... 8**

5.1  Client-Side Parsing Strategy

5.2  Format-Specific Handling

5.3  CSV with WKT Geometry

5.4  v1.0 CRS Limitation

**6\. Sensor Library and Trigger Interfaces ...................................................... 9**

6.1  Database Schema: irontrack\_sensors

6.2  Saved Profiles: irontrack\_mission\_profiles

6.3  Camera Specifications

6.4  LiDAR Specifications

6.5  Trigger Interface Catalog

6.6  LiDAR Continuous Emission vs Camera Spatial Triggering

6.7  Event Feedback and PPK Timing

**7\. Safety Architecture ..................................................................................... 11**

7.1  DO-178C DAL Classification

7.2  Type System Safety

7.3  Fail-Safe Patterns

7.4  Advisory-Only Airspace Philosophy

**8\. Flight Path Optimization and Routing ...................................................... 12**

8.1  ATSP Formulation

8.2  Boustrophedon Short-Circuit

8.3  Recommended Pipeline: NN \+ Or-Opt

8.4  Multi-Block Hierarchical Routing

8.5  Metaheuristics

**9\. Terrain-Following Trajectory Generation .................................................. 13**

9.1  Elastic Band \+ B-Spline Hybrid

9.2  Dubins Paths

9.3  Clothoid (Euler Spiral) Transitions

9.4  3D Extension: Dubins Airplane

9.5  Wind-Corrected Turn Radius

**10\. Aerodynamic and Energy Modeling ........................................................ 14**

10.1  Multirotor Power Model

10.2  Fixed-Wing Drag Polar

10.3  Battery Electrochemistry

10.4  Per-Segment Energy Costing

**11\. Post-Flight QC and Automated Re-Fly Planning ..................................... 15**

11.1  Cross-Track Error (XTE)

11.2  Trigger Event Analysis

11.3  6DOF Image Footprint Reconstruction

11.4  Coverage Heat Map

11.5  Automated Re-Fly Generation

**12\. ASPRS 2024 Standards Compliance ....................................................... 16**

**13\. Computational Geometry Primitives ........................................................ 16**

**14\. Performance Architecture ........................................................................ 16**

**15\. Field Hardware and Avionics Integration ............................................... 17**

15.1  Deployment Platforms

15.2  Aircraft Power Conditioning

15.3  Grounding and EMI

15.4  Connectors

15.5  Airframe Integration

**16\. Testing and Validation Strategy ............................................................... 18**

16.1  Core Engine Unit Testing

16.2  Integration Testing

16.3  End-to-End Pipeline

16.4  Embedded Firmware Testing

16.5  Mock Telemetry

16.6  CI Pipeline

16.7  Eagle Mapping Field Acceptance

**17\. Release Roadmap ..................................................................................... 19**

**18\. Research Foundation ................................................................................ 20**

**19\. Locked Architectural Decisions ............................................................... 23**

**20\. Long-Term Roadmap (Post-v1.0) ............................................................. 25**

**21\. Community Infrastructure ........................................................................ 25**

# **1\. Executive Summary**

IronTrack is an open-source Flight Management System (FMS) for aerial survey operations, built entirely in Rust. It replaces the fragmented legacy toolchain of proprietary flight planners, manual DXF conversions, and disconnected cockpit software with a single, unified system that flows from the planning desk to the aircraft cockpit and back.

The system is designed around a single universal data container — the GeoPackage — which carries the flight plan, terrain data, sensor configuration, safety metadata, and legal attribution through every stage of the mission lifecycle. An operator plans in the office, uploads the GeoPackage to any device running the daemon, and both the pilot and sensor operator see the same plan. No format conversions. No thumb drives with MDB files. No re-entry.

IronTrack operates across four distinct architectural layers: a headless CLI engine for batch planning and CI/CD integration, a persistent Axum-based network daemon that serves REST and WebSocket APIs from any laptop or field device, a dedicated real-time trigger controller that sits directly on the NMEA serial bus to fire camera and LiDAR pulses with sub-meter spatial precision, and two frontend layers — a Tauri desktop application for field operations and a React web interface for office collaboration.

**Target Adopter:** Eagle Mapping. The software must integrate into their existing aerial survey workflows as the primary proof-of-concept environment. Securing production adoption at Eagle Mapping is the gating requirement before wider community release.

**Research Foundation:** This manifest is informed by 52 deep-research documents (Docs 00–52) covering geodesy, photogrammetry, aviation safety, Rust systems programming, avionics hardware integration, computational geometry, and open-source licensing. Every architectural decision traces to a specific research document. No capability is claimed without a researched basis.

# **2\. Licensing and IP Governance**

**License:** GNU General Public License v3 (GPLv3). All modifications, distributions, and derivative works must be distributed under the same open terms, preventing proprietary capture or vendor lock-in. Every Rust source file carries an SPDX-License-Identifier header. \[Docs 00, 51\]

**Intellectual Property:** IronTrack is an independent, founder-led FOSS initiative. All IP, copyright, and initial codebase authorship are retained by the founder. There is no corporate sponsorship, ownership, or exclusive rights granted to any employer or commercial entity.

## **2.1 Dependency License Audit \[Doc 51\]**

The Rust Cargo.toml dependency tree has been exhaustively audited for GPLv3 compatibility. The standard dual-licensed Rust ecosystem (MIT OR Apache-2.0) poses zero legal risk. Apache-2.0 was incompatible with GPLv2 but is explicitly resolved in GPLv3 Section 10\. BSD-3-Clause (MapLibre GL JS) is fully permissive. The serialport crate (MPL-2.0) is compatible via MPL-2.0 Section 3.3, which explicitly names GPLv3 as a “Secondary License.” LGPL crates are compatible because GPLv3 supersedes the LGPL relinking requirement through mandatory Complete Corresponding Source distribution.

SSPL (MongoDB) and BSL (HashiCorp) are explicitly excluded — both introduce “further restrictions” that violate GPLv3 Section 7\. CI enforcement via cargo-deny prevents accidental introduction of incompatible licenses.

## **2.2 Embedded Firmware Boundary \[Doc 51\]**

The trigger controller firmware is a physically separate binary executing on a different CPU architecture (Cortex-M vs x86/ARM64), with zero knowledge of the daemon’s Rust data structures, communicating solely via a generic COBS-framed binary serial protocol. Under the FSF’s “arm’s length” communication test, the firmware is legally a separate work — not infected by GPLv3. It may use MIT/Apache-2.0 for embedded ecosystem compatibility.

## **2.3 Proprietary SDK Isolation \[Doc 51\]**

Phase One Capture One SDK and DJI PSDK have restrictive EULAs prohibiting GPL redistribution. IronTrack uses the IPC Bridge Pattern: a separate plugin daemon binary links against the proprietary SDK (licensed under the SDK’s own terms), while the GPLv3 core communicates with it over standard IPC channels (TCP/gRPC/UNIX domain sockets). No copyleft propagation occurs across the IPC boundary.

## **2.4 Geospatial Data Attribution \[Doc 51\]**

Copernicus DEM: mandatory attribution (“Contains modified Copernicus Sentinel data \[year\]”) embedded in every GeoPackage and GeoJSON export. USGS 3DEP: public domain (U.S. federal government). EGM2008/GEOID18: public domain (NGA/NGS). Voluntary source citations included in irontrack\_metadata.

## **2.5 Contributor Strategy \[Doc 51\]**

CLA (Contributor License Agreement) for the initial phase — preserves relicensing flexibility for future commercial services while the contributor base is small. Option to transition to DCO (Developer Certificate of Origin) as the community matures to reduce contribution friction.

# **3\. Architecture and Geodetic Foundation**

## **3.1 Four-Layer Architecture \[Doc 28\]**

Core Engine (Rust/rayon, synchronous): Pure math — geodesy, photogrammetry, DEM querying, flight line generation. All coordinate arrays use f64 with Kahan summation. Structure of Arrays (SoA) memory layout for SIMD auto-vectorization (AVX2/NEON). \[Docs 02, 06, 52\]

Network Daemon (Axum/Tokio, asynchronous): Persistent daemon serving REST (/api/v1/ CRUD) and WebSocket APIs. Ingests NMEA GPS telemetry, manages mission state via Arc\<RwLock\<FmsState\>\>, broadcasts 10 Hz telemetry via hybrid broadcast+watch channels. Runs in a dedicated OS thread to prevent UI rendering from starving telemetry. \[Doc 28\]

Trigger Controller (STM32/RP2040, bare-metal no\_std): Physically separate MCU on the aircraft serial bus. Parses raw NMEA bytes directly from GNSS UART, dead-reckons at 1000 Hz, fires opto-isolated GPIO at closest-approach derivative zero-crossing. Sub-100 ns GPIO jitter. \[Docs 22, 29\]

Presentation Layers: Tauri 2.0 desktop “Glass Cockpit” (ARINC 661 colors, Track Vector CDI, Canvas 2D, B612 font, 20 mm touch targets) \+ React web UI (MapLibre GL JS at 60 FPS, TanStack Table sensor library). \[Docs 25, 26\]

## **3.2 The Async Boundary Rule \[Doc 06\]**

All geodetic calculations, CRS transforms, and photogrammetric math run synchronously on OS threads via rayon. The async Tokio runtime is strictly reserved for network I/O. Heavy math called from Axum endpoints must be wrapped in tokio::task::spawn\_blocking. Violating this rule starves NMEA telemetry ingestion and severs WebSocket connections.

## **3.3 The Real-Time Boundary \[Docs 22, 29\]**

The daemon NEVER directly actuates cameras or LiDAR. OS-level jitter (10–50 ms from kernel scheduling, buffer flushing, Nagle’s algorithm) translates to 1–4 m spatial error at 80 m/s. Only the bare-metal trigger controller fires hardware GPIO with sub-100 ns jitter. The daemon pre-computes UTM trigger waypoints and uploads them via COBS-framed binary serial with CRC-32 checksums and RTS/CTS flow control before the line is flown.

## **3.4 Geodetic Computation Standards \[Docs 01, 02, 30, 31, 32\]**

Karney’s algorithm for inverse geodesic calculations (geographiclib-rs). Haversine rejected (0.3% spherical error). Vincenty rejected (antipodal convergence failure). Karney-Krüger 6th-order series for UTM projection (sub-mm precision within the zone). 14-parameter kinematic Helmert transformation for NAD83→WGS84 (accounts for 2.2 m geocentric offset \+ 2.5 cm/year tectonic drift). All computations strictly f64 — f32 prohibited. Kahan summation mandatory for all cumulative spatial aggregations.

## **3.5 Geoid Models and Interpolation \[Docs 05, 12, 32\]**

EGM2008: bicubic interpolation on a 4×4 stencil (3.1 cm max error, C¹ continuity — prevents autopilot pitch chatter at grid cell boundaries). Bilinear interpolation disqualified (C⁰ only, creates discontinuous pitch commands). GEOID18: biquadratic interpolation per NGS INTG standard on Big-Endian binary grid (34.3 MB). Little-Endian GEOID18 files are corrupted from a 2019 NGS release — only Big-Endian is valid. EGM96: vendored egm96 crate for DJI exports only (DJI FMS expects EGM96 orthometric heights).

## **3.6 Canonical Pivot Datum: WGS84 Ellipsoidal \[Docs 30, 43\]**

All terrain data transforms to WGS84 Ellipsoidal on ingestion. Copernicus tiles: add EGM2008 undulation. 3DEP tiles: add GEOID18 undulation, then apply 14-parameter Helmert. The entire internal R-tree, flight waypoints, and GeoPackage microtiles exist in this unified geometric space. Exports traverse the conversion graph outward: EGM96 for DJI, NAVD88 for legacy CAD, WGS84 Ellipsoidal for ArduPilot. Rust’s type system enforces datum tagging at compile time via PhantomData zero-sized types — untagged altitudes cause compilation failures. \[Doc 37\]

## **3.7 Navigational vs Analytical Boundary \[Docs 30, 31, 41\]**

\~40 cm tectonic drift between epoch 2010.0 and 2026.2 is imperceptible for real-time navigation and irrelevant to swath overlap. IronTrack safely assumes a unified navigational epoch during flight. For post-processing ASPRS compliance (1–3 cm RMSE), the drift must be resolved externally. IronTrack records horizontal\_datum, vertical\_datum, realization (e.g., NAD83\_2011), reference\_epoch, and observation\_epoch in irontrack\_metadata. Non-rigid epoch propagation is delegated to HTDP/NCAT or Trimble Business Center.

## **3.8 Preprocessing Mandate \[Doc 30\]**

Datum transformations NEVER occur on-the-fly during flight. All terrain data is preprocessed and cached in the GeoPackage before the aircraft takes flight. The GEOID18 biquadratic \+ Helmert pipeline executes per-pixel on rayon threads during the office planning phase, not in the real-time control loop.

# **4\. The GeoPackage: Universal Data Container**

The GeoPackage is the single artifact that flows through the entire IronTrack mission lifecycle. It replaces the MDB files, DXF conversions, and proprietary flight plan formats that currently fragment the Eagle Mapping workflow. \[Docs 04, 42\]

## **4.1 Contents**

Flight lines with full SoA coordinate data (lat, lon, elevation per waypoint). Altitude datum tag per flight line. Sensor configuration profiles in irontrack\_sensors table. Saved mission profiles in irontrack\_mission\_profiles table. Cached DEM microtiles (OGC 32-bit float TIFF via Tiled Gridded Coverage Extension). Embedded raster basemap tiles (GeoPackage Tile Matrix Set). R-tree spatial index. Schema version, engine version, creation timestamp in irontrack\_metadata. Copernicus attribution. DSM penetration safety warning. Trigger event log. Geodetic epoch metadata. \[Docs 04, 42, 45\]

## **4.2 Extension Registration \[Doc 42\]**

irontrack\_metadata registered in gpkg\_contents with data\_type=‘attributes’ and in gpkg\_extensions with scope=‘read-write’. This two-step registration ensures graceful ignorability by QGIS/ArcGIS — external tools see a valid non-spatial table and can safely read/modify standard vector features without dropping or corrupting the proprietary metadata. Official OGC community extension registration is impractical for the v1.0 timeline.

## **4.3 Tiled Gridded Coverage Extension \[Doc 42\]**

DEM data embedded via the OGC Tiled Gridded Coverage Extension. 32-bit float TIFF mandatory (TAG 339=3, TAG 258=32, single-band, LZW compression only). 16-bit PNG quantization rejected — introduces rounding errors unacceptable for ASPRS-compliant terrain-following. Hybrid ingestion pipeline: OGC-compliant TIFF on disk for interoperability, but at daemon startup the engine decompresses LZW, casts 32-bit→f64, and builds an ephemeral contiguous SoA grid in RAM for zero-latency runtime queries.

## **4.4 Offline Basemap: Embedded Tiles \[Doc 42\]**

MBTiles rejected: EPSG:3857 rigidity, single-tileset limitation, sidecar human-error risk (operator forgets the .mbtiles file → pilot flies over a blank canvas). GeoPackage Tile Matrix Set embeds basemap imagery directly alongside vector features and DEM grids in the same file. Supports any CRS, mixed JPEG/PNG per-tile. The irontrack:// custom URI protocol serves tiles from the Tauri backend to MapLibre GL JS with zero network latency.

## **4.5 Concurrency: WAL Mode \[Doc 42\]**

PRAGMA journal\_mode=WAL on daemon startup. Provides snapshot isolation: readers operate against an immutable snapshot, completely unblocked by concurrent writes. Single-writer constraint enforced via Arc\<RwLock\<FmsState\>\>. SQLITE\_BUSY is NOT a transient retry condition — it is a fatal architectural violation that must trigger immediate fail-safe shutdown (the WAL-reset bug can cause permanent, unrecoverable database corruption). WAL fails on NFS/SMB — GeoPackage must be local. \[Doc 42\]

## **4.6 Performance and Maintenance \[Doc 42\]**

R-tree spatial index handles 10–50 GB GeoPackages with negligible degradation. VACUUM prohibited during active flight deployment — exclusive lock freezes the pilot’s cockpit for minutes, and storage doubling can crash the SSD. PRAGMA optimize (lightweight, non-blocking statistics update) is the only acceptable maintenance strategy. Triggered on graceful connection close or every \~6 hours of idle time. PRAGMA auto\_vacuum \= NONE.

# **5\. Boundary Import and CRS Handling \[Doc 43\]**

## **5.1 Client-Side Parsing Strategy**

The React web UI serves as the primary ingestion gateway. GeoJSON (RFC 7946, WGS84 mandated), KML (@tmcw/togeojson), and KMZ (jszip client-side decompression) are parsed entirely in the browser. Shapefiles parsed via shapefile.js. The Rust daemon never touches raw binary files from external clients — mitigates attack vectors and preserves backend compute for trajectory generation.

## **5.2 Format-Specific Handling**

Shapefile: pure Rust crates (shapefile-rs, oxigdal-shapefile) for backend validation. CRS from .prj file via proj-wkt/proj4wkt crates. File Geodatabase (.gdb): native ingestion REJECTED despite MIT/GPLv3 legal compatibility — GDAL FFI bloats the binary and introduces segfault risk. Clients must export to GeoJSON or KML. DXF: transitional support only via dxf-tools-rs crate. LwPolyline extraction with bulge-to-arc discretization and closure validation. Mandatory non-dismissable EPSG modal for missing SRS.

## **5.3 CSV with WKT Geometry \[Doc 43\]**

WKT geometry in a dedicated CSV column (e.g., POLYGON((...))) eliminates all column-heuristic ambiguity and natively supports holes and multi-polygons. Parsed via geoarrow-csv (Apache Arrow columnar layouts) or the wkt crate (TryFromWkt trait to strongly-typed geo-types structs). Recommended over discrete lat/lon columns.

## **5.4 v1.0 CRS Limitation \[Doc 43\]**

IronTrack will NOT automatically reproject arbitrary datums in v1.0. WGS84 \+ UTM only. If a Shapefile .prj or DXF specifies State Plane or a custom datum, the React UI halts import and requires the operator to reproject externally. This is a geodetic safety mechanism — prevents untested grid shifts from silently corrupting life-critical flight lines. Must be documented in operator manuals and displayed in UI warnings.

# **6\. Sensor Library and Trigger Interfaces \[Docs 40, 41, 45\]**

## **6.1 Database Schema: irontrack\_sensors \[Doc 45\]**

Sensor library stored INSIDE the GeoPackage (not a separate file) to prevent split-brain. Normalized relational columns for math-engine parameters: name, manufacturer, type (CAMERA/LIDAR), sensor\_width\_mm, sensor\_height\_mm, pixel\_pitch\_um, resolution\_x, resolution\_y, focal\_lengths\_mm (serialized JSON array), prr\_hz, scan\_rate\_hz, fov\_deg, weight\_kg, trigger\_interface, event\_output, notes (JSON for volatile overrides). Rolling shutter flag drives the non-dismissable warning when mission\_type \= survey\_grade\_ppk.

## **6.2 Saved Profiles: irontrack\_mission\_profiles \[Doc 45\]**

Dedicated relational table (NOT a JSON blob in irontrack\_metadata). Core parameters (sensor\_id FK, mission\_type, overlap\_fwd, overlap\_side) as rigid relational columns. Custom overrides as a JSON column within this dedicated table. Hybrid architecture: strict type safety for the math engine \+ arbitrary platform-specific tuning without schema migrations.

## **6.3 Camera Specifications \[Docs 40, 45\]**

10 photogrammetric cameras catalogued with full specs: Phase One iXM-100 (100 MP, 3.76 µm, 3 fps, LEMO GPIO, MEP), Phase One iXM-50 (50 MP, 5.3 µm, 2 fps), Hasselblad A6D-100c (100 MP, 4.6 µm, 1 fps, 8-camera sync at 20 µs), Sony ILX-LR1 (61 MP, 243 g, Molex GPIO), Sony α7R IV (61 MP, 10 fps, rolling shutter warning), DJI Zenmuse P1 (45 MP, global shutter, PSDK), DJI L2 RGB (20 MP), Vexcel Eagle M3 (450 MP composite, TDI FMC), Vexcel Osprey 4.1 (1.2 gigapixels/cycle, 5-head oblique), Leica DMC-4X (54,400×8,320 composite, 71° cross-track FOV).

## **6.4 LiDAR Specifications \[Docs 41, 45\]**

9 LiDAR sensors catalogued: Riegl VUX-240 (1.8/2.4 MHz), Riegl VQ-1560 II (4 MHz dual-channel cross-fire), Riegl miniVUX-3UAV (1.55 kg, 360° FOV), Leica TerrainMapper-3 (2 MHz, programmable scan patterns, gateless 35 MPiA), Leica CityMapper-2 (hybrid LiDAR \+ 6 cameras), Optech Galaxy Prime (SwathTRAK dynamic FOV, PulseTRAK), DJI Zenmuse L2 (240 kHz, PSDK), Livox Mid-360 (non-repetitive rosette — requires dwell-time planning), Yellowscan Voyager (2.4 MHz UAV-class).

## **6.5 Trigger Interface Catalog \[Doc 40\]**

Phase One LEMO: Pin 4 active-low, V\_IL \< 0.8V, V\_IH \> 2.4V, rise/fall \< 1 µs, hold 10–15 ms. Sony Molex Micro-Fit: Pin 5 TRIGGER active-low, Pin 6 EXPOSURE open-drain (requires external pull-up resistor). Hot shoe: ISO 518 center pin, \~5V, \< 10 mA, optocoupler/MOSFET mandatory. MAVLink MSG 206: acceptable ≤15 m/s UAV speed, rejected for manned 80 m/s. DJI PSDK: 460,800 baud, proprietary C-struct command encapsulation. USB SDK: ground configuration only — polling latency unacceptable for mid-flight triggering.

## **6.6 LiDAR Continuous Emission vs Camera Spatial Triggering \[Doc 41\]**

LiDAR does NOT require external trigger pulses. The laser fires continuously at a steady PRR from an internal oscillator. “Triggering” means start/stop data recording via TCP commands at flight line boundaries. Cameras require discrete spatial GPIO pulses. The dual-trigger hybrid architecture handles both simultaneously: the network daemon sends TCP start/stop to the LiDAR, while the embedded trigger controller autonomously fires camera GPIO from pre-loaded waypoints.

## **6.7 Event Feedback and PPK Timing \[Doc 40\]**

Closed-loop (gold standard): camera generates Mid-Exposure Pulse at actual shutter opening → routed to GNSS event pin. Open-loop (fallback): trigger controller splits pulse to camera and GNSS simultaneously. Phase One MEP: Pin 7, T2 delay 20–30 ms. Sony EXPOSURE: Pin 6 open-drain. EXIF timestamps disqualified (integer-second resolution \= 80 m spatial ambiguity at fixed-wing speeds). PPS-based microsecond timestamping: PPS rising edge latches MCU timer, MEP arrival computes delta → absolute UTC with µs precision.

# **7\. Safety Architecture \[Doc 37\]**

## **7.1 DO-178C DAL Classification \[Doc 37\]**

IronTrack core engine: DAL E (no safety effect on aircraft flight). Trigger controller: DAL D (26 verification objectives). IronTrack never commands or blocks the aircraft — advisory only. PIC retains ultimate authority.

## **7.2 Type System Safety \[Doc 37\]**

PhantomData zero-sized types enforce datum tagging at compile time. Untagged altitudes cause compilation failures. cksumvfs 8-byte page checksums for GeoPackage integrity. Atomic writes (temp→fsync→rename). Warning fatigue mitigation: non-dismissable for datum mismatch and rolling shutter in survey-grade PPK; dismissable for DSM penetration and advisory airspace alerts.

## **7.3 Fail-Safe Patterns \[Doc 37\]**

Hardware watchdog on trigger controller MCU. Heartbeat monitoring between daemon and controller. Graceful degradation: RTK Float triggers automatic spatial firing radius widening \+ permanent event flagging \+ amber CDI warning. Bulkhead isolation: embedded firmware crash has zero effect on aircraft flight control. 20% battery reserve is a blocking enforcement, not just a warning — the LiPo discharge cliff in the final 15–20% SoC combined with current spikes virtually guarantees LVC breach. \[Docs 37, 38\]

## **7.4 Advisory-Only Airspace Philosophy \[Doc 35\]**

IronTrack never enforces geofences by refusing to generate waypoints, deleting planned lines, or stopping triggers. The pilot may need to enter controlled airspace in an emergency, and legitimate survey operations frequently fly inside Class B/C/D under pre-arranged ATC clearances. Mathematical intersection logic enhances awareness — it never restricts pilot agency.

# **8\. Flight Path Optimization and Routing \[Doc 39\]**

## **8.1 ATSP Formulation \[Doc 39\]**

Flight line routing is strictly an Asymmetric Traveling Salesman Problem (ATSP), not symmetric TSP. Two sources of asymmetry: bidirectionality of flight lines (each line can be flown forward or reverse) and wind (headwind vs tailwind changes traversal cost). Averaging directional costs produces sub-optimal or physically infeasible trajectories. Node-doubling transforms each physical line into two mutually exclusive directed nodes.

## **8.2 Boustrophedon Short-Circuit \[Doc 39\]**

For parallel grids (the majority of aerial survey blocks): azimuth variance detection classifies geometry in O(n), deterministic sort in O(n log n) produces the proven global optimum without invoking any TSP solver. The TSP solver only fires for complex boundaries, corridors, or multi-polygon blocks.

## **8.3 Recommended Pipeline: NN \+ Or-Opt \[Doc 39\]**

Nearest-neighbor construction (O(n²)) followed by Or-opt refinement (O(n²)). Or-opt extracts chains of k=1,2,3 lines and reinserts without reversal — zero wind cost recalculation, O(1) per move. Rescues “forgotten node” failures from NN. Sub-millisecond execution for 100-line missions, results within 1–3% of global optimum. 2-opt rejected for ATSP: reversal changes every wind cost, escalating evaluation from O(1) to O(n) per move.

## **8.4 Multi-Block Hierarchical Routing \[Doc 39\]**

Two-level decomposition: outer GTSP for block sequence (each polygon as a single node), inner ATSP per block. Dynamic terminal optimization at the boundary — the inner ATSP can rotate its line sequence to best align with the incoming ferry vector. Prevents catastrophic block-switching where a flat solver intermixes lines from distant blocks.

## **8.5 Metaheuristics \[Doc 39\]**

SA and GA are unnecessary computational bloat for standard 10–200 line surveys. Become necessary only for variable-speed constraints (VS-ATSP), strict time windows (solar angle, tidal level), or extremely irregular boundaries with deep concavities.

# **9\. Terrain-Following Trajectory Generation \[Docs 34, 50\]**

## **9.1 Elastic Band \+ B-Spline Hybrid \[Doc 34\]**

Two-stage pipeline: (1) Elastic Band energy minimization models the path to limit extreme jerk and pitch while maintaining terrain clearance. (2) Adaptive-knot Cubic B-spline fitting achieves C² mathematical continuity — both slope and curvature are continuous, eliminating sudden jerk that would induce autopilot pitch chatter or gimbal instability. The convex hull property of B-splines provides an absolute mathematical guarantee against terrain collision: if the control points clear the DSM, the resulting curve cannot intersect the ground.

## **9.2 Dubins Paths \[Doc 50\]**

Six canonical path types {LSL, RSR, LSR, RSL, LRL, RLR}. CSC family (curve-straight-curve) via outer/inner tangent geometry. CCC family (curve-curve-curve) via Law of Cosines for the intermediate circle. CCC only optimal for d ≤ 4r (close-proximity maneuvers). For long-distance survey line connections (d \>\> 4r), optimal path is guaranteed CSC.

## **9.3 Clothoid (Euler Spiral) Transitions \[Doc 50\]**

Raw Dubins paths have instantaneous curvature jumps at tangent junctions (C¹ only). Clothoid insertion ramps curvature linearly with arc length, upgrading to C² continuity. Transition length: L \= V × (φ\_max / ω\_max). Coordinates via Fresnel integrals approximated by Maclaurin power series. Geometric shift δ requires iterative recalculation of circle centers and shortened straight segments.

## **9.4 3D Extension: Dubins Airplane \[Doc 50\]**

Decoupled altitude: compute planar Dubins length, classify altitude change as Low (shallow constant climb), Medium (artificially lengthen horizontal path), or High (inject helical holding spirals). Axis segregation preference: load altitude changes onto straight segments only — climbing during banked turns increases load factor, reduces stall margin, and destabilizes sensor payloads.

## **9.5 Wind-Corrected Turn Radius \[Doc 50\]**

At constant bank angle in wind, ground speed fluctuates harmonically → asymmetric trochoid ground track. To force a circular ground track: r\_safe \= (V\_TAS \+ V\_wind)² / (g × tan(φ\_max)). Uses worst-case maximum ground speed at maximum safe bank angle. This expanded radius becomes the baseline for ALL Dubins geometric derivations.

# **10\. Aerodynamic and Energy Modeling \[Doc 38\]**

## **10.1 Multirotor Power Model \[Doc 38\]**

Hover: P \= (mg)^1.5 / √(2ρA). Mass raised to 1.5 — minor payload increases cause exponential power growth. Forward flight: induced \+ parasite \+ climb. Parasite power ∝ V³. U-shaped speed-vs-power curve: V\_mp (max endurance) vs max-range speed (max coverage per joule). Wind causes asymmetric energy costs: headwind forces TAS up the steep right side of the curve.

## **10.2 Fixed-Wing Drag Polar \[Doc 38\]**

C\_D \= C\_D0 \+ C\_L²/(πeAR). Oswald efficiency 0.70–0.85. Power: parasite ∝ V³ (dominates at high speed) \+ induced ∝ 1/V (dominates at low speed). V\_md (max range) where parasite \= induced drag. V\_mp ≈ 0.76 × V\_md (max endurance). Propeller efficiency varies with advance ratio J \= V\_TAS/(nD) — NOT a constant. 3rd-order polynomial Ct/Cp fits from UIUC wind tunnel data.

## **10.3 Battery Electrochemistry \[Doc 38\]**

Peukert effect: k ≈ 1.05 for LiPo. Incremental penalties during high-current phases (VTOL, aggressive climb, tight Dubins turns). Thermal: cold (0°C) → internal resistance spike; hot (\>45°C) → SEI layer destruction. Voltage sag: V\_terminal \= V\_oc \- I×R\_internal. LiPo discharge cliff in final 15–20% SoC combined with current spikes virtually guarantees LVC breach.

## **10.4 Per-Segment Energy Costing \[Doc 38\]**

Straight: E \= P(V\_TAS) × d/V\_GS. Turns: 60° bank \= load factor 2.0 \= 4× induced drag. Dubins/clothoid bank limits serve dual purpose: gimbal stability AND energy conservation. Climb: P\_climb \= mg×V\_z (additive). Descent: \~6.7% regenerative recovery for fixed-wing via FOC motor drivers. Total: E\_total via Kahan summation on f64. Feasible \= E\_total\_adjusted ≤ 0.80 × E\_battery\_nominal.

# **11\. Post-Flight QC and Automated Re-Fly Planning \[Doc 46\]**

## **11.1 Cross-Track Error (XTE) \[Doc 46\]**

Transform to local UTM (Karney-Krüger), compute perpendicular vector distance with directional sign (left/right drift). Low-pass filter rejects GNSS multipath spikes. Dynamic threshold from mission parameters (footprint width, sidelap, ASPRS class). Segments exceeding threshold for distance \> footprint length → flagged for re-fly.

## **11.2 Trigger Event Analysis \[Doc 46\]**

Terrain-adjusted GSD per exposure (AGL from DEM interpolation). Forward overlap \= 1 \- (trigger\_spacing / footprint\_length). RTK Float events flagged with widened error margins. Gaps below minimum overlap threshold generate discrete gap markers in the GeoPackage.

## **11.3 6DOF Image Footprint Reconstruction \[Docs 21, 46\]**

Quaternion attitude → DCM rotation matrix → boresight correction → collinearity equation inversion → DEM ray casting → irregular convex quadrilateral on topography. Polygon intersection: Sutherland-Hodgman O(n×m) for standard clipping, O’Rourke O(n+m) for batch convex footprint processing. Crab angle “sawtooth” detection: rotated footprints can have dangerously low side-lap even with perfect line spacing.

## **11.4 Coverage Heat Map \[Doc 46\]**

Explicit intersection meshing (not additive blending): Rust backend shatters the survey boundary into mutually exclusive sub-polygons with exact overlap depth. Streamed as raw binary via WebSocket to MapLibre. Red/yellow/green aviation color schema per AC 25-11B. Offline rendering via irontrack:// tile protocol.

## **11.5 Automated Re-Fly Generation \[Doc 46\]**

Trapezoidal cellular decomposition of non-convex gap polygons → boustrophedon sweep within each convex cell. TSP routing between gaps (NN \+ Or-opt). Dubins/clothoid transitions. EB \+ B-spline terrain-following. Appended to active GeoPackage via WAL (non-blocking). Instant WebSocket propagation to cockpit — no landing required.

# **12\. ASPRS 2024 Standards Compliance \[Doc 36\]**

ASPRS 2023 Edition 2: continuous X-cm horizontal classes (RMSE only, 95% confidence multiplier eliminated). NVA/VVA vertical accuracy (VVA no longer pass/fail). RMSE\_H prediction formula integrating GNSS, IMU, and AGL variables. GSD heuristics: AT ≈ 0.5×pixel, raw GSD ≤ 95% of deliverable. BBA overlap: 60/30 minimum, 80/60 recommended, \>85/85 destroys B/H ratio. GCP requirements: 30 minimum per CLT, scaling to 120 cap, 4× multiplier relaxed. USGS QL0–QL3 quality levels mapped to IronTrack’s reverse-kinematic solver architecture.

# **13\. Computational Geometry Primitives \[Doc 49\]**

Point-in-Polygon: Sunday’s winding number (trigonometry-free, topologically superior to ray casting for self-intersecting polygons). R-tree AABB pre-filtering \+ SoA SIMD vectorization for bulk queries. Line-Polygon Clipping: Weiler-Atherton (inbound/outbound linked-list traversal for concave boundaries) or Vatti sweep-line. Greiner-Hormann disqualified (epsilon perturbation destroys f64 precision). Buffer/Offset: straight skeleton method (geo-buffer crate) for severe topological deformities; geo crate for standard corridor offsets. Convex Hull: rotating calipers for minimum bounding rectangle in O(n) after O(n log n) hull, pure vector algebra (no trig). Area: Karney’s ellipsoidal polygon method (Shoelace on UTM has \~3.5% error for large regions). Delaunay/Voronoi: spade crate selected over delaunator for exact geometric predicates, CDT for breaklines, and native Voronoi extraction.

# **14\. Performance Architecture \[Doc 52\]**

SIMD: Manual AVX2 f64x4 for batch Karney geodesics (LLVM auto-vectorization fails on iterative convergence loops). SoA mandatory for aligned block loads. Lane masking for variable convergence rates. Trigonometric approximations: minimax polynomials via FMA — LUTs rejected (cache misses 200–400 cycles), fast\_math REJECTED (destroys Kahan summation guarantees). Rayon: work-stealing with with\_min\_len(1024) for DEM interpolation pipelines. DEM access: memmap2 for constrained devices (Pi 4/5) with R-tree geographic presorting to prevent thrashing. SQLite: explicit transaction batching for bulk inserts. PGO: cargo-pgo instrumentation with architecture-specific profiling (x86 profiles invalid for ARM64). Criterion benchmarks on every PR for regression detection.

# **15\. Field Hardware and Avionics Integration \[Docs 47, 48\]**

## **15.1 Deployment Platforms \[Doc 47\]**

Raspberry Pi 5 (8 GB, 64-bit mandatory, CNC aluminum passive cooling, $100–135): 2–2.5× faster than Pi 4\. GPIO disqualified for triggering (150 µs–10 ms OS jitter). Intel NUC 12/13 Pro ($300–600+): P-core/E-core hybrid, 64 GB DDR4, Gen4 NVMe. 65–95W peak → requires 19V DC-DC conversion \+ wire-rope vibration dampers. Ruggedized tablets recommended for manned ops: Panasonic Toughbook G2/CF-33 (1200 nits, hot-swap batteries), Dell Latitude 7230, Getac F110 (IP66, glove-touch). Secondary market $300–3000. Custom avionics (Veronte 1x: DO-178C DAL-B, 198 g) only for BVLOS autonomous.

## **15.2 Aircraft Power Conditioning \[Doc 48\]**

GA piston 14V, turboprop 28V, UAV LiPo 4S–12S (14.8–50.4V). Synchronous buck converters mandatory (LDO disqualified: 69W waste heat at 28V→5V). DO-160G Section 16/17 compliance: TVS diode (SMDJ33A: clamps at 53.3V, 3000W peak pulse) as first line of defense. CRITICAL: downstream buck must be rated ≥60V (not 40V). Critically damped LC filter (Middlebrook stability criterion). Hold-up capacitance for 50 ms–1 s power interruptions. Reverse polarity Schottky at front end.

## **15.3 Grounding and EMI \[Doc 48\]**

FAA AC 43.13-1B: \< 2.5 mΩ bond from equipment chassis to airframe. Star ground: signal ground meets chassis at one point. Opto-isolation on trigger controller GPIO provides complete galvanic separation — ground loops physically impossible. USB shield hybrid termination: hard-bond at host, 1 MΩ ∥ 4.7 nF at device (blocks DC loop currents, passes RF shielding). Braided metallic shielding on all 460,800 baud UART cables.

## **15.4 Connectors \[Doc 48\]**

MIL-DTL-38999 Series III (triple-start thread, scoop-proof) for primary power and multi-pin signals. Fischer Core/LEMO K-Series (IP68/IP69) for external/exposed mounting. DB-9 with jackscrews for RS-232 serial. Amphenol USB3F TV for ruggedized USB. Total hardware stack: \< 3 kg.

## **15.5 Airframe Integration \[Doc 48\]**

Cessna 206: MTOW 3,600 lbs, belly camera port STC (20” diameter), Avcon AeroPak pod (15.3 ft³, 300 lbs). King Air 200: MTOW 12,500 lbs, LCAS 400 crash-load-certified racks, pressurized climate-controlled cabin, SOMAG GSM 4000 gyro mount (stabilizes 120 kg payload). IronTrack’s \~3 kg stack is negligible in either airframe.

# **16\. Testing and Validation Strategy \[Doc 44\]**

## **16.1 Core Engine Unit Testing \[Doc 44\]**

Property-based testing (proptest): millions of random coordinates across full global domain (±90° lat, ±180° lon), round-trip invariants for UTM and Helmert. Authoritative test vectors: Karney GeodTest.dat (500K geodesics, 15 nm precision), NGA Gold Data v6.3, NGS NCAT/VERTCON. Kahan summation validation: 100K dead-reckoning integrations — naive vs Kahan vs arbitrary-precision oracle (rug crate).

## **16.2 Integration Testing \[Doc 44\]**

Axum in-process via axum-test (no TCP port binding). Auth rejection (401), structured errors (ERR\_GEOID\_MISSING), Arc\<RwLock\> contention with concurrent reads \+ writes. WebSocket: tokio-tungstenite for 101 Switching Protocols, watch channel sync, broadcast RecvError::Lagged behavior. Concurrent load: thousands of WebSocket clients measuring propagation delay.

## **16.3 End-to-End Pipeline \[Doc 44\]**

Full pipeline: GeoJSON boundary → 3DEP mock → EB \+ B-spline → GeoPackage → export format verification (DJI .kmz EGM96 conversion, Trackair .mdb schema, QGC .plan MAVLink schema). Golden-file regression via insta crate: deterministic coordinate arrays committed as .snap files, sub-mm geometric deviations flagged on CI.

## **16.4 Embedded Firmware Testing \[Doc 44\]**

SIL: embedded-hal-mock on x86\_64, UART NMEA injection, GPIO trigger timing at derivative zero-crossing. HIL: physical RP2040/STM32 via FTDI USB-serial. 15K waypoint pagination (120 KB COBS payload), 460,800 baud DMA saturation, GPIO jitter \< 100 ns via logic analyzer.

## **16.5 Mock Telemetry \[Doc 44\]**

Daemon mock mode: virtual aircraft along B-spline trajectory with synthetic crosswind, 10 Hz NMEA output. Closed-loop CDI verification \+ SIL trigger sync — proves closest-approach derivative decouples lateral error from along-track synchronization.

## **16.6 CI Pipeline \[Doc 44\]**

cargo fmt \+ cargo clippy on every PR. Cross-compilation build matrix: x86\_64 \+ aarch64 \+ thumbv7em-none-eabihf (STM32) \+ thumbv6m-none-eabi (RP2040). Headless Tauri testing via xvfb \+ Playwright. Code coverage via cargo-llvm-cov. All four targets must compile successfully on every PR.

## **16.7 Eagle Mapping Field Acceptance \[Doc 44\]**

Phase 1: Trackair parallel execution \+ spatial delta analysis (IronTrack must demonstrate C² terrain-following vs Trackair’s discontinuous steps). Phase 2: live flight mission with physical trigger controller \+ sensor payload. Phase 3: ASPRS 2024 compliance (RMSE only, 30 minimum checkpoints, NVA/VVA differentiation). Production gate: orthomosaics meet ASPRS class without manual waypoint intervention AND flight crew validates cockpit ergonomics.

# **17\. Release Roadmap**

| Release | Theme | Key Deliverables |
| :---- | :---- | :---- |
| v0.1 | Foundation (Complete) | DEM ingestion (SRTM, Copernicus), flight line generation, GeoPackage/GeoJSON export, Karney geodesics, EGM2008, bilinear terrain interpolation, CLI interface |
| v0.2 | Correctness & Safety (Complete) | Datum-tagged altitudes, EGM96 integration, DSM warnings, Copernicus attribution, QGC/DJI/MDB exports, LiDAR planning, KML/KMZ import, schema versioning, trigger event CSV |
| v0.3 | Kinematics & Routing | EB \+ B-spline terrain-following (C², convex hull safety), corridor mapping, Dubins+clothoid turns, NN+Or-opt TSP, wind-corrected turn radius, bank angle physics, synthetic validation suite \[Docs 34, 39, 50\] |
| v0.4 | Networked FMS Daemon | Axum REST/WebSocket, NMEA parsing, RTK degradation, mDNS discovery, WAL persistence, VID/PID auto-detection, hot-plug resilience, mock telemetry, ephemeral token auth \[Doc 28\] |
| v0.5 | Atmospheric & Energy | Wind triangle, crab angle swath compensation, multirotor/fixed-wing drag models, Peukert battery modeling, per-segment energy costing, 20% reserve enforcement, endurance estimation \[Docs 16, 38\] |
| v0.6 | Glass Cockpit MVP | Tauri 2.0, Track Vector CDI, Canvas 2D, B612 font, ARINC 661 colors, 20 mm touch targets, WebSocket telemetry display, auto-detecting daemon mode \[Docs 25, 26\] |
| v0.7 | Web Planning UI | React \+ MapLibre, client-side boundary parsing (KML/KMZ/SHP/GeoJSON/CSV-WKT/DXF), Turf.js exclusion zones, TanStack Table sensor library, irontrack:// tile protocol, FAA airspace \+ TFR 4D intersection \+ DOF obstacles, advisory-only geofencing \[Docs 35, 42, 43, 45\] |
| v0.8 | Trigger Controller | Embedded Rust (STM32/RP2040), NMEA FSM parser, UBX binary, 1000 Hz dead reckoning, closest-approach derivative zero-crossing, COBS+CRC-32 serial with RTS/CTS, opto-isolated GPIO, PPS µs timestamping, MAVLink hybrid for UAVs, dual-trigger LiDAR+camera, HIL testing \[Docs 22, 29, 40, 41\] |
| v0.9 | 3DEP & US Domestic | 3DEP AWS S3 COG range requests, resolution tier fallback, 256×256 microtile caching (OGC TIFF \+ f64 SoA), GEOID18 biquadratic \+ Helmert per-pixel, 3DEP+Copernicus MBlend feathering, epoch-aware structs, GeopotentialModel trait \[Docs 33, 42\] |
| v1.0 | Production Release | Full E2E pipeline testing, Eagle Mapping field validation (Trackair parallel \+ live flight \+ ASPRS 2024), post-flight QC (XTE \+ 6DOF footprint \+ heat map \+ auto re-fly), PPK open/closed-loop, dual-antenna heading, NSRS 2022 preparation, documentation, community release \[Docs 36, 44, 46\] |

# **18\. Research Foundation**

IronTrack’s design is backed by an extensive body of 52 deep-research documents maintained alongside the codebase and referenced by AI coding agents during implementation. Every architectural decision in this manifest traces to one or more of these documents.

| Doc | Title | Covers |
| :---- | :---- | :---- |
| 00 | Project Charter | Scope boundaries, milestone definitions, licensing |
| 01 | Legal Framework | GPLv3 mechanics, copyright headers, CLA strategy |
| 02 | Geodesy Foundations | WGS84, Karney algorithm, Karney-Krüger UTM, SoA, SIMD, Kahan summation |
| 03 | Photogrammetry Spec | GSD, FOV, swath, dynamic AGL, collinearity, crab angle, sensor stabilization |
| 04 | GeoPackage Architecture | Binary header, flags, envelope codes, R-tree triggers, WAL pragmas |
| 05 | Vertical Datum Engineering | EGM96, EGM2008, NAVD88, GEOID18, barometric ISA, terrain-following control theory |
| 06 | Rust Architecture | FFI, async traps, SIMD, rayon vs tokio, zero-copy mmap |
| 07 | Corridor Mapping | Polyline offsets, self-intersection clipping, Dubins, bank angle, clothoids |
| 08 | Copernicus DEM | TanDEM-X interferometry, X-band penetration, licensing, ingestion |
| 09 | DSM vs DTM | Canopy penetration, AGL safety, FABDEM, pitch/roll dynamics |
| 10 | COG Ingestion | Cloud-Optimized GeoTIFF, HTTP range requests, tiled raster |
| 11 | Autopilot Formats | MAVLink, QGC .plan, DJI WPML/KMZ, KML, ArduPilot missions |
| 12 | EGM2008 Grid | Spherical harmonics, grid interpolation, undulation lookup |
| 13 | LiDAR Flight Planning | PRR, scan rate, point density, spacing, canopy penetration |
| 14 | GeoTIFF/COG Processing | TIFF structure, compression, tiling, overviews |
| 15 | 3DEP Access | USGS API, NAVD88, 1-meter resolution, US coverage |
| 16 | Aerial Wind Data | Wind triangle, atmospheric models, forecast ingestion |
| 17 | UAV Survey Analysis | Multirotor/fixed-wing kinematics, battery limits, regulatory |
| 18 | Oblique Photogrammetry | Oblique angles, urban 3D, multi-head arrays |
| 19 | WGS84 Epoch Management | Epoch propagation, ITRF realizations, tectonic drift |
| 20 | NA Airspace Data | FAA classes, NASR, ADDS API, altitude reconciliation |
| 21 | Dynamic Footprint Geometry | 6DOF ray casting, polygon intersection, IMU error, overlap buffering |
| 22 | Embedded Trigger Controller | STM32/RP2040, RTIC, NMEA FSM, dead reckoning, GPIO, PPS |
| 23 | NMEA Data Requirements | GGA/RMC/GSA parsing, fix quality, HDOP, dual-antenna heading |
| 24 | Survey Photogrammetry PPK | PPK vs RTK, base stations, RINEX, mid-exposure pulse |
| 25 | Aircraft Cockpit Design | ARINC 661, CDI, B612 font, color standards, touch targets, sunlight readability |
| 26 | Tauri Integration | Tauri 2.0, IPC bridge, spawn\_blocking, OTA update safety, irontrack:// protocol |
| 27 | Reserved |  |
| 28 | IronTrack Daemon Design | Axum REST API, WebSocket architecture, Arc\<RwLock\>, broadcast/watch channels, mDNS, serial auto-detection |
| 29 | Avionics Serial Communication | UART, RS-232/RS-422, CP2102/FTDI (CH340 rejected), baud rates, COBS framing, CRC-32, RTS/CTS flow control |
| 30 | Geodetic Datum Transformation | 14-parameter Helmert, GEOID18 hybridization, NAVD88→WGS84, canonical pivot datum |
| 31 | NAD83 Epoch Management | Realization history (1986→HARN→CORS96→2011), 40 cm tectonic drift, navigational vs analytical boundary |
| 32 | Geoid Interpolation | Bilinear/biquadratic/bicubic error tables, C⁰ vs C¹ continuity, SIMD f64x4, polar convergence |
| 33 | 3DEP Data Integration | TNM API, AWS S3 COG, resolution tier fallback, microtile caching, per-pixel datum transformation, MBlend feathering |
| 34 | Terrain-Following Planning | Elastic Band, Cubic B-spline C², convex hull safety, adaptive knots, dynamic look-ahead pure pursuit |
| 35 | Airspace & Obstacle Data | FAA ADDS API, 4D TFR intersection, DOF 10× buffer volumes, AC 25-11B alert hierarchy, advisory-only philosophy |
| 36 | ASPRS Data Standards | 2023 Ed. 2: RMSE-only classes, NVA/VVA, GSD heuristics, BBA overlap, GCP requirements, USGS QL0–QL3 |
| 37 | Aviation Safety Principles | DO-178C DAL, PhantomData datum safety, cksumvfs checksums, fail-safe patterns, warning fatigue mitigation |
| 38 | UAV Endurance Modeling | Multirotor momentum theory, fixed-wing drag polar, Peukert k≈1.05, per-segment energy, 20% reserve enforcement |
| 39 | Flight Path Optimization | ATSP formulation, boustrophedon detection, NN+Or-opt pipeline, 2-opt ATSP trap, 3-opt direction preservation, multi-block hierarchical routing |
| 40 | Sensor Trigger Library | Phase One/Hasselblad/Sony specs, LEMO/Molex pin-level, FMC smear formula, rolling shutter warning, MEP timing, PPS µs timestamping |
| 41 | LiDAR System Control | Continuous emission model, Leica TM-3/Riegl VQ-1560 II/Optech Galaxy Prime, dual-trigger hybrid, navigational-analytical boundary |
| 42 | GeoPackage Extensions | gpkg\_extensions mechanism, TGCE 32-bit float TIFF, MBTiles rejection, irontrack:// tiles, WAL/SQLITE\_BUSY fatal, R-tree scaling, VACUUM prohibition |
| 43 | Boundary Import Formats | Shapefile (pure Rust), .gdb rejection, DXF transitional, GeoJSON/KML/KMZ client-side, CSV-WKT, v1.0 WGS84+UTM-only CRS safety |
| 44 | Testing Strategy | proptest, Karney/NGA/NGS test vectors, Kahan validation, Axum integration, golden-file insta, embedded SIL/HIL, mock telemetry, CI build matrix, Eagle Mapping acceptance |
| 45 | Aerial Sensor Library | 10 cameras \+ 9 LiDAR full specs, irontrack\_sensors schema, irontrack\_mission\_profiles table, JSON+relational hybrid |
| 46 | Post-Flight QC | XTE computation, trigger analysis, 6DOF footprint reconstruction, Sutherland-Hodgman/O’Rourke intersection, coverage heat map, automated re-fly generation |
| 47 | Daemon Field Hardware | Raspberry Pi 4/5, Intel NUC, ruggedized tablets, Veronte 1x, environmental constraints, DO-160 power conditioning, EMI shielding |
| 48 | Avionics Hardware Integration | Aircraft power buses (14V/28V/LiPo), TVS diodes (SMDJ33A), LC filtering, Middlebrook stability, grounding/bonding, MIL-DTL-38999, Cessna 206/King Air 200 spatial budgets |
| 49 | Computational Geometry | Winding number PIP, Weiler-Atherton/Vatti clipping, straight skeleton buffer, rotating calipers MBR, Karney ellipsoidal area, spade Delaunay/Voronoi CDT |
| 50 | Dubins Mathematical Extension | 6 canonical paths, clothoid Fresnel integrals, 3D Dubins Airplane altitude classes, wind correction angle, r\_safe ground-track radius |
| 51 | GPLv3 Compliance | Dependency audit, MPL-2.0 Section 3.3, LGPL static linking, SSPL/BSL exclusion, embedded firmware boundary, IPC bridge for proprietary SDKs, CLA vs DCO |
| 52 | Rust Performance | AVX2 f64x4 SIMD, fast\_math rejection, rayon with\_min\_len tuning, memmap2 \+ R-tree presorting, SQLite transaction batching, PGO, Criterion CI benchmarks |

# **19\. Locked Architectural Decisions**

The following decisions have been researched, evaluated, and are inviolable across all subsequent design phases:

| Decision | Rationale and Source |
| :---- | :---- |
| Advisory-only airspace philosophy | PIC retains ultimate authority. IronTrack never commands or blocks. \[Doc 35\] |
| CRDT rejection | Multi-user collaboration via CRDTs evaluated and rejected as semantic safety risk. \[Doc 28\] |
| Navigational vs analytical epoch boundary | 40 cm tectonic drift acceptable for flight nav, catastrophic for PPK. \[Docs 30, 31\] |
| Big-Endian GEOID18 mandate | Little-Endian GEOID18 files corrupted in 2019 NGS release. \[Doc 32\] |
| Preprocessing mandate for datum transforms | Never on-the-fly during flight. Always preprocessed. \[Doc 30\] |
| CH340 USB-serial rejected | Only CP2102/FTDI approved. CH340 failed qualification. \[Doc 29\] |
| Bilinear interpolation disqualified | Bicubic on EGM2008 (3.1 cm max error); biquadratic on GEOID18. \[Doc 32\] |
| PhantomData compile-time datum safety | Untagged altitudes cause compilation failures. \[Doc 37\] |
| SQLITE\_BUSY \= fatal architectural violation | Not a transient retry. WAL-reset bug risk. \[Doc 42\] |
| VACUUM prohibited in active flight | Exclusive lock \+ storage doubling. Use PRAGMA optimize. \[Doc 42\] |
| 20% battery reserve \= blocking enforcement | LiPo discharge cliff \+ current spikes \= LVC breach. \[Doc 38\] |
| NN \+ Or-opt as recommended TSP pipeline | Sub-ms, 1–3% of optimal. 2-opt rejected for ATSP. \[Doc 39\] |
| .gdb native ingestion rejected | Require export to GeoJSON/KML. GDAL FFI bloat. \[Doc 43\] |
| v1.0 CRS: WGS84 \+ UTM only | No automatic reprojection of arbitrary datums. \[Doc 43\] |
| Sensor library inside GeoPackage | Not separate DB. Prevents split-brain. \[Doc 45\] |
| fast\_math REJECTED | Destroys Kahan summation error compensation. \[Doc 52\] |
| Greiner-Hormann clipping disqualified | Epsilon perturbation destroys f64 precision. \[Doc 49\] |
| spade over delaunator | Exact predicates \+ CDT \+ Voronoi. Speed irrelevant for correctness. \[Doc 49\] |
| EXIF timestamps disqualified | Integer-second resolution \= 80 m ambiguity at fixed-wing speeds. \[Doc 40\] |
| Firmware is a separate work (not GPLv3) | Arm’s-length COBS serial. MIT/Apache-2.0 licensed. \[Doc 51\] |

# **20\. Long-Term Roadmap (Post-v1.0)**

Multi-aircraft coordination: deconflicted flight plans, synchronized line assignment, shared telemetry. International airspace: pluggable Airspace Provider traits for NavCanada, EuroControl. Legacy avionics integration: Applanix GSM, POSView, Riegl RiAcquire (contingent on vendor cooperation). NSRS 2022: NATRF2022/NAPGD2022/GEOID2022 trait implementations when FGCS vote completes. Automatic CRS reprojection for arbitrary datums (post-v1.0 only, after extensive validation).

# **21\. Community Infrastructure**

Repository: GitHub with CI via GitHub Actions. Memory-safe builds, cargo clippy, cargo fmt, cargo-deny license scanning on every push. Dockerized ARM cross-compilation for Pi edge binaries. Documentation: README, CONTRIBUTING.md, issue templates, and the full 52-document research library in docs/. Operator manuals documenting v1.0 CRS limitations and all non-dismissable safety warnings.

# **Document Index**

**A**  
6DOF Image Footprint Reconstruction 15, 22  
Aerodynamic and Energy Modeling 14  
Advisory-only airspace philosophy 11, 23  
Airframe Integration 17  
ASPRS 2024 Standards Compliance 16  
Async Boundary Rule, The 5  
ATSP Formulation (Asymmetric Traveling Salesman Problem) 12  
Attribution (Geospatial Data) 4  
AVX2 f64x4 SIMD 16, 24  
Avionics Hardware Integration 17  
Axum 5, 19, 23

**B**  
Battery Electrochemistry 14  
Big-Endian GEOID18 mandate 23  
Boustrophedon Short-Circuit 12  
Boundary Import and CRS Handling 8  
B-Spline Hybrid 13

**C**  
Camera Specifications 9  
Canonical Pivot Datum: WGS84 Ellipsoidal 6  
Cargo-deny 3  
Cessna 206 17  
CLA (Contributor License Agreement) 4  
Clothoid (Euler Spiral) Transitions 13  
COBS-framed binary serial 4, 5, 19  
Computational Geometry Primitives 16  
Concurrency: WAL Mode 7, 23  
Copernicus DEM 4, 6, 20  
Coverage Heat Map 15, 19  
CRS Handling 8, 24  
Cross-Track Error (XTE) 15  
CSV with WKT Geometry 8

**D**  
DCO (Developer Certificate of Origin) 4  
Dependency License Audit 3  
DO-178C DAL Classification 11  
Dubins Airplane (3D Extension) 13  
Dubins Paths 13

**E**  
Eagle Mapping 3, 18, 19, 24  
Elastic Band \+ B-Spline Hybrid 13  
Embedded Firmware Boundary 4, 24  
EGM2008 6, 20, 23  
Executive Summary 3  
EXIF timestamps disqualified 10, 24

**F**  
Fail-Safe Patterns 11  
Fixed-Wing Drag Polar 14  
Flight Management System (FMS) 3  
Flight Path Optimization and Routing 12  
Four-Layer Architecture 5  
fast\_math REJECTED 24

**G**  
Geodetic Computation Standards 5  
GEOID18 4, 6, 21, 23  
GeoJSON 8, 19  
GeoPackage Tile Matrix Set 7, 22  
GeoPackage Universal Data Container 7  
GPLv3 3, 20, 24  
Greiner-Hormann clipping disqualified 24  
Grounding and EMI 17

**H**  
Helmert transformation 5, 6, 21

**I**  
IPC Bridge Pattern 4, 24  
irontrack:// custom URI protocol 7, 19  
irontrack\_mission\_profiles 9, 22  
irontrack\_sensors 9, 22  
IronTrack Daemon Design 21

**K**  
Kahan summation 5, 14, 18, 20, 24  
Karney's algorithm (geodesic) 5, 16, 20, 24  
King Air 200 17

**L**  
LiDAR Continuous Emission 10  
Licensing and IP Governance 3  
Locked Architectural Decisions 23

**M**  
Mid-Exposure Pulse (MEP) 10, 22  
Multi-Block Hierarchical Routing 12  
Multirotor Power Model 14

**N**  
NAD83 Epoch Management 5, 6, 21  
Navigational vs Analytical Boundary 6, 23  
NN \+ Or-Opt (TSP pipeline) 12, 23

**O**  
Offline Basemap: Embedded Tiles 7  
Or-opt refinement 12, 15

**P**  
Peukert effect (k1.05) 14, 19  
Per-Segment Energy Costing 14  
Performance Architecture 16  
PhantomData (datum safety) 6, 11, 23  
Phase One MEP 10  
Post-Flight QC and Automated Re-Fly Planning 15  
Preprocessing Mandate (Datum Transforms) 6, 23  
Proprietary SDK Isolation (IPC Bridge Pattern) 4, 24

**R**  
R-tree spatial index 7, 16, 20, 22, 24  
Raspberry Pi 5 17  
React web interface 3, 19  
Real-Time Boundary, The 5  
Release Roadmap 19  
Research Foundation 20

**S**  
Safety Architecture 11  
Sensor Library and Trigger Interfaces 9  
Shapefile Parsing 8, 22  
SQLITE\_BUSY \= fatal architectural violation 7, 23  
SSPL (MongoDB) 3  
Structure of Arrays (SoA) 5, 7, 16, 20  
Sutherland-Hodgman intersection 15

**T**  
Tauri 2.0 desktop "Glass Cockpit" 5, 19  
Terrain-Following Trajectory Generation 13  
Testing and Validation Strategy 18  
Tiled Gridded Coverage Extension (TGCE) 7, 22  
Trigger Controller (Embedded Firmware) 4, 5, 19  
TVS diode (SMDJ33A) 17, 22  
Type System Safety 11, 23

**U**  
USGS 3DEP 4, 6, 21

**V**  
v1.0 CRS Limitation (WGS84 \+ UTM only) 8, 24  
VACUUM prohibited 7, 23

**W**  
Weiler-Atherton/Vatti clipping 16, 22  
WGS84 Ellipsoidal 6  
Winding number (Point-in-Polygon) 16  
Wind-Corrected Turn Radius 13  
