# Aviation Safety Principles for Flight Management Software

> **Source document:** 37_Aviation-Safety-Principles.docx
>
> **Scope:** Complete extraction — all content is unique. Covers DO-178C DAL classification, fail-safe embedded patterns, Rust type system for datum safety, SQLite transactional integrity, and human factors warning engineering.

---

## 1. DO-178C Applicability (Awareness, Not Certification)

DO-178C ("Software Considerations in Airborne Systems and Equipment Certification") is objective-based, not prescriptive. It provides a framework for planning, development, verification, configuration management, and quality assurance based on the potential consequences of software failure.

### Design Assurance Levels (DAL)

| DAL | Failure Severity | Impact | Verification Objectives |
|---|---|---|---|
| A | Catastrophic | Multiple fatalities, loss of aircraft | 71 objectives (30 independent), MC/DC coverage |
| B | Hazardous | Severe safety margin reduction, serious injuries | 69 objectives (18 independent), Decision coverage |
| C | Major | Significant safety margin reduction, minor injuries | 62 objectives (5 independent), Statement coverage |
| D | Minor | Slight safety margin reduction, slight workload increase | 26 objectives (2 independent), 100% requirements coverage |
| E | No Safety Effect | No impact on safety or crew workload | 0 objectives, no certification required |

### IronTrack Classification

- **Core FMS (planning engine, daemon, glass cockpit): DAL E.** If the software crashes, the pilot retains ultimate authority. Primary certified instruments remain operational. Airspace warnings are advisory-only and never enforce mechanical control.
- **Trigger controller: borders DAL D.** A silent firmware failure doesn't endanger the aircraft, but compromises the mission objective and increases crew workload (troubleshoot, abort line, re-fly). The high financial cost of ruined survey data justifies treating it with DAL D rigor.

### Applicable Principles (Without Certification)

**Bidirectional requirements traceability:** Every safety-critical feature (e.g., DSM penetration warning) must trace to an integration test that physically verifies the warning fires. Every line of code must trace upward to a validated requirement — no dead code.

**Objective-based test coverage:** DAL D equivalent demands 100% requirements coverage. Robustness testing with abnormal inputs (malformed NMEA, extreme coordinates) must verify no panics propagate into the daemon lifecycle.

**Rigorous configuration management:** Every artifact (source, tests, environments) versioned and baselined. Tauri auto-updater disabled unless grounded + disarmed + Ed25519 verified. Schema versions in GeoPackage prevent mid-campaign logic drift.

---

## 2. Fail-Safe Embedded Design Patterns

### 2.1 Hardware Watchdog Timer

An independent hardware timing circuit that forces a system reset if the main loop fails to "kick" it before countdown reaches zero. **Hardware watchdog is strictly superior to software watchdog** — software watchdogs rely on processor interrupts that may fail if the interrupt vector itself is compromised.

For STM32/RP2040 running no_std Rust with RTIC: any fatal panic causes the hardware watchdog to trigger a reboot. Because waypoints are pre-loaded before the line starts, a watchdog reset allows the controller to reboot, re-initialize DMA UART buffers, and potentially resume within milliseconds.

### 2.2 Heartbeat Protocol

Periodic status messages from the trigger controller to the daemon over serial. If the daemon misses 3 consecutive heartbeats, it registers a fault condition and alerts the operator via the glass cockpit. This detects severed USB cables, crashed serial drivers, and silent firmware lockups that the watchdog alone cannot communicate to the crew.

### 2.3 Graceful Degradation on GPS Loss

When NMEA GGA fix quality drops from RTK Fixed (±2 cm) to Float (±0.2–5 m) or worse:
1. **Stop triggering immediately.** Do not fire at wrong positions — corrupted imagery invalidates the entire flight line.
2. **Flash Caution (amber) on the glass cockpit.**
3. **Continue parsing NMEA.** The controller remains powered and monitors for fix recovery.
4. **Auto-resume when RTK Fixed returns** and uncertainty drops to survey-grade levels.

This allows the pilot to maintain the flight line while preventing acquisition of useless data.

### 2.4 Bulkhead Isolation

If the trigger controller firmware crashes catastrophically (unhandled exception, electrical transient):
- Camera stops firing, GPIO pins return to safe low state.
- **The aircraft continues flying safely.** Triggering is not safety-of-flight critical.
- Pilot's primary instruments, propulsion, and aerodynamic controls are completely unaffected.
- Flight crew aborts the survey line using standard procedures without facing an emergency.

### Summary Table

| Mechanism | Target | Trigger Condition | Response |
|---|---|---|---|
| Hardware Watchdog | Embedded MCU | RTIC main loop stalls or deadlocks | Hardware timer expires → forced reboot |
| Heartbeat Protocol | Controller ↔ Daemon | Serial status messages cease | Daemon alerts operator via glass cockpit |
| Graceful Degradation | GPS/Geodetic Engine | RTK Fixed → Float or standalone | Stop triggering; resume when fix returns |
| Bulkhead Isolation | System-Wide | Catastrophic firmware crash | Payload fails; aircraft flies safely |

---

## 3. Geodetic Datum Conversion Safety

### 3.1 The Failure Mode

Mixing EGM96 and EGM2008 elevations introduces multi-meter vertical discrepancies. If a DJI drone (which expects EGM96) receives EGM2008 altitudes, it flies at the wrong physical height. In steep terrain, a downward error into canopy = controlled flight into terrain (CFIT).

### 3.2 Compile-Time Safety: PhantomData Zero-Sized Types

Rust's `PhantomData<T>` associates type-level datum information with altitude values at zero runtime cost. The compiler completely optimizes it away during monomorphization.

```rust
use std::marker::PhantomData;

struct Wgs84;
struct Egm96;
struct Egm2008;

struct Altitude<Datum> {
    meters: f64,
    _datum: PhantomData<Datum>,
}

// DJI export function ONLY accepts EGM96:
fn export_dji(alt: Altitude<Egm96>) { ... }

// Passing Altitude<Egm2008> to export_dji() = COMPILE ERROR.
// The Rust compiler mathematically prohibits datum mixing.
```

This transforms the "wrong datum" failure from a runtime bug into an impossible-to-compile state. Zero processor cycles during flight.

### 3.3 Runtime Validation: Assert, Don't Assume

Compile-time types protect internal APIs. Runtime assertions protect the serialization boundary:

Before writing the final bytes to a .kmz archive, the system explicitly verifies the altitude datum enum matches the target hardware's requirement (EGM96 for DJI). If mismatched: **abort the export, throw a programmatic error to the UI.** Never silently output a dangerous flight plan.

---

## 4. GeoPackage / SQLite Transactional Integrity

### 4.1 WAL Mode: Necessary but Insufficient

WAL (Write-Ahead Logging) appends modifications to a separate .wal file, allowing concurrent reads during writes. But WAL **does not eliminate corruption risk from power loss**. A power failure during a WAL checkpoint can cause torn pages. SQLite relies on the "powersafe overwrite" hardware property — if the disk controller fails this assumption, the database can become malformed.

### 4.2 Checksum Verification: cksumvfs

The SQLite Checksum VFS shim appends an **8-byte checksum to every database page**. On read, the VFS recalculates and verifies. If a hardware fault, cosmic-ray bit flip, or power loss corrupts a page, the checksum fails → `SQLITE_IOERR_DATA` error instead of silently passing corrupted coordinates to the flight engine.

Verification on load: `PRAGMA quick_check` detects unreadable states and prevents loading a malformed flight plan.

### 4.3 Atomic Write Backup Strategy

For exports and heavy writes:

1. **Write to temp file** alongside the target destination.
2. **`fsync()`** — force the OS to flush file buffers to physical disk. Without this, data may sit in volatile RAM.
3. **`rename()`** — POSIX rename is atomic at the directory level. The file path points either to the old uncorrupted file OR the fully flushed new file. No intermediate state exists.

### Summary Table

| Threat | Defense | Result |
|---|---|---|
| Concurrent write locking | WAL mode | Simultaneous reads during writes |
| Silent bit-flips / torn pages | cksumvfs 8-byte checksums | Detects corruption on load; fails loudly |
| Power loss during export | Atomic writes (temp → fsync → rename) | File is 100% old or 100% new, never partial |

---

## 5. Human Factors: Warning Fatigue Mitigation

### 5.1 The Problem

Warning fatigue occurs when the interface inundates the operator with low-priority, redundant, or irrelevant alerts. The brain acclimatizes and the operator develops muscle memory to dismiss everything — including critical warnings. If every IronTrack output includes 5 safety warnings, pilots will click "Accept" without reading.

### 5.2 AC 25-11B Alert Hierarchy

Red and amber are **strictly reserved** for Warning and Caution states. Never used for non-alerting UI elements (route lines, topographic features, status text).

| Level | Color | Meaning | Example |
|---|---|---|---|
| **Warning** (Critical) | Red | Imminent hazard, immediate action required | Airspace collision, fatal datum mismatch |
| **Caution** | Amber | Potential hazard, awareness + subsequent action | RTK drop to Float, approaching TFR |
| **Advisory** | Cyan/Green | General awareness, no immediate action | Upcoming line start, mission progress |

### 5.3 Non-Dismissable vs Dismissable

**Non-dismissable (datum mismatch):** The UI mechanically blocks the workflow — disables the export button — and forces the user to rectify the datum selection. No "Ignore" or "Continue Anyway" button exists. This is the one failure mode that can kill. Warning fatigue cannot cause it.

**Dismissable (DSM penetration):** Presented as an amber caution the pilot can acknowledge and dismiss. The mission continues, but the pilot has internalized the environmental risk. DSM canopy underestimation (2–8 m) is an operational constraint, not an immediate fatal block. The pilot adjusts AGL safety margins manually.

This distinction — blocking for lethal errors, acknowledging for operational constraints — is the optimal balance between safety and operational flexibility.
