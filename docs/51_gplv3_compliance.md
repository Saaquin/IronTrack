# GPLv3 Compliance: Dependency Audit, Embedded Firmware Boundary, and IP Governance

> **Source document:** 51_GPLv3-Compliance.docx
>
> **Scope:** Complete extraction — all content is unique. Covers Cargo.toml license audit (MIT/Apache-2.0/MPL-2.0/LGPL/BSD), problematic license exclusions (SSPL/BSL), embedded firmware as a separate non-derivative work, proprietary SDK isolation via IPC bridge, geospatial data attribution (Copernicus/3DEP/EGM2008), and CLA vs DCO strategy.

---

## 1. Dependency License Compatibility with GPLv3

### Permissive Foundation (Zero Risk)
- **MIT:** Fully compatible. Requires only copyright notice preservation. No patent restrictions. Most Rust crates.
- **Apache-2.0:** Compatible with GPLv3 (resolved in GPLv3 Section 10). Incompatible with older GPLv2. Provides explicit patent grant.
- **BSD-3-Clause:** Fully permissive. MapLibre GL JS (fork of Mapbox after proprietary relicense) — safe to bundle/serve from GPLv3 daemon.
- **Dual MIT/Apache-2.0:** Standard Rust ecosystem norm. Zero risk.

### MPL-2.0: The serialport Crate
The `serialport` crate is MPL-2.0 (weak copyleft). Historically problematic with GPL, but **MPL-2.0 Section 3.3 explicitly names GPLv3 as a "Secondary License."** A Larger Work combining MPL-covered code with GPLv3 software may be distributed under GPLv3, provided MPL-covered source files remain accessible. The `serialport` crate does NOT contain the "Incompatible With Secondary Licenses" exclusion (Exhibit B). **Statically linking serialport into the GPLv3 daemon binary is legal.**

### LGPL in the Rust Static-Linking Model
LGPL requires end-users be able to modify the library and relink. In C/C++: dynamic linking (.so/.dll) trivially satisfies this. In Rust: the default is static monolithic binaries — would require distributing .rlib/.o object files.

**Resolution:** Because IronTrack is GPLv3 (stronger than LGPL), the LGPL allows upgrading to full GPL. The GPLv3 mandates Complete Corresponding Source distribution → end-users receive all source code + build scripts → can modify any library and recompile from scratch. **LGPL static linking is legal and seamless under GPLv3.**

### Excluded Licenses (CI Enforcement via cargo-deny)
| License | Why Excluded |
|---|---|
| **SSPL** (MongoDB) | Requires open-sourcing entire hosting/management stack for network services. Rejected by OSI. Violates GPLv3 Section 7 ("further restrictions"). |
| **BSL** (HashiCorp) | Restricts commercial production use for up to 4 years. Violates GPLv3 Section 7. |

Any static/dynamic linkage to SSPL/BSL crates renders the entire IronTrack binary legally undeployable under GPLv3.

### Core Dependency Audit Table

| Crate | Function | License | Compatibility |
|---|---|---|---|
| geographiclib-rs | Karney geodesics, UTM | MIT | ✅ Permissive |
| egm96 | Geoid conversions (DJI export) | Custom Zlib/MIT | ✅ Permissive |
| tokio | Async runtime | MIT | ✅ |
| axum | REST/WebSocket daemon | MIT | ✅ |
| rayon | Parallel geodetic math | MIT/Apache-2.0 | ✅ |
| serialport | UART telemetry + trigger | MPL-2.0 | ✅ Via Section 3.3 Secondary License |
| rtic | Bare-metal MCU concurrency | MIT/Apache-2.0 | ✅ |
| heapless | no_std static allocations | MIT/Apache-2.0 | ✅ |
| defmt | Embedded diagnostic logging | MIT/Apache-2.0 | ✅ |
| bspline | C² trajectory generation | MIT | ✅ |
| MapLibre GL JS | WebGL map rendering | BSD-3-Clause | ✅ |

---

## 2. Embedded Firmware Boundary: Separate Work, Not Derivative

### The Legal Question
Does the STM32/RP2040 trigger controller firmware (bare-metal, no_std) become a GPLv3 "derivative work" of the GPLv3 Axum daemon?

### FSF Test: "Arm's Length" Communication
Combined work = shared memory address space, static/dynamic linking, intimately coupled data structures.
Separate work = different memory spaces, communication via standard arm's-length mechanisms (pipes, sockets, serial protocols, CLI arguments).

**IronTrack firmware passes the FSF test:**
- Executes on a completely different CPU architecture (Cortex-M vs x86/ARM64).
- Has zero knowledge of daemon's Rust data structures or memory layout.
- Communicates solely via a generic COBS-framed binary serial protocol over physical UART.
- Operates fully autonomously once waypoints are uploaded (dead-reckons at 1000 Hz, fires GPIO independently).

**Conclusion: The firmware is legally a separate work. Not infected by GPLv3.**

### Firmware License: MIT/Apache-2.0
Permissive licensing ensures frictionless interoperability with the embedded-hal/rtic/heapless ecosystem. Allows third-party developers to fork/adapt the trigger controller for different hardware without being forced to open-source their proprietary flight control stacks.

### Shared Geodesic Code Dilemma
If both the GPLv3 daemon and the MIT firmware use the same custom Kahan summation / coordinate transform code:
- The **original author** (IronTrack founder) holds copyright and can dual-license freely.
- **Solution:** Isolate shared geodesic math into an independent Rust crate published under MIT. The daemon imports it (MIT into GPLv3 = legal). The firmware imports it (MIT into MIT = legal). No GPLv3 infection propagates.

---

## 3. Proprietary SDK Isolation: The IPC Bridge Pattern

### The Problem
Phase One Capture One SDK and DJI PSDK have restrictive EULAs that prohibit GPL redistribution of their proprietary C/C++ libraries. Statically linking these SDKs into the GPLv3 binary would create an irreconcilable license conflict.

### The Solution: Separate Process + IPC
1. **Plugin Daemon:** A separate, standalone binary that links directly against the proprietary SDK via FFI. Licensed under proprietary/restrictive terms to satisfy the SDK EULA. NOT a derivative of IronTrack.
2. **IronTrack Core:** Remains pristine GPLv3 with zero proprietary bindings.
3. **IPC Bridge:** Two separate OS processes communicate via standard arm's-length channels (TCP/UDP sockets, gRPC, UNIX domain sockets, Protocol Buffers, or JSON). This satisfies the FSF's separate-works criteria.

The plugin daemon wraps the proprietary SDK functions and exposes them as a network service. IronTrack calls this service like any external API — no copyleft propagation occurs across the IPC boundary.

---

## 4. Geospatial Data Attribution Requirements

### Copernicus DEM (GLO-30)
Licensed under Copernicus Data Access policy. **Mandatory attribution** in documentation/UI: "Contains modified Copernicus Sentinel data [year]." Must preserve and display the attribution string. IronTrack embeds this in GeoPackage irontrack_metadata and exported GeoJSON properties.

### USGS 3DEP
U.S. federal government data — **public domain** (no copyright). No legal attribution required, but scientific best practice includes citing the USGS National Map as source. IronTrack includes voluntary attribution.

### EGM2008 / GEOID18
NGA (EGM2008) and NGS (GEOID18) data — **public domain**. No legal restrictions. IronTrack documents the specific grid file versions and Big-Endian format requirements in irontrack_metadata.

---

## 5. Contributor License Agreements vs Developer Certificates of Origin

### CLA (Contributor License Agreement)
Contributors sign a legal document granting the project maintainer broad rights (including relicensing). Enables future dual-licensing / commercial SaaS offerings. **Friction:** Open-source contributors frequently refuse to sign CLAs (corporate legal review, perceived power asymmetry).

### DCO (Developer Certificate of Origin)
Contributors add a `Signed-off-by:` line to commits, certifying they have the right to submit the contribution under the project's license. **No relicensing rights.** Low friction — Linux kernel and many major projects use DCO.

### IronTrack Strategy
For a solo-founder project targeting eventual commercial services (e.g., hosted flight planning SaaS), a **CLA preserves relicensing flexibility** while the GPLv3 ensures the core engine remains open. If community growth is prioritized over commercial flexibility, **DCO reduces contribution friction.** The document recommends starting with CLA while the contributor base is small, with the option to transition to DCO as the community matures.
