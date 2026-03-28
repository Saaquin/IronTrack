# Advanced Cockpit Display Systems for Aerial Survey

> **Source document:** 25_Aircraft-Cockpit-Design.docx
>
> **Scope:** Complete extraction — all content is unique to the project. Covers PFD information architecture, CDI design, altitude guidance, aviation color standards, vector rendering, offline basemaps, cockpit typography/ergonomics, and 3D SVS/HITS feasibility.

---

## 1. Primary Flight Display: Information Architecture

### 1.1 Minimum Critical Information Set

Survey pilots require a refined subset of telemetry to maintain optimal flight technical error (FTE) margins. Visual scanning degrades under stress — scanning entropy increases, reducing the ability to suppress irrelevant stimuli. The PFD must minimize cognitive load while maximizing spatial awareness.

**Minimum information set for safe line-following:**

| Parameter | Definition | Control Function |
|---|---|---|
| **Cross-Track Error (XTE)** | Lateral orthogonal deviation from the planned flight line | Primary manual control variable |
| **Track Angle Error (TAE)** | Angular difference between desired and actual ground track | Proportional to the rate of change of XTE |
| **Altitude Deviation** | Vertical error relative to AGL (for GSD) or AMSL (for airspace) | Terrain-following compliance |
| **Ground Speed** | Current speed over ground | Camera trigger interval / LiDAR PRR maintenance |
| **Distance to Next Waypoint** | Distance or time to line end or next turn | Anticipate roll-in maneuvers and Dubins transitions |

### 1.2 Lessons from Existing Systems

**Leica FlightPro:** "Nose-up" view + 3D virtual globe. Strict color-coding: aircraft/track in cyan, current line/targets in red. Explicitly separates the pilot's simplified guidance view from the sensor operator's complex data view to reduce pilot stress.

**Applanix POSView:** Highly numeric readouts of XTE, true heading, and heave. Excellent for GNSS/INS accuracy monitoring but requires higher cognitive effort during active maneuvers vs analog graphical representations.

**Trimble Aerial Imaging:** Tabular navigation, block overviews, wind vectors. Efficient for automated platforms where the pilot is a systems manager, but insufficient for continuous manual survey flying.

**IronTrack synthesis:** Combine Leica's color-coded simplicity and segregated views, Applanix's mathematical precision, and Trimble's macro-level workflow automation — in a unified, hardware-agnostic vector display that prioritizes analog haptic-visual feedback.

---

## 2. Cross-Track Deviation Indicator (CDI)

### 2.1 Track Vector Display — Recommended Design

A traditional CDI needle shows only lateral deviation, requiring the pilot to mentally differentiate heading and attitude indicators to judge closure rate. This mental differentiation increases workload.

**Two proven design patterns:**

**Sliding Pointer:** Conventional "fly-to" XTE needle supplemented with a sliding Track Angle Error triangle underneath. If the triangle moves in the same direction as the aircraft bank, it provides immediate intuitive feedback on closure rate.

**Track Vector (recommended):** Replaces the sliding needle with a combined sliding+rotating pointer. Horizontal position = cross-track error. Tilt angle = track angle error. Creates a "mail slot view" of a track-up moving map in an aircraft-centered frame.

**Simulator results:** Pilots using the Track Vector display showed a **35% improvement** in flight technical error envelope width, particularly after complex turning maneuvers.

### 2.2 Scale Ranges by Mission Type

| Mission Profile | Lateral Tolerance | Recommended XTE Full-Scale | Auto-Range Policy |
|---|---|---|---|
| Wide-area photogrammetry | Moderate | ±100 m to ±150 m | Permitted during transit; **must lock during active acquisition** |
| LiDAR topography | Tight | ±50 m | Fixed — ensures exact swath overlap |
| Corridor mapping | Very tight | ±25 m to ±50 m | Strictly fixed — auto-ranging prohibited |

### 2.3 Auto-Ranging Lockout Rule

Auto-ranging is useful during the turn-in phase to help the pilot acquire the line from a distance. But dynamically changing the scale during active data acquisition destroys pilot muscle memory. If the scale shifts from ±100 m to ±50 m without warning, a standard bank angle input appears to cause twice the deviation, leading to severe overcorrection.

**Rule:** The CDI scale must automatically lock to the mission-type default the moment the aircraft crosses the start threshold of the flight line. Scale changes during active acquisition are prohibited.

---

## 3. Altitude Guidance

### 3.1 Vertical Deviation Indicator (VDI) for Terrain-Following

The VDI guides the pilot along a dynamic, undulating vertical path rather than a static planar slope. The vertical guidance algorithm computes a continuous spline respecting the aircraft's climb/descent performance limits while maintaining constant AGL. The VDI displays the error between current altitude and this spline.

**Performance standard (from RNP approach evaluations):** Half-dot VDI deflection = desired performance. Full-dot deflection = merely adequate.

The VDI requires predictive filtering to prevent erratic jumping as the aircraft passes over localized terrain spikes.

### 3.2 Dual Display: AGL vs AMSL

AGL is essential for maintaining GSD (cameras) and point density (LiDAR). AMSL is legally required for airspace deconfliction and ATC communication.

**Display pattern:** Primary barometric AMSL altitude on a traditional vertical tape (right side of the attitude indicator). Secondary, color-coded "AGL ERROR" deviation indicator adjacent to it, explicitly labeled.

**DSM warning integration:** When flying over DSM-derived terrain (Copernicus), the VDI tape should show an amber hazard zone at the bottom, indicating that the true AGL may be 2–8 m less than displayed due to X-band canopy penetration.

### 3.3 Color-Coded Altitude Zones

Altitude tapes induce dangerous visual fixation — pilots correct deviations as small as 1–2 feet at the expense of the broader instrument cross-check.

**Mitigation:** Color-coded bounding zones on the altitude tape. An acceptable AGL deviation zone of ±15 m shaded green directly on the tape. As long as the altitude bug remains in the green zone, the pilot knows they're within photogrammetric tolerances without reading exact numerics. This reduces cognitive load and speeds the scan back to the primary attitude and XTE indicators.

---

## 4. Block Overview and Aviation Color Standards

### 4.1 ARINC 661 / FAA Color Semantics

FAA Advisory Circular AC 25-11A, RTCA DO-257A, and ARINC 661 dictate strict color usage on the flight deck. These standards prevent desensitization to emergency alerts.

| Color | Semantic Meaning | Usage Rule |
|---|---|---|
| **Red** | Warnings, immediate hazards, flight envelope exceedances | Reserved exclusively — requires immediate corrective action |
| **Amber/Yellow** | Cautions, abnormal conditions | Pilot action required but not immediately. Extensive use for non-alerting functions is **prohibited** (diminishes attention-getting value) |
| **Green** | Normal, safe operating conditions | Avoid extensive use to prevent confusion with critical "safe" indications (e.g., gear deployment) |
| **Blue** | — | **Avoid for small, detailed symbols** — low luminance on LCD, easily confused with dark backgrounds under bright light |
| **White** | Primary flight data, text | Default for numeric readouts and labels |

### 4.2 IronTrack Flight Line Color Palette

Compliant with aviation color standards:

| Line State | Color | Rationale |
|---|---|---|
| Planned | **Gray** | Neutral background information, no visual clutter |
| Selected/Active | **Amber** | System armed for this line, pilot action required to navigate — aligns with "caution/action" amber semantics |
| In-Progress | **Green** | Active, safe data acquisition state |
| Complete | **Blue/Cyan** | Past data, rendered as large thick strokes (mitigates blue's low-luminance issue) |
| Failed/Flagged | **Red** | Data acquisition failed, immediate attention or re-fly required |

### 4.3 Track History Rendering

A continuous solid "snail trail" quickly occludes the map during long survey blocks with dozens of tightly spaced lines. Track history should be rendered with a low-opacity, dashed line that dynamically fades after a set time or distance threshold. This provides context for recent turn maneuvers and wind-drift effects during line entry without cluttering the display.

---

## 5. Vector Graphics Rendering

### 5.1 API Selection for the Tauri WebView Context

| API | Architecture | Advantages | Drawbacks | Cockpit Use Case |
|---|---|---|---|---|
| **SVG** | Retained mode (DOM) | Perfect infinite scaling, native CSS, built-in hit-testing | Severe degradation > 1000 nodes. GC spikes at 10 Hz updates. | Static UI overlays, text labels, menus |
| **Canvas 2D** | Immediate mode (CPU) | High performance for thousands of primitives. Zero DOM overhead. | Manual hit-testing, custom redraw. No auto-scaling. | **Primary 2D block overview** (flight lines, aircraft icon) |
| **WebGL** | Immediate mode (GPU) | Maximum throughput for 10K+ objects. Required for 3D. | High complexity (GLSL). ARM driver issues. | Future 3D SVS/HITS perspective view |

**Decision: Canvas 2D for the primary map layer.** SVG at 10 Hz with hundreds of flight lines causes DOM layout thrashing and paint cycles, dropping below 60 FPS. Canvas 2D clears and redraws the entire screen in a single animation frame.

**Hit-testing for touch interaction:** Use an R-tree spatial index (already in the GeoPackage architecture) to resolve which flight line intersects the pilot's touch coordinates. No DOM event dependency.

### 5.2 High-DPI Rendering

Cockpit displays viewed at arm's length (24–30 inches) under high-glare need explicit Retina scaling. In Canvas:

1. Multiply the internal pixel buffer dimensions by `devicePixelRatio`.
2. Scale the Canvas context mathematically.
3. Use CSS to shrink the physical element back to 100%.

Without this, lines appear blurry and unreadable under high-frequency aircraft vibration.

### 5.3 Dark Background (Mandatory)

Dark backgrounds reduce glare and visual fatigue. High-contrast vector graphics in bright, saturated colors over deep earth-tone or black backgrounds maximize the luminance contrast ratio. Dark backgrounds also draw less power on OLED screens — reducing thermal footprint on fanless edge hardware in hot climates.

---

## 6. Offline Basemap Strategies

### 6.1 Storage Format Comparison

| Format | Type | Storage Cost | CPU Cost | Suitability |
|---|---|---|---|---|
| Raster tiles (MBTiles) | Pre-rendered PNG/JPEG | High (30–40 GB for state-wide high-zoom) | Very low (direct blit) | Background context |
| Vector tiles (MVT) | Raw geometry | Very low (compact) | High (client-side rendering competes with telemetry updates) | Not recommended for constrained edge hardware |
| GeoPackage raster tiles | Same as MBTiles but inside the universal GeoPackage container | Moderate | Very low | **Recommended** — single-file workflow |

### 6.2 Hybrid Fallback Strategy

1. **Vector primary (default):** Display only operational vector data — flight lines, boundaries, exclusion zones — from the GeoPackage feature tables onto Canvas. Zero basemap storage, minimal processing.
2. **Embedded raster tiles:** For terrain-following context, embed pre-rendered shaded relief tiles (from DEM) in the GeoPackage. Slightly more storage but zero CPU rendering overhead.
3. **GeoTIFF fallback:** A single georeferenced raster image as a sidecar file for simple, low-fidelity background in areas without tile pyramids.

---

## 7. Typography, Color, and Cockpit Ergonomics

### 7.1 Typography for Turbulence

FAA guidelines for electronic flight displays: characters must be at least **16 arc-minutes** in height, with 20–22 arc-minutes preferred. At a 30-inch (760 mm) viewing distance, the minimum physical character height is **3.8 mm**.

Sans-serif fonts are mandatory — serif strokes distort on lower-resolution screens and blur under vibration.

**Recommended font: B612** — developed by Airbus specifically for cockpit readability. Features open counters, high x-height, and distinct letterforms preventing confusion between `I`, `1`, and `l`. Superior to Arial/Inter for cockpit environments.

### 7.2 Color-Blind Safety

~8% of male population has red-green color vision deficiency. Pilots pass stringent color vision tests, but sensor operators and secondary crew may not.

**Rule: Color must never be the sole method of conveying critical information.** Every status indicator needs a secondary visual cue: unique geometric shape, varying stroke thickness, or explicit text label alongside the color.

Specific red/green hues should be adjusted in luminance and saturation to maintain distinguishable contrast ratios for protanopia/deuteranopia.

### 7.3 Touch Targets for Gloved Operation

Flight gloves reduce capacitance and precision. Vibration introduces significant pointing errors.

**Empirical data:** Error rates drop to near-zero at **20–21 mm** square targets. Performance plateaus beyond 22.5 mm (larger wastes screen space without benefit). Below 15 mm, error rates increase exponentially in vibrating cockpits.

**Design constraint:** All interactive elements (line selection, zoom controls, mode toggles) must have a minimum invisible touch hit-box of **20 mm × 20 mm**. The visual icon can be rendered smaller (e.g., 12 mm) for aesthetics, but the active hit area must meet the safety threshold.

---

## 8. 3D Perspective View (SVS / HITS)

### 8.1 Highway-In-The-Sky Efficacy

HITS displays project virtual gates or cubes representing the 3D flight path. Simulator results: pilots using HITS spent **90% of flight time within desired altitude layers** vs 40% with traditional 2D alerts. SVS also reduces Controlled Flight Into Terrain risk by providing terrain visibility regardless of weather.

**Critical hazard — visual fixation:** The 3D tunnel is so compelling that pilots fixate on it to the exclusion of outside visual scanning. In simulator trials, traffic detection was measurably degraded during HITS use. **The 3D view must never replace the primary 2D situational awareness display.**

### 8.2 Hardware Constraints on Edge Devices

WebGL inside Tauri's WebView (Chromium-based) can be unoptimized on Linux/ARM due to driver overhead. Rendering a 3D scene while parsing NMEA, computing geodesics, and broadcasting WebSockets could overwhelm a Raspberry Pi 4/5.

**Mitigations:** Heavily decimated low-poly terrain meshes. Limited HITS cube draw distance. 3D rendering must not starve the core geodetic engine.

### 8.3 Engine Selection

| Framework | Characteristics | Verdict |
|---|---|---|
| **Three.js** | Lightweight, unopinionated, small footprint, fast load times | **Recommended** — minimal overhead for gate/cube rendering on constrained hardware |
| Babylon.js | Full game engine (Microsoft), built-in physics/collision, heavy | Overkill for static cubes + terrain mesh. Starves edge hardware. |

### 8.4 Implementation Status

The 3D SVS/HITS view is a **stretch goal** — prototyped only after the 2D block overview and line navigation views are stable and validated in flight testing. It must integrate traffic and terrain warnings dynamically and must always be available alongside (never replacing) the primary 2D display.
