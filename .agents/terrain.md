# Terrain Ingestion Implementation Guide

## SRTM HGT File Format
SRTM1 (1-arc-second) files are dead simple binary grids with no header.

### Physical layout
- File size: exactly 25,934,402 bytes (3601 × 3601 × 2)
- Grid: 3601 rows × 3601 columns
- Each cell: signed 16-bit big-endian integer (i16 BE)
- Units: meters above EGM96 geoid (orthometric height)
- Void/no-data: -32768
- Spatial extent: 1° × 1° tile
- Resolution: 1/3600 degree ≈ 30 meters at equator

### Filename convention
`N47W122.hgt` → southwest corner at 47°N, 122°W.
The tile covers lat [47°, 48°] and lon [-122°, -121°].

Letter codes: N/S for latitude, E/W for longitude.
Latitude is 2 digits, longitude is 3 digits: `S34E018.hgt`.

### Grid indexing
Row 0 = north edge of tile (highest latitude).
Column 0 = west edge of tile (lowest longitude).
Row 3600 = south edge. Column 3600 = east edge.
Adjacent tiles share edge rows/columns (hence 3601, not 3600).

For a coordinate (lat, lon) within the tile:
```
row = (north_edge - lat) * 3600.0
col = (lon - west_edge) * 3600.0
```

### Parsing in Rust
```rust
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

fn parse_hgt(data: &[u8]) -> Vec<i16> {
    assert_eq!(data.len(), 3601 * 3601 * 2);
    let mut cursor = Cursor::new(data);
    let mut grid = Vec::with_capacity(3601 * 3601);
    for _ in 0..(3601 * 3601) {
        grid.push(cursor.read_i16::<BigEndian>().unwrap());
    }
    grid
}
```

For better performance, use `byteorder::ReadBytesExt::read_i16_into()` to
read the entire buffer in one pass, or memory-map the file with `memmap2`.

## Bilinear Interpolation
DEM grid posts are point samples at exact grid intersections. For coordinates
that fall between posts, interpolate the four surrounding values.

Given fractional row `r` and column `c`:
```
r0 = floor(r) as usize
c0 = floor(c) as usize
dr = r - r0 as f64   // fractional row offset [0, 1)
dc = c - c0 as f64   // fractional col offset [0, 1)

z00 = grid[r0 * 3601 + c0]       // top-left
z01 = grid[r0 * 3601 + c0 + 1]   // top-right
z10 = grid[(r0+1) * 3601 + c0]   // bottom-left
z11 = grid[(r0+1) * 3601 + c0+1] // bottom-right

elevation = z00*(1-dr)*(1-dc) + z01*(1-dr)*dc + z10*dr*(1-dc) + z11*dr*dc
```

Handle void cells (-32768): if ANY of the four surrounding posts is void,
return None. Do not interpolate with voids — it produces nonsense elevations.

## Cache Architecture
```
{dirs::cache_dir()}/irontrack/srtm/
├── N47W122.hgt
├── N47W121.hgt
├── N46W122.hgt
└── ...
```

### Tile selection
Given a query coordinate (lat, lon):
```rust
fn tile_key(lat: f64, lon: f64) -> (i32, i32) {
    // Floor toward negative infinity for both
    let lat_floor = lat.floor() as i32;
    let lon_floor = lon.floor() as i32;
    (lat_floor, lon_floor)
}

fn tile_filename(lat: i32, lon: i32) -> String {
    let ns = if lat >= 0 { 'N' } else { 'S' };
    let ew = if lon >= 0 { 'E' } else { 'W' };
    format!("{}{:02}{}{:03}.hgt", ns, lat.abs(), ew, lon.abs())
}
```

### Lazy loading
The DemProvider should NOT preload all tiles. Load tiles on first query and
cache the parsed grid in a HashMap<(i32, i32), SrtmTile>. For v0.1, an
`RwLock<HashMap>` is fine. For higher concurrency later, use `dashmap`.

### Edge cases
- Points exactly on a tile boundary (e.g., lat = 48.0000) technically belong
  to two tiles. Convention: the boundary row/column is shared. Query the tile
  whose SW corner is floor(lat), floor(lon). The northernmost row (row 0) of
  tile N47 equals the southernmost row (row 3600) of tile N48.
- Points over water may have void values throughout. Return a clear error.
- SRTM coverage: latitudes 60°S to 60°N only. Polar regions require alternative
  DEMs (ArcticDEM, REMA).

## Downloading Tiles (Stretch Goal for v0.1)
SRTM data is freely available from:
- NASA Earthdata: https://e4ftl01.cr.usgs.gov/MEASURES/SRTMGL1.003/
- Requires Earthdata login (free registration)

For v0.1, implement a stub that prints a helpful message:
```
Tile N47W122.hgt not found in cache.
Download from: https://e4ftl01.cr.usgs.gov/MEASURES/SRTMGL1.003/2000.02.11/N47W122.SRTMGL1.hgt.zip
Place the extracted .hgt file in: /home/user/.cache/irontrack/srtm/
```

Actual download automation (with Earthdata auth) is a post-MVP feature.

## Concurrency & Performance
### The Two-Phase I/O Pattern
To support high-concurrency (e.g., Phase 2 Photogrammetry) without stalling the Rayon pool:
1. **Phase 1 (Sequential)**: Identify required `tile_keys`, load/download them on the calling thread, and perform a dummy query to warm the cache.
2. **Phase 2 (Parallel)**: Dispatch the SoA math to `rayon::par_iter()`. Since tiles are cached, the Rayon threads hit the "fast-path" read-lock and never block on I/O.

### Coastal Boundary Strategy
Bilinear kernels near the ocean will hit "Void" posts (sentinel values). 
- **Rule**: If a kernel hits a void post, return `DemError::NoData`.
- **Reasoning**: This allows the top-level `query()` to fall back to `OceanFallback` (Sea Level + Geoid), which is physically safer and more useful than a hard error or a 1-pixel-inland elevation for a point offshore.

## TerrainEngine Integration
The TerrainEngine composes DEM lookups with geoid correction.

### Public API (Coordinate Exchange)
The public API for `TerrainEngine` and `GeoidModel` MUST accept `GeoCoord` or slices of radians. This ensures validation happens once and units are consistent.

### Bulk Processing API
To support Phase 2 (Photogrammetry), the engine MUST provide "bulk" methods (e.g., `elevations_at(lats: &[f64], lons: &[f64]) -> Result<Vec<f64>>`). These methods:
- Process slices in parallel using `rayon`.
- Minimize `RwLock` contention by batching lookups within tiles.
- Return orthometric (MSL) or ellipsoidal (WGS84) heights as requested.

### Graceful Degradation
If a coordinate falls outside SRTM coverage (e.g., Alaska, Antarctica), the engine should NOT necessarily error if it's part of a larger flight plan. Consider returning a "fallback" elevation (0.0 m or geoid surface) with a warning, rather than a hard `Result::Err`, to allow planning to continue in non-SRTM regions.

### Implementation Example
```rust
pub struct TerrainEngine {
    dem: DemProvider,     // SRTM tile cache + interpolation
    geoid: GeoidModel,    // EGM96 undulation lookup
}

impl TerrainEngine {
    /// Returns orthometric elevation (meters above MSL) for a single coordinate
    pub fn elevation(&self, coord: &GeoCoord) -> Result<f64> {
        self.dem.elevation_at(coord.lat_deg(), coord.lon_deg())
    }

    /// Returns ellipsoidal elevation (for GNSS/autopilot targets)
    pub fn ellipsoidal_elevation(&self, coord: &GeoCoord) -> Result<f64> {
        let h_ortho = self.elevation(coord)?;
        let n = self.geoid.undulation(coord.lat_deg(), coord.lon_deg())?;
        Ok(h_ortho + n)  // h = H + N
    }
}
```
