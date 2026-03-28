# Airspace Integration, Geofencing, and Regulatory Constraint Architecture

> **Source document:** 35_Airspace-Obstacle-Data.docx
>
> **Scope:** Complete extraction — all content is unique. Covers FAA airspace classification ingestion, altitude geodetic reconciliation, TFR/NOTAM 4D intersection, Digital Obstacle File integration, advisory-vs-enforcement philosophy, cockpit alerting per AC 25-11B, and Transport Canada expansion roadmap.

---

## 1. Core Philosophy: Advisory Only, Never Enforcement

**The PIC has ultimate authority.** IronTrack never enforces geofences by refusing to generate waypoints, deleting planned lines, or stopping camera/LiDAR triggers. A pilot may need to enter controlled or restricted airspace in an emergency. Legitimate survey operations frequently fly inside Class B/C/D under pre-arranged ATC clearances.

When a flight plan intersects controlled airspace, the system generates a clear visual warning. The planner acknowledges the penetration, signaling intent to acquire clearances. The mathematical intersection logic enhances awareness — it never restricts pilot agency.

---

## 2. FAA Airspace Classification

| Class | Structure | Vertical Limits | IronTrack Rendering |
|---|---|---|---|
| A | High-altitude IFR-only | FL180–FL600 | Solid red; typically hidden in low-level survey profiles |
| B | "Inverted wedding cake" multi-layer | Surface–10,000 ft MSL + 30 NM Mode C veil | Solid blue multi-polygon with altitude labels per tier |
| C | Inner core + outer shelf | Core: surface–4,000 AGL. Shelf: 1,200–4,000 AGL | Solid magenta multi-polygon |
| D | Single cylindrical boundary | Surface–2,500 AGL (charted MSL) | Dashed blue cylinder |
| E | Fills gaps for IFR | Generally 700 or 1,200 AGL to FL180 | Dashed/shaded magenta vignettes |
| G | Uncontrolled | Surface to overlying Class E floor | Implied by absence of other boundaries |

---

## 3. Data Ingestion Pipeline

### 3.1 FAA ADDS REST API

Source: ArcGIS Hub endpoints (hub.arcgis.com). GeoJSON format, natively compatible with Rust serde and MapLibre GL JS.

**Pagination:** APIs cap at 1,000–4,000 features per request. The ingestion module must use recursive offset querying to reconstruct the complete national feature collection.

**Update cycles:**
- Airspace boundaries: 56-day cycle (synchronized with chart releases)
- NASR subscriber files (airports, frequencies, procedures): 28-day AIRAC cycle

### 3.2 Airspace Altitude Geodetic Reconciliation

Airspace ceilings and floors use MSL, AGL, or Flight Level — all must be converted to WGS84 Ellipsoidal for the Rust engine's 3D spatial queries.

| Altitude Type | Conversion Path |
|---|---|
| **MSL** (≈ NAVD88) | Apply GEOID18 or EGM2008 undulation → WGS84 Ellipsoidal |
| **AGL** | Query DEM at boundary coordinates → add AGL value → apply geoid undulation |
| **Flight Level** | Static ISA approximation during planning; **dynamic recalculation during flight** using live NMEA telemetry and local altimeter setting |

All converted boundaries are serialized into the GeoPackage R-tree spatial index alongside flight lines and DEM tiles.

---

## 4. Temporary Flight Restrictions and Digital NOTAMs

### 4.1 NMS/SWIM Modernization

The FAA is replacing legacy teletype NOTAMs with the cloud-based NOTAM Management Service (NMS), standardizing on AIXM 5.1 (XML schema with precise geospatial polygons and absolute timestamps). Available via SWIM REST APIs and JMS queues. Third-party aggregators (Laminar Data Hub, DTN) offer AIXM→GeoJSON translation.

### 4.2 TFR Ingestion

TFRs (14 CFR 91.137–91.145) cover disasters, space ops, stadiums, VIP movements. Dedicated data feed at tfr.faa.gov (JSON/XML). Geometries range from simple circles (AIXM CircleByCenterPoint) to complex multi-vertex polygons (PolygonPatch/LinearRing).

### 4.3 4D Temporal-Spatial Intersection

A TFR is a 4D construct: 3D geometry + effectiveStart/effectiveEnd timestamps.

**Algorithm:**
1. Compute the aircraft's ETA at the TFR spatial boundary (using planned ground speed, cumulative flight plan length, scheduled mission start time).
2. Evaluate: does the ETA interval overlap with the TFR's active time window?
3. **Only flag the line as compromised if both spatial AND temporal intersections are true.**

This eliminates alert fatigue from stadium TFRs active on days the mission isn't flying.

### 4.4 Hybrid Synchronization Model

1. **Plan-time ingestion:** During office planning, the daemon queries NMS/SWIM APIs for all NOTAMs/TFRs intersecting the project bounding box. Serialized into the GeoPackage.
2. **Pre-flight validation:** The irontrack_metadata table embeds the UTC timestamp of the last successful NOTAM sync.
3. **Live delta updates:** If the aircraft has network (Iridium, airborne LTE, ramp WiFi), the daemon fetches NOTAMs issued since the GeoPackage creation timestamp and hot-patches the mission state.
4. **Offline fallback:** If no network, the system uses cached GeoPackage data and warns the pilot about NOTAM cache staleness. **The pilot remains legally responsible for a standard pre-flight briefing via Flight Service.**

---

## 5. Digital Obstacle File (DOF) Integration

### 5.1 Data Source

FAA Daily Digital Obstacle File (DDOF): CSV format, WGS84 coordinates, includes AGL and MSL heights.

**Verification status:**
- "O" / "V" = verified (geodetically surveyed and confirmed) → solid red icons
- "U" = unverified (reported but not confirmed) → hollow amber icons

**Both types must be ingested.** Unverified structures pose equal physical hazard. Distinct symbology communicates data reliability to the pilot.

**Update cycle:** 56-day base file + daily DDOF change files. The Axum daemon executes automated scheduled fetches and merges changes into its local spatial database.

### 5.2 Buffer Volumes

A single dimensionless point is inadequate — guy-wires, structure width, and coordinate uncertainty require safety buffers.

**Buffer radius: 10× the obstacle's AGL height.** Example: 150 m AGL antenna → 1,500 m cylindrical avoidance volume.

The rayon parallel processing loop casts 3D flight lines against thousands of buffered obstacle cylinders simultaneously. Intersecting waypoints are tagged with severe hazard warnings. The glass cockpit renders the obstacle with a semi-transparent hazard perimeter ring.

### 5.3 Mission Packaging

When exporting to the field, the Rust engine spatially queries its local obstacle database for the survey bounding box and embeds only the relevant obstacles into the mission GeoPackage's R-tree index. Microsecond-latency spatial queries during flight without massive embedded storage.

---

## 6. Maximum Altitude Constraints

### 6.1 Part 107 (UAS): 400 ft AGL Ceiling

With structural exception: may fly above 400 ft AGL if within 400 ft lateral radius of a structure, not exceeding 400 ft above the structure's top.

IronTrack fuses DEM + DOF data: if a DOF structure is at 1,200 ft AGL, the planning engine dynamically expands the legal ceiling to 1,600 ft AGL within a 400 ft 3D buffer of the structure's coordinates. Lines extending beyond this buffer at 1,600 ft are instantly flagged.

### 6.2 Part 91/135 (Manned): Minimum Safe Altitudes

- Congested areas: 1,000 ft above highest obstacle within 2,000 ft horizontal radius.
- Uncongested areas: 500 ft above surface.
- VFR cloud clearances: 500 ft below, 1,000 ft above, 2,000 ft horizontal.

### 6.3 Dynamic AGL Computation

The daemon processes NMEA GGA at 10–100 Hz, queries the DEM at the current position, subtracts terrain from WGS84 ellipsoidal height (corrected via GEOID18) → true instantaneous AGL. Used for advisory warnings only — the React UI flags line segments violating MSA in red during planning; the cockpit VDI renders an amber hazard zone if AGL projects below MSA.

---

## 7. Glass Cockpit Alerting (AC 25-11B Compliant)

### 7.1 Time-to-CPA Proximity

Not static distance alerts. The Rust engine continuously projects the aircraft's forward vector (ground speed + COG + vertical speed from 1000 Hz dead reckoning) and evaluates temporal intersection with 3D R-tree airspace/obstacle boundaries.

**Default threshold:** 3 minutes to boundary penetration + 500 ft vertical buffer.

### 7.2 Alert Hierarchy

| Level | Color | Audio | Trigger |
|---|---|---|---|
| **Warning** | Red (flashing) | Continuous loud tone | Imminent collision with verified obstacle, prohibited area/TFR penetration, severe terrain conflict |
| **Caution** | Amber | Single/repeating chime | 3-minute approach to TFR/controlled airspace without known clearance; RTK degradation |
| **Advisory** | Green/Cyan/White | Optional low-intrusion | Upcoming line start threshold; approaching uncontrolled Class G boundaries |

**Red and amber are NEVER used for non-alerting UI elements** (route lines, topographic features). This preserves the attention-getting characteristic of warnings.

### 7.3 Acknowledgment Workflow

Caution alerts display a 20 mm touch hit-box. Pilot taps to acknowledge → silences the aural tone → amber visual banner remains pinned until the aircraft diverges from the conflict vector. If the situation escalates to actual penetration → banner shifts to red, text flashes, continuous tone until evasive action.

---

## 8. International Expansion: Transport Canada (v2.0)

### 8.1 Current State

Canadian airspace data is fragmented: Designated Airspace Handbook (DAH, TP 1820) published as PDFs, Open Government Portal raw datasets, legacy OpenAir/SUA text formats. No standardized GeoJSON API equivalent to FAA ADDS.

### 8.2 v1.0 Strategy

Validate the core pipeline against FAA data. The React web UI supports drag-and-drop ingestion of GeoJSON, KML, and Shapefiles — operators on Canadian missions manually overlay official boundary files from the Open Government Portal or NavCanada.

### 8.3 v2.0 Strategy

Introduce pluggable **Airspace Provider** traits in the Rust engine. Developers write dedicated ETL pipelines for NavCanada, EuroControl, and other international authorities, normalizing global data into the unified WGS84 GeoPackage format without rewriting the core spatial query logic.
