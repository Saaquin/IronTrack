# Precision Kinematics and Event Synchronization in Survey-Grade Aerial Photogrammetry

> **Source document:** 24_Survey-Photogrammetry-PPK.docx
>
> **Scope:** Complete extraction — all content is unique to the project. Covers the full PPK workflow, event marking hardware, shutter physics, RINEX isolation boundary, and processing suite integration formats.

---

## 1. The Post-Processed Kinematic (PPK) Workflow

### 1.1 Why PPK over RTK for Aerial Survey

RTK delivers real-time centimeter corrections via a continuous radio/NTRIP link from base to rover. But in dynamic aerial environments (banking, urban canyons, RF interference, canopy), the link frequently degrades or drops. Loss of the RTK data link instantly reverts the solution to float or autonomous, introducing multi-meter errors that corrupt the photogrammetric block.

PPK decouples the differential correction from the flight. Both base and rover log raw observables independently. No real-time link is required — the aircraft can operate BVLOS or in extreme terrain without jeopardizing positional integrity.

| Attribute | RTK | PPK |
|---|---|---|
| Correction timing | Real-time during flight | Post-flight via software |
| Data link dependency | Absolute — failure degrades solution | None — resilient to all link failures |
| Ephemeris data | Broadcast only | Broadcast + rapid + precise |
| Trajectory filtering | Forward only (causal) | Forward + backward smoothing (non-causal) |
| Absolute accuracy | 2–5 cm (link-dependent) | **1–3 cm** (consistent across flight envelope) |

### 1.2 Independent Raw GNSS Observable Logging

Both base station and rover independently log raw carrier phase measurements, pseudoranges, Doppler shifts, and signal-to-noise ratios across multiple constellations (GPS, GLONASS, Galileo, BeiDou) and frequencies (L1, L2, L5).

The base station sits on a known geodetic monument (or uses a CORS network). It records the exact atmospheric interference, ionospheric delays, tropospheric refraction, and satellite clock drifts within the survey envelope. The rover on the aircraft records identical uncorrected observations alongside its dynamic trajectory.

### 1.3 Event Marking

Logging raw observables is insufficient unless the precise aircraft position at the exact moment of each camera exposure can be extracted. This synchronization is achieved through **event marking**: when the camera fires, an electrical pulse is routed to the GNSS rover receiver. The receiver logs the exact GNSS time of that pulse — the event epoch — directly into its raw observation file.

### 1.4 Post-Flight Trajectory Interpolation

Post-processing software ingests base + rover logs and applies:

1. **Precise satellite ephemerides** (not broadcast) for orbital accuracy.
2. **Forward + backward Kalman filtering** to bridge brief GNSS outages (banking, obstructions).
3. **High-order interpolation** along the corrected trajectory to extract exact coordinates (lat, lon, ellipsoidal height) at each discrete event timestamp.

Events rarely align with the receiver's standard 10/20 Hz output epochs — the software interpolates between epochs to find the sub-millisecond position.

---

## 2. Event Marking Precision Requirements

### 2.1 The Velocity-Time-Error Relationship

At 100 m/s (typical fixed-wing survey speed), the aircraft travels **100 mm per millisecond**. A 1 ms timestamp error injects 10 cm of along-track spatial error. For ASPRS survey-grade accuracies of 1–3 cm, millisecond jitter is fundamentally unacceptable.

**Required temporal accuracy: sub-microsecond (< 1 μs).**

### 2.2 Hardware Event Timestamp Generation

Sub-microsecond precision requires hardware-level electrical interaction, bypassing all OS scheduling:

1. The trigger controller fires a GPIO pulse to the camera.
2. A corresponding pulse reaches the GNSS receiver's dedicated event marker input pin.
3. The receiver's internal FPGA detects the voltage transition (e.g., 1.0V → 2.0V in < 100 ns).
4. The receiver's internal clock logs the GNSS epoch at the moment of detection — nanosecond-level precision, zero OS latency.

Survey-grade receivers (Trimble, Septentrio, NovAtel) have specific TTL event marker input pins designed for this purpose.

---

## 3. Pulse Routing: Open-Loop vs Closed-Loop

### 3.1 Closed-Loop (Hot-Shoe Feedback) — Gold Standard

1. IronTrack trigger controller sends the primary pulse to the camera.
2. The camera's internal circuit generates a secondary feedback pulse at the **exact moment the physical shutter is fully open** (true mid-exposure point).
3. This camera-generated pulse is routed directly to the GNSS receiver's event input.

**Advantage:** Fundamentally bypasses all internal camera processing delays. The receiver timestamps the actual instant light strikes the sensor, not the command to start the exposure sequence.

**IronTrack responsibility:** Fire the camera only. The camera handles the event marking pulse.

**Supported by:** Phase One iXU-RS (mid-exposure pulse via LEMO connector), Hasselblad A6D, Vexcel imaging arrays, RTK-enabled DJI enterprise payloads.

### 3.2 Open-Loop (Split Pulse) — Fallback

When the camera lacks hot-shoe or electronic feedback:

1. IronTrack trigger controller outputs a single pulse.
2. The pulse is **electrically bifurcated at the hardware level**: one branch fires the camera, the parallel branch hits the GNSS receiver's event pin.
3. The receiver timestamps the moment the command was issued, not the moment the shutter opened.

**Limitation:** Blind to the camera's internal mechanical shutter lag (10–50 ms depending on model). This lag introduces a systematic, velocity-dependent along-track error.

**Compensation:** The shutter lag must be mathematically quantified (from camera spec sheets or calibration) and compensated during post-processing. IronTrack's sub-microsecond GPIO determinism ensures the split is virtually simultaneous — the only error is the camera's internal delay.

### 3.3 IronTrack's Pulse Output Decision Tree

- **Camera has hot-shoe/mid-exposure feedback?** → Closed-loop. IronTrack fires camera only. Camera handles receiver event marking.
- **Camera lacks feedback?** → Open-loop. IronTrack must output two simultaneous pulses: one to camera, one to GNSS receiver event input. Both paths must be opto-isolated (Doc 22 §5.3).

---

## 4. Shutter Physics: Rolling vs Global

### 4.1 Rolling Shutter — Violates Collinearity

Electronic rolling shutters expose the sensor sequentially, row by row. At 80 m/s, the aircraft's position and attitude change continuously during readout. The top of the image is captured at a different geographic coordinate than the bottom.

**Mathematical consequence:** The collinearity equation requires that the perspective center, the image point, and the ground point lie on a single straight line. A rolling shutter image has **thousands of slightly spatially offset perspective centers** (one per row), fundamentally violating the central projection model. This produces non-linear geometric distortion ("jello effect") that degrades the final surface model accuracy.

Software (Metashape, Pix4D) can partially compensate, but the mathematical uncertainty inherently limits achievable accuracy.

### 4.2 Mechanical Global Shutter — Preserves Collinearity

Physical blades or leaf mechanisms expose the entire sensor simultaneously. Every pixel integrates photons during the exact same temporal window. A single 3D coordinate and a single set of exterior orientation angles can be assigned to the principal point.

**Mandatory for ASPRS survey-grade work.**

| Characteristic | Rolling Shutter | Global Shutter |
|---|---|---|
| Exposure mechanism | Sequential row-by-row | Simultaneous full-sensor |
| Motion distortion | High (jello effect) | Immune |
| Perspective centers | Thousands per frame | Single per frame |
| Collinearity compliance | Violated | Preserved |
| Survey suitability | Low-speed visual mapping only | **Mandatory for survey-grade PPK** |

### 4.3 IronTrack Sensor Library Implication

The sensor library (Doc 45) should flag rolling-shutter cameras with a warning when the mission type is set to survey-grade PPK work. The warning should not prevent use (the operator may have valid reasons) but should be non-dismissable in the export metadata.

---

## 5. Mid-Exposure Correction

### 5.1 The Problem

The GNSS event timestamp corresponds to the start of exposure (or the command pulse in open-loop mode). But the geometrically correct position is where the aircraft was at the **temporal center** of the exposure.

**Worked example:** At 80 m/s with 1/500s shutter (2 ms exposure), the aircraft moves 16 cm during the exposure. The mid-exposure position is 8 cm along-track from the event mark position.

### 5.2 The Correction Formula

```
T_mid = T_event + (shutter_duration / 2)
```

The PPK processing software:

1. Reads `T_event` from the GNSS receiver's event log.
2. Reads `shutter_duration` from the image EXIF metadata (or direct electronic feedback).
3. Computes `T_mid`.
4. Interpolates the differentially corrected trajectory (100–200 Hz) to extract the exact lat/lon/height at `T_mid`.

### 5.3 IronTrack's Responsibility Boundary

**Mid-exposure correction is NOT IronTrack's responsibility.** The trigger controller operates in real-time and is unaware of the camera's exposure settings or auto-exposure variations. The correction is deferred entirely to post-processing software, which has access to the EXIF metadata and the full corrected trajectory.

This deliberate separation ensures that dynamic auto-exposure changes (varying terrain albedo → varying shutter speeds) don't corrupt the real-time trigger timing.

---

## 6. RINEX: IronTrack's Isolation Boundary

### 6.1 What RINEX Is

Receiver Independent Exchange Format — the international open standard for storing raw GNSS observables. A PPK dataset requires:

- **Observation file** (.obs/.xxo): raw pseudoranges, carrier phases, Doppler shifts, SNR at every logging epoch.
- **Navigation file** (.nav/.xxn): satellite ephemeris data (orbital positions, velocities, clock health).

### 6.2 IronTrack Does Not Touch RINEX

**This is a deliberate, non-negotiable architectural boundary.**

Raw GNSS logging at 10–20 Hz is computationally intensive and handled by the receiver's internal firmware and ASICs. Receivers log to proprietary compressed binary formats (NovAtel .GPB, Trimble .T02/.T04, u-blox .UBX). Post-flight, these are converted to RINEX using manufacturer utilities (NovAtel Convert, Trimble Convert to RINEX, RTKLIB).

IronTrack's responsibilities are entirely decoupled from this pipeline:

- IronTrack reads processed real-time NMEA (GGA/RMC) for navigation and dead-reckoning.
- IronTrack fires GPIO trigger pulses.
- The GNSS receiver independently detects the trigger pulse on its event pin, timestamps it, and injects the event into its internal binary log.
- The event timestamps propagate into the RINEX observation file during post-flight conversion — **no IronTrack intervention required**.

### 6.3 Why This Boundary Matters

Attempting to have IronTrack parse, generate, or modify RINEX would:

- Duplicate functionality that the receiver hardware already handles with nanosecond precision.
- Introduce a massive, complex file format dependency with no operational benefit.
- Create a potential corruption vector for survey-critical raw observation data.

---

## 7. Integration with PPK Processing Suites

### 7.1 Trimble Business Center (TBC)

- **Input formats:** Trimble .T02 or universal RINEX.
- **Event marking:** Searches for time tags in the raw observation file. For non-Trimble platforms, accepts `.MRK` files (ASCII, exposure timestamps + sequence numbers) imported alongside .OBS files.
- **Custom integration:** Supports custom `.CSV` import via the Import Format Editor. Schema: event point ID, UTC timestamp, approximate lat/lon/elev.
- **Processing:** Interpolates the continuous kinematic trajectory at each event timestamp, displays events as diamond symbols along the flight path.

### 7.2 NovAtel Waypoint Inertial Explorer

- **Input formats:** NovAtel native binary → Waypoint generic format.
- **Event marking:** Uses `.STA` (Station File) format with C-like bracketed syntax:

```
Mrk {
  *Event: 001
  Desc: "Camera_Trigger"
  *GTim: 345600.123456789 2250
  Pos: 49.2827 -123.1207 105.45 ELL
  Std: 0.015 0.015 0.030
  Vel: 55.2 12.1 0.05
}
```

- **Critical fields:** `*Event` (sequence number) and `*GTim` (GPS seconds of week + GPS week number).
- **Time standard:** GPS Time strongly preferred over UTC (avoids leap-second complications).
- **Custom integration:** If events aren't in the native binary log, provide an ASCII file with exact Mrk syntax.

### 7.3 Emlid Studio

- **Input formats:** Base RINEX + Rover RINEX + timestamp file.
- **Event marking:** Ingests DJI `.MRK` files natively, or a simple `.CSV` with image sequence number + UTC timestamp.
- **Processing:** Matches timestamps against the corrected trajectory, outputs `events.pos` with high-accuracy coordinates for each image.
- **Downstream:** Output formatted for direct ingestion into Pix4D or Agisoft Metashape.

### 7.4 Summary Table

| Processing Suite | Event File Format | Key Fields | Time Standard |
|---|---|---|---|
| Trimble Business Center | .MRK or custom .CSV | Sequence ID, Time, Lat, Lon, Elev | GPS Time or UTC |
| NovAtel Inertial Explorer | .STA (Station File) | Mrk block: *Event, *GTim, Pos | GPS Time (strictly preferred) |
| Emlid Studio | .MRK or .CSV | Image name/number, Timestamp | GPS Time or UTC |

---

## 8. IronTrack Event Log Export

### 8.1 Mandate

IronTrack must export a supplementary event log as a redundancy layer, independent of the GNSS receiver's hardware event marking.

### 8.2 Format

Simple CSV:

```
event_number,utc_timestamp,approximate_lat,approximate_lon,approximate_alt,fix_quality
001,2026-07-15T14:23:01.123456,48.1172,-122.7634,152.3,4
002,2026-07-15T14:23:01.287654,48.1173,-122.7631,152.1,4
```

### 8.3 Precision Caveat

The UTC timestamps from IronTrack's NMEA parsing engine lack the nanosecond precision of the GNSS receiver's hardware event pin. These are approximate positions derived from the dead-reckoning engine at the moment of firing.

### 8.4 Operational Value

Despite lower precision, the IronTrack event log enables:

1. **Sequence verification:** Confirm image count matches trigger count.
2. **Missed frame detection:** Identify gaps where the camera failed to respond.
3. **Data salvage:** If the hardware event link fails (cable disconnected, receiver log corrupted), approximate trajectory matching can salvage expensive flight data.
4. **Processing suite import:** The CSV maps directly into TBC custom import templates, can be converted to NovAtel .STA blocks via a simple script, or dropped into Emlid Studio.

---

## 9. ASPRS Positional Accuracy Standards (2024 Edition 2)

### 9.1 Key Changes from Legacy Standards

- Replaces map-scale-based standards (NMAS 1947) with a sensor-agnostic, digital-first framework.
- Uses **RMSE exclusively** to quantify accuracy — eliminates the ambiguous 95% confidence metrics.
- Relaxes GCP requirements when highly precise PPK is employed.

### 9.2 Implications for IronTrack

The ASPRS framework validates the IronTrack architecture: with sub-microsecond event marking, mechanical global shutters, and PPK post-processing, the system can achieve the 1–3 cm accuracies required by the highest ASPRS accuracy classes **without dense GCP arrays**. IronTrack should present achievable ASPRS accuracy class to the operator based on the selected sensor, flight altitude, and processing method.
