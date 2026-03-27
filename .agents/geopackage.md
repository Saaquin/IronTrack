# GeoPackage Implementation Guide

## Database Initialization
A GeoPackage is a SQLite 3 database with specific header bytes and mandatory tables.

### PRAGMAs (execute immediately on connection open)
```sql
PRAGMA application_id = 1196444487;   -- 0x47504B47 = ASCII "GPKG"
PRAGMA user_version = 10300;           -- GeoPackage 1.3.0
PRAGMA journal_mode = WAL;             -- Write-Ahead Log (critical for perf)
PRAGMA synchronous = NORMAL;           -- Safe with WAL, avoids fsync per write
PRAGMA cache_size = -64000;            -- 64MB page cache
PRAGMA page_size = 4096;               -- Must be set before any table creation
```

### Mandatory Tables
**gpkg_spatial_ref_sys** — CRS dictionary. Seed with 3 required records:
- srs_id=4326: WGS 84 (EPSG:4326) with full WKT definition
- srs_id=-1: Undefined Cartesian
- srs_id=0: Undefined Geographic

**gpkg_contents** — Master inventory. One row per feature/tile/attribute table.
Columns: table_name, data_type ('features'), identifier, description, last_change,
min_x, min_y, max_x, max_y, srs_id.

**gpkg_geometry_columns** — Maps table → geometry column. Columns: table_name,
column_name ('geom'), geometry_type_name ('LINESTRING'), srs_id, z (0/1/2), m (0/1/2).

**gpkg_extensions** — Extension registry. Used for R-tree declaration.

## StandardGeoPackageBinary Format
Every geometry stored as a BLOB with this exact byte layout:

### Header
| Offset | Size | Field | Value |
|--------|------|-------|-------|
| 0-1 | 2 bytes | Magic | 0x47 0x50 ("GP") |
| 2 | 1 byte | Version | 0x00 (version 1) |
| 3 | 1 byte | Flags | Bitfield (see below) |
| 4-7 | 4 bytes | SRS ID | int32, matches gpkg_spatial_ref_sys.srs_id |
| 8-n | variable | Envelope | f64 sequence (size depends on flags) |
| n+ | variable | WKB | Standard ISO Well-Known Binary |

### Flags Byte (byte 3) — bit layout from MSB to LSB
```
Bit 7-6: Reserved (R) = 00
Bit 5:   Extended type (X) = 0 for standard geometries
Bit 4:   Empty geometry (Y) = 1 if empty, 0 otherwise
Bit 3-1: Envelope code (E):
         0 = no envelope (0 bytes)
         1 = 2D [minx, maxx, miny, maxy] (32 bytes)
         2 = 3D with Z [minx, maxx, miny, maxy, minz, maxz] (48 bytes)
         3 = 2D with M [minx, maxx, miny, maxy, minm, maxm] (48 bytes)
         4 = 4D [minx, maxx, miny, maxy, minz, maxz, minm, maxm] (64 bytes)
Bit 0:   Byte order (B) = 1 for little-endian, 0 for big-endian
```

For flight lines, use: B=1 (LE), E=1 (2D envelope), Y=0, X=0, R=00
→ Flags byte = 0b0000_0011 = 0x03

### Constructing the flags byte in Rust:
```rust
let flags: u8 = (byte_order & 0x01)
    | ((envelope_code & 0x07) << 1)
    | ((empty as u8) << 4);
```

### WKB Geometry Types (little-endian type codes)
- Point: 1
- LineString: 2
- Polygon: 3
- MultiPoint: 4
- MultiLineString: 5
- MultiPolygon: 6

### WKB LineString Layout
```
[1 byte]  byte order (0x01 = LE)
[4 bytes] geometry type (2 = LineString, as u32 LE)
[4 bytes] num_points (as u32 LE)
[num_points × 16 bytes] coordinates (f64 x, f64 y pairs in LE)
```

### Empty Geometry Encoding
When serializing POINT EMPTY:
- Flags: Y=1, E=0 (no envelope)
- WKB coordinate values = IEEE-754 quiet NaN
- Little-endian NaN bytes: 0x00 0x00 0x00 0x00 0x00 0x00 0xF8 0x7F

### Coordinate Axis Order
GeoPackage WKB always uses (x, y) = (easting/longitude, northing/latitude).
This matches GeoJSON [lon, lat] but is opposite to many APIs that take (lat, lon).
Double-check every coordinate write.

## R-Tree Spatial Index
Virtual table using SQLite R*Tree module. Required for any GeoPackage that external
GIS software will render efficiently.

### Setup
```sql
-- Register extension
INSERT INTO gpkg_extensions (table_name, column_name, extension_name, definition, scope)
VALUES ('<t>', '<c>', 'gpkg_rtree_index',
        'http://www.geopackage.org/spec/#extension_rtree', 'write-only');

-- Create virtual table
CREATE VIRTUAL TABLE rtree_<t>_<c> USING rtree(id, minx, maxx, miny, maxy);
```

### All 7 Synchronization Triggers
Replace `<t>` with table name, `<c>` with geometry column, `<i>` with PK column.

```sql
-- 1. INSERT: new row with geometry
CREATE TRIGGER rtree_<t>_<c>_insert AFTER INSERT ON <t>
WHEN NEW.<c> NOT NULL AND NOT ST_IsEmpty(NEW.<c>)
BEGIN
  INSERT INTO rtree_<t>_<c> VALUES (
    NEW.<i>, ST_MinX(NEW.<c>), ST_MaxX(NEW.<c>),
    ST_MinY(NEW.<c>), ST_MaxY(NEW.<c>));
END;

-- 2. UPDATE1: geometry changed, PK unchanged, both non-null
CREATE TRIGGER rtree_<t>_<c>_update1 AFTER UPDATE OF <c> ON <t>
WHEN OLD.<i> = NEW.<i> AND NEW.<c> NOT NULL AND NOT ST_IsEmpty(NEW.<c>)
  AND OLD.<c> NOT NULL AND NOT ST_IsEmpty(OLD.<c>)
BEGIN
  UPDATE rtree_<t>_<c> SET
    minx=ST_MinX(NEW.<c>), maxx=ST_MaxX(NEW.<c>),
    miny=ST_MinY(NEW.<c>), maxy=ST_MaxY(NEW.<c>)
  WHERE id = NEW.<i>;
END;

-- 3. UPDATE2: PK changed, both geometries valid
CREATE TRIGGER rtree_<t>_<c>_update2 AFTER UPDATE OF <c> ON <t>
WHEN OLD.<i> != NEW.<i> AND NEW.<c> NOT NULL AND NOT ST_IsEmpty(NEW.<c>)
  AND OLD.<c> NOT NULL AND NOT ST_IsEmpty(OLD.<c>)
BEGIN
  DELETE FROM rtree_<t>_<c> WHERE id = OLD.<i>;
  INSERT INTO rtree_<t>_<c> VALUES (
    NEW.<i>, ST_MinX(NEW.<c>), ST_MaxX(NEW.<c>),
    ST_MinY(NEW.<c>), ST_MaxY(NEW.<c>));
END;

-- 4. UPDATE3: PK changed, new geometry valid, old was null
CREATE TRIGGER rtree_<t>_<c>_update3 AFTER UPDATE OF <c> ON <t>
WHEN OLD.<i> != NEW.<i> AND NEW.<c> NOT NULL AND NOT ST_IsEmpty(NEW.<c>)
  AND (OLD.<c> IS NULL OR ST_IsEmpty(OLD.<c>))
BEGIN
  INSERT INTO rtree_<t>_<c> VALUES (
    NEW.<i>, ST_MinX(NEW.<c>), ST_MaxX(NEW.<c>),
    ST_MinY(NEW.<c>), ST_MaxY(NEW.<c>));
END;

-- 5. UPDATE4: PK changed, old geometry valid, new is null
CREATE TRIGGER rtree_<t>_<c>_update4 AFTER UPDATE OF <c> ON <t>
WHEN OLD.<i> != NEW.<i> AND OLD.<c> NOT NULL AND NOT ST_IsEmpty(OLD.<c>)
  AND (NEW.<c> IS NULL OR ST_IsEmpty(NEW.<c>))
BEGIN
  DELETE FROM rtree_<t>_<c> WHERE id = OLD.<i>;
END;

-- 6. UPDATE6: geometry set to null/empty, PK unchanged
CREATE TRIGGER rtree_<t>_<c>_update6 AFTER UPDATE OF <c> ON <t>
WHEN OLD.<i> = NEW.<i> AND (NEW.<c> IS NULL OR ST_IsEmpty(NEW.<c>))
  AND OLD.<c> NOT NULL AND NOT ST_IsEmpty(OLD.<c>)
BEGIN
  DELETE FROM rtree_<t>_<c> WHERE id = OLD.<i>;
END;

-- 7. DELETE: row removed
CREATE TRIGGER rtree_<t>_<c>_delete AFTER DELETE ON <t>
WHEN OLD.<c> NOT NULL AND NOT ST_IsEmpty(OLD.<c>)
BEGIN
  DELETE FROM rtree_<t>_<c> WHERE id = OLD.<i>;
END;
```

### Custom ST_ Functions
Since we're not using SpatiaLite, register custom SQLite scalar functions that
parse the GPKG binary header envelope to extract bbox values. This is fast — just
read 4 f64 values at known byte offsets (8, 16, 24, 32 for 2D LE envelope).

```rust
// Register via rusqlite::Connection::create_scalar_function
db.create_scalar_function("ST_MinX", 1, true, |ctx| {
    let blob = ctx.get::<Vec<u8>>(0)?;
    // bytes 8..16 = minx as f64 LE
    let minx = f64::from_le_bytes(blob[8..16].try_into().unwrap());
    Ok(minx)
})?;
```

## Required GeoPackage Contents (v0.2+)

Every GeoPackage produced by IronTrack must contain:

- Flight lines with full SoA coordinate data (lat, lon, elevation per waypoint)
- `altitude_datum` tag per flight line (EGM2008 / EGM96 / WGS84_ELLIPSOIDAL / AGL) ✅
- Sensor configuration profile (focal length, sensor dims, pixel pitch, overlap)
- R-tree spatial index with all 7 sync triggers ✅
- `irontrack_metadata` table: `schema_version` (e.g. "0.2.0"), `engine_version` (CARGO_PKG_VERSION)
- Copernicus attribution and disclaimer string (legal compliance)
- DSM penetration safety warning row (if terrain source is Copernicus DSM)
- Mission summary statistics (line count, total distance, bounding box, est. endurance)
- Cached DEM microtiles for the survey area (future — deferred past v0.2)

### irontrack_metadata table DDL
```sql
CREATE TABLE IF NOT EXISTS irontrack_metadata (
    key   TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
);
```
Seed rows on GeoPackage creation:
- `schema_version` = `"0.2.0"` (bump on breaking schema changes)
- `engine_version` = value of `env!("CARGO_PKG_VERSION")` at compile time

### Schema Version Compatibility
- Newer engine opening older file: print warning to stderr, suggest re-export.
- Older engine opening newer file: return `GpkgError::Init` with upgrade instructions.
- Check on `GeoPackage::open()` by reading `irontrack_metadata` where key='schema_version'.
