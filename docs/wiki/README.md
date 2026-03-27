# IronTrack Wiki

This wiki is the human-readable layer above the research-heavy `docs/` set. It is meant for maintainers and early adopters.

## Start Here

- [Engine Technical Reference](../99_engine_technical_reference.md) — Architectural overview, module map, and critical workflows.
- [Maintainer Guide](maintainers.md) — Coding conventions, project pillars, and the future roadmap.
- [User Guide (Early Adopters)](early-adopters.md) — CLI usage examples and current limitations.

## Current Engine State (v0.1/v0.2-alpha)

The IronTrack Core Engine is a functional CLI tool capable of:

- **Terrain Ingestion:** Copernicus GLO-30 tile parsing with automatic AWS S3 fetching.
- **Geodesy:** Karney-Krüger 6th-order UTM and dual-geoid (EGM96/EGM2008) support.
- **Planning:** Terrain-aware flight line generation with dynamic AGL adjustment.
- **I/O:** 3D GeoPackage (LINESTRINGZ) and RFC 7946 GeoJSON export.

## Relationship To The Research Docs

Use this wiki for orientation. For deep mathematical formulas and domain-specific specifications, refer to the root `docs/` directory:

- **Foundation:** `02_geodesy_foundations.md`, `03_photogrammetry_spec.md`.
- **Infrastructure:** `04_geoPackage_architecture.md`, `10_Rust_COG_Ingestion.md`.
- **Engineering:** `05_vertical_datum_engineering.md`, `09_dsm_vs_dtm.md`.
- **Reference:** `formula_reference.md`, `formula_reference_08_10.md`.
