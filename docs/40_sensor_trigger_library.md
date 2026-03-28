# Aerial Camera Systems: Models, Interfaces, and Trigger Protocols

> **Source document:** 40_Sensor-Trigger-Library.docx
>
> **Scope:** Complete extraction — all content is unique. Covers medium-format and small-format sensor specs, FMC/shutter physics, trigger electrical topologies (GPIO/hot shoe/MAVLink/PSDK), event feedback (MEP/PPS), and calibration boundary.

---

## 1. Medium-Format Sensor Specifications

### Comparative Table

| Specification | Phase One iXM-100 | Hasselblad A6D-100c | Sony ILX-LR1 |
|---|---|---|---|
| Resolution | 100 MP (11,664 × 8,750) | 100 MP (11,600 × 8,700) | 61 MP (9,504 × 6,336) |
| Sensor Dimensions | 43.9 × 32.9 mm | 53.4 × 40.0 mm | 35.7 × 23.8 mm |
| Pixel Pitch | 3.76 μm | 4.6 μm | ~3.76 μm |
| Focal Lengths | 35, 40, 50, 80, 150, 300 mm (RSM) | 24–300 mm (H System) | Interchangeable E-Mount |
| Max Trigger Rate | 3 fps | 1 fps (60/min) | 3 fps |
| Shutter | Mechanical leaf | Mechanical leaf | Mechanical / Electronic |
| Max Shutter Speed | 1/2500 s | 1/4000 s | 1/4000 (mech), 1/8000 (elec) |
| Trigger Interface | LEMO (GPIO active-low) | LEMO / Flash Sync / USB | Molex Micro-Fit 3.0 (GPIO) |
| Event Feedback | Mid-Exposure Pulse (MEP) | Flash Sync Out | EXPOSURE (open drain) |
| Dynamic Range | 83 dB (BSI CMOS) | 15 stops | — |
| Weight (body) | 630 g | 1360 g | 243 g |
| Multi-camera sync | — | Up to 8 daisy-chained (20–50 μs sync) | — |

### Phase One iXM-100
BSI CMOS for superior photon capture at fast shutter speeds. RSM (Reliance Shutter Mechanism) leaf shutter. Dedicated LEMO connector with trigger input (Pin 4, active-low) and Mid-Exposure Pulse output (Pin 7). Supports PPS correlation for microsecond event marking.

### Hasselblad A6D-100c
Largest sensor in the comparison (53.4 × 40.0 mm) — superior SNR and dynamic range (15 stops). Lenses shipped with focus locked at infinity for aircraft vibration environments. Multi-camera synchronization bus: 8 cameras with 20–50 μs temporal deviation for oblique arrays.

### Sony ILX-LR1
Headless industrial camera — no viewfinder, LCD, grip, or internal battery. 243 g. Identical pixel pitch to Phase One (3.76 μm) but smaller total footprint → narrower swath → tighter line spacing. Unified power + control via Molex Micro-Fit (10–18 VDC, discrete Focus/Trigger/Exposure pins).

---

## 2. Survey-Specific Camera Dynamics

### 2.1 Forward Motion Compensation (FMC)
Image smear on the sensor:
```
smear_mm = V_ground × t_exposure × f / AGL
```
If smear > pixel pitch → motion blur → degrades BBA tie-point matching.

**Modern mitigation (CMOS sensors, no TDI):**
- Ultra-fast leaf shutters (1/2500–1/4000 s) reduce t_exposure to sub-millisecond.
- Gyrostabilized mounts (e.g., Somag GSM 4000) pitch camera backward by micro-radians during exposure.
- The FMS calculates maximum allowable ground speed for the selected shutter/altitude/focal length. If smear exceeds pixel pitch → throttle airspeed or alert operator.

### 2.2 Rolling Shutter → Non-Dismissable Warning
Electronic rolling shutters read the CMOS row-by-row. At 50–80 m/s, the top and bottom of the image represent different 3D coordinates. **Violates the collinearity equation** (perspective center smeared across a vector, not frozen at a point).

IronTrack: if a rolling-shutter camera is selected for survey-grade PPK, the FMS throws a **non-dismissable warning** and refuses to silently authorize the plan. Mechanical global shutters (Phase One, Hasselblad, DJI P1) are mandatory for ASPRS survey-grade work.

### 2.3 Camera Calibration: Navigational vs Analytical Boundary
Interior Orientation parameters (Brown-Conrady model):
- Principal distance (calibrated focal length, ≠ nominal)
- Principal point offset (xp, yp)
- Radial distortion (K1, K2, K3)
- Tangential distortion (P1, P2)

**IronTrack does NOT store full calibration certificates.** For flight planning and spatial triggering, only nominal focal length, sensor dimensions, and pixel pitch are needed. Radial/tangential distortion has negligible impact on macro-level trajectory or UTM trigger coordinates. The Brown-Conrady model is applied by downstream analytical software (Pix4D, Metashape, TBC) during self-calibrating BBA. Storing calibration arrays in the GeoPackage sensor library would bloat the payload and risk double-corrections.

---

## 3. Small-Format UAV Cameras

### DJI Zenmuse P1
45 MP full-frame (35.9 × 24.0 mm), 4.4 μm pixel pitch. **Global mechanical leaf shutter** — satisfies collinearity. Interchangeable DJI DL mount lenses (24/35/50 mm). Triggering is entirely via the proprietary **DJI Payload SDK (PSDK)** — not GPIO. PSDK uses high-speed UART at 460,800 baud through SkyPort/E-Port connections, with proprietary C-struct command encapsulation. DJI maintains an internal classified time-sync protocol between P1 shutter and the drone's RTK clock.

### Sony α7R IV
61 MP full-frame. No dedicated trigger terminal — interface via **ISO 518 hot shoe** or multi-terminal USB. For PPK with Emlid Reach M2 or u-blox F9P: hot shoe shorts center pin to ground rail on exposure → PPK receiver logs the time-mark. Must use optocoupler or N-channel MOSFET to safely pull the center pin (legacy flash units pushed hundreds of volts through the hot shoe — would fry CMOS logic).

---

## 4. Trigger Interface Catalog

### 4.1 Hardware GPIO (Phase One LEMO)
- Pin 4 (Purple): Trigger In, active-low.
- V_IL < 0.8V to register trigger, V_IH > 2.4V to reset.
- Rise/fall time < 1 μs for sharp temporal resolution.
- Hold low for 10–15 ms minimum (debounce).
- **Opto-isolation mandatory** between MCU and camera (aircraft dirty bus).

### 4.2 Hardware GPIO (Sony Molex Micro-Fit 3.0)
- Pin 5: TRIGGER, active-low (0V = fire, open = idle).
- Pin 4: FOCUS — tie permanently low for infinity-locked aerial lenses.
- Pin 6: EXPOSURE output (open-drain, requires external pull-up resistor).

### 4.3 Hot Shoe (ISO 518)
- Center sync contact + ground rail. Short to trigger.
- ~5V logic, <10 mA current draw.
- Must use optocoupler/MOSFET — never direct GPIO (legacy thyristor flash risk).

### 4.4 MAVLink (ArduPilot/PX4)
- `MAV_CMD_DO_SET_CAM_TRIGG_DIST` (Message 206): spatial intervalometry.
- Autopilot internal 400 Hz loop → 2.5 ms polling → ~3.75 cm error at 15 m/s.
- **Acceptable for slow UAVs (≤15 m/s). Rejected for manned fixed-wing (80 m/s).**

### 4.5 DJI PSDK
- UART at 460,800 baud through SkyPort/E-Port.
- Trigger command encapsulated in proprietary C-struct on "Command Signaling Channel."
- Software abstraction layers add latency, mitigated by DJI's internal RTK-shutter time sync.

### 4.6 USB/SDK (Phase One Capture One, Sony Remote)
- Full camera control (ISO, shutter, aperture) for ground configuration.
- **USB triggering has unacceptable polling latency** — reserved for session config and image offloading only. Mid-flight triggering always delegated to hardware GPIO.

---

## 5. Event Feedback and PPK Timing

### 5.1 Shutter Lag Problem
Electromechanical delay between trigger command and physical shutter opening: 10–50 ms depending on temperature, mechanical wear, spring fatigue. At 80 m/s, 50 ms = 4 m spatial error. **Command timestamp ≠ exposure timestamp.**

### 5.2 Open-Loop vs Closed-Loop

| Architecture | Mechanism | Accuracy | Limitation |
|---|---|---|---|
| **Open-loop** | Trigger controller splits pulse: one to camera, one to GNSS event pin. | Timestamps the command, not the exposure. | Blind to shutter lag (10–50 ms). Post-processing must apply assumed lag correction. |
| **Closed-loop** (gold standard) | Camera generates MEP at actual shutter opening → routed to GNSS event pin. | Timestamps the actual exposure. | Requires camera with dedicated feedback output. |

### 5.3 Mid-Exposure Pulse (MEP) Signals

**Phase One iXM:** Pin 7 (Light Trigger) on LEMO. Drops low when shutter opens, rises when it closes. T2 delay (trigger command → shutter open) = 20–30 ms. GNSS receiver latches on the **leading edge** of the MEP.

**Sony ILX-LR1:** Pin 6 (EXPOSURE) on Molex. **Open-drain output** — requires external pull-up resistor. When the front curtain fully opens, the internal transistor turns ON, pulling the voltage to 0V. GNSS receiver detects the **falling edge**. Pull-up voltage and resistance must satisfy the receiver's V_IL/V_IH thresholds.

### 5.4 EXIF Timestamps: DISQUALIFIED
Camera internal RTCs have integer-second resolution with rapid thermal drift. At 80 m/s, 1 second = 80 m spatial ambiguity. **EXIF is never acceptable for survey-grade PPK.**

### 5.5 PPS-Based Microsecond Timestamping
1. GNSS receiver outputs PPS (1 Hz, UTC-synchronized rising edge).
2. Trigger controller latches its internal microsecond timer on PPS.
3. When the camera's MEP arrives mid-second, the controller reads the timer delta.
4. Absolute UTC timestamp = PPS_time + timer_delta → **microsecond precision**.
5. Routed to the daemon, saved in the GeoPackage event log, combined with RINEX during post-processing.

---

## 6. IronTrack Sensor Library Schema Notes

The sensor library stores planning-relevant parameters only (not full calibration):
- name, manufacturer, sensor_type (camera/lidar)
- sensor_width_mm, sensor_height_mm, pixel_pitch_um
- resolution_x, resolution_y
- focal_lengths_mm (array of available options)
- shutter_type (mechanical_leaf / mechanical_focal_plane / electronic_rolling)
- max_trigger_rate_fps
- trigger_interface (lemo_gpio / molex_gpio / hot_shoe / mavlink / psdk)
- event_feedback (mep / exposure_open_drain / flash_sync / none)
- weight_kg

**Rolling shutter flag drives the non-dismissable warning when mission_type = survey_grade_ppk.**
