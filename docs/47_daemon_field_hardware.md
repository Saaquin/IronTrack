# Edge Computing Hardware for Airborne Survey Systems

> **Source document:** 47_Daemon-Field-Hardware.docx
>
> **Scope:** Complete extraction — all content is unique. Covers Raspberry Pi 4/5 comparison, Intel NUC deployment, ruggedized tablets (Panasonic/Dell/Getac), custom avionics (Veronte/Piccolo), aircraft network topology, environmental constraints (-20°C to +50°C, vibration, EMI), and DO-160-grade power conditioning.

---

## 1. Aircraft Network Topology

Four primary communication pathways:

| Path | Medium | Constraint |
|---|---|---|
| Daemon ↔ Glass cockpit | Ethernet (preferred) or WiFi | 10 Hz telemetry + terrain-following lines. Ethernet eliminates WiFi latency spikes inside metal RF cavity. |
| Daemon ↔ Operator laptop | WiFi | React web UI. Secured via ephemeral bearer tokens. |
| Daemon ↔ GNSS receiver | USB-Serial or UART (230,400–460,800 baud) | 100 Hz GGA+RMC exceeds 164 kbps → 115200 baud insufficient. |
| Daemon ↔ Trigger controller | USB-Serial (115,200+ baud) | COBS-framed binary with CRC-32. Daemon pre-computes UTM waypoints, uploads before line. Controller operates autonomously. |

**Real-Time Boundary:** The daemon NEVER fires cameras/LiDAR. OS jitter (10–50 ms) = 1–4 m error at 80 m/s. Only the bare-metal STM32/RP2040 trigger controller actuates GPIO with < 100 ns jitter.

---

## 2. Raspberry Pi 4 vs 5

| Spec | Pi 4 (8 GB) | Pi 5 (8 GB) |
|---|---|---|
| CPU | Cortex-A72 (1.8 GHz) | **Cortex-A76 (2.4 GHz)** |
| Memory | LPDDR4-3200 | **LPDDR4X-4267** |
| GPU | VideoCore VI (500 MHz) | **VideoCore VII (800 MHz)** |
| Peak power | ~7.5 W (15W PSU) | **~15 W (27W PSU)** |
| Thermal throttle | 80°C | 82–85°C |
| Cost (complete) | $75–95 | $100–135 |
| Math performance | Baseline | **2–2.5× faster** |

**Mandate:** 8 GB variant, 64-bit (aarch64) OS. 32-bit OS imposes severe f64 penalties and memory limits → OOM panics on complex B-spline paths. Cross-compile via Dockerized qemu (ghcr.io/tauri-apps/tauri/aarch64), match glibc versions.

**Pi 5 for combined daemon + cockpit display:** VideoCore VII (800 MHz, OpenGL ES 3.1, Vulkan 1.2) handles Canvas 2D CDI + WebGL at 60 FPS alongside the daemon. Pi 4 exhibits visible UI stutter under dual load.

### The GPIO Fallacy
Pi GPIO pins are **disqualified for survey-grade triggering.** Linux PREEMPT_RT cannot guarantee sub-millisecond determinism. Pi 5 moves GPIO to the RP1 peripheral chip via PCIe → additional microsecond-level bus latency. Measured OS jitter: 150 μs to 10 ms. At 80 m/s, 10 ms = ~1 m spatial error. The external STM32/RP2040 bare-metal controller achieves < 100 ns.

### Thermal in Aircraft
Pi 5 at 15W in passive enclosure will thermal-throttle within seconds without heat sinking. **CNC-milled aircraft-grade aluminum enclosure** mates directly to processor/PMIC/RAM via thermal paste, wicks heat to external fins. Active fans fail in vibration environments — bearings destroyed by engine harmonics.

---

## 3. Intel NUC Mini-PC

### Performance
NUC 12/13 Pro: 12th/13th Gen Intel Core i5/i7, hybrid P-core + E-core (12–14 total cores, turbo 4.7–5.0 GHz). Up to 64 GB DDR4-3200, Gen4 NVMe (4,800 MB/s). Tokio async tasks → E-cores; rayon B-spline math → P-cores. Sub-millisecond terrain-following computation.

### SWaP Challenges
- Physical: 117 × 112 × 54 mm, ~500–600 g.
- Power: 5–12 W idle, **65–95 W peak.** Requires 19V DC input. Do NOT use consumer AC power brick through an inverter — EMI, vibration disconnect, thermal inefficiency.
- Cooling: 35W TDP processor needs active blower fan. Wire-rope vibration dampeners required to prevent micro-fractures and fan bearing failure from engine harmonics.
- **Cost: $300–600+** depending on generation/config.

### Power Conditioning
Aircraft bus (14V/28V nominal) requires isolated DC-DC step-down to 19V. Mil-COTS converters mandatory — see §7.

---

## 4. Ruggedized Tablets (Recommended for Manned Operations)

### Why Tablets Win
Collapses daemon host + glass cockpit display + offline tile server into ONE device. Tauri spawns Axum daemon as internal background thread → all IPC is in-memory, zero network latency. Eliminates Ethernet cables, router saturation, and WiFi packet loss. USB ports connect directly to trigger controller and GNSS. Internal WiFi serves as AP for operator laptop.

### Hardware Comparison

| Model | Display | Brightness | MIL-STD | Processor | Battery | Secondary Market |
|---|---|---|---|---|---|---|
| **Panasonic CF-33** | 12" QHD (3:2) | 1200 nits | 810G, IP65 | 7th Gen i5/i7 | Hot-swap dual | $300–900 |
| **Panasonic FZ-G2** | 10.1" WUXGA | 1000 nits | 810H, IP65 | 10th/12th Gen | Hot-swap | $900–2,500 |
| **Dell Latitude 7230** | 12" FHD (16:10) | 1200 nits | 810G, IP65 | 12th Gen i7 | Hot-swap dual | $1,500–3,000 |
| **Getac F110 G6/G7** | 11.6" FHD | 1000 nits | 810H, IP66 | 11th/13th Gen | Hot-swap dual | $1,500–3,000 |

**Key requirements:** 1000+ nits (direct sunlight cockpit), hot-swap batteries (daemon runs indefinitely independent of aircraft bus), glove-touch calibration (Getac LumiBond), modular serial ports (Panasonic xPAK RS-232 eliminates USB-serial dongles).

---

## 5. Custom Avionics Computers

### Veronte Autopilot 1x
- 198 g. Triple-redundant IMU (3 IMUs, 3 magnetometers, dual barometers, dual GNSS with RTK).
- **6.5–36 VDC native input** — direct to any aircraft bus, no external converter.
- 15W max power.
- **DO-178C / DO-254 compliant up to DAL-B.**
- Cost: €5,900 base to €18,000+ (DAA/ADS-B developer kit).

### Piccolo Elite
- Milled aluminum EMI-shielded case. 8–30 VDC input, 7W power.
- Manages thousands of internal flight plan waypoints.

### When Custom Avionics Are Needed
IronTrack is **advisory-only** — computes trajectory, warns of airspace/terrain, but never mechanically controls the aircraft. For manned operations with a human pilot, a COTS tablet is superior (display, storage, cost).

Custom avionics become necessary for **fully autonomous BVLOS UAV operations** where the flight plan must be translated to MAVLink/CAN bus commands and executed by a certified safety-critical system managing physical control loops, dissimilar failsafe supervisors, and catastrophic fault management.

---

## 6. Environmental Constraints

### Temperature (-20°C to +50°C)
- Unpressurized cabin: +50°C on hot tarmac, -20°C at 15,000–18,000 ft survey altitude.
- **Conformal coating** on PCBs prevents condensation shorts during descent from cold altitude into warm humid air.
- Custom avionics (Veronte) rated -40°C to +60°C natively.

### Vibration
- Piston engines produce intense low/high-frequency harmonics through airframe.
- **HDD prohibited** — head crashes within hours. NVMe/SATA SSD mandatory.
- **Rubber-in-shear isolators or wire-rope shock mounts** tuned to hardware mass. Prevents RAM SO-DIMM unseating, fan bearing failure, connector loosening.

### Power Conditioning (DO-160 Category-Z)
Aircraft bus: nominal 14V/28V but experiences severe sags (engine cranking) and transient spikes (landing gear, flap motors, pitot heat).

**Mil-COTS DC-DC converters mandatory:**

| Manufacturer | Model | Input Range | Output | Efficiency | Key Feature |
|---|---|---|---|---|---|
| Vicor | DCM3623 | ~28V erratic | 5V/12V regulated | 96% peak | ZVS topology, MIL-COTS |
| SynQor | MCOTS series | Surges to 50 VDC | Various | High | Over-voltage/short-circuit/thermal protection |
| XP Power | MTC05 | Wide transient | Various | High | Baseplate-cooled, vetronic/avionic |
| Mean Well | DC-DC modules | Various | Various | High | Conformal-coated, 5G vibration, 5000m altitude |

**UPS recommendation:** Small supercapacitor bank or mini-UPS provides critical seconds for graceful OS shutdown on sudden brownout — prevents GeoPackage WAL corruption.

### EMI Shielding
VHF radios, ADS-B transponders, radar altimeters create hostile RF environment. Unshielded USB 3.0 cables act as antennas (absorb and emit EMI).
- **Grounded aluminum enclosure** for all COTS hardware.
- **Braided metallic shielding** on all cables (especially 460,800 baud UART to GNSS/trigger controller).
- Without shielding, high-frequency UART square waves leak RF noise that can desensitize aircraft navigation radios and interfere with ATC communications.
