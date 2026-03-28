# NAD83 Realization Dynamics and Epoch Management for Aerial Survey FMS

> **Source document:** 31_NAD83-Epoch-Management.docx
>
> **Scope:** Complete extraction — all content is unique. Covers the full NAD83 realization history with quantified shifts, 14-parameter Helmert coefficients, epoch propagation math, plate motion model comparison (NNR-NUVEL-1A vs HTDP), the delegation decision, base station mismatch cascade, and the navigational vs analytical boundary.

---

## 1. NAD83 Realization History

NAD83 has never been a single static datum. It has undergone multiple readjustments (realizations) as GNSS precision improved.

| Realization | Operational Span | Primary Method | Network Accuracy |
|---|---|---|---|
| NAD83 (1986) | 1986–1990 | Classical optical triangulation + Transit Doppler | ~1.0 m |
| NAD83 (HARN) | 1990–1997 | State-by-state GNSS integration with classical control | ~0.1 m |
| NAD83 (NSRS2007) | 2007–2011 | Simultaneous nationwide GNSS vector adjustment | 0.01–0.03 m |
| **NAD83 (2011)** | **2011–Present** | **Multi-Year CORS Solution (MYCS), absolute antenna calibrations** | **< 0.01 m** |

### 1.1 NAD83 (1986) — The Original

Utilized classical optical observations and early Transit Doppler. Removed the severe arc-based distortions of NAD27 (shifts of 10–100 m horizontal, ~200 m in UTM coordinates). But accuracy was limited to ~1 m because it predated widespread GPS.

### 1.2 NAD83 (HARN) — State-by-State GPS

Conducted 1990–1997. Integrated dual-frequency GPS baselines with classical control. Pushed accuracy to ~0.1 m. The shift from 1986 to HARN was **0.3–1.0 meters** horizontal, with ellipsoid heights dropping ~0.6 m from VLBI scale adjustments.

**Critical flaw:** Adjustments performed state-by-state in temporal blocks. Discontinuities of **up to 7 cm at state borders** where adjacent adjustments failed to align. HARN is technically not a unified realization — not all values share the same epoch.

### 1.3 NAD83 (CORS96 / NSRS2007)

CORS96 defined by permanent active GNSS tracking stations. NSRS2007 was a simultaneous nationwide GNSS-only adjustment of ~70,000 passive monuments, discarding all legacy optical data. Achieved 0.01–0.03 m accuracy.

**Limitations:** Treated vertical velocities as zero for many stations. Applied a mix of computed and modeled horizontal velocities without rigorous corrections for vertical crustal motion.

### 1.4 NAD83 (2011) Epoch 2010.00 — The Current Standard

Product of the Multi-Year CORS Solution (MYCS): exhaustive reprocessing of all CORS data from January 1994 to April 2011 with latest antenna calibrations and orbit ephemerides. Aligned with IGS08 (functionally equivalent to ITRF2008) at epoch 2005.0, then propagated to reference epoch 2010.00. The 2011 national adjustment incorporated 80,000+ passive stations and 400,000+ GNSS vectors. Network accuracy surpasses 0.01 m.

**3DEP data is delivered in NAD83(2011) epoch 2010.00.** This is the authoritative frame for US domestic survey work.

---

## 2. Coordinate Differences Between Realizations

**NAD83(1986) → NAD83(2011):** > 1.0 m horizontal across CONUS.

**NAD83(CORS96/NSRS2007) → NAD83(2011):**
- Median nationwide: ~2 cm horizontal, ~1.5 cm vertical.
- **California/Oregon coast:** up to **13 cm** horizontal between epochs 2002.0 and 2010.0.
- Geologically volatile areas: some marks shifted > 1 m horizontal and up to 67 cm vertical (coseismic displacement).

**Danger:** Mixing data from CORS96 and 2011 realizations without rigorous transformation introduces a hard geometric bias into the flight planning or photogrammetric pipeline.

---

## 3. The 14-Parameter Helmert Transformation: ITRF → NAD83

### 3.1 Why the Transform is Necessary

GNSS satellites broadcast ephemerides in ITRF/WGS84 (Earth-centered, dynamic). NAD83 is plate-fixed (rotates with the North American plate). A raw GNSS solution at observation epoch `t_obs` must be transformed to NAD83(2011) at reference epoch 2010.0.

### 3.2 Published Helmert Coefficients (IGS08 → NAD83(2011))

Base reference epoch: `t₀ = 1997.0`

| Parameter | Symbol | Value at t₀ | Rate of Change |
|---|---|---|---|
| X Translation | T_x | +0.99343 m | +0.00079 m/yr |
| Y Translation | T_y | −1.90331 m | −0.00060 m/yr |
| Z Translation | T_z | −0.52655 m | −0.00134 m/yr |
| X Rotation | R_x | +25.91467 mas | +0.06667 mas/yr |
| Y Rotation | R_y | +9.42645 mas | −0.75744 mas/yr |
| Z Rotation | R_z | +11.59935 mas | −0.05133 mas/yr |
| Scale | s | ~0 ppb | −0.10201 ppb/yr |

The Y-axis translation (−1.90331 m) reveals the dominant component of the ~2.2 m geocentric offset between NAD83 and ITRF/WGS84.

### 3.3 Time-Dependent Parameter Computation

For observation epoch `t_obs`:

```
T_x(t) = T_x(t₀) + Ṫ_x × (t_obs - t₀)
R_x(t) = R_x(t₀) + Ṙ_x × (t_obs - t₀)
s(t)   = s(t₀)   + ṡ   × (t_obs - t₀)
```

Then apply the standard 7-parameter similarity transformation with these time-dependent values.

---

## 4. Epoch Propagation and Tectonic Drift

### 4.1 The 40-Centimeter Reality

The North American plate drifts ~2.5 cm/year relative to ITRF. For a PPK survey in March 2026 (epoch 2026.2):

```
Drift = 2.5 cm/yr × (2026.2 - 2010.0) = 2.5 × 16.2 = 40.5 cm
```

If an FMS overlays a 2026.2 GNSS trajectory onto a 2010.0 DEM without epoch propagation, the dataset has a **40 cm geometric shear**.

### 4.2 When It Matters

**For real-time navigation:** 40 cm is imperceptible at 80 m/s flight speed and irrelevant to 100 m swath overlap calculations. The assumption of a unified epoch is **safe for flight execution**.

**For post-processing (PPK):** Sub-decimeter accuracy (1–3 cm RMSE for ASPRS) requires the epoch delta to be resolved. A 40 cm bias is **catastrophic** — it invalidates the photogrammetric deliverable.

---

## 5. Base Station Realization Mismatch Cascade

The most common real-world failure mode in aerial survey geodesy:

**Scenario:** Operator deploys a field base station whose coordinates were established in NAD83(CORS96) (epoch ~2002). The flight plan and 3DEP terrain are in NAD83(2011) (epoch 2010.0). The shift between CORS96 and 2011 averages 2 cm nationally but reaches **10–15 cm on the California/Oregon coast**.

**Cascade:**

1. **Base coordinate error:** The base is biased by 10–15 cm relative to 2011.
2. **Trajectory contamination:** PPK software computes the aircraft trajectory relative to the base. The entire 100 Hz trajectory is homogeneously shifted by 15 cm in the opposite direction.
3. **Collinearity violation:** The BBA constrains aerial perspective centers to the shifted trajectory. The resulting DSM/point cloud reflects the CORS96 base geometry, misaligning with 3DEP (2011) ground truth.
4. **ASPRS failure:** Vertical separation between the point cloud and 3DEP flags the deliverable as failing accuracy requirements. The survey is legally invalid.

**Prevention:** All base station coordinates must be transformed via NCAT or NADCON5 from the legacy realization to NAD83(2011) epoch 2010.0 before PPK processing.

---

## 6. Plate Motion Models: NNR-NUVEL-1A vs HTDP

### 6.1 NNR-NUVEL-1A

A global geophysical model describing long-term plate motion averaged over ~3 million years of geological data (marine magnetic anomalies, transform fault azimuths, earthquake slip vectors).

**Strengths:** Mathematically simple (rigid-plate Euler pole rotation). Stable. Used by IERS to constrain ITRF time evolution.

**Fatal weakness:** Assumes rigid plates. Fails completely at plate boundaries where elastic deformation, strain accumulation, and coseismic displacement occur. Present-day GPS observations diverge from NUVEL-1A predictions by several mm/year. Would introduce **decimeter-level errors** in California, Alaska, and the Pacific Northwest.

### 6.2 NGS HTDP

Uses NNR-NUVEL-1A internally for broad plate rotation, but supplements it with:
- Dense 2D rectangular deformation velocity grids from decades of empirical CORS tracking data (captures non-rigid intra-plate deformation at boundaries).
- Coseismic and postseismic slip models for dozens of major earthquakes (M > 6.0) dating back to 1934. Example: the 2002 Denali earthquake moved survey monuments **4 meters** horizontally. HTDP incorporates this dislocation. NUVEL-1A ignores it entirely.

**Weakness for embedding:** Complex legacy Fortran codebase. Relies on large, frequently updated empirical grid files. Embedding would bloat the FOSS repository and create fragile update dependencies.

---

## 7. IronTrack Architectural Decision: Delegation

### 7.1 The Three Options

| Approach | Accuracy | Complexity | IronTrack Decision |
|---|---|---|---|
| Embed NNR-NUVEL-1A | Decimeter-level errors near faults | Trivial (matrix multiplication) | **Rejected** — violates sub-mm mandate |
| Embed HTDP | Sub-cm everywhere | Massive (Fortran port, large grids, constant updates) | **Rejected** — bloats repository, fragile dependencies |
| **Delegate to external tools** | Sub-cm via HTDP/NCAT/TBC | Minimal (metadata recording only) | **Selected** — IronTrack handles macro-level Helmert; operators use HTDP/NCAT for sub-decimeter epoch propagation |

### 7.2 What IronTrack v1.0 Does

1. **Embeds the static 14-parameter Helmert** for macro-level NAD83↔WGS84 translation at a fixed reference epoch.
2. **Records the observation epoch** (`epoch: f64`) in the GeoPackage `irontrack_metadata` table and in every spatial struct.
3. **Forces the operator to acknowledge the epoch** upon boundary or terrain data ingestion.
4. **Documents the limitation:** epoch propagation for sub-decimeter post-processing accuracy must be performed via HTDP/NCAT or Trimble Business Center before finalizing the survey trajectory.

### 7.3 The Navigational vs Analytical Boundary

**For flight execution** (guiding aircraft, triggering sensors): the system safely assumes all coordinates are in the same epoch. The 40 cm tectonic drift is imperceptible for navigation at survey speeds and swath widths.

**For post-processing** (PPK trajectory, BBA, ASPRS compliance): the epoch delta must be resolved externally. IronTrack's role ends at providing the observation epoch metadata and the approximate event log. The operator is responsible for epoch alignment during final processing.

---

## 8. GeoPackage Metadata Requirements

The `irontrack_metadata` table must include:

| Key | Example Value | Purpose |
|---|---|---|
| `horizontal_datum` | `NAD83_2011` | Which NAD83 realization the ingested data uses |
| `vertical_datum` | `NAVD88` | Vertical reference |
| `realization` | `2011` | Specific adjustment (HARN, CORS96, NSRS2007, 2011) |
| `epoch` | `2010.0` | Reference epoch of the coordinate data |
| `observation_epoch` | `2026.2` | Epoch of the GNSS observations (flight date) |

A 3D coordinate in North America is **mathematically meaningless without a 4th dimension: time.** The same physical point on the ground has coordinates that differ by 40 cm between epoch 2010.0 and 2026.2.
