# Documentation Index

All research docs live in `docs/`. Docs 02–10 have formula gaps (images) — use
`docs/formula_reference.md` (docs 02–07) and `docs/formula_reference_08_10.md` (docs 08–10).
Docs 11+ have formulas inline.

| Doc | Contents |
|-----|----------|
| `00_project_charter.md` | Scope, milestones, Eagle Mapping relationship |
| `01_legal_framework.md` | GPLv3 mechanics, IP carve-out, CLA strategy, copyright headers |
| `02_geodesy_foundations.md` | WGS84 params, geoid/ellipsoid, Karney algorithm, UTM Karney-Krüger, SoA, SIMD, Kahan |
| `03_photogrammetry_spec.md` | GSD, FOV, swath, dynamic AGL, collinearity, sensor stabilization |
| `04_geopackage_architecture.md` | Byte-level binary format, flags bitfield, envelope codes, R-tree triggers, WAL pragmas |
| `05_vertical_datum_engineering.md` | EGM96, EGM2008, NAVD88, GEOID18, CGVD2013, barometric ISA, terrain-following |
| `06_rust_architecture_analysis.md` | FFI, async overhead trap, SIMD auto-vectorization, Rayon vs Tokio, zero-copy mmap |
| `07_corridor_mapping.md` | Polyline offsets, self-intersection clipping, catenary, bank angles, clothoids |
| `08_copernicus_dem.md` | Copernicus GLO-30/GLO-90 specs, TanDEM-X bistatic InSAR, LE90<4m, EGM2008, GPLv3-compatible license |
| `09_dsm_vs_dtm.md` | DSM vs DTM for AGL: pitch dynamics, X-band canopy penetration (2–8m), FABDEM, CHM dual-layer |
| `10_cog_ingestion.md` | COG byte-range architecture, TIFF IFD, STAC, affine transforms, latitude grid widths, Tokio, DEFLATE |
| `11_autopilot_formats.md` | QGC .plan JSON, MAVLink 2.0 MISSION_ITEM_INT, DJI WPML/KMZ, senseFly, Leica HxMap, altitude datums per platform |
| `12_egm2008_grid.md` | EGM2008 .grd/.gtx/.pgm formats, 1-arcmin 10801×21600 grid, bilinear vs bicubic, Tide-Free system, EGM96↔EGM2008 discrepancies |
| `13_lidar_planning.md` | LiDAR swath/density math, scanner types, ρ=PRR(1-LSP)/(V×W), slope distortion, canopy penetration Beer-Lambert, USGS QL0-QL2 |
| `14_geotiff_cog.md` | Rust GeoTIFF ecosystem, PREDICTOR=3 support, benchmarks, manual DEFLATE+predictor reversal |
| `15_3dep_access.md` | USGS 3DEP COGs on AWS, TNM API, NAVD88/GEOID18 vertical datum, blending equation |
| `16_aerial_wind_data.md` | Wind triangle, atmospheric models, wind shear, forecast ingestion |
| `17_uav_survey_analysis.md` | Multirotor kinematics, acceleration dynamics, power modeling, endurance estimation |
| `18_oblique_photogrammetry.md` | Multi-camera systems, trapezoidal footprints, Maltese cross BBA, facade capture |
| `19_wgs84_epoch_management.md` | Plate tectonics, ITRF realization epochs, coordinate drift, PPK implications |
| `21_dynamic_footprint_geometry.md` | 6DOF ray casting, polygon intersection overlap, IMU error propagation, dynamic overlap buffering |
| `IronTrack_Manifest_v2.md` | Full system architecture, layer model, GeoPackage content spec, release roadmap |
| `formula_reference.md` | All equations from docs 02–07 in UTF-8 |
| `formula_reference_08_10.md` | All equations from docs 08–10 in UTF-8 |
