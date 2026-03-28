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

// I/O subsystem: RFC 7946 GeoJSON FeatureCollection export,
// QGroundControl .plan mission export, and flight plan report generation.

pub mod dji;
pub mod geojson;
pub mod kml;
pub mod qgc;

pub use dji::{build_dji_kmz, write_dji_kmz};
pub use geojson::{geojson_to_linestrings, geojson_to_polygons, write_geojson};
pub use kml::{parse_boundary, parse_centerline};
pub use qgc::write_qgc_plan;
