# Early Adopters

## What You Can Try Today

IronTrack is a high-performance headless CLI. There is no GUI.

Two commands are currently available:

- `irontrack terrain`
- `irontrack plan`

## Terrain Query

Example:

```powershell
./irontrack terrain --lat 49.2827 --lon -123.1207
```

**What it does:**
1.  Determines which Copernicus GLO-30 tile (DSM) contains the coordinate.
2.  Checks your local cache (`%APPDATA%/irontrack/cache/dem/` on Windows).
3.  If missing, it **automatically downloads** the tile from the Copernicus AWS S3 bucket.
4.  Calculates **EGM2008** geoid undulation (N) and returns orthometric height (MSL), undulation, and ellipsoidal height (WGS84).

## Mission Planning

Example:

```powershell
./irontrack plan \
  --min-lat 49.0 --min-lon -123.1 \
  --max-lat 49.1 --max-lon -123.0 \
  --gsd-cm 3.0 --sensor phantom4pro \
  --terrain \
  --output survey.gpkg --geojson survey.geojson
```

**What it does:**
1.  Generates a grid of parallel flight lines over the bounding box.
2.  Calculates nominal AGL required to achieve 3.0 cm GSD for the Phantom 4 Pro.
3.  With `--terrain`, it queries the Copernicus DEM across all waypoints and **automatically raises altitude** locally where ridges or peaks would violate the side-lap overlap or GSD target.
4.  Exports a **3D GeoPackage** (OGC standard) and **RFC 7946 GeoJSON**.

## Visualization

- **QGIS:** Open the `.gpkg` file. It includes a LINESTRINGZ feature layer with an R-tree spatial index.
- **GeoJSON.io:** Drag and drop the `.geojson` file to preview the 3D flight lines.
- **Google Earth:** The GeoJSON can be converted to KML/KMZ using standard tools for field viewing.

## Known Limitations (v0.1)

- **DSM Bias:** Copernicus GLO-30 is a surface model. Reported altitudes include canopy. Clearance over forests may be understated by 2–8m.
- **Rectangular Bounding Boxes only:** Arbitrary polygon boundaries are coming in v0.2.
- **Fixed-Wing Turns:** No kinematic turn generation (teardrops/bowties) yet.

## Safe to use for?

- **Pre-flight planning and feasibility analysis.**
- **Generating high-precision geodetic references.**
- **Comparing GSD targets against terrain relief.**

## Not A Good Fit Yet

- **Safety-critical mission dispatch without independent validation.**
- **Direct autopilot-native export workflows (e.g., DJI WPML, QGC .plan).**
- **Legacy SRTM-only workflows.**
