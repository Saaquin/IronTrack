# GeoPackage Implementation Guide

**Source modules:** `src/gpkg/` — `init.rs`, `binary.rs`, `rtree.rs`
**Source docs:** 04, 42, 45

## Database Initialization
PRAGMAs (execute immediately on connection open):
```sql
PRAGMA application_id = 1196444487;  -- 0x47504B47 = "GPKG"
PRAGMA user_version = 10300;          -- GeoPackage 1.3.0
PRAGMA journal_mode = WAL;            -- MANDATORY for concurrent access [Doc 42]
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -64000;           -- 64MB page cache
PRAGMA page_size = 4096;
```

## WAL Concurrency Architecture [Doc 42]
- Snapshot isolation: readers see immutable snapshot, unblocked by writes.
- Single-writer via `Arc<RwLock<FmsState>>`. WebSocket/REST clients submit to daemon.
- **SQLITE_BUSY = FATAL architectural violation.** Not a transient retry. WAL-reset bug
  risk causes permanent unrecoverable corruption. Trap immediately → ERR_DB_LOCKED →
  fail-safe shutdown of the offending process.
- WAL fails on NFS/SMB — GeoPackage MUST be local filesystem.
- Use dedicated SQLite connection pool (e.g., `deadpool-sqlite`) to prevent Tokio thread
  contention. Never share a single connection across async tasks.

## Extension Registration [Doc 42]
irontrack_metadata registered in gpkg_contents with `data_type='attributes'` and in
gpkg_extensions with `scope='read-write'`. Ensures graceful ignorability by QGIS/ArcGIS.

## Tiled Gridded Coverage Extension [Doc 42]
DEM embedded via OGC TGCE. **32-bit float TIFF mandatory** (TAG 339=3, TAG 258=32,
single-band, LZW only). 16-bit PNG rejected (quantization errors). Hybrid pipeline:
OGC TIFF on disk → at daemon startup, decompress LZW, cast 32→f64, build ephemeral
contiguous SoA grid in RAM for zero-latency runtime queries.

## Offline Basemap [Doc 42]
MBTiles REJECTED (EPSG:3857 rigidity, sidecar human-error risk). GeoPackage Tile Matrix
Set embeds basemap alongside vector features. `irontrack://` custom URI protocol serves
tiles from Tauri backend to MapLibre with zero network latency.

## Performance [Doc 42]
- R-tree handles 10–50 GB with negligible degradation.
- **VACUUM PROHIBITED during active flight** — exclusive lock + storage doubling.
- `PRAGMA optimize` on graceful close or every ~6 hours. `PRAGMA auto_vacuum = NONE`.
- Bulk inserts: explicit `BEGIN TRANSACTION`/`COMMIT` blocks. [Doc 52]

## Sensor Library Tables [Doc 45]
- `irontrack_sensors`: Normalized columns (name, manufacturer, type, sensor_width_mm,
  pixel_pitch_um, focal_lengths_mm JSON array, prr_hz, trigger_interface, event_output).
  Inside the GeoPackage (not separate DB) to prevent split-brain.
- `irontrack_mission_profiles`: Dedicated table with FK to sensors. Core params as relational
  columns, custom overrides as JSON column. NOT a JSON blob in irontrack_metadata.

## Required irontrack_metadata Rows
schema_version, engine_version, created_utc, horizontal_datum, vertical_datum, epoch,
observation_epoch, copernicus_attribution, safety_dsm_warning, notam_sync_utc.
