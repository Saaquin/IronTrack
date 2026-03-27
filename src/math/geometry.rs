// IronTrack - Open-source flight management and aerial survey planning engine
// Copyright (C) 2026 [Founder Name]
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

// Computational geometry: point-in-polygon tests, polygon representation.
//
// The ray-casting algorithm handles:
//   (a) Convex and concave exterior boundaries.
//   (b) Interior holes (exclusion zones): a point is "inside" if it is
//       inside the exterior ring AND outside ALL interior rings.
//
// All coordinates are f64 (decimal degrees, WGS84). The algorithm operates
// in the Cartesian plane — valid for survey-scale polygons (< 200 km)
// where the Earth's curvature is negligible.

/// A polygon with an exterior boundary and zero or more interior holes.
///
/// Rings are stored as `Vec<(f64, f64)>` where each tuple is (latitude, longitude)
/// in decimal degrees. Rings are implicitly closed: the last vertex connects
/// back to the first. The exterior ring should be counter-clockwise and
/// interior rings (holes) clockwise, but the ray-casting algorithm is
/// winding-order agnostic.
#[derive(Debug, Clone)]
pub struct Polygon {
    /// Exterior boundary ring (lat, lon) in decimal degrees.
    pub exterior: Vec<(f64, f64)>,
    /// Interior rings (holes / exclusion zones).
    pub interiors: Vec<Vec<(f64, f64)>>,
}

impl Polygon {
    /// Create a polygon with no holes.
    pub fn new(exterior: Vec<(f64, f64)>) -> Self {
        Self {
            exterior,
            interiors: Vec::new(),
        }
    }

    /// Create a polygon with interior holes.
    pub fn with_holes(exterior: Vec<(f64, f64)>, interiors: Vec<Vec<(f64, f64)>>) -> Self {
        Self {
            exterior,
            interiors,
        }
    }

    /// Test whether a point (lat, lon) lies inside this polygon.
    ///
    /// A point is "inside" if it passes the ray-casting test against the
    /// exterior ring AND fails against every interior ring (i.e., is
    /// outside all holes).
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        if !point_in_ring(lat, lon, &self.exterior) {
            return false;
        }
        for hole in &self.interiors {
            if point_in_ring(lat, lon, hole) {
                return false;
            }
        }
        true
    }

    /// Axis-aligned bounding box of the exterior ring: (min_lat, min_lon, max_lat, max_lon).
    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        let mut min_lat = f64::INFINITY;
        let mut min_lon = f64::INFINITY;
        let mut max_lat = f64::NEG_INFINITY;
        let mut max_lon = f64::NEG_INFINITY;
        for &(lat, lon) in &self.exterior {
            min_lat = min_lat.min(lat);
            min_lon = min_lon.min(lon);
            max_lat = max_lat.max(lat);
            max_lon = max_lon.max(lon);
        }
        (min_lat, min_lon, max_lat, max_lon)
    }
}

/// Ray-casting point-in-ring test.
///
/// Casts a ray from (lat, lon) in the +lon direction and counts edge
/// crossings. An odd count means the point is inside.
///
/// This handles concave polygons correctly — the ray naturally crosses
/// edges in and out of concavities.
fn point_in_ring(lat: f64, lon: f64, ring: &[(f64, f64)]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (yi, xi) = ring[i];
        let (yj, xj) = ring[j];

        /*
         * Test whether the edge (j→i) straddles the ray's latitude.
         * The ray extends from (lat, lon) in the +lon direction.
         * If the edge crosses the ray's latitude, compute the lon
         * intersection and check if it's to the right of the point.
         */
        if (yi > lat) != (yj > lat) {
            let x_intersect = xj + (lat - yj) / (yi - yj) * (xi - xj);
            if lon < x_intersect {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple unit square: (0,0), (0,1), (1,1), (1,0).
    fn unit_square() -> Polygon {
        Polygon::new(vec![
            (0.0, 0.0),
            (0.0, 1.0),
            (1.0, 1.0),
            (1.0, 0.0),
        ])
    }

    #[test]
    fn convex_polygon_interior_point() {
        let poly = unit_square();
        assert!(poly.contains(0.5, 0.5));
    }

    #[test]
    fn convex_polygon_exterior_point() {
        let poly = unit_square();
        assert!(!poly.contains(2.0, 0.5));
        assert!(!poly.contains(0.5, 2.0));
        assert!(!poly.contains(-1.0, 0.5));
    }

    #[test]
    fn concave_l_shaped_polygon() {
        /*
         * L-shaped polygon (concave):
         *
         *   (0,0)---(0,2)
         *     |         |
         *   (1,0)---(1,1)
         *             |
         *           (2,1)---(2,2)
         *             |         |
         *           (2,0)      ...wait, let me draw this properly.
         *
         * Vertices (lat, lon):
         *   (0,0) → (0,2) → (1,2) → (1,1) → (2,1) → (2,0) → back to (0,0)
         *
         * The "missing" corner at (2,2) makes this concave.
         * Point (1.5, 1.5) is in the concave notch — should be OUTSIDE.
         * Point (0.5, 1.0) is inside the top bar of the L.
         * Point (1.5, 0.5) is inside the vertical bar of the L.
         */
        let poly = Polygon::new(vec![
            (0.0, 0.0),
            (0.0, 2.0),
            (1.0, 2.0),
            (1.0, 1.0),
            (2.0, 1.0),
            (2.0, 0.0),
        ]);
        assert!(poly.contains(0.5, 1.0), "inside top bar");
        assert!(poly.contains(1.5, 0.5), "inside vertical bar");
        assert!(!poly.contains(1.5, 1.5), "in the concave notch — outside");
    }

    #[test]
    fn polygon_with_hole() {
        /*
         * Outer square: (0,0) to (10,10).
         * Inner hole: (3,3) to (7,7).
         * Point (5,5) is in the hole — should be OUTSIDE.
         * Point (1,1) is between outer and hole — should be INSIDE.
         */
        let outer = vec![
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
        ];
        let hole = vec![
            (3.0, 3.0),
            (3.0, 7.0),
            (7.0, 7.0),
            (7.0, 3.0),
        ];
        let poly = Polygon::with_holes(outer, vec![hole]);
        assert!(poly.contains(1.0, 1.0), "between outer and hole");
        assert!(!poly.contains(5.0, 5.0), "inside hole");
        assert!(!poly.contains(11.0, 5.0), "outside outer");
    }

    #[test]
    fn bbox_computed_correctly() {
        let poly = Polygon::new(vec![
            (51.5, -0.1),
            (51.5, 0.0),
            (51.6, 0.0),
            (51.6, -0.1),
        ]);
        let (min_lat, min_lon, max_lat, max_lon) = poly.bbox();
        assert!((min_lat - 51.5).abs() < 1e-9);
        assert!((min_lon - (-0.1)).abs() < 1e-9);
        assert!((max_lat - 51.6).abs() < 1e-9);
        assert!((max_lon - 0.0).abs() < 1e-9);
    }

    #[test]
    fn degenerate_ring_returns_false() {
        assert!(!point_in_ring(0.5, 0.5, &[(0.0, 0.0), (1.0, 1.0)]));
        assert!(!point_in_ring(0.5, 0.5, &[]));
    }

    #[test]
    fn triangle_interior() {
        let tri = Polygon::new(vec![
            (0.0, 0.0),
            (0.0, 4.0),
            (3.0, 2.0),
        ]);
        assert!(tri.contains(1.0, 2.0));
        assert!(!tri.contains(0.0, 5.0));
    }
}
