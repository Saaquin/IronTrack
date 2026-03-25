# Early Adopters

## What You Can Try Today

IronTrack is a headless CLI. There is no GUI yet.

Two commands are currently available:

- `irontrack terrain`
- `irontrack plan`

## Terrain Query

Example:

```powershell
irontrack terrain --lat 47.6200 --lon -122.3500
```

Expected output:

- Orthometric elevation in metres above mean sea level
- EGM2008 geoid undulation
- Ellipsoidal elevation in metres above WGS84

Notes:

- The engine uses Copernicus DEM, which is a DSM, not a bare-earth DTM.
- Over vegetation or structures, AGL clearance may be understated.
- Missing ocean/void coverage is treated as sea-level fallback in the user-facing terrain query.

## Flight Planning

Example:

```powershell
irontrack plan `
  --min-lat 51.0 --min-lon 0.0 `
  --max-lat 51.1 --max-lon 0.1 `
  --gsd-cm 3.0 `
  --sensor phantom4pro `
  --side-lap 60 `
  --end-lap 80 `
  --azimuth 0 `
  --output plan.gpkg `
  --geojson plan.geojson
```

Optional terrain-aware pass:

```powershell
irontrack plan `
  --min-lat 51.0 --min-lon 0.0 `
  --max-lat 51.1 --max-lon 0.1 `
  --gsd-cm 3.0 `
  --sensor phantom4pro `
  --terrain `
  --output plan.gpkg
```

## Sensor Presets

Built-in presets:

- `phantom4pro`
- `mavic3`
- `ixm100`

You can also supply a custom sensor by passing all of:

- `--focal-length-mm`
- `--sensor-width-mm`
- `--sensor-height-mm`
- `--image-width-px`
- `--image-height-px`

## Outputs

### GeoPackage

The main output is a GeoPackage with a `flight_lines` feature table and spatial index.

### GeoJSON

If `--geojson` is supplied, IronTrack also writes a GeoJSON preview file with 3D coordinates:

- longitude
- latitude
- altitude (MSL)

## Current Limitations

- The project is still pre-v0.1-release software.
- Terrain-aware planning depends on Copernicus DEM availability and local cache access.
- The GeoPackage export is not yet a full altitude-preserving mission container.
- Some deeper geodesy and interoperability checks are still under active development.

## Good Fit Right Now

- Technical evaluation
- CLI smoke-testing
- Reviewing line placement, spacing, and terrain-adjusted altitude behavior
- Early interoperability experiments with GIS tools that can open GeoPackage and GeoJSON

## Not A Good Fit Yet

- Safety-critical mission dispatch without independent validation
- Autopilot-native export workflows
- Fully documented external data interchange guarantees for third-party consumers
