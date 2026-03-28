# GeoPackage Advanced Features: Extensions, Tiling, Concurrency, and Performance

> **Source document:** 42_GeoPackage-Extension-Research.docx
>
> **Scope:** Complete extraction — all content is unique. Covers gpkg_extensions mechanism, irontrack_metadata registration for graceful ignorability, Tiled Gridded Coverage Extension (32-bit float TIFF), offline basemap embedding vs MBTiles sidecar, WAL concurrency patterns, split-brain prevention, R-Tree performance, and VACUUM avoidance strategy.

---

## 1. Extension Mechanism and irontrack_metadata Registration

### gpkg_extensions Table
Every extension must be registered with: table_name, column_name, extension_name, definition URI, and **scope** (read-write or write-only).

- **write-only:** Standard GIS readers (QGIS, ArcGIS) can safely read the file without triggering unrecognized SQL functions.
- **read-write:** Extension alters fundamental interpretation of geometry/coordinates — unaware clients are explicitly blocked.

### irontrack_metadata: Custom Table Strategy

**Official OGC community extension is impractical for v0.2+** — requires Architecture Board approval, open comment periods, and technical committee votes. Instead, use the standard SQLite extension mechanisms to make the table gracefully ignorable.

### Two-Step Registration for Graceful Ignorability

1. **Register in gpkg_contents** with `data_type = 'attributes'`. GDAL/OGR (which underpins QGIS) natively recognizes this as ASPATIAL_VARIANT=GPKG_ATTRIBUTES — a valid non-spatial table. Prevents rendering engines from parsing non-existent geometry columns.

2. **Register in gpkg_extensions** with vendor-specific extension_name (e.g., `irontrack_metadata`), scope = `read-write`. Signals to compliant clients that the extension exists but doesn't preclude reading standard vector features elsewhere in the file.

### Risk Comparison

| Strategy | Compliance | Discoverability | Corruption Risk |
|---|---|---|---|
| Unregistered custom table | Violates core standard | None — invisible or errors | **High** — dropped by VACUUM or third-party edits |
| Registered as `attributes` | Fully compliant (v1.2+) | High — visible in QGIS/ArcGIS | Low — preserved during file ops |
| Official OGC community extension | Fully compliant | Very high | Zero |

**The `attributes` approach is the v0.2+ mandate.** Operators can inspect flight lines in QGIS, modify boundaries, and save without destroying epoch metadata.

---

## 2. Tiled Gridded Coverage Extension (DEM Embedding)

### Purpose
Eliminate external DEM files. The GeoPackage carries its own terrain data — "single artifact" philosophy.

### OGC TGCE Standard
Two ancillary tables:
- **gpkg_2d_gridded_coverage_ancillary:** Global coverage parameters — datatype (int/float), scale, offset, data_null (void value), unit of measure.
- **gpkg_2d_gridded_tile_ancillary:** Per-tile foreign key mapping with individual scale/offset for variable-range mountains in limited integer bit-spaces.

### 32-bit Float TIFF (Mandatory) vs 16-bit PNG (Rejected)

| Parameter | 16-bit PNG | 32-bit Float TIFF |
|---|---|---|
| Data type | Unsigned integer | IEEE floating point |
| Precision | Quantization via scale/offset (rounding errors) | **Absolute physical value (lossless)** |
| Compression | DEFLATE (native PNG) | **LZW** (only permitted extension) |
| Suitability | Insufficient for trajectory math | **Mandatory for ASPRS compliance** |

TIFF constraints: TAG 339 SampleFormat = 3 (float), TAG 258 BitsPerSample = 32, TAG 277 SamplesPerPixel = 1. No NaN/Inf — use data_null for voids. 32-bit float TIFFs do NOT use scale/offset columns — pixel values are exact physical elevations.

### Hybrid Ingestion Pipeline (OGC on Disk → f64 in RAM)

The microtile cache approach (raw f64 SoA arrays memory-mapped directly) is critical for Rayon's sub-millisecond terrain queries. But OGC TIFF tiles require SQLite blob extraction → TIFF header parsing → LZW decompression before access — unacceptable jitter for the real-time control loop.

**Resolution:** Dual-format strategy:
1. **On disk:** OGC-compliant 32-bit float TIFF (interoperable with QGIS/GDAL).
2. **At daemon startup (pre-flight):** Synchronous blocking init phase — extract TIFF blobs, decompress LZW, cast 32-bit → f64, build contiguous SoA grid in RAM.
3. **During flight:** All terrain queries hit the zero-latency f64 memory map. LZW decompression cost is paid once, before the aircraft leaves the tarmac.

---

## 3. Offline Basemap: Embedded GeoPackage vs MBTiles Sidecar

### MBTiles Limitations
- **CRS rigidity:** Strictly EPSG:3857 (Web Mercator) — massive distortion at high latitudes (northern Canada surveys).
- **Homogeneous:** Single tileset, single format per file.
- **No vector data:** Can't store flight lines or metadata alongside tiles.
- **Sidecar risk:** Operator uploads flight plan GeoPackage but forgets the multi-gigabyte .mbtiles file → pilot flies over a blank canvas.

### GeoPackage Tile Matrix Set (Embedded) — The Mandate
- **Single artifact:** Basemap lives inside the same file as flight lines, DEM, and metadata.
- **Universal CRS:** Supports WGS84, UTM, any valid EPSG — no on-the-fly reprojection overhead.
- **Mixed formats:** GDAL AUTO mode uses JPEG for opaque tiles, PNG for edge tiles with alpha transparency.
- **Multiple tilesets:** Register separate basemap layers (satellite, topo, blank) in the same file.

### irontrack:// Custom URI Protocol
Tauri registers a custom protocol interceptor at the OS level. MapLibre GL JS requests `irontrack://tiles/basemap/14/2345/3456.png` → Tauri intercepts before TCP/IP → Rust backend queries tile_data BLOB from SQLite → returns binary directly to WebView with MIME headers. **Zero-latency embedded offline tile server** — sustains 60 FPS map panning without external HTTP server, IPC serialization, or CORS restrictions.

---

## 4. Concurrent Access: WAL Mode and Split-Brain Prevention

### WAL Mode (Mandatory)
`PRAGMA journal_mode=WAL;` on daemon startup.

- **Snapshot isolation:** Readers register an "end mark" in the WAL and operate against an immutable snapshot. Completely unblocked by concurrent writes.
- **wal-index:** Shared memory (-shm) file enables millisecond page lookup without scanning the full WAL.
- **Non-blocking reads:** Critical for cockpit display — trigger event writes never freeze the pilot's navigation instruments.

### Single-Writer Constraint
Only one process can append to the WAL at any microsecond. IronTrack centralizes all writes through the daemon via `Arc<RwLock<FmsState>>`. WebSocket/REST clients submit payloads to the daemon, which sequences writes sequentially.

### Split-Brain Prevention
If two daemon instances mount the same GeoPackage:
- First writer acquires lock. Second receives **SQLITE_BUSY**.
- Blind retry under load risks the **WAL-reset bug** — wal-index header set incorrectly during overlapping checkpoint + WAL reset → silent transaction skips → permanent, unrecoverable database corruption.
- WAL mode also **fails catastrophically on network filesystems** (NFS/SMB) — wal-index relies on OS-level shared memory primitives.

**Mandate:** SQLITE_BUSY in the write path is NOT a transient networking delay. It is a **fatal architectural violation**. Trap it immediately → ERR_DB_LOCKED → fail-safe shutdown of the secondary daemon to protect mission data.

---

## 5. Size Limits and Performance

### Theoretical Limit
SQLite max file size: 281 TB. Irrelevant for field operations.

### Practical Performance: R-Tree Spatial Index
- R-Tree (R*Tree module) creates a hierarchical bounding-box index — logarithmic query time instead of sequential table scan.
- **10–50 GB GeoPackages perform well** with properly constructed R-Tree indexes and contiguous pages.
- Without R-Tree: query times degrade linearly with file size → unacceptable cockpit display lag.

### VACUUM: Catastrophic in Flight

When rows are deleted, SQLite marks blocks as "free pages" but file size doesn't shrink. Over multi-week campaigns, fragmentation destroys sequential cache locality → inflated I/O for tile streaming.

**VACUUM is the standard fix but MUST NEVER run during active deployment:**
1. **Exclusive lock:** Disables WAL concurrency entirely → freezes pilot's glass cockpit for minutes/tens of minutes on a 10 GB file.
2. **Storage doubling:** Creates a full duplicate during defragmentation → 15 GB GeoPackage needs 15 GB free space. If SSD lacks headroom → crash → potential corruption.

### PRAGMA optimize (The Alternative)
Lightweight, non-blocking. Analyzes tables and indexes to update internal statistics for the query planner. Does NOT physically move data or lock the database.

**Schedule:** Trigger automatically on graceful connection close, or via internal timer (every ~6 hours of idle time). Pair with `PRAGMA auto_vacuum = NONE` to prevent unexpected I/O spikes.

---

## 6. IronTrack GeoPackage Architecture Summary (v0.2+)

| Layer | Implementation | Purpose |
|---|---|---|
| Flight lines + metadata | Standard gpkg vector features | Core mission data |
| irontrack_metadata | Registered as `attributes` in gpkg_contents + gpkg_extensions | Schema version, epoch, safety warnings |
| Terrain DEM | Tiled Gridded Coverage Extension (32-bit float TIFF, LZW) | Terrain-following, AGL computation |
| Offline basemap | GeoPackage Tile Matrix Set (JPEG/PNG mixed) | Glass cockpit visual context |
| Spatial index | R*Tree virtual table | Logarithmic obstacle/feature queries |
| Concurrency | WAL mode, single-writer daemon, SQLITE_BUSY = fatal | Non-blocking cockpit reads |
| Maintenance | PRAGMA optimize (lightweight) — no VACUUM in flight | Sustained performance over multi-week campaigns |
