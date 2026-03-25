# IronTrack — Formula Supplement for Docs 08–10

> Fills all formula gaps in docs 08 (Copernicus DEM), 09 (DSM vs DTM),
> and 10 (COG Ingestion) where equations were stripped as images.
> Append to or reference alongside `docs/formula_reference.md`.

---

## Doc 08: Copernicus DEM

### GSD with DEM Vertical Error

```
GSD = (H_agl × p) / f

where:
  H_agl = altitude above ground level (m)
  p = pixel pitch (physical size of one sensor photosite, mm)
  f = focal length (mm)
```

If the DEM contains a vertical error δz:
```
H_actual = H_planned ± δz
GSD_actual = (H_actual × p) / f
```

Swath width sensitivity:
```
W_s = 2 × H_agl × tan(FOV/2)
```

DEM vertical error propagates directly into swath width, which changes the
lateral overlap OL between adjacent flight lines:
```
OL = 1 - (line_spacing / W_s)
```

Slope error amplification from DEM noise:
```
slope_error ≈ arctan(δz_DEM / pixel_spacing)
```
Example: 9m vertical error over 30m pixel → slope error up to ~17°.

---

## Doc 09: DSM vs DTM for AGL Flight Generation

### Camera Footprint Geometry (Flat Terrain, Nadir)

```
W_ground = (S_w / f) × H_agl       (ground footprint width, m)
H_ground = (S_h / f) × H_agl       (ground footprint height, m)

where:
  S_w = sensor physical width (mm)
  S_h = sensor physical height (mm)
  f   = focal length (mm)
  H_agl = altitude above ground level (m)
```

### Ground Sample Distance

```
GSD = (H_agl × S_w) / (f × I_w)

where I_w = image width in pixels
```

### DSM-Induced GSD Degradation Example

Aircraft at H_agl = 100m over open terrain, GSD = 2.0 cm/px.
Transitions to 30m forest tracked by DSM → aircraft climbs 30m.
Distance to ground floor becomes 130m:
```
GSD_forest = GSD_open × (130 / 100) = 2.0 × 1.3 = 2.6 cm/px
```
30% degradation in spatial resolution at ground level.

### DTM-Based Flight: Canopy Clearance

Aircraft flying 100m AGL over DTM, forest canopy = 30m:
```
clearance_to_canopy = H_agl_DTM - canopy_height = 100 - 30 = 70 m
```
GSD at canopy top (not ground) shrinks proportionally.

### Longitudinal State-Space Aircraft Dynamics

State vector:
```
x = [Δu, Δα, q, Δθ]ᵀ

where:
  Δu = perturbation in forward velocity (m/s)
  Δα = perturbation in angle of attack (rad)
  q   = pitch rate (rad/s)
  Δθ = perturbation in pitch angle (rad)
```

Control input:
```
u = [δ_e]

where δ_e = elevator deflection (rad)
```

Continuous-time state-space equation:
```
ẋ = Ax + Bu
```

System matrix A (longitudinal stability derivatives):
```
        ┌                                          ┐
        │  X_u      X_α       0      -g×cos(θ₀)   │
A   =   │  Z_u      Z_α      U₀       -g×sin(θ₀)  │
        │  M_u      M_α      M_q        0          │
        │   0        0        1          0          │
        └                                          ┘

where:
  X_u, X_α       = speed/AoA derivatives affecting axial force
  Z_u, Z_α       = speed/AoA derivatives affecting normal force
  M_u, M_α, M_q  = speed/AoA/pitch-rate derivatives affecting pitching moment
  M_α             = static longitudinal stability (must be negative for stability)
  M_q             = pitch damping derivative
  U₀, θ₀         = steady-state trim velocity and pitch angle
  g               = 9.80665 m/s²
```

Input matrix B (elevator control effectiveness):
```
        ┌      ┐
B   =   │  X_δe │
        │  Z_δe │
        │  M_δe │
        │   0   │
        └      ┘

where M_δe = elevator control power derivative (pitching moment per radian of elevator)
```

### Step Response (DSM Edge Transition)

The 30m canopy edge acts as a Heaviside step function in the altitude reference.
The underdamped (ζ < 1) second-order step response:
```
y(t) = 1 - (e^(-ζ×ω_n×t) / √(1-ζ²)) × sin(ω_d×t + φ)

where:
  ω_n = natural frequency of the short-period mode (rad/s)
  ζ   = damping ratio
  ω_d = ω_n × √(1-ζ²)   (damped natural frequency)
  φ   = arccos(ζ)          (phase angle)
```

During the transient oscillation, optical sensors experience severe motion blur
and geometric distortion. If pitch rate exceeds critical angle of attack → stall.

### Canopy Height Model (CHM) / Normalized DSM

```
CHM(x,y) = Z_DSM(x,y) - Z_DTM(x,y)

where:
  Z_DSM = Copernicus DEM elevation (includes canopy/buildings)
  Z_DTM = FABDEM elevation (bare earth estimate)
  CHM   = height of above-ground objects at each pixel
```

Dual-layer flight safety logic:
```
flight_altitude_primary = Z_DTM(x,y) + H_agl_target      (smooth DTM tracking)
obstacle_clearance      = flight_altitude_primary - Z_DSM(x,y)

if obstacle_clearance < safety_buffer:
    trigger evasive climb via MPC look-ahead
```

### X-Band Radar Penetration Bias

The Copernicus DEM scattering phase center sits below true canopy top:
```
Z_copernicus = Z_canopy_top - penetration_depth

Collision risk at commanded clearance C above DEM:
  actual_clearance = C - penetration_depth

If actual_clearance < 0 → aircraft is INSIDE the canopy
```

| Forest Type              | Penetration (% height) | Absolute Bias  |
|--------------------------|------------------------|----------------|
| Coniferous (evergreen)   | ~15%                   | 1.9–3.4 m      |
| Deciduous (leaf-on)      | ~18%                   | up to 6.2 m    |
| Deciduous (leaf-off)     | ~24%                   | up to 8.2 m    |
| Sparse/mixed             | variable               | 5.7–6.1 m      |

Example: 5m commanded clearance above DEM over winter deciduous forest:
```
actual_clearance = 5.0 - 8.2 = -3.2 m  → aircraft is 3.2m inside canopy
```

---

## Doc 10: Rust COG Ingestion

### Affine Transformation (Geographic ↔ Pixel Coordinates)

Six-parameter affine transform from the GeoTIFF ModelTiepointTag/ModelPixelScaleTag:
```
Parameters:
  x₀ = longitude of top-left pixel origin
  y₀ = latitude of top-left pixel origin
  dx = pixel resolution in X (degrees/pixel, positive)
  dy = pixel resolution in Y (degrees/pixel, negative — latitude decreases downward)
  sx, sy = shear/rotation terms (zero for north-up Copernicus DEM)

Forward (pixel → geographic):
  lon = x₀ + col × dx + row × sx
  lat = y₀ + col × sy + row × dy

Inverse (geographic → pixel), with sx = sy = 0:
  col = (lon - x₀) / dx
  row = (lat - y₀) / dy        (dy is negative, so row increases as lat decreases)
```

### Pixel Bounding Box from Survey Polygon

```
col_min = floor((lon_min - x₀) / dx)
col_max = floor((lon_max - x₀) / dx)
row_min = floor((lat_max - y₀) / dy)     (lat_max → smallest row index since dy < 0)
row_max = floor((lat_min - y₀) / dy)
```

### COG Internal Tile Index Calculation

GLO-30 internal tile size: 512 × 512 pixels.
GLO-90 internal tile size: 512 × 512 pixels.

```
tile_col_min = col_min / 512        (integer division)
tile_col_max = col_max / 512
tile_row_min = row_min / 512
tile_row_max = row_max / 512
```

### Linear Tile Index (Flattening 2D → 1D)

```
tiles_across = COG_width / 512

I = tile_row × tiles_across + tile_col

where:
  tile_row, tile_col = 2D tile coordinates
  tiles_across = total number of tile columns in the image
```

Example at equator: COG width = 3600px → tiles_across = 3600/512 = 7 (integer).

### Latitude-Dependent COG Width Table (GLO-30)

| Latitude Band | Lon Spacing | Original Width | COG Width | Tiles Across |
|---------------|-------------|----------------|-----------|-------------|
| 0°–50°        | 1.0×        | 3601           | 3600      | 7           |
| 50°–60°       | 1.5×        | 2401           | 2400      | 4           |
| 60°–70°       | 2.0×        | 1801           | 1800      | 3           |
| 70°–80°       | 3.0×        | 1201           | 1200      | 2           |
| 80°–85°       | 5.0×        | 721            | 720       | 1           |
| 85°–90°       | 10.0×       | 361            | 360       | 1           |

Height is always 3600 pixels (7 tile rows) for all latitude bands.

### HTTP Byte-Range Request for a Single Tile

```
Start byte = TileOffsets[I]
End byte   = TileOffsets[I] + TileByteCounts[I] - 1

HTTP header: Range: bytes={start}-{end}
Expected response: 206 Partial Content
```

### Merged Range Requests (Adjacent Tiles)

If tile I and tile I+1 are contiguous in the file:
```
TileOffsets[I+1] == TileOffsets[I] + TileByteCounts[I]
```
Then merge into a single request:
```
Range: bytes={TileOffsets[I]}-{TileOffsets[I+1] + TileByteCounts[I+1] - 1}
```
Reduces HTTP round-trips for spatially adjacent data.

### Initial Metadata Fetch

```
Range: bytes=0-16383      (first 16 KB — heuristic from GDAL)
```
Contains: TIFF signature, primary IFD, overview IFDs, TileOffsets array, TileByteCounts array.

### Copernicus DEM Data Format

- Pixel type: 32-bit IEEE 754 float (f32)
- Vertical datum: EGM2008 (EPSG:3855), orthometric height in meters
- Horizontal CRS: WGS84 (EPSG:4326)
- Compression: DEFLATE with floating-point predictor (PREDICTOR=3)
- Tile size: 512 × 512 pixels (both GLO-30 and GLO-90)
- Byte order: little-endian
