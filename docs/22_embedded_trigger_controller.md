# Embedded Trigger Controller: Hard Real-Time Architecture and Spatial Synchronization

> **Source document:** 22_Embedded-Trigger-Controller.docx
>
> **Scope:** Complete extraction — this document is entirely unique to the project. No content overlaps with existing Docs 01–21.

---

## 1. Hardware Platform Comparison for Deterministic GPIO Timing

The trigger controller must parse continuous NMEA serial data at 115200 baud while independently running a kilohertz-rate dead-reckoning loop and producing nanosecond-accurate GPIO pulses. This demands heterogeneous concurrent processing with minimal interrupt latency.

### 1.1 STM32 (ARM Cortex-M4/M7)

The STM32 family is the industry standard for deterministic industrial control. The Nested Vectored Interrupt Controller (NVIC) provides hardware-enforced interrupt latency strictly bounded to **12 clock cycles**. At 168 MHz, the time from hardware event assertion to ISR execution is approximately **71 nanoseconds**.

The DMA controllers offload NMEA byte transfers from the UART peripheral into circular memory buffers without a single cycle of CPU intervention. At 115200 baud, a new byte arrives approximately every 86 μs — without DMA, the CPU would be interrupted thousands of times per second just to move characters.

Hardware timers offer 32-bit resolution with configurable prescalers, enabling jitter-free microsecond pulse widths using hardware one-pulse modes independent of software execution. Active power consumption: **30–40 mA**.

### 1.2 ESP32 (Dual-Core Xtensa/RISC-V)

The ESP32 operates at up to 240 MHz but its integrated WiFi/Bluetooth radio stack destroys hard real-time determinism. The FreeRTOS scheduler must constantly service the proprietary, closed-source radio baseband interrupts.

Empirical GPIO interrupt latency measurements show worst-case jitter regularly spiking to **~10 milliseconds** as the OS yields to wireless tasks. Mitigation strategies (pinning the trigger task to core 1, forcing ISRs into IRAM) help but cannot eliminate the fundamental architecture prioritizing throughput over determinism. Active power: **50–160 mA** with radio engaged.

**Verdict:** For an isolated trigger controller communicating via serial cables, the ESP32 introduces profound timing risks with no operational benefit. **Disqualified.**

### 1.3 RP2040 (Raspberry Pi Pico)

Dual-core ARM Cortex-M0+ at 133 MHz. No hardware FPU, but the defining feature is **Programmable I/O (PIO) state machines** — independent hardware processors that execute bit-banging, protocol decoding, and pulse generation entirely decoupled from the main CPU cores.

GPIO actuation via PIO achieves **15–25 nanosecond** delay from software flag to pin transition — cycle-accurate timing.

**Caveat:** The RP2040 uses Execute-In-Place (XIP) from external QSPI flash. If the CPU fetches an instruction not in the XIP cache, the resulting memory stall introduces hundreds of nanoseconds of non-deterministic latency. All time-critical code (interrupts, NMEA parsing, dead-reckoning math) must be explicitly placed in the **264 KB of internal SRAM** via linker script annotations.

Active power: **18 mA**.

### 1.4 Raspberry Pi 4/5 with PREEMPT_RT Kernel

Full Linux application processor with the PREEMPT_RT patch. Empirical cyclictest benchmarking under heavy CPU/memory/IO load shows worst-case scheduling latency of approximately **100–150 microseconds** on a Pi 5.

At 80 m/s flight speed, 150 μs introduces 12 mm of positional uncertainty — statistically negligible for photogrammetry. But the overhead of maintaining a full Linux filesystem, boot sequences, and high-wattage power draw introduces severe reliability concerns compared to an instant-on, solid-state MCU.

**Verdict:** Viable as a fallback but not recommended. **Disqualified for primary trigger role.**

### 1.5 Hardware Evaluation Summary

| Specification | STM32 (Cortex-M4/M7) | RP2040 (Cortex-M0+) | ESP32 (Xtensa LX6) | Raspberry Pi 5 (PREEMPT_RT) |
|---|---|---|---|---|
| Max Clock Speed | 168–480 MHz | 133 MHz | 240 MHz | 2.4 GHz |
| Worst-Case GPIO Jitter | < 100 ns | ~15–25 ns (via PIO) | ~10,000 μs | ~150 μs |
| Hardware FPU | Yes (Single/Double) | No | Yes | Yes (NEON) |
| Active Power Draw | 30–40 mA | 18 mA | 50–160 mA | > 1000 mA |
| UART DMA Support | Comprehensive | Strong | Moderate | OS Drivers |
| Rust no_std Maturity | Industry Standard | Highly Mature | Evolving | N/A (requires std) |

**Conclusion:** STM32 and RP2040 are the only viable candidates. STM32 provides native floating-point for dead-reckoning and impeccable DMA. RP2040 offers unparalleled timing control via PIO. ESP32 and Raspberry Pi are disqualified due to unmanageable OS-induced jitter.

---

## 2. NMEA 0183 Sentence Parsing at the Firmware Level

### 2.1 Key Sentences

**GGA (Global Positioning System Fix Data):**
Provides absolute 3D coordinates — UTC timestamp, latitude, longitude, altitude (MSL), fix quality indicator (0=invalid, 1=GPS, 2=DGPS, 4=RTK fixed, 5=RTK float), HDOP, and geoid separation.

**RMC (Recommended Minimum Specific GNSS Data):**
Provides kinematic vectors — UTC timestamp, coordinates, ground speed (knots), and true course (degrees true). These two velocity components are the foundation of the dead-reckoning engine.

**Talker ID agnosticism:** Modern multi-constellation receivers use `GN` (multi-GNSS), `GL` (GLONASS), `GA` (Galileo), or `GP` (GPS-only) as the talker prefix. The firmware parser must evaluate payloads based on the `GGA` and `RMC` substring identifiers only, ignoring the talker ID.

### 2.2 Parsing Strategy: State Machine over Line Buffering

**Buffered line parsing** accumulates characters into a static array until `\r\n` is detected, then tokenizes the full line. This is fragile in aircraft environments. Aircraft electrical systems (alternators, strobes) generate intense EMI that corrupts serial transmissions. If a newline character is dropped, the buffer overflows or forces the system to discard multiple valid updates while waiting to resynchronize.

**Character-by-character finite state machine (FSM)** is the robust alternative. The parser evaluates every incoming byte against the NMEA specification:

1. **Idle:** Waiting for `$` start delimiter.
2. **Recording:** Accumulating payload bytes.
3. **Recovery:** If a spurious `$` appears mid-sentence (from noise), the FSM instantly drops the corrupted data, flushes temporary registers, and resets to begin parsing the new sentence.

The FSM recovers from synchronization loss within a single UART cycle without risking memory overflow. The Rust `nmea0183` crate implements this allocation-free, heapless state machine pattern natively.

### 2.3 Checksum Validation

Every NMEA sentence ends with `*` followed by a two-character hexadecimal checksum, computed as a bitwise XOR of all characters between `$` and `*`.

The firmware must independently compute this XOR for every incoming byte and reject any sentence where the computed and transmitted checksums differ. Operating on corrupted kinematic data would cause the dead-reckoning engine to extrapolate massive positional deviations. Failed sentences force the controller to coast on existing DR vectors until the next valid epoch arrives.

---

## 3. Sub-GGA Dead Reckoning Between Position Fixes

At 10 Hz GPS and 80 m/s flight speed, the aircraft traverses **8 meters** between consecutive fixes. Relying solely on raw GPS limits spatial resolution to an 8-meter grid. If a target waypoint falls in the middle of this gap, the controller cannot recognize proximity until 4 meters too late. The controller must interpolate at **1000 Hz** internally.

### 3.1 Linear Extrapolation Kinematics

The DR engine extracts ground speed `V_g` and true course `θ` from the most recent valid RMC sentence. After converting speed from knots to m/s and heading to radians, the predicted position at any internal tick `Δt` after the last fix is:

```
lat_predicted  = lat_fix + (V_g × cos(θ) × Δt) / R_earth
lon_predicted  = lon_fix + (V_g × sin(θ) × Δt) / (R_earth × cos(lat_fix))
```

where `R_earth ≈ 6,371,000 m`. At 1000 Hz, `Δt` increments by 1 ms per tick.

### 3.2 Error Accumulation

Dead reckoning is an open-loop integration process — errors accumulate without bound. Positional error grows linearly:

```
ε(t) ≈ δV × t
```

where `δV` is the velocity vector error.

**Quantified bound:** If the GNSS-reported heading is inaccurate by 0.5°, the cross-track velocity error at 80 m/s is approximately 0.7 m/s. Over the full 100 ms inter-fix window, the maximum accumulated drift is **~7 centimeters**. This is negligible for photogrammetric triggering, validating the first-order approach without requiring inertial sensor fusion.

DR becomes unnecessary only if the GNSS/INS receiver itself outputs tightly coupled interpolated positions at > 100 Hz, shrinking the inter-fix gap to < 1 meter natively.

### 3.3 Snap-to-Fix Correction (Not Smoothing)

When the next GGA arrives, the true position will differ slightly from the DR prediction. The firmware must reconcile this residual.

**Exponential smoothing is harmful** for a spatial intervalometer. Smoothing delays the propagation of truth, causing the controller to compute distances based on a lagging position estimate.

**Snap-to-fix is mandatory:** The moment a valid GGA passes checksum validation, the DR engine instantly overwrites its internal state with the absolute truth, resetting the integration baseline to zero error. The magnitude of the snap can be monitored — if the correction vector exceeds a safety threshold (e.g., 3 meters), the system can flag severe GNSS multipath interference.

---

## 4. Trigger Proximity Detection Algorithm

### 4.1 UTM Cartesian Coordinates — No Geodesic Math on the Controller

Karney's geodesic distance algorithm is computationally expensive. Executing it 1000 times per second on a Cortex-M0+ is a catastrophic misallocation of cycles. But a flat-earth equirectangular approximation introduces ~0.5% error at 1 km scale — **5 meters of error**, which is unacceptable for RTK-grade work.

**Solution:** The daemon pre-projects all trigger waypoint coordinates from WGS84 into the local UTM zone before uploading them to the controller. The controller receives flat Cartesian `(E, N)` values in meters (encoded as millimeter-resolution i32 integers). Distance evaluation reduces to pure 2D Euclidean:

```
d = sqrt((E_aircraft - E_waypoint)² + (N_aircraft - N_waypoint)²)
```

No trigonometric functions in the hot loop. Mathematical accuracy is guaranteed by the UTM projection (sub-mm within the zone), and the computational cost is trivial.

### 4.2 Firing Logic: Closest Approach Derivative (Not Threshold)

**Naive threshold approach fails:** If the controller fires when `d < threshold`, a crosswind pushing the aircraft 3 meters off-line means the distance to a waypoint with a 2-meter threshold will never be crossed. The trigger is silently missed.

**Correct approach — derivative zero-crossing:** Monitor the rate of change of distance:

- As the aircraft approaches the waypoint: `d` decreases → `dd/dt < 0`
- At the moment the aircraft passes the waypoint: `d` reaches minimum → `dd/dt = 0`
- As the aircraft flies away: `d` increases → `dd/dt > 0`

**Fire on the negative-to-positive zero-crossing of `dd/dt`.** This detects the moment the aircraft passes the waypoint regardless of lateral cross-track offset. The trigger is perfectly synchronized to the along-track position even if the aircraft is 5 meters to the side.

### 4.3 Spatial Debounce State Machine

DR jitter and GNSS multipath can cause the distance to oscillate, potentially flipping the derivative sign multiple times and causing double-fires. The firing logic is wrapped in a four-state machine:

1. **ARMED:** Next target waypoint loaded, aircraft > 50 m away. Derivative calculation suspended to save cycles.
2. **READY:** Aircraft crosses the 50 m arming radius. Controller begins actively computing `dd/dt`.
3. **FIRED:** Derivative transitions negative → positive. Controller locks the sequence number, fires the GPIO pulse, increments the waypoint pointer.
4. **LOCKOUT:** The state machine refuses to evaluate the next waypoint until the aircraft has flown at least 20 m past the previous trigger location. This immunizes the system against coordinate bounce.

---

## 5. GPIO Pulse Characteristics for Camera Triggering

### 5.1 Voltage Levels and Active-Low Logic

The majority of professional aerial sensors use **active-low trigger circuitry**. The camera's shutter terminal is pulled up to 3.15–5.0 V via an internal pull-up resistor. The external intervalometer triggers a capture by shorting the line to ground.

**Sony (Multi-Terminal / RM-VPR1):** Pin 4 (Shutter) and Pin 5 (Focus) must both be pulled to Pin 2 (Ground) simultaneously. If Focus is not asserted, the camera introduces a non-deterministic autofocus delay.

**Canon (N3 Connector):** Three-pin locking terminal — Ground, Focus, Shutter. Shutter line pulled to ground to fire.

**Phase One / Hasselblad Aerial Systems:** LEMO connectors with active-low CMOS logic thresholds. Trigger-In requires a voltage drop from 5V high to < 0.8V to register a valid signal.

### 5.2 Minimum Pulse Duration

Camera hardware filters out microsecond transients to prevent static or EMI from causing accidental exposures. The external trigger line must be held at ground for a sustained duration:

| Camera Class | Minimum Pulse Width |
|---|---|
| Consumer (Sony, Canon) | 20–50 ms |
| Metric aviation (Phase One, Hasselblad A6D) | **10 ms** (hard minimum) |

**Non-blocking implementation:** The firmware cannot use blocking `delay_ms(15)` — this halts NMEA parsing and dead-reckoning for 15 ms, risking dropped serial bytes. Instead, pull the GPIO low and start a **non-blocking hardware timer**. The main loop continues executing. When the timer interrupt fires, the GPIO is released to high-impedance.

### 5.3 Opto-Galvanic Isolation (Mandatory)

A direct copper connection between the MCU's GPIO and a $50K aerial camera is catastrophically dangerous. Aircraft 14V/28V alternators generate severe EMI, voltage spikes, and ground loop transients as strobes, avionics, and motors cycle.

**Optocoupler isolation:** The MCU's GPIO powers a tiny sealed LED inside the optocoupler. The LED's photons strike a phototransistor connected to the camera's circuit, completing the ground path. Signal transmission via photons, not electrons — the two electrical domains are physically segregated.

**Minimum alternative:** Isolated open-drain BJT networks using 2N2222 NPN transistors as digital switches, preventing reverse current flow into the MCU.

**Mid-exposure pulse:** Phase One iXU-RS cameras emit a precise mid-exposure pulse confirming the exact microsecond the leaf shutter opened. If this pulse is routed back to the GNSS receiver for PPK event marking, the return path must also be opto-isolated.

---

## 6. Serial Communication Protocol (Daemon ↔ Controller)

### 6.1 Binary Protocol with COBS Framing

Text protocols (JSON, XML) introduce string manipulation overhead and heap-allocation risks on a constrained MCU. The protocol is **strictly binary**.

**COBS (Consistent Overhead Byte Stuffing)** framing encodes arbitrary binary data to guarantee that delimiter byte `0x00` never appears within the payload. The parser reads bytes into a circular buffer until it encounters `0x00`, guaranteeing absolute frame alignment without complex length-prefix tracking.

**Waypoint packet structure:**

| Field | Size | Encoding |
|---|---|---|
| Command ID | 1 byte | e.g., `0x01` = Waypoint Load |
| Sequence ID | 2 bytes | uint16 |
| E Coordinate (UTM) | 4 bytes | i32, millimeters |
| N Coordinate (UTM) | 4 bytes | i32, millimeters |
| CRC32 Checksum | 4 bytes | uint32 |

Total: 15 bytes per waypoint.

### 6.2 Handshake and Telemetry State Machine

Before the aircraft enters a flight line, the daemon and controller synchronize:

1. **CLEAR_BUFFER:** Daemon sends clear command. Controller flushes its waypoint array. ACK returned.
2. **TRANSMIT:** Daemon streams the full waypoint array (typically 50–300 points per line) as rapid COBS frames.
3. **VALIDATE:** Controller verifies each frame's CRC32, pushes coordinates into memory, increments counter.
4. **VERIFY_COUNT:** Daemon queries total parsed count. Controller replies.
5. **ARM:** If counts match, daemon issues ARM command. Controller begins proximity evaluations.

**During acquisition:** Each time the controller fires, it generates a telemetry packet containing: sequence ID, UTC timestamp, and residual distance (offset from the theoretical target at the moment of firing). This packet flows back over serial to the daemon, which relays it via WebSocket to the glass cockpit for real-time trigger visualization.

### 6.3 Graceful Degradation

Once the ARM command is received, the trigger controller has **complete autonomy**. If the serial cable is severed or the daemon crashes mid-flight, the controller continues operating. It holds the full waypoint array in local SRAM and relies exclusively on its direct NMEA UART connection for position data. It will silently ignore the lost daemon connection and continue firing with perfect precision until the waypoint array is exhausted.

---

## 7. Embedded Rust (`no_std`) Ecosystem

### 7.1 Concurrency: RTIC vs Embassy

**Embassy-rs:** Async cooperative multitasking for `no_std` targets. Ergonomic `async/await` syntax. But cooperative yielding means a heavy math computation could delay another task, introducing microscopic jitter.

**RTIC (Real-Time Interrupt-driven Concurrency):** Leverages the hardware NVIC for preemptive multitasking. High-priority interrupts physically suspend lower-priority tasks. Guarantees nanosecond determinism.

**Architectural decision:** RTIC (or raw hardware interrupts) for the NMEA parsing and GPIO actuation path. Embassy can manage the serial protocol state machine for daemon communication. The trigger-critical path must execute at the highest hardware interrupt priority with zero scheduling latency.

### 7.2 Hardware Abstraction: `embedded-hal`

The `embedded-hal` traits define a standardized Rust API for GPIO, SPI, UART, and other peripherals. By programming the DR engine and NMEA parser against `embedded-hal` traits rather than hardware-specific registers, the firmware is **platform-agnostic**. Migrating from STM32F4 to RP2040 requires only swapping the Peripheral Access Crate (PAC) in the config — the math and parsing logic compiles identically for both targets without code changes.

### 7.3 Deterministic Memory: `heapless` Crate

Dynamic heap allocation is **strictly prohibited** in hard real-time aviation firmware. Heap fragmentation risks Out-Of-Memory panics.

The `heapless` crate provides standard data structures (Vec, Queue, HashMap) backed entirely by static, compile-time memory allocations. The waypoint buffer is stored as:

```rust
let waypoints: heapless::Vec<Waypoint, 256> = heapless::Vec::new();
```

If the daemon attempts to upload 257 waypoints, `.push()` returns `Result::Err` — the firmware traps the error and transmits a failure code back to the daemon. No panic, no crash.

### 7.4 Diagnostics: `defmt` (Deferred Formatting)

Traditional `println!` forces the MCU to execute string formatting, integer-to-ASCII conversion, and character padding at runtime — destroying determinism and bloating binary size.

`defmt` transmits a compressed binary token (not a formatted string) over the Real-Time Transfer (RTT) debug probe interface. The host laptop performs the heavy string formatting externally. This enables deep diagnostic telemetry from the trigger controller during flight testing without impacting the timing integrity of the DR loop.

---

## 8. Architectural Summary

The complete data flow for a single trigger event:

1. **Pre-flight:** Daemon computes trigger waypoints using Karney geodesics, projects to UTM, uploads via COBS-framed serial to controller.
2. **Controller armed:** Waypoints stored in `heapless::Vec` in SRAM. State machine enters ARMED.
3. **In-flight (1000 Hz loop):**
   - NMEA FSM parses GGA/RMC from UART DMA buffer.
   - On valid GGA: snap-to-fix, update DR baseline.
   - Between fixes: linear extrapolation at 1 ms intervals.
   - Compute Euclidean distance to next waypoint (UTM coordinates).
   - Within 50 m: transition to READY, begin computing `dd/dt`.
   - On `dd/dt` zero-crossing (negative → positive): transition to FIRED.
4. **GPIO actuation:** Pull trigger pin LOW via optocoupler. Start 15 ms non-blocking hardware timer. Main loop continues.
5. **Timer ISR fires:** Release pin to high-impedance. Transition to LOCKOUT.
6. **Telemetry:** Send binary packet (sequence, timestamp, residual distance) back to daemon via serial. Daemon relays to glass cockpit via WebSocket.
7. **Advance:** Increment waypoint pointer. Transition to ARMED for next target. If the serial link is lost, continue firing autonomously.
