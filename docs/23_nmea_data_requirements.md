# GNSS Telemetry Ingestion, Protocol Engineering, and Spatial Precision Analysis

> **Source document:** 23_NMEA-Data-Requirements.docx
>
> **Scope:** Complete extraction — all content is unique to the project. Doc 22 covers firmware-level NMEA parsing (FSM, checksum, state machine); this doc covers the protocol layer above it: sentence catalog, RTK degradation logic, baud rate math, binary alternatives, PPS timing, and dual-antenna heading.

---

## 1. NMEA 0183 Protocol Foundations

### 1.1 Sentence Structure

All NMEA sentences follow this rigid format:

```
$TTFFF,field1,field2,...,fieldN*CC<CR><LF>
```

| Component | Bytes | Description |
|---|---|---|
| `$` | 1 | Start delimiter (0x24). Encapsulated data uses `!` (0x21). |
| Talker ID | 2 | `GP` = GPS, `GL` = GLONASS, `GA` = Galileo, `GB`/`BD` = BeiDou, `GN` = multi-constellation fusion. |
| Formatter | 3 | Sentence type mnemonic (GGA, RMC, GSA, etc.). |
| Data fields | Variable | Comma-separated ASCII. Null fields = adjacent commas with no data (not zero). |
| `*` | 1 | Checksum delimiter (0x2A). |
| Checksum | 2 | Uppercase hexadecimal, 8-bit XOR of all bytes between `$` and `*` (exclusive). |
| Terminator | 2 | `<CR><LF>` (0x0D 0x0A). |

Maximum sentence length: **82 bytes** (including delimiters and terminator).

### 1.2 Checksum Algorithm

The checksum is computed by sequentially XOR-ing the ASCII byte values of all characters between (but excluding) `$` and `*`. The resulting 8-bit integer is split into two 4-bit nibbles and encoded as a two-digit uppercase hex string, MSB first.

**XOR vulnerability:** Because XOR is a parity-like operation, if the same bit position is flipped in two separate characters, the errors cancel and the checksum passes. The IronTrack parser must apply additional boundary checks: verify coordinates fall within planetary limits (lat ±90°, lon ±180°) and velocities remain within aerodynamic possibility.

---

## 2. NMEA Sentence Catalog for Aerial Survey

### 2.1 GGA — Global Positioning System Fix Data

Primary source for 3D coordinates and geodetic height.

| Field | Example | Format | Description |
|---|---|---|---|
| 0 | `$GNGGA` | String | Start + Talker ID + Formatter |
| 1 | `123519.00` | hhmmss.ss | UTC of position fix. Critical for PPK synchronization. |
| 2 | `4807.0381` | ddmm.mmmm | Latitude in degrees and decimal minutes |
| 3 | `N` | Char | Latitude hemisphere (N/S) |
| 4 | `01131.0001` | dddmm.mmmm | Longitude in degrees and decimal minutes |
| 5 | `E` | Char | Longitude hemisphere (E/W) |
| 6 | `4` | Int | **Fix Quality Indicator** (0–8). Dictates RTK state. |
| 7 | `12` | Int | Satellites in use |
| 8 | `0.9` | Float | HDOP (Horizontal Dilution of Precision) |
| 9 | `545.4` | Float | **Orthometric height** (MSL) in meters |
| 10 | `M` | Char | Altitude unit |
| 11 | `46.9` | Float | **Geoid separation** (geoid height above WGS84 ellipsoid) |
| 12 | `M` | Char | Geoid separation unit |
| 13 | `1.5` | Float | Age of differential correction (seconds) |
| 14 | `0000` | Int | Differential reference station ID |
| 15 | `*47` | Hex | Checksum |

**Geodetic height derivation:** IronTrack can extract the raw WGS84 ellipsoidal height by combining the orthometric height (Field 9) and geoid separation (Field 11), bypassing the receiver's internal low-resolution EGM96 grid in favor of IronTrack's native high-fidelity EGM2008 interpolation.

### 2.2 RMC — Recommended Minimum Specific GNSS Data

Primary source for kinematic vectors (speed, heading, time).

| Field | Example | Format | Description |
|---|---|---|---|
| 0 | `$GNRMC` | String | Start + Talker ID + Formatter |
| 1 | `210230.00` | hhmmss.ss | UTC time |
| 2 | `A` | Char | Status: A = Valid, V = Navigation Warning |
| 3 | `3855.4487` | ddmm.mmmm | Latitude |
| 4 | `N` | Char | Latitude hemisphere |
| 5 | `09446.0071` | dddmm.mmmm | Longitude |
| 6 | `W` | Char | Longitude hemisphere |
| 7 | `125.5` | Float | **Speed Over Ground** (knots) |
| 8 | `076.2` | Float | **Course Over Ground** (degrees true) |
| 9 | `130495` | ddmmyy | UTC date |
| 10 | `003.8` | Float | Magnetic variation |
| 11 | `E` | Char | Magnetic variation direction |
| 12 | `A` | Char | FAA Mode: A=Autonomous, D=Differential, F=Float RTK, R=RTK Fixed |
| 13 | `*69` | Hex | Checksum |

SOG (Field 7) and COG (Field 8) are the foundational inputs for the trigger controller's dead-reckoning engine. The controller converts SOG from knots to m/s to establish the linear extrapolation vector between NMEA epochs.

### 2.3 GSA — GNSS DOP and Active Satellites

Delivers real-time geometric confidence metrics.

| Field | Example | Format | Description |
|---|---|---|---|
| 0 | `$GNGSA` | String | Start + Talker ID + Formatter |
| 1 | `A` | Char | Mode 1: M=Manual, A=Auto 2D/3D selection |
| 2 | `3` | Int | Mode 2: 1=No Fix, 2=2D, 3=3D |
| 3–14 | `04,05,...` | Int | PRN numbers of satellites in solution (12 fields, null if unused) |
| 15 | `2.5` | Float | **PDOP** (Position Dilution of Precision) |
| 16 | `1.3` | Float | **HDOP** (Horizontal DOP) |
| 17 | `2.1` | Float | **VDOP** (Vertical DOP) |
| 18 | `*39` | Hex | Checksum |

**Operational use:** A VDOP spike directly correlates to vertical positioning drift. The daemon should trigger a glass cockpit warning when VDOP exceeds a configurable threshold, alerting the pilot that terrain-following altitude safety constraints may be compromised.

### 2.4 GSV — GNSS Satellites in View

Verbose constellation visibility data — satellite PRN, elevation, azimuth, and SNR.

| Field | Example | Format | Description |
|---|---|---|---|
| 0 | `$GPGSV` | String | Start + Talker ID + Formatter |
| 1 | `3` | Int | Total messages in sequence |
| 2 | `1` | Int | Current message number |
| 3 | `11` | Int | Total satellites in view |
| 4–7 | `08,74,042,45` | Int×4 | Satellite 1: PRN, Elevation (°), Azimuth (°), SNR (dB) |
| 8–15 | `...` | Int×4 | Satellites 2, 3, 4 (same format) |
| 16 | `*77` | Hex | Checksum |

**Critical constraint:** GSV supports a maximum of 4 satellites per sentence. A modern multi-constellation receiver tracking 24 satellites requires **6 consecutive GSV sentences per constellation**. At 100 Hz, this generates catastrophic UART bandwidth consumption. **GSV must be disabled on the UART port feeding the trigger controller** to prevent buffer overflow. The daemon can optionally parse GSV on a separate, lower-rate port for diagnostic display.

### 2.5 VTG — Course Over Ground and Ground Speed

| Field | Example | Format | Description |
|---|---|---|---|
| 0 | `$GNVTG` | String | Start + Talker ID + Formatter |
| 1 | `140.88` | Float | COG relative to True North |
| 2 | `T` | Char | True North reference |
| 3 | `135.50` | Float | COG relative to Magnetic North |
| 4 | `M` | Char | Magnetic reference |
| 5 | `8.04` | Float | Speed (knots) |
| 6 | `N` | Char | Knots unit |
| 7 | `14.89` | Float | Speed (km/h) |
| 8 | `K` | Char | Km/h unit |
| 9 | `D` | Char | FAA Mode indicator |
| 10 | `*05` | Hex | Checksum |

**Redundancy rule:** VTG is largely redundant if RMC is successfully parsed (both provide SOG and COG). The trigger controller firmware should ignore VTG when RMC is available to save embedded CPU cycles. VTG is retained as a fallback for legacy autopilot integrations.

### 2.6 Trimble Proprietary: PTNL,GGK

Extended-precision sentence that exceeds the 82-byte NMEA limit. Provides native ellipsoidal heights and 8-decimal-place coordinate resolution.

| Field | Example | Format | Description |
|---|---|---|---|
| 0 | `$PTNL` | String | Proprietary Talker ID |
| 1 | `GGK` | String | Message ID |
| 2 | `102939.00` | hhmmss.ss | UTC time |
| 3 | `051910` | ddmmyy | UTC date |
| 4 | `5000.97323841` | ddmm.mmmmmmmm | Latitude — **8 decimal places** of minutes (~0.018 mm resolution) |
| 5 | `N` | Char | Latitude hemisphere |
| 6 | `00827.62010742` | dddmm.mmmmmmmm | Longitude — 8 decimal places |
| 7 | `E` | Char | Longitude hemisphere |
| 8 | `3` | Int | **Trimble RTK Quality** (3=RTK Fix, 2=RTK Float — differs from GGA!) |
| 9 | `09` | Int | Satellites in fix |
| 10 | `1.9` | Float | PDOP |
| 11 | `EHT150.790` | String+Float | **Ellipsoidal height** (WGS84), prefixed with `EHT` |
| 12 | `M` | Char | Unit |
| 13 | `*73` | Hex | Checksum |

**Operational advantages for IronTrack:**

1. **Native ellipsoidal height** (`EHT` prefix) — no internal orthometric conversion needed, ensuring geodetic consistency with the planning engine.
2. **8 decimal places of minutes** — eliminates quantization errors from standard 5-decimal GGA.
3. **Warning:** Trimble quality codes differ from standard GGA: Trimble `3` = RTK Fixed (vs GGA `4`), Trimble `2` = RTK Float (vs GGA `5`). The parser must use receiver-specific lookup tables.

**Priority rule:** If IronTrack detects a Trimble receiver (by the `$PTNL` talker ID), the daemon must prioritize PTNL,GGK over standard GGA.

---

## 3. RTK Fix Quality and Degradation Protocol

### 3.1 Standard GGA Quality Codes

| Code | Name | Accuracy | Operational Meaning |
|---|---|---|---|
| 0 | Invalid | No fix | No GNSS solution. Insufficient satellite geometry. |
| 1 | GPS/SPS | 3–10 m | Autonomous, no corrections. Inadequate for survey. |
| 2 | DGPS/SBAS | 0.5–2 m | Pseudo-range corrections (WAAS/EGNOS). Navigation-grade only. |
| 4 | **RTK Fixed** | **1–3 cm** | Carrier-phase integers resolved. Survey-grade. |
| 5 | RTK Float | 0.2–5 m | Carrier-phase integers unresolved. **Unpredictable constant bias.** |
| 6 | Dead Reckoning | Exponential drift | GNSS lost, IMU-only estimation. |

### 3.2 Float Solution Danger: Invisible Constant Bias

An RTK Float solution (Code 5) is operationally deceptive. It may appear smooth and stable on a trajectory map, but it can mask a **constant, invisible directional bias of up to 5 meters**. Because Float errors manifest as a constant offset rather than visible jitter, the pilot has no immediate visual cue that the acquired data is corrupted.

### 3.3 Degradation Response Protocol

**Daemon response:** When the GGA Quality Indicator drops from 4 to 5 (or lower) during an active survey line, the glass cockpit must immediately flash an amber/red warning advising the pilot to abort the line, perform a teardrop maneuver, and re-fly the segment once a Fixed solution is reacquired.

**Trigger controller response:** The transition from Fixed (±2 cm) to Float (±50 cm) causes a sudden baseline shift between consecutive 10 Hz fixes. This corrupts the velocity derivative used by the dead-reckoning engine, causing the linear extrapolator to misjudge proximity to target waypoints. The camera fires early or late, shattering spatial overlap requirements.

When the controller detects a transition away from Code 4, it must:

1. **Pause dead-reckoning extrapolation** — do not trust the velocity vectors derived from corrupted position deltas.
2. **Widen the spatial firing radius** — compensate for increased positional uncertainty.
3. **Flag every subsequent GPIO pulse as geometrically compromised** — mark trigger events in the telemetry stream so post-processing software can identify suspect exposures.
4. **Resume normal operation** only when Code 4 returns and remains stable for a configurable number of consecutive epochs (e.g., 10 epochs = 1 second at 10 Hz).

---

## 4. High-Rate NMEA: Baud Rate Mathematics and Epoch Integrity

### 4.1 Bandwidth Proof

Standard UART uses 8N1 framing: 10 electrical bits per data byte (8 data + 1 start + 1 stop).

Minimum operational sentence set for the trigger controller: GGA (82 bytes) + RMC (82 bytes) = **164 bytes per epoch**.

At 100 Hz update rate:

```
Required throughput = 164 bytes × 100 Hz × 10 bits/byte = 164,000 bits/second
```

**115200 baud is insufficient.** Attempting to push 164,000 baud through a 115,200 baud pipe overflows the GNSS receiver's internal transmit buffer, causing dropped sentences, truncated packets, or `$GxTXT txbuf,alloc` errors (common in saturated u-blox receivers).

**Required baud rates:**

| Baud Rate | Capacity (bits/s) | Headroom at 100 Hz GGA+RMC | Verdict |
|---|---|---|---|
| 115,200 | 115,200 | **-30%** (overflow) | Insufficient |
| 230,400 | 230,400 | +28% | Minimum viable |
| 460,800 | 460,800 | +64% | Recommended |

At 460,800 baud, a new byte arrives every 21.7 μs. The trigger controller must use **DMA** to ingest the UART stream into a circular ring buffer without CPU intervention, preventing ISR starvation at these data rates.

### 4.2 Epoch Consistency and Sentence Interleaving

At high rates, NMEA is asynchronous and stream-based. When multiple sentences (GGA, RMC, GSA) are output within the same 10 ms epoch window, there is **no protocol guarantee** of sequence or atomic grouping.

**Interleaving hazard:** Under heavy CPU load, serial buffers can interleave sentences from epoch N with sentences from epoch N+1. The parser must never assume an arriving RMC belongs to the preceding GGA.

**Solution:** Buffer the parsed GGA coordinates and explicitly wait for an RMC bearing the **exact same UTC timestamp** before releasing the combined state to the dead-reckoning engine.

**Epoch collision hazard:** If verbose sentences like GSV (spanning 6+ consecutive 82-byte messages) are left enabled, their transmission time spills over the 10 ms epoch boundary, causing Epoch 1 data to still be transmitting when Epoch 2 computation completes. **All superfluous NMEA sentences must be disabled** on the trigger controller's UART port at receiver boot via binary configuration commands.

---

## 5. Binary Protocol Alternatives: UBX and GSOF

### 5.1 The Case Against ASCII at High Rates

NMEA encodes coordinates as ASCII strings. A double-precision coordinate like `4807.038156` requires 11 bytes. The equivalent UBX NAV-PVT message transmits lat/lon as raw 32-bit scaled integers in 4 bytes each. A complete UBX NAV-PVT payload (time, 3D position, 3D velocity, fix quality, satellite metrics) requires **92 bytes** vs **~164 bytes** for GGA+RMC.

### 5.2 Advantages of Binary (UBX/GSOF)

| Advantage | Detail |
|---|---|
| **Bandwidth compression** | ~44% reduction in serial traffic. Relaxes baud rate constraints. |
| **Parsing efficiency** | Cast incoming byte array directly into a packed Rust struct. No `atof()` float parsing (hundreds of CPU cycles per field). |
| **Error detection** | UBX uses 16-bit Fletcher checksum (Checksum A + Checksum B). Far more robust than NMEA's 8-bit XOR. GSOF uses CRC. |

### 5.3 UBX Protocol Structure

UBX messages begin with sync bytes `0xB5 0x62`, followed by class ID, message ID, payload length (2 bytes, little-endian), payload, and two checksum bytes. The parser must hunt for the sync sequence, validate payload length, and verify the Fletcher checksum before accepting data.

### 5.4 Architectural Decision

**NMEA 0183 must be retained as the universal fallback** — it is the only protocol guaranteed across all receivers. **UBX is mandated as the high-rate execution path** for the trigger controller when connected to a u-blox receiver. The firmware supports both protocols, auto-detecting based on the incoming byte stream (ASCII `$` vs binary `0xB5 0x62`).

NMEA 2000 (CAN-bus based) is unsuited for simple UART serial links and is excluded from trigger controller consideration.

---

## 6. Pulse Per Second (PPS) for PPK Event Marking

### 6.1 The Timing Problem

NMEA data arriving over serial UART is subject to transmission delays, buffer caching, and scheduling jitter. Depending on baud rate and processing load, the data describing a position may arrive at the controller **10–50 ms after the aircraft actually occupied that space**. This temporal ambiguity destroys PPK precision.

### 6.2 PPS Signal

Survey-grade GNSS receivers generate a **Pulse Per Second** — a TTL voltage spike with nanosecond-class rise time, physically synchronized to the absolute boundary of a UTC second.

### 6.3 Hardware Integration Flow

1. GNSS receiver drives the PPS pin HIGH at exactly `12:00:00.000 UTC`.
2. The MCU's **hardware interrupt** triggers instantly on the PPS rising edge, latching the CPU's high-resolution internal timer (microsecond cycle counter).
3. 5–20 ms later, the serial port finishes receiving the NMEA RMC/GGA sentence tagged with `12:00:00.00`.
4. The controller correlates the latched hardware timer value with the parsed UTC timestamp. The timer is now synchronized to absolute UTC.

### 6.4 Camera Event Timestamping

When the intervalometer fires the camera mid-second, it records the exact hardware timer value at the moment the GPIO trigger output goes HIGH. The absolute UTC timestamp of the exposure is:

```
T_exposure = T_last_PPS + (timer_at_fire - timer_at_PPS) / timer_frequency
```

This timestamp (microsecond precision) is sent back to the daemon via serial, stored in the GeoPackage event log, and used by PPK processing software to interpolate the exact sub-centimeter position of the camera at the moment of exposure.

**Without PPS hardware correlation**, the FMS is limited to the latency jitter of the serial bus — effectively destroying RTK/PPK precision for the final orthomosaic product.

### 6.5 Hardware Requirement

The trigger controller must dedicate a GPIO pin (configured as a high-priority external interrupt input) to the PPS signal. This is a separate pin from the NMEA UART RX and the camera trigger output. The PPS input path must be electrically isolated from aircraft noise sources using the same opto-isolation strategy as the camera trigger output (Doc 22 §5.3).

---

## 7. Multi-Receiver Configurations and Dual-Antenna Heading

### 7.1 The Heading Problem

Single-antenna RTK provides exceptional 3D positioning but **zero orientation data when stationary** and only kinematic heading (Course Over Ground) when moving. In crosswinds, the aircraft crabs into the wind — COG cannot reveal this crab angle. Only true aircraft heading (yaw) relative to the flight path reveals the geometric offset of the camera footprint.

### 7.2 Moving Baseline RTK

Dual-antenna configurations (e.g., Septentrio mosaic-H, dual u-blox F9P) deploy a Primary receiver computing absolute position globally and a Secondary receiver performing a Moving Baseline RTK calculation. The secondary resolves the exact millimeter-level vector (distance + angle) between the two antennas.

Because the antenna baseline is fixed to the rigid airframe, the resolved vector angle provides **true heading (yaw)** and, depending on alignment, pitch or roll — entirely immune to magnetic interference from avionics or airframe metals.

### 7.3 Trimble PTNL,AVR Sentence

The moving baseline vector is communicated via the proprietary AVR sentence:

| Field | Example | Format | Description |
|---|---|---|---|
| 0 | `$PTNL,AVR` | String | Message ID — moving baseline vector |
| 1 | `212405.20` | hhmmss.ss | UTC of vector fix |
| 2 | `+52.1531` | Float | **Yaw angle** (degrees) |
| 3 | `Yaw` | String | Identifier |
| 4 | `-0.0806` | Float | Tilt/Roll angle (degrees) |
| 8 | `12.575` | Float | Baseline range between antennas (meters) |
| 9 | `3` | Int | RTK Quality (3=Fixed, 2=Float, Trimble convention) |

### 7.4 Daemon Integration

The daemon must ingest the yaw data from AVR, contrast it with the RMC Course Over Ground, and dynamically compute and display the real-time **crab angle** on the glass cockpit:

```
crab_angle = yaw - COG
```

This feeds directly into the photogrammetric crab angle compensation system (effective swath width adjustment for flight line spacing).

### 7.5 Dual-Receiver Failover Logic

When two separate GNSS receivers stream telemetry to the daemon:

1. **Primary stream** feeds both the daemon and the trigger controller.
2. If the primary drops from RTK Fixed (Code 4) to Float (Code 5) while the secondary maintains Fixed, the daemon **seamlessly switches** its coordinate ingestion to the secondary.
3. The daemon applies the known physical baseline offset between the antennas to prevent coordinate jumping:

```
position_corrected = position_secondary - baseline_vector
```

4. When the primary reacquires Fixed, the daemon switches back.

This failover must be transparent to the glass cockpit — the pilot sees continuous, stable positioning regardless of which receiver is active.

---

## 8. Firmware Configuration Requirements

At boot, the trigger controller must issue binary configuration commands to the GNSS receiver:

1. **Set baud rate** to 230,400 or 460,800.
2. **Enable** only GGA and RMC on the trigger controller UART (plus PTNL,GGK if Trimble detected).
3. **Disable** GSA, GSV, VTG, and all other verbose sentences on this port.
4. **Set output rate** to 10–20 Hz for standard photogrammetry, or 100 Hz for high-speed LiDAR work.
5. **Enable PPS output** on the receiver's dedicated PPS pin.
6. **Optional:** Switch from NMEA to UBX binary protocol on this port for bandwidth efficiency.

The daemon's separate UART (or the same receiver on a second port) can optionally receive GSA, GSV, and VTG at a lower rate for diagnostic display on the glass cockpit.
