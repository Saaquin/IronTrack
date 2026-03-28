# 3DEP Data Integration Architecture for IronTrack FMS

> **Source document:** 33_3DEP-Data-Integration.docx
>
> **Scope:** Complete extraction — all content is unique. Covers data access modalities (TNM API, ScienceBase, AWS S3 COG), resolution tier fallback, COG HTTP range request ingestion, microtile caching, vertical datum transformation pipeline, and multi-source terrain fusion with Copernicus.

---

## 1. Data Access Modalities

### 1.1 TNM Access API (Metadata Discovery)

**Endpoint:** `GET https://tnmaccess.nationalmap.gov/api/v1/products`

| Parameter | Value | Purpose |
|---|---|---|
| `bbox` | `min_lon,min_lat,max_lon,max_lat` | Spatial filter in WGS84 (EPSG:4326) |
| `datasets` | `"1 meter DEM"`, `"1/3 arc-second DEM"`, `"1 arc-second DEM"` | Resolution tier targeting |
| `prodFormats` | `"GeoTIFF"` | Target COG format, discard legacy ArcGrid/IMG |
| `outputFormat` | `JSON` | Structured response for Rust serde parsing |

Returns product metadata with download URLs. Effective for discovery, but download URLs route through USGS staging servers with rate limiting — suboptimal for parallelized high-throughput ingestion.

### 1.2 ScienceBase Catalog API (Secondary/Manual Override)

**Endpoint:** `GET https://www.sciencebase.gov/catalog/items`

Useful for bespoke LiDAR projects not yet in the TNM pipeline (e.g., municipal collections). Requires recursive parent/child item traversal to locate raster binaries — adds latency and parsing complexity. **Relegated to manual-override data source, not the primary automated path.**

### 1.3 AWS S3 COG Direct Access (Primary Ingestion Path)

USGS migrated standard DEMs to Cloud-Optimized GeoTIFF on AWS S3 in 2019–2020. Direct S3 access via HTTP range requests:
- Bypasses institutional rate limits
- Enables parallel async retrieval via tokio
- Exploits COG internal structure for minimal bandwidth
- Critical for field laptops on cellular/satellite uplinks

**This is IronTrack's primary ingestion architecture.**

---

## 2. Resolution Tiers

| Tier | Spacing | Coverage | Source |
|---|---|---|---|
| **1-Meter** | ~1.0 m | ~70% CONUS, project-based (not seamless across projects) | LiDAR QL2+ |
| **1/3 Arc-Second** | ~10.0 m | Full CONUS, Hawaii, PR, territories. Nationally seamless. | Blended from highest available |
| **1 Arc-Second** | ~30.0 m | Full CONUS + partial Alaska + Canada/Mexico | Seamless, transborder |
| **2 Arc-Second** | ~60.0 m | Alaska interior only | Lower-res source data |

Note: USGS is developing a Seamless 1-Meter (S1M) product to eliminate inter-project artifacts, but it is not yet available for current IronTrack deployment.

---

## 3. Bounding Box Fallback Strategy

When an operator defines a mission boundary, the engine autonomously sources the optimal terrain:

**Step 1 — Primary 1-Meter Query:** Query TNM API for "1 meter DEM" intersecting the bounding box. Returns metadata for available 1-meter project tiles.

**Step 2 — Geometric Coverage Validation:** Compute the union of all returned 1-meter tile footprints (via `geo-types` and `geo` Rust crates). Calculate intersection with the flight plan bounding box. If 100% covered → proceed with 1-meter COGs.

**Step 3 — 1/3 Arc-Second Fallback:** If 1-meter coverage has voids (common at county lines, remote forests without LiDAR), **reject the partial 1-meter dataset entirely** and fall back to the 1/3 arc-second seamless tier. A DEM void causes the altitude to drop to zero/NoData → the autopilot commands descent into unmapped terrain. **Trajectory safety takes absolute precedence over resolution.**

**Step 4 — Alaska/Transborder Escalation:** Remote Alaska → 1 arc-second or 2 arc-second. Missions crossing into Canada/Mexico → 1 arc-second (the only tier with transborder coverage).

**The FMS never generates a flight line over mathematically undefined topography.**

---

## 4. COG Ingestion Mechanics

### 4.1 COG Internal Structure

A COG places the IFD, projection parameters, and byte-offset tables at the beginning of the file. Pixel data is organized into independent **512×512 pixel internal tiles** (not sequential strips). This structure allows treating a remote multi-GB file as an indexed spatial database.

### 4.2 HTTP Range Request Pipeline

**Step 1 — Header Fetch:** Issue `Range: bytes=0-16384` to retrieve only the TIFF header, IFD, and tile byte-offset arrays. No pixel data downloaded.

**Step 2 — Spatial Intersection:** Decode the IFD in memory. Project the flight bounding box into the COG's native UTM CRS. Evaluate the geotransform matrix to identify which 512×512 tiles intersect the boundary. Map target tiles to their absolute byte offsets.

**Step 3 — Parallel Chunk Retrieval:** Spawn a pool of async tokio tasks. Each issues an HTTP 206 Partial Content request for a specific byte range (one 512×512 tile). Dozens of compressed blocks download simultaneously, maximizing TCP window scaling.

**Result:** Transfer reduced from gigabytes (entire tile) to megabytes (only the intersecting 512×512 blocks). Enables rapid mission planning on degraded cellular networks.

### 4.3 Microtile Cache in GeoPackage

Retrieved 512×512 COG tiles are decompressed (LZW/DEFLATE) and resampled into standard **256×256 Web Mercator microtiles** (XYZ/TMS schema). Dual-encoded:

- **Visual layer:** Compressed color-mapped PNGs for the glass cockpit offline basemap.
- **Mathematical layer:** Dense f64 SoA byte arrays for the core engine's elevation lookups during trajectory planning and AGL calculations.

Both stored in the GeoPackage with an R-tree spatial index. Sub-millisecond elevation queries at any coordinate, completely offline.

---

## 5. Vertical Datum Transformation Pipeline

### 5.1 The Problem

3DEP data is in NAVD88 (vertical) / NAD83 (horizontal). IronTrack's canonical internal datum is WGS84 Ellipsoidal. Every ingested DEM pixel must be transformed.

### 5.2 Two-Step Conversion (Per Pixel)

**Step 1 — GEOID18 undulation:**
```
h_NAD83_ellipsoidal = H_NAVD88_orthometric + N_GEOID18(lat, lon)
```
Biquadratic interpolation (3×3 quadrant-aware window) on the mmap'd 34.3 MB Big-Endian binary grid. EGM2008 is NOT a substitute — discrepancy exceeds 1 m in the Pacific Northwest.

**Step 2 — 14-Parameter Helmert:**
```
(X, Y, Z)_WGS84 = Helmert_14param( (X, Y, Z)_NAD83, t_obs, t_ref=2010.0 )
```
Converts from plate-fixed NAD83(2011) to geocentric WGS84. Accounts for the 2.2 m geocentric offset and 2.5 cm/year tectonic drift. Skipping this introduces 1.0–2.2 m horizontal error.

### 5.3 Preprocessing Architecture (Not On-the-Fly)

All datum transformations are executed **once during ingestion** on rayon parallel threads. The final WGS84 Ellipsoidal values are cached permanently in the GeoPackage microtiles.

**On-the-fly Helmert computation during active flight is explicitly prohibited.** It would starve the NMEA parsing loops and trigger controller serial communication, causing delayed sensor triggering and failed spatial intervals.

---

## 6. Multi-Source Terrain Fusion: 3DEP + Copernicus

### 6.1 When Fusion is Required

Transborder operations: Pacific Northwest forestry (US-Canada), Great Lakes hydrology, southern border surveys. 3DEP coverage terminates at the international border. Copernicus GLO-30 fills the gap.

### 6.2 Datum Homogenization

Both datasets routed through the ingestion graph into WGS84 Ellipsoidal:

| Source | Conversion Path |
|---|---|
| **3DEP** (NAVD88/NAD83) | + GEOID18 N → NAD83 ellipsoidal → 14-param Helmert → WGS84 |
| **Copernicus** (EGM2008/WGS84) | + EGM2008 N (bicubic) → WGS84 ellipsoidal |

Once both are in the same geometric space, they can be fused.

### 6.3 Resolution Mismatch Handling (1 m vs 30 m)

Directly abutting 1 m LiDAR against 30 m SAR creates abrupt topological artifacts. The autopilot interprets the resolution discontinuity as a terrain cliff → aggressive pitch correction.

**Solution — transition zone feathering (MBlend-style):**

1. Detect where the bounding box intersects both datasets.
2. Construct a spatial transition corridor along the boundary.
3. Compute the vertical delta between 3DEP and Copernicus surfaces.
4. Apply bilinear feathering: 100% 3DEP at the near edge → gradual blend → 100% Copernicus at the far edge.
5. The result is a C⁰ continuous terrain gradient — no pitch chatter from raw stitching.

### 6.4 Precedence Rules

**3DEP always takes absolute precedence over Copernicus** where both are available.

**Reason 1 — Resolution:** 1 m vs 30 m. Tighter, safer terrain-following.

**Reason 2 — DTM vs DSM safety:** 3DEP is a LiDAR-derived bare-earth DTM (true ground surface). Copernicus is an X-band SAR DSM (canopy top + structures). X-band penetrates only 2–8 m into forest canopy — flying terrain-following on a DSM is inherently dangerous over forest. When Copernicus is the only source, IronTrack embeds a mandatory non-dismissable DSM penetration warning in the GeoPackage and displays an amber hazard zone on the cockpit VDI tape.
