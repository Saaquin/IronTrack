# Terrain Ingestion & Trajectory Generation Guide

**Source docs:** 08, 09, 10, 14, 15, 33, 34, 38, 50, 52

## Terrain Data Sources

### Copernicus GLO-30 (Global Primary — IMPLEMENTED)
30m COG, X-band SAR DSM. EGM2008/WGS84. DSM WARNING mandatory (2-8m canopy penetration).
Ingestion: add EGM2008 undulation (bicubic) → WGS84 Ellipsoidal.

### USGS 3DEP (US Domestic Primary — v0.9) [Doc 33]
LiDAR bare-earth DTM. NAVD88/NAD83. Resolution: 1m (~70% CONUS), 1/3″ seamless, 1″ transborder.
COG on AWS S3 with 512×512 internal tiles. Resolution tier fallback: reject partial 1m
entirely → fall back to 1/3″. NEVER generate flight lines over a DEM void.

#### COG HTTP Range Request Pipeline [Docs 10, 33]
1. Header fetch: Range: bytes=0-16384 (TIFF header + IFD + byte offsets).
2. Spatial intersection: project flight bbox into COG's CRS, identify 512×512 tiles.
3. Parallel retrieval: tokio tasks issuing HTTP 206 for specific byte ranges.
4. Decompress LZW/DEFLATE in memory. Reduces GB → MB.

#### Per-Pixel Datum Transformation (Preprocessing Only) [Doc 30]
```
H_NAVD88 + N_GEOID18(biquadratic) → h_NAD83 → 14-param Helmert → h_WGS84
```
Executed ONCE on rayon threads. Cached in GeoPackage 256×256 microtiles (OGC 32-bit float
TIFF on disk + f64 SoA in RAM at daemon startup). **On-the-fly Helmert PROHIBITED.** [Doc 42]

### Multi-Source Fusion [Doc 33]
3DEP/Copernicus boundary: MBlend-style transition zones. 3DEP always takes precedence.

## Terrain-Following: EB + B-Spline Hybrid [Doc 34]

**Stage 1 — Elastic Band:** Tension (path shortening) + torsion (curvature penalty) +
asymmetric terrain repulsion + weak gravity. Gradient descent on tridiagonal Jacobian.
200-500 iterations for 30 km / 3000 nodes. Initialize above highest peak.

**Stage 2 — Cubic B-Spline C²:** Adaptive knots (dense at features, sparse on flat).
Convex hull property: control points clear min AGL → entire curve does. bspline + nalgebra.

## Dubins Path Turns [Doc 50]
6 canonical types {LSL,RSR,LSR,RSL,LRL,RLR}. Clothoid (Euler spiral) transitions for C²
curvature continuity at tangent junctions (Fresnel integrals via Maclaurin series).
3D Dubins Airplane: Low/Medium/High altitude classes with axis segregation preference.
Wind-corrected turn radius: r_safe = (V_TAS + V_wind)² / (g × tan(φ_max)).

## Flight Path Optimization [Doc 39]
Boustrophedon short-circuit for parallel grids: O(n log n) deterministic sort = proven
global optimum. For complex boundaries: NN construction → Or-opt refinement (sub-ms,
1-3% optimal). 2-opt REJECTED for ATSP (O(n³) due to wind cost recalculation on reversal).
Multi-block: outer GTSP for block sequence + inner ATSP per block.

## Energy Modeling [Doc 38]
Multirotor: P_hover = (mg)^1.5/√(2ρA). Fixed-wing: C_D = C_D0 + C_L²/(πeAR).
Per-segment: E_line + E_turn (60° bank = 4× induced drag) + E_climb - E_regen (~6.7%).
Kahan-summed E_total. 20% battery reserve = blocking enforcement (LiPo discharge cliff).

## Performance [Doc 52]
Manual AVX2 f64x4 for batch Karney (LLVM auto-vec fails). SoA mandatory. Lane masking
for variable convergence. rayon with_min_len(1024) for DEM interpolation.
memmap2 for constrained Pi: R-tree geographic presorting prevents thrashing.
fast_math REJECTED (destroys Kahan). Minimax polynomials for trig via FMA.
