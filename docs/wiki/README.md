# IronTrack Wiki

This wiki is the human-readable layer above the research-heavy `docs/` set.
It is meant for:

- Maintainers who need to understand what the code does today.
- Early adopters who need to know what is safe to try and what is not ready yet.

## Start Here

- [Maintainers](C:\Users\saaqu\dev\ironTrack\docs\wiki\maintainers.md) explains the current module layout, runtime flow, and operational constraints.
- [Early Adopters](C:\Users\saaqu\dev\ironTrack\docs\wiki\early-adopters.md) explains the CLI, expected outputs, and present limitations.

## Current State

As of March 24, 2026, the codebase has working implementations for:

- Copernicus DEM tile parsing and cache-backed terrain lookup
- EGM96 and EGM2008 geoid height conversion
- Flat-terrain flight-line generation
- Terrain-aware overlap checking and altitude adjustment
- GeoPackage creation and GeoJSON export

The codebase also still has material v0.1 gaps:

- Geodesic and UTM integration-test coverage is still mostly stubbed
- The GeoPackage export path currently stores 2D geometry only
- Human-facing maintainer/adopter docs were previously sparse

## Relationship To The Research Docs

Use this wiki for orientation first. Then drop into the deeper docs when you need formulas or specification detail:

- `docs/02_geodesy_foundations.md`
- `docs/03_photogrammetry_spec.md`
- `docs/04_geoPackage_architecture.md`
- `docs/05_vertical_datum_engineering.md`
- `docs/10_Rust_COG_Ingestion.md`
- `docs/formula_reference.md`
- `docs/formula_reference_08_10.md`
