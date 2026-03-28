# Avionics Serial Communication and Precision Trigger System Protocols

> **Source document:** 29_Avionics-Serial-Communication.docx
>
> **Scope:** Complete extraction — all content is unique to the project. Covers UART fundamentals, flow control, USB-serial bridges, binary protocol framing, CRC selection, MAVLink hybrid architecture, Rust serial crates, VID/PID auto-detection, and hot-plug resilience.

---

## 1. UART Serial Fundamentals for Avionics

### 1.1 Baud Rate Selection

Standard UART uses 8-N-1 framing (1 start + 8 data + 1 stop = 10 bits per byte).

| Baud Rate | Throughput (bytes/s) | Time per Byte (μs) | Avionics Application |
|---|---|---|---|
| 9600 | 960 | 1041.6 | Legacy NMEA 0183, basic telemetry. Resilient over long unshielded runs. Insufficient for binary payloads. |
| 38400 | 3,840 | 260.4 | High-speed NMEA, AIS transponders. Adequate for 10 Hz GNSS without overwhelming 8-bit MCUs. |
| 57600 | 5,760 | 173.6 | Default for MAVLink telemetry radios (3DR SiK). Balances range and throughput for UAV GCS links. |
| 115200 | 11,520 | 86.8 | Modern standard for USB-serial, RTK GNSS, high-speed MAVLink. Optimal for short intra-fuselage wiring. |
| 230400 | 23,040 | 43.4 | Required for high-frequency IMU logging and rapid waypoint array uploads to LiDAR intervalometers. |

**Clock synchronization:** At 115200 baud, each bit occupies ~8.68 μs. The receiver oversamples at 16× the baud rate to locate bit centers. If transmitter and receiver clocks drift by > 2–5%, framing errors and data corruption occur.

**IronTrack requirement:** 115200 or 230400 baud for waypoint upload to the trigger controller. Lower rates introduce unacceptable serialization latency before a flight line.

### 1.2 Flow Control: RTS/CTS Hardware (Mandatory)

When the daemon uploads thousands of waypoints to an MCU with limited SRAM, flow control prevents receiver buffer overflow.

**Software flow control (XON/XOFF) — rejected:**
- Uses in-band ASCII control bytes (XON = 0x11, XOFF = 0x13) inserted into the data stream.
- **Fatal flaw for binary protocols:** The byte values 0x11 and 0x13 naturally occur in waypoint payloads. The UART hardware falsely interprets them as flow control commands, halting transmission and corrupting the array.
- Mitigation (byte-stuffing) bloats payload, wastes bandwidth, and increases MCU computational load.
- Subject to software latency — by the time the host processes an XOFF, the FIFO may already have overflowed.

**Hardware flow control (RTS/CTS) — mandated:**
- Dedicated physical signaling lines, entirely out-of-band.
- When the receiver's buffer hits a high-water mark, it toggles the RTS voltage. The transmitting UART halts at the hardware level, instantaneously, independent of the host CPU.
- Permits pure, unescaped binary transmission with zero CPU overhead.
- Guarantees zero data loss at 230400 baud in EMI-heavy aircraft environments.

**No flow control — prohibited for bulk transfers.** If the MCU is delayed by a high-priority interrupt (firing a trigger pulse, parsing NMEA), the UART buffer overruns and the CRC fails for the entire upload.

### 1.3 USB-to-Serial Bridge Chipsets

| Chipset | Manufacturer | RX/TX Buffer | Avionics Suitability |
|---|---|---|---|
| **FT232RL (FTDI)** | FTDI | 256/128 bytes | **Gold standard.** Native drivers across Linux, macOS, Windows. Genuine silicon is rock-solid. |
| **CP2102 / CP2104** | Silicon Labs | **512/512 bytes** | **Recommended.** Massive buffers mitigate data loss at high speed. Full RTS/CTS support. Stable drivers on all platforms. |
| CH340 / CH341 | WCH | ~128 bytes | **Rejected for field deployment.** macOS driver is catastrophic on Apple Silicon — kernel panics, port enumeration failures, SIP bypass required. |

**IronTrack mandate:** CP2102 or FTDI adapters only. CH340 introduces OS-level driver conflicts that are unacceptable in field deployment.

---

## 2. Binary Protocol Design

### 2.1 Frame Structure

| Byte Offset | Field | Type | Purpose |
|---|---|---|---|
| 0–1 | HEADER | u16 | Magic bytes (0xAA 0x55) for frame synchronization |
| 2 | MSG_TYPE | u8 | Command/state enumeration |
| 3–4 | PAYLOAD_LEN | u16 | Exact byte length of the subsequent payload |
| 5..N | PAYLOAD | [u8] | Variable-length data (waypoints, status, events) |
| N+1..N+4 | CHECKSUM | u32 | CRC-32 integrity validation |

### 2.2 CRC-32 (Mandatory for Large Payloads)

| Algorithm | Register Size | Reliable Detection Range | IronTrack Decision |
|---|---|---|---|
| CRC-16-CCITT | 16-bit | ~4 KB max payload | Insufficient for dense LiDAR missions |
| **CRC-32 (IEEE 802.3)** | 32-bit | Multi-megabyte payloads | **Mandatory.** A 15,000-waypoint LiDAR upload exceeds 120 KB. |

Modern ARM MCUs (Cortex-M0+, Cortex-M4) have **hardware CRC-32 peripherals** — single-cycle computation per 32-bit word, zero software penalty.

### 2.3 Protocol State Machine: Seven Message Types

| Message | Direction | Description |
|---|---|---|
| `UPLOAD_WAYPOINTS` | Daemon → Controller | Chunked arrays of scaled i32 UTM coordinates. Controller verifies CRC-32, appends to SRAM buffer, awaits next chunk. |
| `ACK` | Controller → Daemon | Acknowledges successful CRC-validated receipt of a chunk. If no ACK within timeout, daemon retransmits. |
| `START_LINE` | Daemon → Controller | Arms the GPIO pin, begins the 1000 Hz dead-reckoning evaluation loop. Controller is now fully autonomous. |
| `STOP_LINE` | Daemon → Controller | Disarms GPIO, flushes waypoint buffer, returns to idle standby. |
| `TRIGGER_EVENT` | Controller → Daemon | Sent the instant a GPIO pulse fires. Contains sequence number, lat/lon, internal timestamp. Daemon relays to glass cockpit. |
| `STATUS` | Controller → Daemon | Periodic heartbeat: buffer capacity, GNSS lock status, flow control state, firmware health. |
| `ERROR` | Controller → Daemon | Critical failures: CRC mismatch, UART framing error, GNSS signal loss, SRAM allocation failure. |

### 2.4 Waypoint Encoding: Scaled i32 (MAVLink-Compatible)

f64 is too bandwidth-heavy for serial. f32 has only 7 significant digits (~1 m precision at planetary scale) — unacceptable for RTK/PPK.

**Solution: i32 scaled by 10⁷** (matches MAVLink MISSION_ITEM_INT):

```
lat_i32 = (lat_degrees × 10,000,000) as i32
lon_i32 = (lon_degrees × 10,000,000) as i32
```

A change of 1 in the i32 = 10⁻⁷ degrees ≈ **1.11 cm** at the equator. Max planetary longitude (180°) = 1,800,000,000 — fits within i32 range (±2,147,483,647).

**Bandwidth savings:** 8 bytes per waypoint (2 × i32) vs 16 bytes (2 × f64) = **50% reduction**.

### 2.5 Microcontroller RAM Constraints

15,000 LiDAR waypoints × 8 bytes = **120 KB** minimum buffer.

| MCU | SRAM | 15K Waypoints? | Notes |
|---|---|---|---|
| STM32F401 | 96 KB | **Impossible** | Total RAM < requirement |
| STM32F407 | 192 KB | **Marginal** | RTOS + stack + .bss may exhaust remaining memory |
| **RP2040** | **264 KB** | **Sufficient** | Requires custom linker scripts across interleaved memory banks |
| ESP32 (standard) | ~320 KB | **Insufficient (static)** | Max contiguous DRAM block limited to ~160 KB |
| ESP32-WROVER | 4–8 MB PSRAM | **Unlimited** | PSRAM via QSPI; internal SRAM reserved for GPIO ISRs |

**Architectural requirement:** The `UPLOAD_WAYPOINTS` protocol must enforce **chunked pagination** (e.g., 512-byte chunks with per-chunk ACK and CRC-32 validation). Even if the MCU can hold the full array, the UART DMA buffers cannot accept a 120 KB blast.

---

## 3. MAVLink: Complement, Not Replacement

### 3.1 MAVLink 2.0 Coordinate Compatibility

MAVLink `MISSION_ITEM_INT` (Message ID 73) uses i32 coordinates scaled by 10⁷ — identical to IronTrack's serial protocol encoding. Encapsulating terrain-following waypoints as MAVLink mission items is fully supported.

### 3.2 Why MAVLink Fails for High-Speed Triggering

`MAV_CMD_DO_SET_CAM_TRIGG_DIST` (Command ID 206) instructs the autopilot to fire a hardware pulse at spatial intervals. Jitter sources:

1. **Polling loop quantization:** Flight controllers run at 400 Hz. Trigger logic evaluated every 2.5 ms. The exact threshold crossing may not align with the tick.
2. **OS network stack:** If the command is passed from the daemon over TCP/UDP to a MAVLink router, kernel scheduling and socket buffering add 10–50 ms of non-deterministic jitter.
3. **Spatial error at speed:** At 80 m/s, 50 ms jitter = **4 meters** of positional error. Destroys photogrammetric overlap and LiDAR colorization.

### 3.3 The Hybrid Architecture Decision

| Platform | Trigger Mechanism | Rationale |
|---|---|---|
| **Slow UAV (5–15 m/s)** | MAVLink `MAV_CMD_DO_SET_CAM_TRIGG_DIST` via autopilot | Jitter ≤2.5 ms → ~3.75 cm at 15 m/s. Acceptable. |
| **Fast fixed-wing / manned (50–120 m/s)** | **Dedicated bare-metal trigger controller** via custom serial protocol | Jitter < 100 ns. Sub-centimeter precision. The only option for survey-grade work. |

The IronTrack daemon supports both paths. The operator selects the trigger mode based on the platform. The same waypoint computation pipeline feeds either MAVLink export or serial upload.

---

## 4. Rust Serial Port Implementation

### 4.1 Crate Architecture

| Crate | Role | API | Use in IronTrack |
|---|---|---|---|
| `serialport` | Device discovery, synchronous I/O | Blocking `Read`/`Write`, `available_ports()` for VID/PID enumeration | Hardware discovery at startup. Port enumeration after hot-plug. |
| `tokio-serial` | Async data path | `AsyncRead`/`AsyncWrite` via mio (epoll/kqueue/IOCP) | Runtime serial I/O integrated into the Tokio event loop. Non-blocking. |

Both are required: `serialport` for hardware discovery, `tokio-serial` for the async data path. This satisfies the Async Boundary Rule — serial I/O never blocks the Tokio executor.

### 4.2 VID/PID Auto-Detection (No Hardcoded Paths)

Hardcoding `/dev/ttyUSB0` or `COM3` is catastrophic — USB device enumeration order changes between reboots, potentially swapping the GNSS receiver and trigger controller paths.

**Solution:** The daemon maintains a configuration manifest mapping logical roles to hardware signatures:

```toml
[serial.trigger_controller]
vid = 0x10C4  # Silicon Labs CP2102
pid = 0xEA60
serial_number = "ABC123"

[serial.gnss_receiver]
vid = 0x1546  # u-blox
pid = 0x01A9
```

On startup, the daemon iterates `serialport::available_ports()`, inspects `UsbPortInfo` for each device, and binds the correct physical path by matching VID/PID/serial. This eliminates the risk of sending binary waypoints into the wrong avionics device.

### 4.3 Hot-Plug Resilience: 5-Step Recovery

Aircraft vibration and EMI cause USB brown-outs — microscopic disconnections that a naive daemon treats as fatal errors.

**Recovery sequence when `tokio_serial::SerialStream` returns `ConnectionReset` or `BrokenPipe`:**

1. **Drop the file descriptor** — explicitly drop the `SerialStream` to free the OS-level handle and allow kernel cleanup.
2. **Async backoff polling** — `tokio::time::sleep(Duration::from_millis(500))` without blocking the executor. Prevents CPU thrashing while the kernel re-enumerates the USB tree.
3. **Dynamic re-enumeration** — the reconnected device likely has a new node (e.g., `/dev/ttyUSB0` → `/dev/ttyUSB1`). Re-invoke `serialport::available_ports()` and match VID/PID to find the new path.
4. **Stream re-establishment** — instantiate a fresh `tokio_serial::SerialStream` on the new node. Resume the async read/write loop.
5. **State recovery** — the trigger controller has been firing autonomously during the daemon's blackout (pre-loaded waypoints, direct NMEA). Send a `STATUS` request. The controller responds with its current waypoint index + backlog of `TRIGGER_EVENT` messages. The daemon resyncs the glass cockpit without missing a single spatial data point.

**Alternative:** The `stream-reconnect` crate provides an `UnderlyingStream` trait that abstracts the entire recovery state machine. Implement `is_read_disconnect_error` and `establish()` to isolate physical-layer failures from the flight management logic.
