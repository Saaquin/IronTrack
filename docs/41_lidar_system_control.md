# LiDAR System Integration and Control Interfaces

> **Source document:** 41_LiDAR-System-Control.docx
>
> **Scope:** Complete extraction — all content is unique. Covers airborne LiDAR physical architecture, beam deflection mechanisms, control interfaces (TCP/UDP/serial/PPS), manned system specs (Leica TM-3, Riegl VQ-1560 II, Optech Galaxy Prime), UAV systems (DJI L2, Riegl miniVUX, Livox), continuous emission vs spatial intervalometry, dual-trigger hybrid architecture, and the navigational-analytical post-processing boundary.

---

## 1. Airborne LiDAR Physical Architecture

Four principal subsystems form a tightly coupled optomechanical network:

### 1.1 Laser Ranging (Time-of-Flight)
- **Topographic:** Nd:YAG solid-state lasers at 1064 nm (NIR) or 1550 nm (eye-safe, higher altitude).
- **Bathymetric:** Frequency-doubled YAG at 532 nm (green, low water attenuation).
- **Detectors:** Avalanche Photodiodes (APD, linear mode) standard. Single-Photon Avalanche Diodes (SPAD, Geiger mode) for extreme sensitivity with statistical noise filtering.
- **Digitization:** Discrete echo (hardware thresholding for first/last/intermediate returns) or **full-waveform recording** (entire echo envelope digitized at ~1 GHz, post-processed via Gaussian decomposition for amplitude, echo width, reflectance).

### 1.2 Beam Deflection Mechanisms

| Scanner Type | Pattern | Characteristics |
|---|---|---|
| Rotating Polygon | Parallel, linear, equidistant | Highly efficient for wide-area mapping |
| Oscillating Mirror | Sinusoidal/zigzag | Higher density at swath edges, lower at nadir (deceleration at endpoints) |
| Palmer Scanner | Elliptical/spiral | Multiple illumination angles per pass — minimizes data shadows on vertical structures |
| MEMS / OPA | Solid-state, no moving macroscopic parts | Reduced SWaP for UAV platforms |

### 1.3 INS/GNSS Tight Coupling
- GNSS alone: 10–20 Hz update, no angular measurement → insufficient for millions of pulses/second.
- **IMU (FOG preferred):** 200–400 Hz accelerometers + gyroscopes measure roll/pitch/heading.
- **Kalman filter** fuses GNSS absolute position with IMU angular rates → Smooth Best Estimate of Trajectory (SBET).

### 1.4 System Controller
Synchronizes laser emission, beam deflection, GNSS/IMU, and mass storage. Modern systems at 2 MHz PRR with full-waveform generate terabytes per mission. Ruggedized redundant SSDs (e.g., Leica MM60: 4 TB, 8 hours continuous recording).

---

## 2. Control Interfaces and Communication Protocols

| Interface | Protocol | Medium | Function |
|---|---|---|---|
| Command & Control | TCP/IP | Ethernet | Reliable config, start/stop, flight plan upload |
| Live Monitoring | UDP/IP | Ethernet | Low-latency decimated point cloud + health metrics (tolerates loss) |
| Time Sync | NMEA 0183 / PPS | RS-232 / TTL GPIO | UTC time from GGA/RMC + microsecond PPS hardware latch |
| Bulk Data | Proprietary binary | Optical fiber / SATA | Full-waveform data to mass storage (e.g., Riegl RXP via dual SFP links) |
| Hardware Actuation | GPIO (active-low) | Opto-isolated | External camera triggering / event markers |

### TCP for Commands
Connection-oriented with error-checking and acknowledgment. Ensures critical commands (laser power, scan frequency, arm/disarm) are delivered reliably.

### UDP for Monitoring
Connectionless, low overhead. Operator views live 3D point cloud during flight. Frame loss is preferable to TCP retransmission stalling.

### PPS Time Synchronization
GNSS receiver generates a TTL rising edge exactly at the top of every UTC second → high-priority hardware interrupt on the LiDAR controller latches the internal high-frequency clock. Laser pulses fired mid-second are timestamped as PPS_time + timer_delta → sub-microsecond absolute timing.

---

## 3. Continuous Emission vs Spatial Triggering

### LiDAR: Continuous Emission Model
LiDAR does NOT require external trigger pulses. The laser fires continuously at a steady PRR governed by an internal oscillator. The scanner sweeps at a constant user-defined rate. The flight plan controls the aircraft trajectory — the laser swath is swept over the AOI automatically.

**"Triggering" for LiDAR = start/stop data recording**, not firing individual pulses. The FMS sends TCP commands at flight line boundaries: start recording at line ingress, stop at line egress. Between lines (turns, transits), the laser continues firing but data is not logged to mass storage.

### Camera: Spatial Intervalometry
Cameras demand exact discrete spatial triggers. Time-based intervalometers fail because ground speed fluctuates with wind. The FMS must track 3D coordinates continuously and fire GPIO pulses at precise spatial intervals via dead-reckoning and closest-approach zero-crossing detection.

### Dual-Trigger Hybrid Architecture
Modern hybrid payloads (LiDAR + metric camera) require BOTH simultaneously:
1. Network-based start/stop to LiDAR at line boundaries.
2. Sub-meter GPIO triggers to camera at calculated spatial intervals throughout the line.

General-purpose OS cannot handle both deterministically. IronTrack solves this by decoupling:
- **Network daemon** (Axum/Tokio): sends TCP start/stop to LiDAR controller.
- **Embedded trigger controller** (STM32/RP2040, bare-metal): autonomously fires camera GPIO from pre-loaded waypoint array + live NMEA dead-reckoning.

If the laptop crashes or serial link severs mid-line: LiDAR continues logging internally, trigger controller continues firing cameras autonomously from local SRAM waypoints.

---

## 4. Major Manned LiDAR Systems

### 4.1 Leica TerrainMapper-3

| Spec | Value |
|---|---|
| Max PRR | 2 MHz |
| MPiA | Gateless, up to 35 simultaneous pulses |
| FOV | Programmable 20°–40° |
| Scan Patterns | Circle, Ellipse, Skew Ellipse (up to 250 lines/s) |
| Integrated Camera | Leica MFC150 (247 MP, 4-band RGB+NIR, Sony BSI CMOS) |
| IMU | NovAtel SPAN CNUS5-H FOG |
| GNSS | NovAtel SPAN OEM7 |
| Data Storage | Leica MM60 (4 TB redundant SSD, 8 hours recording) |
| Control Software | **Leica FlightPro** |
| Data Format | Proprietary → ingested by **Leica HxMap** |

**Circle scanning:** illuminates vertical facades from multiple angles (urban 3D). **Ellipse:** uniform bare-earth distribution. **Skew Ellipse:** concentrated density for corridor/infrastructure.

### 4.2 Riegl VQ-1560 II

| Spec | Value |
|---|---|
| PRR | 2 MHz (dual channel combined) |
| Effective Measurements | 2.66 million pts/sec |
| Max Altitude | 5,200 m AGL |
| Scan Geometry | Rotating polygon, dual-channel **cross-fire** (±14° forward/backward look) |
| FOV | 58° |
| Dual-Wavelength Variant | VQ-1560i-DW (Green + NIR) |
| Control Software | **RiACQUIRE** (TCP server on port 20002) |
| Data Format | **RXP** (RIEGL eXtended Packets) via dual optical fiber SFP links |
| SDK | **RiVLib** (C++ library for RXP decoding) |

Cross-fire geometry: forward/backward look penetrates dense canopy and captures building sides in urban canyons. RiACQUIRE uniquely acts as a TCP server — third-party FMS platforms connect as clients and issue start/stop commands.

### 4.3 Teledyne Optech Galaxy Prime

| Spec | Value |
|---|---|
| PRR | 1 MHz |
| Scan Mechanism | Oscillating galvanometer |
| FOV | Dynamic 20°–60° (**SwathTRAK**) |
| Key Technologies | **PulseTRAK** (eliminates blind zones), **SwathTRAK** (dynamic FOV) |
| Control Software | **Optech FMS** (Airborne Mission Manager + FMS Nav) |
| Edge Computing | **Onboard** module — processes trajectory + ToF in-air, generates rapid LAS before landing |

**PulseTRAK:** eliminates altitude-dependent blind zones where outgoing/incoming pulses interfere. Continuous operating envelope regardless of elevation changes.

**SwathTRAK:** actively measures range to ground in real-time and adjusts galvanometer scan angle to maintain constant physical swath width. Reduces required flight lines by 40–70% in variable terrain.

### Comparison Table

| Feature | Leica TM-3 | Riegl VQ-1560 II | Optech Galaxy Prime |
|---|---|---|---|
| Max PRR | 2 MHz | 2 MHz (dual) | 1 MHz |
| Scan Mechanism | Multi-pattern | Rotating polygon (cross-fire) | Oscillating galvanometer |
| FOV | Programmable 20°–40° | 58° (±14° cross-fire) | Dynamic 20°–60° |
| Key Innovation | Gateless MPiA (35 pulses) | Cross-fire geometry | SwathTRAK dynamic FOV |
| Control Interface | Leica FlightPro | RiACQUIRE (TCP:20002) | Optech FMS / Onboard |
| Data Format | Proprietary (HxMap) | RXP | Real-time LAS / proprietary |

---

## 5. UAV LiDAR Systems

### 5.1 DJI Zenmuse L2
- Exclusive to DJI Matrice 300/350 RTK.
- 1.2 million pts/sec, penta-returns, integrated 20 MP RGB camera on 3-axis gimbal.
- Repetitive pattern (70° FOV) or non-repetitive pattern (70° FOV, density increases with dwell time).
- **Control:** DJI PSDK only. State machine via `DJILidarPointCloudRecord` enum: `START_POINT_CLOUD_RECORDING`, `STARTING`, `STARTED`, `PAUSE`, `STOPPED`.
- **Closed ecosystem:** DJI Pilot 2 app manages grid, PSDK commands, camera triggering, and LiDAR recording. Data → microSD → **DJI Terra** for PPK/LAS/DEM.

### 5.2 Riegl miniVUX-1UAV
- 360° FOV rotating mirror, 100 kHz PRR, up to 5 target echoes per pulse.
- **Open architecture:** Dual Gigabit LAN (config + data), RS-232 (NMEA), TTL 1PPS input, TTL outputs for triggering 2 external cameras.
- Any FMS can control via standard TCP/IP — platform agnostic. Ideal for custom builds and open-source integration.

### 5.3 Livox Mid-360 / Avia
- Non-repetitive rosette scanning via rotating optical wedges. Coverage density increases with dwell time.
- **Ethernet SDK:** General Command Set (0x00) for handshake/polling, specialized LiDAR Command Set for start/stop and coordinate formatting (Cartesian XYZ vs Spherical). All via hex commands over UDP/TCP.
- Cost-effective for custom mapping rigs.

### UAV System Comparison

| System | FOV | Scan Pattern | Interface | Ecosystem |
|---|---|---|---|---|
| DJI Zenmuse L2 | 70° | Repetitive / Non-repetitive | DJI PSDK | Closed (Matrice / DJI Terra) |
| Riegl miniVUX-1UAV | 360° | Parallel (rotating mirror) | GigE / RS-232 / TTL PPS | **Open / agnostic** |
| Livox Mid-360/Avia | Varies | Non-repetitive rosette | Ethernet / Livox SDK (hex) | Open / custom builds |

---

## 6. Post-Processing: The Navigational vs Analytical Boundary

### What IronTrack DOES (Navigational)
- Guide the aircraft along terrain-following corridors.
- Parse NMEA telemetry for real-time position awareness.
- Dead-reckon at 1000 Hz for sub-meter spatial triggers.
- Send TCP start/stop commands to LiDAR at line boundaries.
- Fire GPIO camera triggers at closest-approach zero-crossings.
- Record geodetic metadata (datum, realization, epoch) in GeoPackage.
- Export supplementary trigger event CSV (event_number, UTC timestamp, coordinates, fix quality).

### What IronTrack does NOT do (Analytical)
- SBET generation (forward + backward Kalman filter smoothing of GNSS/IMU).
- Boresight calibration (correcting microscopic roll/pitch/yaw misalignment between IMU and laser axis — millimeter mounting errors → meters of ground distortion from altitude).
- Lever arm application (physical offsets between GNSS antenna, IMU origin, laser firing point).
- Point cloud classification, colorization, atmospheric noise reduction.
- Epoch propagation (HTDP/NCAT for tectonic drift correction).
- RINEX parsing — strictly isolated from IronTrack.

### Vendor-Specific Post-Processing Suites

| Suite | Vendor | Key Capabilities |
|---|---|---|
| **HxMap** + CloudWorx | Leica | End-to-end: NovAtel IE SBET, MFC150 colorization, atmospheric filtering, QA reporting |
| **RiProcess** + RiVLib | Riegl | System calibration, planar surface matching, RXP decoding via C++ API |
| **LMS Pro** | Teledyne Optech | Self-calibration engine: planar extraction + redundant feature matching for boresight, project-wide accuracy quantification |
| **DJI Terra** | DJI | PPK from PSDK + base station RINEX, ground classification, DEM/LAS generation |
