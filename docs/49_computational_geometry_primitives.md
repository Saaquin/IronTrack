# Computational Geometry Primitives for Aerial Survey FMS

> **Source document:** 49_Computational-Geometry-Primitives.docx
>
> **Scope:** Complete extraction — all content is unique. Covers point-in-polygon (ray casting, winding number), line-polygon clipping (5 algorithms), polygon offset/buffer (straight skeleton), convex hull + rotating calipers MBR, geodetic area (Shoelace vs Karney), and Delaunay/Voronoi triangulation (delaunator vs spade).

---

## 1. Point-in-Polygon (PIP)

### Ray Casting (Even-Odd)
Projects horizontal ray from test point, counts polygon edge intersections. Odd = inside, even = outside. O(n) per point.

**Degeneracy fixes (mandatory):**
- Ray passes through vertex → half-open intervals: intersection valid only if point y ≥ edge y_min AND < edge y_max. Vertices on the ray are treated as infinitesimally above it.
- Horizontal edges: explicitly ignored (accounted for by adjacent non-horizontal edges).
- Fails on self-intersecting polygons (ambiguous "inside" definition under even-odd rule).

### Winding Number (Sunday's Algorithm) — PREFERRED
Counts total signed revolutions the polygon boundary makes around the test point. Non-zero = inside, zero = outside. Correctly handles self-intersecting polygons and complex topologies.

**Sunday (2001) eliminates trigonometry:** Tracks quadrant transitions via edge-crossing direction. Upward crossing increments, downward decrements. Reduces to simple f64 coordinate comparisons → **performance-equivalent to ray casting with vastly superior topological robustness.**

### R-Tree Acceleration for Bulk Queries
Brute-force PIP across m waypoints × n vertices = O(m×n). Unacceptable for millions of LiDAR triggers.

**Optimization chain:**
1. R-tree spatial index (native in GeoPackage) for coarse filtering.
2. AABB rejection in O(1) per point — eliminates most candidates instantly.
3. SoA memory layout → SIMD vectorization (AVX2/NEON) processes 8 f64 waypoints per clock cycle.
4. rayon data-parallel multi-threading across independent flight lines.

---

## 2. Line-Polygon Clipping

### Algorithm Comparison

| Algorithm | Polygon Support | Handles Holes | Degeneracy Risk | Use Case |
|---|---|---|---|---|
| **Cohen-Sutherland** | Rectangular only | No | None | AABB/UTM zone/tile clipping |
| **Sutherland-Hodgman** | Convex only | No | Low (generates coincident edges on concave) | Simple convex boundaries |
| **Weiler-Atherton** | Concave | Yes | Moderate (shared edges) | **Complex survey boundaries** |
| **Greiner-Hormann** | Concave + self-intersecting | Yes | **High** (requires epsilon perturbation → destroys f64 precision) | Disqualified for survey-grade |
| **Vatti** | Concave + self-intersecting | Yes | Low (sweep-line robust) | Ultimate robustness |

### Sutherland-Hodgman Failure on Concave Boundaries
When a flight line bridges across a concave void (enters, exits through inlet, re-enters), the algorithm generates extraneous coincident edges along the boundary perimeter. These "phantom segments" would command the autopilot to fly non-survey paths along the boundary edge — violating efficiency and safety.

### Weiler-Atherton (Recommended for IronTrack)
Double-linked lists for subject (flight line) and clip (boundary) vertex sequences. All intersections computed and classified as **inbound** (entering) or **outbound** (exiting). Traversal starts at unvisited inbound intersection, follows subject through interior until outbound intersection, terminates segment. Repeated launches from every unvisited inbound intersection isolate multiple independent disjoint segments — exactly what concave boundaries require.

### Vatti (Alternative)
Sweep-line technique divides polygon into horizontal "scanbeams." Handles extreme topological complexity without vertex perturbation. Use when Weiler-Atherton edge-case handling proves insufficient.

---

## 3. Polygon Offset and Buffer

### Corner Join Types
| Join | Method | Trade-off |
|---|---|---|
| **Round** | Circular arc at vertex | Mathematically ideal distance guarantee; expensive (high vertex count from arc discretization) |
| **Miter** | Extend offset segments to intersection | Sharp corners; requires miter limit for acute angles (spike → infinity) |
| **Bevel** | Flat perpendicular segment at bisector | Blunted corners; safer aerodynamic turning boundary for corridor mapping |

### Self-Intersection Resolution: Straight Skeleton
Naive offsetting creates twisted, invalid geometries when kinked polylines are buffered. Inward buffer (deflation) of narrow bottlenecks splits polygon into disjoint pieces. Outward buffer (inflation) of nearby footprints merges them.

**Straight skeleton** simulates continuous shrinking/expanding:
- Edges move inward at constant orthogonal speed; vertices trace angular bisecting lines.
- **Edge event:** Shrinking edge reaches zero length → adjacent edges collapse.
- **Split event:** Reflex vertex hits opposing edge → polygon severs.
- Extract temporal snapshot at t = buffer_distance → perfectly resolves all topological phase shifts.

### Rust Crate Selection
- **`geo` crate:** `Buffer` trait with `BufferStyle` (Miter/Round/Bevel, LineCap). Suitable for standard corridor polyline offsets and UI presentation.
- **`geo-buffer` crate:** Implements buffer via **straight skeleton** method. Handles severe geometric deformities (overlapping footprint unification, safety setback deflation inside airspace). Requires OGC-valid input; no Bevel join support yet.

**FMS mandate:** `geo-buffer` for topologically complex operations; `geo` for standard styling.

---

## 4. Convex Hull and Minimum Bounding Rectangle

### Convex Hull: B-Spline Safety Guarantee
The B-spline convex hull property: the generated curve cannot stray outside the convex hull of its control points. If the convex hull clears the DSM → the entire flight path is guaranteed collision-free, independent of spline evaluation resolution.

### Minimum-Area Bounding Rectangle (MBR)
AABB is trivial O(n) but wasteful for diagonal corridors. The MBR may be arbitrarily oriented.

**Freeman-Shapira theorem:** The minimum-area enclosing rectangle always has at least one side collinear with a convex hull edge.

### Rotating Calipers Algorithm — O(n) after O(n log n) hull
1. Initialize: align one caliper flush with a hull edge; push 3 orthogonal calipers to antipodal extremes.
2. Compute angle to next adjacent hull edge for all 4 calipers.
3. Rotate entire apparatus by the smallest angle → next caliper snaps to a new hull edge.
4. Recalculate area. Track minimum over full 360° rotation.

**Implementation:** Pure vector algebra (dot products for projections, cross products for angle comparison). **No trigonometric functions** → eliminates f64 precision loss. The resulting orientation matrix projects WGS84 coordinates into the ideal local UTM system without floating-point approximation drift.

---

## 5. Polygon Area: Geodetic Considerations

### Shoelace Formula (Planar)
```
A = 0.5 × |Σ(x_i × y_{i+1} - x_{i+1} × y_i)|
```
O(n). Handles concave polygons natively (signed cross products subtract folded areas). But **only valid in Cartesian space** — applying to raw WGS84 lat/lon yields meaningless "square degrees."

### UTM Projection Distortion Problem
- Scale factor = 0.9996 at central meridian, increases toward zone edges.
- Large regional surveys: **~3.5% absolute area error** from UTM scale distortion.
- Zone boundary crossing requires splitting polygon or forcing into single zone with compounding distortion.

### Karney's Ellipsoidal Polygon Method — MANDATORY for Survey-Grade
Computes true area directly on the WGS84 oblate ellipsoid surface via geographiclib-rs. Solves inverse geodetic problem and integrates area under geodesic lines between consecutive vertices. **No projection distortion. No zone boundary problem.** Sub-meter area accuracy at any planetary scale.

IronTrack reports all hectare/acre metrics via Karney's method — legally defensible and scientifically exact.

---

## 6. Delaunay Triangulation and Voronoi Diagrams

### FMS Applications
- **Delaunay TIN:** Ideal framework for DEM surface from mass points. Empty circumcircle property maximizes minimum triangle angles → prevents sliver triangles. Used for 6DOF ray casting of camera footprints over terrain.
- **Voronoi diagram (Delaunay dual):** Each cell encloses one input point; any location in a cell is closer to its point than to any other. Used to evaluate GCP spatial distribution density and identify voids requiring additional control points for ASPRS BBA compliance.

### Rust Crate Selection

| Feature | delaunator | **spade** |
|---|---|---|
| Design focus | Maximum speed | **Mathematical robustness** |
| Memory layout | Flat contiguous usize arrays | Object handles / hierarchical nodes |
| Exact geometric predicates | No (float heuristics) | **Yes (prevents topology collapse)** |
| Constrained Delaunay (CDT) | No | **Yes (mandatory for breaklines)** |
| Voronoi extraction | No | **Yes (native iteration)** |
| Performance (N > 1M) | ~10× faster | Moderate / acceptable |

**IronTrack selects `spade`** despite slower bulk insertion because:
1. **Exact predicates** prevent catastrophic cascading failures from floating-point round-off in circumcircle determinants (intersecting edges, non-planar graphs, infinite loops on collinear points).
2. **CDT** is mandatory for integrating sharp hydrographic breaklines (rivers, cliffs, ridgelines) into the TIN without the mesh illegally bridging across physical boundaries.
3. **Voronoi dual** provides programmatic iteration for GCP placement analytics.
4. **Hierarchy hint generators** achieve O(√n) access for nearest-neighbor ray-casting queries.

For life-critical aviation data, topological correctness trumps raw throughput.
