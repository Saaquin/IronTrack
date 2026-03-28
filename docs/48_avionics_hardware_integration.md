# Aircraft Electrical Integration and Avionics Hardware Architecture

> **Source document:** 48_Avionics-Hardware-Integration.docx
>
> **Scope:** Complete extraction — all content is unique. Covers aircraft power bus architectures (GA/turboprop/UAV), DC-DC buck regulation, DO-160G transient protection (TVS/LC filtering), grounding and bonding (ground loops, star ground, opto-isolation), ruggedized connectors (MIL-DTL-38999, LEMO, Fischer, DB-9), and spatial/mass budgets for Cessna 206 and King Air 200 integration.

---

## 1. Aircraft Power Bus Architectures

| Platform | Power Source | Nominal V | Peak V | Min Operating V |
|---|---|---|---|---|
| GA piston (legacy) | Alternator + lead-acid | 14.0 VDC | 14.4 | 11.0 |
| GA piston / turboprop | Starter-generator + battery | 28.0 VDC | 28.25 | 22.0 |
| UAV (4S LiPo) | LiPo battery | 14.8 VDC | 16.8 | 12.0 |
| UAV (6S LiPo) | LiPo battery | 22.2 VDC | 25.2 | 18.0 |
| UAV (12S semi-solid) | High-density lithium | 44.4 VDC | 50.4 (53.4 LiHV) | 36.0 |

### GA Piston (Cessna 206)
Traditional 14V: 12V lead-acid + 60A/95A alternator. Many modern survey aircraft retrofitted to **28V** — doubles bus voltage → halves current (Joule's Law P=I²R) → lighter wire gauge = weight savings. 28V also provides wider voltage decay buffer before avionics dropout (10-11V threshold) during alternator-out emergency.

### Turboprop (King Air 200/350)
28V universal. Dual starter-generators (250A, regulated to 28.25V ± 0.25V). Split-bus or triple-fed bus architecture isolates avionics from heavy mechanical loads. But inductive load switching (gear, hydraulics) introduces severe momentary sags and flyback surges.

### UAV
Voltage decays continuously throughout flight. A 12S system drops from 50.4V at takeoff to ~42V at RTL. IronTrack's internal power supply must regulate 5V/3.3V logic across the entire shifting input curve without dropout or overheating.

---

## 2. DC-DC Step-Down Regulation

### LDO Disqualified
Dropping 28V to 5V at 3A via linear regulator: P_waste = (28-5) × 3 = **69W waste heat.** Unacceptable thermal hazard in confined avionics bay.

### Synchronous Buck Converter (Mandatory)
High-Side/Low-Side MOSFETs chop input at 300 kHz–2 MHz → LC tank averages pulses to smooth DC. Peak efficiency 90–95%. Wide-input aerospace converters (9V–75V) span all aircraft and UAV bus voltages.

**Recommended converters:** SynQor MilQor, Vicor VI-200, Texas Instruments TPS54233-Q1.

### Pulse-Skipping for Trigger Controller
The bare-metal RTIC trigger controller draws negligible current between active triggers. Standard PWM converters suffer degraded efficiency at light loads (parasitic switching losses). **Pulse-skipping/PFM mode** dynamically lowers switching frequency during idle → preserves battery during prolonged ground operations.

---

## 3. EMC: DO-160G Compliance

### Airborne Noise Sources
- **Alternator ripple:** 1.5–2.0 Vpp on 28V bus from imperfect 6-diode rectification. Failed diode → drastically increased ripple.
- **Ignition noise:** Magnetos + unshielded spark plug leads → broad-spectrum EMI hash (piston aircraft).
- **Load switching:** Flaps, gear motors, hydraulic pumps → massive flyback spikes.
- **RF coupling:** VHF comms, HF transceivers, L-band transponders inductively couple into power cables.

### DO-160G Section 16 (Power Input) & Section 17 (Voltage Spike)
28V DC equipment must survive:
- **Long-duration surge:** 80V peak for 100 ms, decaying to 48V for 1 second.
- **Microsecond spikes:** Up to **600V.**

A COTS buck converter rated at 36–40V max input → **destroyed instantly** by an unsuppressed DO-160G surge.

### Protection Chain

#### Stage 1: TVS Diode (First Line of Defense)
Placed in parallel across power input. Avalanche breakdown in picoseconds clamps bus voltage.

**SMDJ33A** (industry standard for 28V bus):
| Parameter | Value | Significance |
|---|---|---|
| Stand-off voltage (V_WM) | 33.0V | No conduction during normal 28–30V operation |
| Breakdown voltage (V_BR) | 36.7V | Begins shunting as surge climbs past 36.7V |
| Clamping voltage (V_C) | **53.3V** (at 56.3A) | Maximum downstream voltage during catastrophic surge |
| Peak pulse power | 3000W | Absorbs lightning/load-dump energy |

**CRITICAL:** Downstream buck converter must be rated ≥ 60V (not 40V). TVS clamps to 53.3V — a 40V-rated converter is destroyed even with the TVS protecting it.

#### Stage 2: LC Low-Pass Filter
Attenuates continuous high-frequency differential noise (alternator ripple, RF).

Corner frequency f_c = 1/(2π√(LC)), positioned one decade below buck converter switching frequency → -40 dB/decade attenuation.

**Middlebrook stability criterion:** Switching regulator has negative input impedance (draws more current as voltage drops). If LC filter output impedance peaks at resonance and intersects the negative impedance → violent oscillations → chaotic resets. **Must critically damp** with a parallel RC branch (DC blocking capacitor + damping resistor) to suppress the resonant Q-factor.

#### Stage 3: Hold-Up Capacitance
DO-160G mandates survival through 50 ms to 1 second power interruptions. Bulk electrolytic capacitor bank stores sufficient energy to keep the edge computer + STM32 alive during micro-blackouts.

#### Stage 0: Reverse Polarity Protection
Series Schottky diode at absolute front end — protects against accidental reverse wiring during field maintenance.

---

## 4. Grounding and Bonding

### Metallic Bonding Network (MBN)
All conductive structures share the same electrical potential. FAA AC 43.13-1B Chapter 11: **< 2.5 mΩ** bond resistance from equipment chassis to primary aircraft structure.

### Ground Loop Physics
Two devices grounded at different airframe points + data cable between them. Airframe has small but nonzero impedance. Heavy return currents (strobes, pitot heat, hydraulics) create fluctuating voltage drop between the two ground points → drives parasitic loop current through the data cable shield → corrupts NMEA serial or triggers false GPIO pulses.

### Mitigation: Signal vs Chassis Ground Separation
1. **Chassis ground:** Metal enclosure bonded to airframe (safety, ESD, AC 43.13-1B).
2. **Signal ground:** PCB's internal 0V reference meets chassis at **ONE star ground point** (power supply entrance). For RF isolation: AC-couple signal ground to chassis via 2200 pF / 2 kV capacitor — bleeds RF but blocks DC/low-frequency loop currents.

### Opto-Isolation: The Ultimate Ground Loop Breaker
IronTrack trigger controller uses opto-isolators for all GPIO trigger outputs. **Photons replace electrons** — complete galvanic separation between FMS logic ground and camera/LiDAR payload ground. Ground loops physically impossible. Highest possible EMI immunity tier.

### USB Shield Hybrid Termination
USB shield grounded at both ends = ground loop. Floating at one end = dipole antenna (violates DO-160G radiated emission limits).

**Aerospace solution:**
- **Host side (edge computer):** Shield hard-bonded to chassis ground (360° low-impedance RF drain).
- **Device side (trigger controller):** Shield tied to local ground via **1 MΩ resistor ∥ 4.7 nF capacitor.** The resistor bleeds static charge; the capacitor shorts high-frequency RF to ground but blocks DC/low-frequency loop currents.

---

## 5. Ruggedized Connectors

| Type | Mechanism | IP Rating | Use Case | Vibration Resistance |
|---|---|---|---|---|
| **MIL-DTL-38999 Series III** | Triple-start thread + anti-decoupling spring | IP67/68 | Primary power, high-density signals | Exceptional |
| LEMO B-Series | Push-pull sliding sleeve | IP50 | In-cabin audio, quick-disconnect | Moderate |
| **Fischer Core / LEMO K** | Push-pull enclosed | IP68/69 | External sensors, exposed pods, UAV | High |
| **Amphenol USB3F TV** | Threaded shell over USB | IP68 | Field laptop interfacing | Exceptional |
| **DB-9 / DB-25** | Trapezoidal + jackscrews | IP20 | RS-232 GNSS/IMU, trigger controller | High |

### MIL-DTL-38999 III
- Triple-start threaded coupling with ratcheting spring — cannot back off in flight.
- Scoop-proof: elongated shell prevents pin damage on angular insertion.
- Titanium or composite shells: 20–50% weight savings over stainless steel, EMI shielding via electroless nickel plating.

### LEMO B vs Fischer Core
LEMO B: sliding sleeve mechanism leaves a gap → dust/fluid ingress → IP50 max. Cabin-only.
Fischer Core: fully enclosed mechanism → IP68/IP69. Mandatory for external/unpressurized mounting.

### DB-9 with Jackscrews
Still the standard for RS-232 avionics serial. Trapezoidal shape forces correct polarization. Screw-locking mechanism is immune to piston engine resonant frequencies. Used by Applanix POS AV and Riegl LiDAR — explains its persistence in tier-one survey hardware.

---

## 6. Spatial and Mass Budgets

### IronTrack Hardware Stack: < 3 kg Total
| Component | Mass |
|---|---|
| Edge computer (Intel NUC Rugged) | 0.7–1.0 kg |
| Trigger controller (STM32/RP2040 in EMI enclosure) | ~0.4 kg |
| Cabling, power conditioning, connectors | 0.5–1.0 kg |
| **Total** | **< 3.0 kg** |

### Cessna 206 Stationair
- MTOW: 3,600 lbs, useful load > 1,400 lbs (~932 lbs with full fuel).
- Belly camera port STC: 16×21" to 20" diameter cutouts for gyro-stabilized mounts.
- **Avcon AeroPak underbelly pod:** 15.3 ft³, rated 300 lbs — ideal isolated environment for compute stack (requires heating at altitude).

### Beechcraft King Air 200
- MTOW: 12,500 lbs, useful load ~5,000 lbs.
- Pressurized, climate-controlled cabin eliminates extreme thermal/atmospheric housings.
- **LCAS 400 series avionics racks:** Crash-load certified, 12–50" height, 24" reduced-dimension option.
- SOMAG GSM 4000 gyro mount: 29 kg, stabilizes up to 120 kg of sensor payload.
- IronTrack's ~3 kg stack is spatially and gravitationally imperceptible in the King Air rack environment.
