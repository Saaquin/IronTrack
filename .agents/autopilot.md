# Autopilot Export Implementation Guide

**Source docs:** `docs/11_autopilot_formats.md`

## Architecture Overview
IronTrack generates flight plans as GeoPackage/GeoJSON. To be flyable, those plans
must be exported to format-specific files that actual autopilots can ingest. Each
platform has different altitude datum expectations and camera trigger encoding.

## Altitude Datum Conversion (Safety-Critical)
Every export format expects altitudes in a specific datum. The FlightLine's
`AltitudeDatum` tag and `to_datum()` method handle conversion.

| Platform | Expected Datum | How to convert from EGM2008 |
|----------|---------------|----------------------------|
| ArduPilot (frame 3) | MSL relative to home | h_msl = H_egm2008 (direct, close enough) |
| ArduPilot (frame 11) | AGL over internal SRTM | altitude = desired AGL offset (scalar, not absolute) |
| DJI WPML (EGM96 mode) | EGM96 orthometric | H_egm96 = H_egm2008 + N_egm2008(lat,lon) - N_egm96(lat,lon) |
| DJI WPML (WGS84 mode) | WGS84 ellipsoidal | h = H_egm2008 + N_egm2008(lat,lon) |
| DJI RTK | WGS84 ellipsoidal | h = H_egm2008 + N_egm2008(lat,lon) |

**WARNING:** DJI terrain-following compiles static altitudes per-waypoint. The drone
does NOT dynamically sense terrain — it just follows the pre-baked altitude array.
IronTrack must pre-compute every waypoint altitude correctly.

## QGroundControl .plan JSON (ArduPilot/PX4)

### File Structure
```json
{
  "fileType": "Plan",
  "version": 1,
  "groundStation": "IronTrack",
  "mission": {
    "cruiseSpeed": 15.0,
    "firmwareType": 3,
    "hoverSpeed": 5.0,
    "vehicleType": 2,
    "plannedHomePosition": [47.6062, -122.3321, 50.0],
    "items": []
  },
  "geoFence": { "circles": [], "polygons": [], "version": 2 },
  "rallyPoints": { "points": [], "version": 2 }
}
```

### Mission Item (SimpleItem)
```json
{
  "autoContinue": true,
  "command": 16,
  "doJumpId": 1,
  "frame": 3,
  "params": [0, 0, 0, null, 47.6062, -122.3321, 100.0],
  "type": "SimpleItem"
}
```

Key fields:
- `command`: MAVLink command ID (16 = NAV_WAYPOINT, 206 = DO_SET_CAM_TRIGG_DIST)
- `frame`: 3 = GLOBAL_RELATIVE_ALT, 11 = GLOBAL_TERRAIN_ALT_INT
- `params[4,5]`: latitude, longitude (decimal degrees, NOT scaled integers — QGC handles MAVLink conversion)
- `params[6]`: altitude in meters (interpretation depends on frame)

### Camera Trigger Encoding
Insert before first survey waypoint:
```json
{ "command": 206, "params": [25.0, 0, 1, 0, 0, 0, 0], "frame": 2, "type": "SimpleItem" }
```
- `params[0]` = trigger distance in meters (calculated from photo interval)
- `params[2]` = 1 to trigger shutter, 0 to stop

Insert after last survey waypoint with `params[0] = 0` to stop triggering.

### Terrain Following
Set `frame: 11` on all survey waypoints. ArduPilot requires:
- `TERRAIN_ENABLE = 1`
- `TERRAIN_FOLLOW = 1`
- Altitude in params[6] = desired AGL offset (e.g., 100.0 = 100m above terrain)
- ArduPilot queries its internal SRTM database to compute the absolute MSL target

### MAVLink 2.0 Coordinate Precision
QGC .plan uses float64 in JSON. Under the hood, MAVLink MISSION_ITEM_INT encodes:
```
lat_int = (int32_t)(lat_deg × 1e7)
lon_int = (int32_t)(lon_deg × 1e7)
```
This gives sub-centimeter precision globally. The JSON→MAVLink conversion is handled
by QGC, not by IronTrack. Just write full f64 precision to the JSON.

## DJI WPML/KMZ

### Package Structure (ZIP archive)
```
waypoints_mission.kmz
└── wpmz/
    ├── template.kml     # Mission config, survey geometry, global params
    └── waylines.wpml    # Compiled waypoint array with actions
```

### template.kml — Mission Config
```xml
<wpml:missionConfig>
  <wpml:flyToWaylineMode>safely</wpml:flyToWaylineMode>
  <wpml:finishAction>goHome</wpml:finishAction>
  <wpml:exitOnRCLost>executeLostAction</wpml:exitOnRCLost>
  <wpml:executeHeightMode>EGM96</wpml:executeHeightMode>
  <wpml:globalTransitionalSpeed>10.0</wpml:globalTransitionalSpeed>
  <wpml:takeOffSecurityHeight>30.0</wpml:takeOffSecurityHeight>
</wpml:missionConfig>
```

### executeHeightMode values
- `WGS84` — ellipsoidal height
- `EGM96` — orthometric height relative to EGM96 geoid
- `RELATIVE` — AGL relative to takeoff point

For mapping missions, use `EGM96`. Convert from EGM2008 at each waypoint.

### waylines.wpml — Waypoint Array
Each waypoint in a `<Placemark>` with:
```xml
<Point>
  <coordinates>lon,lat</coordinates>  <!-- NOTE: lon,lat order (KML convention) -->
</Point>
<wpml:executeHeight>350.5</wpml:executeHeight>  <!-- EGM96 meters -->
<wpml:waypointSpeed>12.0</wpml:waypointSpeed>
```

### Camera Actions (actionGroup)
```xml
<wpml:actionGroup>
  <wpml:actionGroupId>0</wpml:actionGroupId>
  <wpml:actionGroupStartIndex>0</wpml:actionGroupStartIndex>
  <wpml:actionGroupEndIndex>47</wpml:actionGroupEndIndex>
  <wpml:actionGroupMode>sequence</wpml:actionGroupMode>
  <wpml:actionTrigger>
    <wpml:actionTriggerType>reachPoint</wpml:actionTriggerType>
  </wpml:actionTrigger>
  <wpml:action>
    <wpml:actionId>0</wpml:actionId>
    <wpml:actionActuatorFunc>takePhoto</wpml:actionActuatorFunc>
  </wpml:action>
</wpml:actionGroup>
```

For distance-based triggering, use `<wpml:actionTriggerType>betweenAdjacentPoints</wpml:actionTriggerType>`
with `<wpml:intervalDistance>` set to the photo interval in meters.

### Implementation Notes
- Use the `zip` crate to create the KMZ archive
- Use `quick-xml` or manual string building for XML (schema is rigid)
- KML coordinates are lon,lat (opposite of most Rust geo APIs)
- DJI payload position: `0` = main gimbal, `1` = right gimbal
- RTK missions: set `wpml:positioningType` to `RTKBaseStation` or `NetworkRTK`

## Coordinate Order Cheat Sheet
| Format | Order | Example |
|--------|-------|---------|
| QGC .plan JSON params | lat, lon | `[..., 47.6, -122.3, alt]` |
| GeoJSON | lon, lat | `[-122.3, 47.6]` |
| KML/WPML coordinates | lon, lat | `-122.3,47.6` |
| GeoPackage WKB | x(lon), y(lat) | `lon, lat` as f64 |
| MAVLink MISSION_ITEM_INT | lat×1e7, lon×1e7 | `476062000, -1223321000` |
