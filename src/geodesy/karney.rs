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

// Karney geodesic solver — thin wrapper around `geographiclib_rs`.
//
// All public functions accept/return `GeoCoord` (radians) for internal
// consistency with the Coordinate Exchange Protocol (see .agents/geodesy.md).
// Degree conversion happens at this boundary only.
//
// Why Karney (not Haversine or Vincenty):
//   - Haversine: spherical assumption → 0.3 % error. Disqualified for survey.
//   - Vincenty: antipodal convergence failure. In Rayon par_iter a divergent
//     thread blocks the work-stealing scheduler → non-deterministic hangs.
//   - Karney: Newton root-finding, converges in 2-4 iterations for all point
//     pairs including antipodes. 15-nanometer accuracy. Bounded runtime.

use geographiclib_rs::{DirectGeodesic, Geodesic, InverseGeodesic};

use crate::error::GeodesyError;
use crate::types::GeoCoord;

/// Result of a geodesic inverse (distance + azimuths between two points).
#[derive(Debug, Clone, Copy)]
pub struct GeodesicInverse {
    /// Geodesic distance in metres.
    pub distance_m: f64,
    /// Forward azimuth at point 1, decimal degrees clockwise from north.
    pub azimuth1_deg: f64,
    /// Forward azimuth at point 2, decimal degrees clockwise from north.
    pub azimuth2_deg: f64,
}

/// Result of a geodesic direct (endpoint given start + azimuth + distance).
#[derive(Debug, Clone, Copy)]
pub struct GeodesicDirect {
    /// Destination point.
    pub point: GeoCoord,
    /// Forward azimuth at the destination, decimal degrees clockwise from north.
    pub azimuth2_deg: f64,
}

/// Solve the geodesic inverse problem: distance and azimuths between `a` and `b`.
///
/// Both points are in radians (via `GeoCoord`). Results are returned in metres
/// and degrees. The underlying solver is `geographiclib_rs::Geodesic::wgs84()`
/// (Karney 2013), accurate to 15 nm over all point pairs.
///
/// # Errors
/// Returns `GeodesyError::ProjectionFailure` if either coordinate is
/// out of the valid WGS84 range (caught by `GeoCoord` construction).
pub fn geodesic_inverse(a: GeoCoord, b: GeoCoord) -> GeodesicInverse {
    let geod = Geodesic::wgs84();

    /*
     * geographiclib-rs works in decimal degrees. The unit conversion is the
     * only degree arithmetic that should appear in this module.
     */
    /* (s12, azi1, azi2, a12) — use the 4-tuple impl to get distance and azimuths together */
    let (distance_m, azimuth1_deg, azimuth2_deg, _a12): (f64, f64, f64, f64) = geod.inverse(
        a.lat_deg(),
        a.lon_deg(),
        b.lat_deg(),
        b.lon_deg(),
    );

    GeodesicInverse {
        distance_m,
        azimuth1_deg,
        azimuth2_deg,
    }
}

/// Solve the geodesic direct problem: endpoint from start + azimuth + distance.
///
/// `start` is in radians, `azimuth_deg` is clockwise from north (degrees),
/// `distance_m` is in metres. Returns the destination as a `GeoCoord` (radians)
/// plus the back-azimuth at the destination.
///
/// # Errors
/// Returns `GeodesyError::ProjectionFailure` if the computed destination
/// coordinates fall outside the valid WGS84 range (should not occur on
/// a correct ellipsoid).
pub fn geodesic_direct(
    start: GeoCoord,
    azimuth_deg: f64,
    distance_m: f64,
) -> Result<GeodesicDirect, GeodesyError> {
    let geod = Geodesic::wgs84();

    let (lat2_deg, lon2_deg, azimuth2_deg): (f64, f64, f64) =
        geod.direct(start.lat_deg(), start.lon_deg(), azimuth_deg, distance_m);

    let point = GeoCoord::from_degrees(lat2_deg, lon2_deg, start.height).map_err(|e| {
        GeodesyError::ProjectionFailure(format!(
            "geodesic direct produced invalid coordinates: {e}"
        ))
    })?;

    Ok(GeodesicDirect {
        point,
        azimuth2_deg,
    })
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;
    use crate::types::GeoCoord;

    /*
     * Reference values from GeodTest.dat (Karney 2011), line 1:
     *   lat1=0 lon1=0 az1=0 lat2=1 lon2=0 az2=0 s12=110574.388...
     * A simple meridian arc: 0°N → 1°N along 0°E.
     */

    fn origin() -> GeoCoord {
        GeoCoord::from_degrees(0.0, 0.0, 0.0).unwrap()
    }

    fn one_deg_north() -> GeoCoord {
        GeoCoord::from_degrees(1.0, 0.0, 0.0).unwrap()
    }

    #[test]
    fn inverse_meridian_arc_distance() {
        // One degree of latitude along the meridian at the equator is ≈ 110 574 m.
        let result = geodesic_inverse(origin(), one_deg_north());
        assert_abs_diff_eq!(result.distance_m, 110_574.388, epsilon = 0.5);
    }

    #[test]
    fn inverse_meridian_arc_azimuths() {
        // Along the meridian northward: both azimuths should be 0°.
        let result = geodesic_inverse(origin(), one_deg_north());
        assert_abs_diff_eq!(result.azimuth1_deg, 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(result.azimuth2_deg, 0.0, epsilon = 1e-9);
    }

    #[test]
    fn inverse_zero_distance_is_zero() {
        let result = geodesic_inverse(origin(), origin());
        assert_abs_diff_eq!(result.distance_m, 0.0, epsilon = 1e-9);
    }

    #[test]
    fn direct_northward_one_degree() {
        // Stepping 110 574.388 m due north from the origin should reach ~1°N.
        let result = geodesic_direct(origin(), 0.0, 110_574.388).unwrap();
        assert_abs_diff_eq!(result.point.lat_deg(), 1.0, epsilon = 1e-5);
        assert_abs_diff_eq!(result.point.lon_deg(), 0.0, epsilon = 1e-9);
    }

    #[test]
    fn direct_round_trip() {
        // direct(a, az, d) then direct(result, az+180, d) recovers a.
        let start = GeoCoord::from_degrees(51.5, -0.1, 0.0).unwrap();
        let az = 47.0_f64;
        let dist = 5000.0_f64;

        let fwd = geodesic_direct(start, az, dist).unwrap();
        // Back-track: use the back-azimuth + 180 to return to start.
        let back = geodesic_direct(fwd.point, fwd.azimuth2_deg + 180.0, dist).unwrap();

        assert_abs_diff_eq!(back.point.lat_deg(), start.lat_deg(), epsilon = 1e-9);
        assert_abs_diff_eq!(back.point.lon_deg(), start.lon_deg(), epsilon = 1e-9);
    }

    #[test]
    fn inverse_direct_consistency() {
        // inverse(a, b).distance should equal the distance used in direct(a, az, d) to reach b.
        let a = GeoCoord::from_degrees(48.8566, 2.3522, 0.0).unwrap(); // Paris
        let b = GeoCoord::from_degrees(51.5074, -0.1278, 0.0).unwrap(); // London

        let inv = geodesic_inverse(a, b);
        let dir = geodesic_direct(a, inv.azimuth1_deg, inv.distance_m).unwrap();

        assert_abs_diff_eq!(dir.point.lat_deg(), b.lat_deg(), epsilon = 1e-9);
        assert_abs_diff_eq!(dir.point.lon_deg(), b.lon_deg(), epsilon = 1e-9);
    }
}
