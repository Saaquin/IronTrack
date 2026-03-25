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

// Photogrammetry subsystem: sensor geometry, GSD, FOV, swath width,
// flight line generation, side-lap validation over variable DEM terrain,
// and dynamic AGL adjustment.

pub mod flightlines;
pub mod sensor;

pub use flightlines::FlightPlan;

#[cfg(test)]
mod tests {
    // Phantom 4 Pro at 120 m AGL → GSD ≈ 3.29 cm/px (within 0.01 cm/px).
    #[test]
    #[ignore = "stub: implement when GSD formula is complete"]
    fn gsd_phantom4pro_120m() {}

    // Doubling AGL must exactly double GSD (linear relationship).
    #[test]
    #[ignore = "stub: implement when GSD formula is complete"]
    fn gsd_linear_with_agl() {}

    // Side overlap outside [0.0, 1.0) returns PhotogrammetryError::InvalidOverlap.
    #[test]
    #[ignore = "stub: implement when overlap validation is complete"]
    fn invalid_side_overlap_returns_error() {}
}
