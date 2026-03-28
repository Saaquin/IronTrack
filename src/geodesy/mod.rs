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

// Geodesy subsystem: Karney geodesic solver, UTM Karney-Krüger projection,
// and geoid undulation lookup (EGM96 + EGM2008).

pub mod geoid;
pub mod karney;
pub mod utm;

pub use geoid::Egm2008Model;
pub use karney::{geodesic_direct, geodesic_inverse, GeodesicDirect, GeodesicInverse};
pub use utm::{central_meridian, utm_to_wgs84, utm_zone, wgs84_to_utm, wgs84_to_utm_in_zone};

#[cfg(test)]
mod tests {
    // Karney inverse: distance and azimuth between two known points.
    // Reference values from GeodTest.dat (see .agents/testing.md).
    #[test]
    #[ignore = "stub: implement when karney inverse is complete"]
    fn karney_inverse_known_points() {}

    // Karney direct: destination from origin + azimuth + distance.
    #[test]
    #[ignore = "stub: implement when karney direct is complete"]
    fn karney_direct_known_points() {}

    // UTM zone 32N: London (51.5°N, 0.1°W) easting must be ~699 311 m.
    #[test]
    #[ignore = "stub: implement when utm projection is complete"]
    fn utm_zone_32n_london_spot_check() {}

    // EGM96 undulation: 0°N 0°E → N ≈ 17.16 m (NIMA TR8350.2).
    #[test]
    fn egm96_undulation_origin_spot_check() {
        use approx::assert_abs_diff_eq;
        let n = super::geoid::geoid_undulation_egm96(0.0, 0.0).expect("valid coordinates");
        assert_abs_diff_eq!(n, 17.16, epsilon = 1.5);
    }
}
