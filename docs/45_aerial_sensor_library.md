# Sensor Library Database Design and Hardware Specifications

> **Source document:** 45_Aerial-Sensor-Library.docx
>
> **Scope:** Complete extraction — all content is unique. Covers full specs for 10 photogrammetric cameras and 9 LiDAR sensors, the irontrack_sensors SQLite schema, saved profile architecture (irontrack_mission_profiles table), and versioning strategy.

---

## 1. Photogrammetric Camera Specifications

### Consolidated Table

| Model | Sensor (mm) | Resolution (px) | Pitch (μm) | Focal Lengths (mm) | Max Rate | Shutter | Weight |
|---|---|---|---|---|---|---|---|
| **Phase One iXM-100** | 43.9 × 32.9 | 11,664 × 8,750 | 3.76 | 40, 50, 80 (RS/RSM) | 3 fps | Mech leaf 1/2500s | 630 g |
| **Phase One iXM-50** | 43.9 × 32.9 | 8,280 × 6,208 | 5.30 | 40, 50, 80 (RS/RSM) | 2 fps | Mech leaf 1/2500s | ~630 g |
| **Hasselblad A6D-100c** | 53.4 × 40.0 | 11,600 × 8,700 | 4.60 | 24–300 (H System) | 1 fps | Mech leaf 1/4000s | 1360 g |
| **Sony ILX-LR1** | 35.7 × 23.8 | 9,504 × 6,336 | 3.76 | E-Mount interchangeable | 3 fps | Focal plane (mech/elec) | 243 g |
| **Sony α7R IV** | 35.7 × 23.8 | 9,504 × 6,336 | 3.76 | E-Mount interchangeable | 10 fps | Focal plane (mech/elec) | 665 g |
| **DJI Zenmuse P1** | 35.9 × 24.0 | 8,192 × 5,460 | 4.40 | 24, 35, 50 (DL Mount) | ~1.4 fps | Global mech 1/2000s | — |
| **DJI Zenmuse L2 (RGB)** | 4/3 CMOS | 5,280 × 3,956 | 3.30 | 24 (equiv) | ~1.4 fps | Global mech 1/2000s | 905 g* |
| **Vexcel Eagle M3** | 105.85 × 68.03 | 26,460 × 17,004 | 4.00 | 80, 100, 120, 210 | 0.66 fps | Prontor Magnetic (TDI FMC) | — |
| **Vexcel Osprey 4.1** | Multi-head | 20,544 × 14,016 (nadir) | 3.76 | 80 (nadir), 120 (oblique) | ~1.4 fps | Prontor Magnetic (AMC) | — |
| **Leica DMC-4X** | Multi-head | 54,400 × 8,320 | 3.76 | Custom array | 1.25 fps | Mech central (FMC) | — |

*L2 weight is full hybrid payload including LiDAR.

### Key Notes
- **iXM-50 vs iXM-100:** Same sensor dimensions, lower resolution → larger pixel pitch (5.3 vs 3.76 μm). Achieving the same GSD requires lower AGL.
- **A6D-100c:** Largest sensor (53.4 × 40.0 mm) → best SNR/dynamic range (15 stops). 8-camera sync bus (20 μs deviation) for oblique arrays. Slowest trigger rate (1 fps).
- **Sony α7R IV:** 10 fps but uses focal-plane shutter → rolling shutter skew at high speeds. Hot-shoe flash sync for PPK event marking. FMS must warn for survey-grade PPK.
- **DJI P1:** Global mechanical leaf shutter + TimeSync 2.0 (microsecond camera/gimbal/RTK alignment). Closed PSDK ecosystem.
- **Vexcel Eagle M3:** 450 MP panchromatic composite. TDI Forward Motion Compensation. Exchangeable lens systems.
- **Leica DMC-4X:** 54,400 × 8,320 composite swath → 71° cross-track FOV. Mechanical FMC.

---

## 2. LiDAR Sensor Specifications

### Consolidated Table

| Model | PRR | Scan Rate | FOV | MPiA / Returns | Max Range | Weight |
|---|---|---|---|---|---|---|
| **Riegl VUX-240** | 1.8 MHz (2.4 MHz) | 400 (600) lines/s | 75° | MTA / multi-target | 1200 m | 4.1 kg |
| **Riegl VQ-1560 II** | 4.0 MHz (dual) | 40–600 lines/s | 58° (28° cross-fire) | 35 MPiA / multi | ~4500 m | 55 kg |
| **Riegl miniVUX-3UAV** | 100/200/300 kHz | 10–100 scans/s | 360° (120° @ 300k) | Multi / 5 returns | 170 m | 1.55 kg |
| **Leica TerrainMapper-3** | 2.0 MHz | 66–333 scans/s | 10°–60° (prog) | Gateless / 15 returns | >6000 m AGL | 47.6 kg |
| **Leica CityMapper-2** | 2.0 MHz | 120–300 scans/s | 20°–40° | 35 MPiA / 15 returns | >5500 m AGL | 58 kg |
| **Optech Galaxy Prime** | 1.0 MHz | Up to 240 lines/s | 10°–60° (dynamic) | PulseTRAK / 8 returns | >6000 m AGL | 27 kg |
| **DJI Zenmuse L2** | 240 kHz | Rep / non-rep | 70°×3° / 70°×75° | Multi / 5 returns | 250 m | 905 g |
| **Livox Mid-360** | Non-repetitive | Cumulative | 360° × 59° | Multi-target | 40 m | 265 g |
| **Yellowscan Voyager** | 2.4 MHz | Ultra-fast continuous | 100° | Dual-return | — | Compact |

### Key Notes
- **VQ-1560 II:** Dual-channel cross-fire (28° tilt between channels) → eliminates data shadows. 2.66M effective measurements/second.
- **CityMapper-2:** Hybrid — 2 MHz LiDAR + six 150 MP cameras in one payload. Oblique scan pattern.
- **Galaxy Prime SwathTRAK:** Dynamic FOV (10°–60°) maintains constant physical swath width on ground regardless of terrain → 40–70% fewer flight lines in mountains.
- **Livox Mid-360:** Non-repetitive rosette pattern fills FOV over time → FMS must calculate dwell time, not instantaneous swath.
- **Yellowscan Voyager:** 2.4 MHz from a compact UAV payload — highest PRR in the UAV class.

---

## 3. Database Schema: irontrack_sensors Table

**Architecture decision:** Sensor library lives INSIDE the GeoPackage (not a separate SQLite file). Eliminates split-brain: if a flight plan references sensor ID X, the sensor definition travels with the plan. Self-describing, fully portable.

### Column Definitions

| Column | Type | Nullable | Purpose |
|---|---|---|---|
| `name` | TEXT NOT NULL | No | Commercial identifier ("Phase One iXM-100") |
| `manufacturer` | TEXT NOT NULL | No | OEM ("Riegl", "DJI") |
| `type` | TEXT NOT NULL | No | 'CAMERA' or 'LIDAR' — branches math engine |
| `sensor_width_mm` | REAL | Yes (null for LiDAR) | Physical array width → cross-track FOV computation |
| `sensor_height_mm` | REAL | Yes (null for LiDAR) | Physical array height → along-track footprint |
| `pixel_pitch_um` | REAL | Yes (null for LiDAR) | Pixel size → GSD at altitude |
| `resolution_x` | INTEGER | Yes | Horizontal pixel count |
| `resolution_y` | INTEGER | Yes | Vertical pixel count |
| `focal_lengths_mm` | TEXT | Yes | Serialized JSON array (e.g., "[40.0, 50.0, 80.0]") — no native SQLite array type |
| `prr_hz` | REAL | Yes (null for cameras) | Pulse Repetition Rate → point density validation |
| `scan_rate_hz` | REAL | Yes (null for cameras) | Scan line frequency |
| `fov_deg` | REAL | Yes | Transverse Field of View |
| `weight_kg` | REAL | Yes | Payload mass → UAV endurance / CG estimation |
| `trigger_interface` | TEXT | Yes | 'GPIO_ACTIVE_LOW', 'MAVLINK_CMD', 'PSDK', 'HOT_SHOE' |
| `event_output` | TEXT | Yes | 'MID_EXPOSURE' (closed-loop PPK), 'FLASH_SYNC', 'EXPOSURE_OPEN_DRAIN', 'NONE' |
| `notes` | TEXT | Yes | JSON field for volatile/platform-specific overrides (gimbal tilt offsets, etc.) |

**Why NOT JSON blobs for core parameters:** The rayon math engine must cast thousands of trajectory permutations against sensor limits. Constant JSON deserialization would be a severe CPU bottleneck. Normalized columns give instant, low-overhead reads.

---

## 4. Saved Profiles: irontrack_mission_profiles Table

### Profile = 5 Components
1. **Sensor selection:** Foreign key → irontrack_sensors row.
2. **Custom overrides:** Runtime adjustments (e.g., selecting 80 mm from available array, restricting LiDAR FOV from 75° to 45°).
3. **Overlap settings:** Target forward-lap and side-lap percentages → line spacing and spatial firing radius.
4. **Mission type:** Nadir Block, Corridor, Oblique.
5. **Aircraft/platform pairing:** Kinematic limits (pitch, path curvature, gimbal stabilization bounds).

### Storage Architecture: Hybrid Relational + JSON

**JSON blob in irontrack_metadata is REJECTED.** The metadata table is the canonical source for global file parameters (legal attributions, schema version, epoch metadata). Injecting volatile sensor configs violates normalization, creates "junk drawer" columns, and risks locking collisions.

**Dedicated irontrack_mission_profiles table:**
- Core parameters (sensor_id FK, mission_type, overlap_fwd, overlap_side) as **rigid relational columns** → instant, type-safe reads for the math engine.
- Custom overrides as a **JSON column within this dedicated table** → allows arbitrary platform-specific tuning (gimbal yaw limits, DJI-specific offsets) without schema migrations.

### Schema Versioning
The irontrack_metadata `schema_version` key governs compatibility:
- Newer software reads older tables gracefully (with warning modal).
- Older software encountering a newer schema_version **rejects the file immediately** — prevents silent misinterpretation of missing columns (e.g., a new MPiA limitation field) that could cause disastrous flight execution errors.
