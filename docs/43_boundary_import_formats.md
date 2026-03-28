# Legacy Spatial Format Import for Flight Management

> **Source document:** 43_Boundary-Import-Formats.docx
>
> **Scope:** Complete extraction — all content is unique. Covers Shapefile native Rust parsing, File Geodatabase rejection rationale, DXF transitional support, GeoJSON/KML/KMZ client-side processing, CSV/WKT ingestion, CRS handling strategy, and v1.0 WGS84 limitation.

---

## 1. Shapefile (.shp/.shx/.dbf/.prj)

### Pure Rust Parsing (No GDAL)

| Crate | Architecture | Key Characteristics |
|---|---|---|
| **shapefile** (shapefile-rs) | Uses `dbase` crate for .dbf. Mature, widely used in geo-rust. | Iterative reading via ShapeReader. Handles mixed-endianness 100-byte .shp header. |
| **oxigdal-shapefile** | Zero C/C++ deps. "No unwrap" panic-free policy. | Native Z/M support. Apache Arrow columnar ops. Zero-copy mmap I/O. |

Both crates avoid GDAL FFI → lightweight statically-linked binary for edge deployment.

### Client-Side Parsing Strategy
The React web UI uses **shapefile.js** to parse binary geometry directly in the browser. The Rust backend daemon is never burdened with parsing potentially malformed client binaries → mitigates attack vectors and preserves backend compute for trajectory generation.

### .prj CRS Resolution
The .prj sidecar contains a WKT string defining the CRS. Parsed via **proj-wkt** crate (WKT → CrsDef) or **proj4wkt** (WKT → PROJ.4 string for proj4rs). If the CRS is not WGS84 or UTM → import halted with geodetic error (v1.0 limitation).

---

## 2. ESRI File Geodatabase (.gdb)

### OpenFileGDB Driver
GDAL's reverse-engineered driver provides read/write/update access to ArcGIS 9+ .gdb files. Thread-safe, no proprietary deps, supports spatial filtering via bounding-box geometry blobs.

### License Compatibility
GDAL is MIT/X11 licensed. MIT is **unequivocally compatible with GPLv3** — under GPLv3 Clause 5(c), the combined binary falls entirely under copyleft. Legally acceptable.

### Architectural Rejection
Despite legal compatibility, native .gdb ingestion is **rejected for v1.0:**
- Statically linking GDAL (massive C++ library) bloats the binary exponentially.
- FFI boundary introduces segmentation fault risk, violating pure-Rust memory safety guarantees.
- .gdb ingestion occurs only in the non-critical office planning phase — not worth the architectural penalty.

**Operational directive:** Require clients/technicians to export .gdb boundaries to GeoJSON or Shapefile in QGIS/ArcGIS before importing into IronTrack.

---

## 3. DXF (AutoCAD) — Transitional Support

### Purpose
Eagle Mapping's legacy workflow converts client boundaries to DXF for Trackair. IronTrack's goal is to eliminate this, but DXF import remains as a **transitional pathway** for zero-friction adoption by technicians accustomed to CAD-centric workflows.

### Extraction from Unstructured CAD Data
DXF is a graphical vector format lacking inherent topology. Boundaries are lines/arcs/polylines on named layers.

**Parser:** dxf-tools-rs or dxf crate (pure Rust).

**Extraction logic:**
1. Filter entities by target layer names ("BOUNDARY", "LIMITS").
2. Identify **LwPolyline** entities (ideal extraction target).
3. **Closure verification:** Polyline must have `is_closed = true` OR first/last vertex identical. Open lines → rejected.
4. **Bulge arc discretization:** Non-zero bulge parameters (circular arcs) → discretized into high-density linear segments for R-tree compatibility.
5. **Hatch/Region avoidance:** ACIS entities require proprietary parsers → discarded entirely. Mandate pre-closed polylines from clients.

| CAD Entity | Extraction Strategy |
|---|---|
| Single Line | Rejected (chaining into closed loops is error-prone) |
| LwPolyline | Ideal target — validate `is_closed`, discretize bulge arcs |
| Region/Hatch | Discarded (proprietary ACIS data) |

### Missing Spatial Reference
DXF has **no standardized SRS**. Coordinates could be meters (UTM), US Survey Feet (State Plane), or arbitrary local grid units. The React UI must display a **mandatory, non-dismissable modal** requiring the operator to assign the EPSG code before the boundary can be projected onto the WGS84 planning map.

---

## 4. GeoJSON, KML, KMZ (Phase 4I)

### GeoJSON (RFC 7946)
- CRS is **mandated as WGS84 (EPSG:4326)** — eliminates geodetic guesswork entirely.
- Coordinate order: [longitude, latitude, optional altitude].
- No CRS resolution logic needed. Feed directly to Turf.js for polygon operations.
- IronTrack embeds Copernicus attribution and safety disclaimers in exported GeoJSON properties.

### KML
- XML-based, WGS84 lateral + EGM96 vertical.
- Coordinate strings: `lon,lat,alt` separated by spaces within a single text block.
- Parsed client-side via **@tmcw/togeojson** → converts XML DOM to GeoJSON.

### KMZ
- ZIP archive containing root doc.kml + optional image overlays.
- React UI intercepts upload → **jszip** decompresses entirely in browser memory → extracts doc.kml → passes to parser.
- Client-side extraction prevents the Rust daemon from handling potentially malicious zip bombs.

---

## 5. CSV and Spreadsheet Import

### Column Header Heuristics
No structural guarantees in CSV. IronTrack scans headers via regex:
- **Latitude:** `lat`, `latitude`, `y`, `lati`, `northing`
- **Longitude:** `lon`, `long`, `longitude`, `x`, `easting`
- **Altitude:** `z`, `alt`, `elevation`, `height`
- **Vertex order:** `order`, `sequence`, `vid` (vertex ID) — critical for polygon geometry

**Closure verification:** First and last coordinate pair must be identical → closed polygon. Otherwise it's an open LineString → rejected for boundary planning.

### WKT Geometry in CSV Columns
Far more robust than discrete lat/lon columns. A single cell contains the entire polygon:
```
POLYGON ((-122.3 47.6, -122.2 47.6, -122.2 47.5, -122.3 47.5, -122.3 47.6))
```

**Supported:** Yes. Rust crates:
- **geoarrow-csv:** Streaming WKT → Apache Arrow columnar layout.
- **wkt** crate: Parse WKT strings → strongly-typed `geo-types` structs via `TryFromWkt` trait.

WKT natively supports interior holes and multi-polygon geometries — handles complex boundaries from simple text files.

---

## 6. CRS Handling and v1.0 Limitations

### Reprojection Options

| Approach | Deps | Pros | Cons |
|---|---|---|---|
| **proj** crate (C-FFI PROJ) | External C library | Comprehensive, authoritative | Violates static-linking, FFI blocking risk |
| **proj4rs** (pure Rust) | None | WASM-compatible, memory-safe | Lacks 4D epoch transforms |
| **geographiclib-rs** | None | Exact Karney algorithms | Geodesic only, not projections |

For highest precision (terrain-following, swath overlap): **geographiclib-rs** with Karney's algorithms. Rejects Haversine (0.3% spherical error) and Vincenty (antipodal convergence failure).

### v1.0 Limitation: WGS84 + UTM Only

**IronTrack will NOT auto-reproject arbitrary CRS at v1.0.** If a Shapefile .prj specifies State Plane or a custom datum:
1. React UI halts the import.
2. Displays descriptive geodetic error.
3. Requires operator to reproject to WGS84 (EPSG:4326) in external GIS software.

This is a **geodetic safety mechanism** — untested grid shifts must not silently corrupt life-critical flight lines. Documented in operator manuals and displayed in UI warnings.

### Canonical Pivot: WGS84 Ellipsoidal
All ingested data is transformed to WGS84 Ellipsoidal:
- **Copernicus DEM:** EGM2008 geoid undulation via memory-mapped bicubic interpolation (4×4 stencil, C¹ continuity — prevents pitch chatter at grid boundaries).
- **3DEP:** NAVD88 → NAD83 ellipsoidal via biquadratic GEOID18 interpolation → WGS84 via 14-parameter Helmert.

### Epoch Management
Navigational boundary: ~40 cm tectonic drift between 2010.0 and 2026.2 is imperceptible for aircraft navigation → unified epoch assumption during flight.

Analytical boundary: irontrack_metadata records horizontal_datum, vertical_datum, realization, observation_epoch. Epoch propagation delegated to post-processing suites (HTDP/NCAT/TBC).

### Memory Architecture
- f64 throughout all ingestion logic.
- Kahan summation for distance aggregations over long corridors.
- Structure of Arrays (SoA) memory layout: `{x:[], y:[], z:[]}` instead of `[{x,y,z}]` → CPU cache-line coherency → SIMD auto-vectorization (AVX2/NEON) for bounding boxes, affine transforms, and UTM projection.
