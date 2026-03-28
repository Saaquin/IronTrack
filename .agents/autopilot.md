# Autopilot Export & Trigger System Implementation Guide

**Source modules:** `src/io/` — `qgc.rs`, `dji.rs`; `src/network/` — `nmea.rs`, `serial_manager.rs`
**Source docs:** 11, 22, 24, 29, 40, 41, 48, 51

## Altitude Datum Conversion (Safety-Critical)
| Platform | Expected Datum | Conversion |
|----------|---------------|------------|
| ArduPilot (frame 3) | MSL | H_egm2008 direct |
| ArduPilot (frame 11) | AGL over SRTM | scalar offset |
| DJI WPML (EGM96) | EGM96 orthometric | H_egm96 = H_egm2008 + N_egm2008 - N_egm96 |
| DJI WPML (WGS84) | WGS84 ellipsoidal | h = H_egm2008 + N_egm2008 |
| IronTrack Trigger | UTM i32 (×10⁷) | Pre-computed WGS84 Ellipsoidal |

## Trigger Controller Serial Protocol (v0.8) [Docs 22, 29]
STM32/RP2040 bare-metal. ESP32 and Raspberry Pi DISQUALIFIED (GPIO jitter).
Binary frame: magic 0xAA55, MSG_TYPE, PAYLOAD_LEN, PAYLOAD, CRC-32.
7 message types: UPLOAD_WAYPOINTS, ACK, START_LINE, STOP_LINE, TRIGGER_EVENT, STATUS, ERROR.
Baud 115200+ (upload), 230400+ (NMEA UART). RTS/CTS MANDATORY. CP2102/FTDI only (CH340 rejected).
Chunked 512-byte uploads with per-chunk ACK for 15K LiDAR waypoints (120 KB).

## Sensor Trigger Interfaces [Doc 40]
- Phase One LEMO: Pin 4 active-low, V_IL<0.8V, V_IH>2.4V, rise/fall <1µs, hold 10-15ms
- Sony Molex Micro-Fit: Pin 5 TRIGGER active-low, Pin 6 EXPOSURE open-drain (needs pull-up)
- Hot shoe ISO 518: optocoupler/MOSFET mandatory (legacy thyristor voltage risk)
- MAVLink MSG 206: acceptable ≤15 m/s UAV speed, rejected for manned 80 m/s
- DJI PSDK: 460,800 baud, proprietary C-struct on "Command Signaling Channel"
- USB SDK: ground config only — polling latency unacceptable for mid-flight triggering

## LiDAR Dual-Trigger Architecture [Doc 41]
LiDAR does NOT need external trigger pulses (continuous emission). "Triggering" = TCP
start/stop at line boundaries. Cameras need discrete spatial GPIO. For hybrid payloads:
daemon sends TCP start/stop to LiDAR, trigger controller fires camera GPIO autonomously.

## Event Feedback and PPK [Docs 24, 40]
Closed-loop (gold standard): camera MEP → GNSS event pin (timestamps actual exposure).
Open-loop (fallback): trigger controller splits pulse to camera + GNSS simultaneously.
Phase One MEP: Pin 7, T2 delay 20-30ms. Sony EXPOSURE: Pin 6 open-drain.
EXIF timestamps DISQUALIFIED (integer-second = 80m ambiguity at fixed-wing speeds).
PPS-based µs timestamping: PPS latch → MEP delta → absolute UTC.

## Avionics Hardware Integration [Doc 48]
Power: synchronous buck from 14V/28V/LiPo bus. TVS SMDJ33A (clamps 53.3V, downstream
buck must be ≥60V). Critically damped LC filter. Hold-up capacitance for 50ms-1s dropout.
Grounding: FAA AC 43.13-1B <2.5mΩ, star ground, opto-isolation on GPIO.
Connectors: MIL-DTL-38999 III (primary), Fischer Core IP68 (external), DB-9 (serial).

## Proprietary SDK Isolation [Doc 51]
Phase One/DJI SDKs have restrictive EULAs. IPC Bridge Pattern: separate plugin daemon
links against SDK, communicates with GPLv3 core over TCP/gRPC. No copyleft propagation.
